//! Synchronous per-invocation foreign-dictionary diagnostics.
//!
//! A guard cannot move across threads. Nested calls record into the innermost
//! scope for their provider, and errors remain observable until that scope ends.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use vinary_tree_interop::VtStatus;

#[derive(Clone, Copy)]
enum Boundary {
    Provider(usize),
    Computation,
}

struct Frame {
    boundary: Boundary,
    fault: Rc<Cell<Option<VtStatus>>>,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct FaultScope {
    fault: Rc<Cell<Option<VtStatus>>>,
}

impl FaultScope {
    pub(super) fn enter(provider: usize) -> Result<Self, VtStatus> {
        Self::push(Boundary::Provider(provider))
    }

    pub(super) fn computation() -> Result<Self, VtStatus> {
        Self::push(Boundary::Computation)
    }

    fn push(boundary: Boundary) -> Result<Self, VtStatus> {
        let fault = Rc::new(Cell::new(None));
        FRAMES
            .try_with(|frames| {
                frames.borrow_mut().push(Frame {
                    boundary,
                    fault: Rc::clone(&fault),
                });
            })
            .map_err(|_| VtStatus::ProviderError)?;
        Ok(Self { fault })
    }

    pub(super) fn check(&self) -> Result<(), VtStatus> {
        self.fault.get().map_or(Ok(()), Err)
    }
}

impl Drop for FaultScope {
    fn drop(&mut self) {
        // Remove first and drop outside the TLS borrow, including during unwind.
        let frame = FRAMES
            .try_with(|frames| {
                let mut frames = frames.borrow_mut();
                frames
                    .iter()
                    .rposition(|frame| Rc::ptr_eq(&frame.fault, &self.fault))
                    .map(|index| frames.remove(index))
            })
            .ok()
            .flatten();
        drop(frame);
    }
}

pub(super) fn record(provider: usize, status: VtStatus) {
    let sinks = FRAMES
        .try_with(|frames| {
            let frames = frames.borrow();
            let Some((index, frame)) = frames.iter().enumerate().rev().find(|(_, frame)| {
                matches!(frame.boundary, Boundary::Computation)
                    || matches!(frame.boundary, Boundary::Provider(id) if id == provider)
            }) else {
                return [None, None];
            };
            let mut sinks = [Some(Rc::clone(&frame.fault)), None];
            if matches!(frame.boundary, Boundary::Computation) {
                // Keep the enclosing with_checked contract, but never poison
                // an older computation whose nested failure may be handled.
                for outer in frames[..index].iter().rev() {
                    match outer.boundary {
                        Boundary::Computation => break,
                        Boundary::Provider(id) if id == provider => {
                            sinks[1] = Some(Rc::clone(&outer.fault));
                            break;
                        }
                        Boundary::Provider(_) => {}
                    }
                }
            }
            sinks
        })
        .unwrap_or([None, None]);
    // DictionaryNode is infallible. Unscoped use must fail loudly rather than
    // turn a provider error into an empty branch or nonfinal node.
    assert!(
        sinks[0].is_some(),
        "foreign dictionary callback failed outside a checked scope: {status:?}"
    );
    for sink in sinks.into_iter().flatten() {
        if sink.get().is_none() {
            sink.set(Some(status));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computation_captures_its_provider_beneath_an_unrelated_checked_scope() {
        let provider_a = FaultScope::enter(1).expect("provider A");
        let provider_b = FaultScope::enter(2).expect("provider B");
        let computation = FaultScope::computation().expect("computation");
        record(1, VtStatus::IoError);
        assert_eq!(computation.check(), Err(VtStatus::IoError));
        assert_eq!(provider_a.check(), Err(VtStatus::IoError));
        assert_eq!(provider_b.check(), Ok(()));
    }

    #[test]
    fn nested_recovery_boundaries_shield_outer_computations() {
        let provider_a = FaultScope::enter(1).expect("provider A");
        let outer = FaultScope::computation().expect("outer computation");
        {
            let inner = FaultScope::computation().expect("inner computation");
            record(1, VtStatus::IoError);
            assert_eq!(inner.check(), Err(VtStatus::IoError));
        }
        assert_eq!(outer.check(), Ok(()));
        assert_eq!(
            provider_a.check(),
            Ok(()),
            "forwarding stops at the older computation"
        );
        {
            let provider_b = FaultScope::enter(2).expect("nested provider B");
            let inner = FaultScope::computation().expect("inner computation");
            record(2, VtStatus::LimitExceeded);
            assert_eq!(inner.check(), Err(VtStatus::LimitExceeded));
            assert_eq!(provider_b.check(), Err(VtStatus::LimitExceeded));
        }
        assert_eq!(outer.check(), Ok(()));
        {
            let recovery = FaultScope::enter(1).expect("explicit recovery boundary");
            record(1, VtStatus::Closed);
            assert_eq!(recovery.check(), Err(VtStatus::Closed));
        }
        assert_eq!(outer.check(), Ok(()));
        record(1, VtStatus::ProviderError);
        record(2, VtStatus::IoError);
        assert_eq!(
            outer.check(),
            Err(VtStatus::ProviderError),
            "first error wins across providers"
        );
        assert_eq!(provider_a.check(), Err(VtStatus::ProviderError));
    }

    #[test]
    fn nested_scopes_keep_first_fault_and_route_by_provider() {
        let outer = FaultScope::enter(1).expect("TLS available");
        record(1, VtStatus::LimitExceeded);
        {
            let inner = FaultScope::enter(1).expect("nested scope");
            assert_eq!(inner.check(), Ok(()));
            record(1, VtStatus::InvalidArgument);
            record(1, VtStatus::ProviderError);
            assert_eq!(inner.check(), Err(VtStatus::InvalidArgument));
            assert_eq!(outer.check(), Err(VtStatus::LimitExceeded));
        }
        {
            let other = FaultScope::enter(2).expect("other provider");
            record(1, VtStatus::ProviderError);
            assert_eq!(other.check(), Ok(()));
            assert_eq!(outer.check(), Err(VtStatus::LimitExceeded));
        }
        assert_eq!(outer.check(), Err(VtStatus::LimitExceeded));
    }

    #[test]
    fn unwind_and_concurrent_threads_do_not_leak_faults() {
        let caught = std::panic::catch_unwind(|| {
            let _scope = FaultScope::enter(3).expect("TLS available");
            record(3, VtStatus::LimitExceeded);
            panic!("test unwind");
        });
        assert!(caught.is_err());
        assert!(FRAMES.with(|frames| frames.borrow().is_empty()));
        let outer = FaultScope::enter(3).expect("fresh scope");
        let thread = std::thread::spawn(|| {
            let scope = FaultScope::enter(3).expect("thread scope");
            record(3, VtStatus::LimitExceeded);
            scope.check()
        });
        assert_eq!(thread.join().expect("worker"), Err(VtStatus::LimitExceeded));
        assert_eq!(outer.check(), Ok(()));
    }
}
