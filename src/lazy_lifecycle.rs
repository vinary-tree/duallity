//! Shared bridge between direct state computation and lling-llang lifecycles.

use lling_llang::prelude::{ExpansionRequest, Semiring, StateExpansion, StateId};

/// A state source that can be evaluated directly outside a managed lazy cache.
///
/// lling-llang's [`StateSource`] receives an [`ExpansionRequest`] so its lazy
/// wrapper can enforce snapshots, attempts, and cooperative cancellation. This
/// companion trait exposes the underlying deterministic state computation for
/// tests, specialized caches, and downstream integrations that already manage
/// those lifecycle concerns.
pub trait DirectStateSource<L, W: Semiring>: Clone + Send + Sync {
    /// Compute one state without allocating a managed expansion request.
    fn expand_state(&self, state: StateId) -> StateExpansion<L, W>;
}

/// Apply the request-scoped cancellation contract before direct computation.
#[inline]
pub(crate) fn fulfill_expansion_request<S, L, W>(
    source: &S,
    request: ExpansionRequest<'_>,
) -> StateExpansion<L, W>
where
    S: DirectStateSource<L, W>,
    W: Semiring,
{
    if let Some(reason) = request.cancellation().reason() {
        StateExpansion::cancelled(reason)
    } else {
        source.expand_state(request.state())
    }
}
