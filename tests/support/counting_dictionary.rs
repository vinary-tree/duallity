//! An instrumented, hand-rolled `vt.dictionary.v1` provider.
//!
//! This is the duallity-side counterpart of liblevenshtein's
//! `tests/support/high_degree_dictionary.rs`. It exists so the family/ABI
//! suites can observe the *foreign consumer* behavior of `src/bindings.rs`
//! directly, without a real libdictenstein dictionary in the loop:
//!
//!   - **Call metrics** — every dictionary callback (`snapshot`, `root`,
//!     `len`, `node_is_final`, `node_edges`) bumps an atomic counter shared by
//!     the source resource and every snapshot it hands out. The
//!     snapshot-capture-once suite asserts `snapshot` fires exactly once at
//!     `duallity_wfst_new` and never during traversal, and that construction is
//!     O(1) in the dictionary (zero `node_edges` before the first expansion).
//!
//!   - **Retain/release ledger** — `retain`, `release`, and every context
//!     handed out (`into_raw`) are counted, so a test can assert the balance
//!     `release == into_raw + retain` after teardown (no leaked snapshot
//!     retains, no double frees).
//!
//!   - **Programmable paging misbehavior** — the target node's `node_edges`
//!     can emit exactly the adversarial replies the `ConsumerAcceptance`
//!     rejection lemmas name (over-fill, past-end, stalled progress, inflated
//!     total), so the harmonized `expanded_edges` predicate (finding F3) can be
//!     shown to reject each one as a provider error without an allocation
//!     abort.
//!
//! The honest `node_edges` path uses the same ascending-label firstn/skipn
//! paging shape proved in `docs/verification/abi/theories/PagingLaws.v`.

#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use vinary_tree_interop::{
    dictionary_flags, VtDictionaryEdge, VtDictionaryVTable, VtInterfaceId, VtResource,
    VtResourceVTable, VtStatus, VtUnitDomain, VtValueDomain, VT_ABI_VERSION,
    VT_DICTIONARY_INTERFACE_ID, VT_DICTIONARY_INTERFACE_VERSION,
};

/// A paging misbehavior injected into one node's `node_edges` reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Misbehavior {
    /// Report `written` greater than the requested capacity.
    Overfill,
    /// Report `written` greater than `total - start`.
    PastEnd,
    /// Report `written == 0` while items still remain (no progress).
    StalledProgress,
    /// Report an enormous `out_total` (the preallocation-abort vector).
    InflatedTotal,
    /// Return an explicit error after one complete successful page.
    LatePageFailure,
    /// Change the supposedly immutable total on the second page.
    ChangingTotal,
}

/// Constructor callback at which to return an explicit provider failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureCallback {
    Snapshot,
    Root,
    Len,
}

/// One immutable dictionary node: finality plus ascending `(label, child)`.
#[derive(Clone, Default)]
struct Node {
    is_final: bool,
    edges: Vec<(u64, u64)>,
}

#[derive(Default)]
struct Metrics {
    snapshot: AtomicUsize,
    root: AtomicUsize,
    len: AtomicUsize,
    is_final: AtomicUsize,
    edges: AtomicUsize,
    into_raw: AtomicUsize,
    retain: AtomicUsize,
    release: AtomicUsize,
    max_capacity: AtomicUsize,
}

struct Model {
    nodes: Vec<Node>,
    len: usize,
    /// `(node, mode)`: inject `mode` into `node`'s `node_edges` reply only.
    misbehavior: Option<(u64, Misbehavior)>,
    capture_failure: Option<(CaptureCallback, VtStatus)>,
    metrics: Metrics,
}

struct Context {
    model: Arc<Model>,
}

fn make_resource(model: Arc<Model>) -> VtResource {
    model.metrics.into_raw.fetch_add(1, Ordering::Relaxed);
    VtResource {
        context: Arc::into_raw(Arc::new(Context { model })).cast_mut().cast(),
        vtable: &RESOURCE_VTABLE,
    }
}

unsafe fn context<'a>(raw: *mut c_void) -> &'a Context {
    &*raw.cast::<Context>()
}

unsafe extern "C" fn retain(raw: *mut c_void) {
    context(raw)
        .model
        .metrics
        .retain
        .fetch_add(1, Ordering::Relaxed);
    Arc::increment_strong_count(raw.cast::<Context>());
}

unsafe extern "C" fn release(raw: *mut c_void) {
    // Record before dropping: this drop may be the last owner of the context,
    // but the owning `CountingDictionary` handle keeps its own `Arc<Model>`, so
    // the metrics themselves outlive every released context.
    context(raw)
        .model
        .metrics
        .release
        .fetch_add(1, Ordering::Relaxed);
    drop(Arc::from_raw(raw.cast::<Context>()));
}

unsafe extern "C" fn query_interface(
    raw: *mut c_void,
    interface_id: *const VtInterfaceId,
    minimum_version: u32,
    out_vtable: *mut *const c_void,
) -> u32 {
    if interface_id.is_null() || out_vtable.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    if (*interface_id).bytes != VT_DICTIONARY_INTERFACE_ID.bytes
        || minimum_version > VT_DICTIONARY_INTERFACE_VERSION
    {
        return VtStatus::Unsupported.to_raw();
    }
    let _ = context(raw);
    out_vtable.write((&DICTIONARY_VTABLE as *const VtDictionaryVTable).cast());
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn snapshot(raw: *mut c_void, out: *mut VtResource) -> u32 {
    if out.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let source = context(raw);
    source
        .model
        .metrics
        .snapshot
        .fetch_add(1, Ordering::Relaxed);
    if let Some((CaptureCallback::Snapshot, status)) = source.model.capture_failure {
        return status.to_raw();
    }
    // The model is already immutable, so a snapshot is a fresh retain of the
    // same revision (shared metrics included).
    out.write(make_resource(Arc::clone(&source.model)));
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn root(raw: *mut c_void, out_node: *mut u64) -> u32 {
    if out_node.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    context(raw)
        .model
        .metrics
        .root
        .fetch_add(1, Ordering::Relaxed);
    if let Some((CaptureCallback::Root, status)) = context(raw).model.capture_failure {
        return status.to_raw();
    }
    out_node.write(0);
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn len(raw: *mut c_void, out_len: *mut usize, out_known: *mut u8) -> u32 {
    if out_len.is_null() || out_known.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let model = &context(raw).model;
    model.metrics.len.fetch_add(1, Ordering::Relaxed);
    if let Some((CaptureCallback::Len, status)) = model.capture_failure {
        return status.to_raw();
    }
    out_len.write(model.len);
    out_known.write(1);
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn node_is_final(raw: *mut c_void, node: u64, out: *mut u8) -> u32 {
    if out.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let model = &context(raw).model;
    model.metrics.is_final.fetch_add(1, Ordering::Relaxed);
    let Some(node) = model.nodes.get(node as usize) else {
        return VtStatus::InvalidArgument.to_raw();
    };
    out.write(u8::from(node.is_final));
    VtStatus::Ok.to_raw()
}

unsafe extern "C" fn node_edges(
    raw: *mut c_void,
    node: u64,
    start: usize,
    out_edges: *mut VtDictionaryEdge,
    capacity: usize,
    out_written: *mut usize,
    out_total: *mut usize,
) -> u32 {
    if (capacity != 0 && out_edges.is_null()) || out_written.is_null() || out_total.is_null() {
        return VtStatus::NullPointer.to_raw();
    }
    let model = &context(raw).model;
    model.metrics.edges.fetch_add(1, Ordering::Relaxed);
    model
        .metrics
        .max_capacity
        .fetch_max(capacity, Ordering::Relaxed);
    let node_index = node as usize;
    let Some(node) = model.nodes.get(node_index) else {
        return VtStatus::InvalidArgument.to_raw();
    };
    let total = node.edges.len();

    if let Some((target, misbehavior)) = model.misbehavior {
        if target as usize == node_index {
            if misbehavior == Misbehavior::LatePageFailure && start > 0 {
                return VtStatus::IoError.to_raw();
            }
            let honest = total.saturating_sub(start).min(capacity);
            let (written, reported_total) = match misbehavior {
                Misbehavior::Overfill => (capacity + 1, total),
                Misbehavior::PastEnd => (total.saturating_sub(start) + 1, total),
                Misbehavior::StalledProgress => (0, total.max(1)),
                Misbehavior::InflatedTotal => (honest, usize::MAX),
                Misbehavior::LatePageFailure => (honest, total),
                Misbehavior::ChangingTotal => (honest, total + usize::from(start > 0)),
            };
            // Fill only addressable slots but REPORT the adversarial counts: the
            // consumer must reject on the reported counts, not the buffer.
            for offset in 0..honest {
                let (label, child) = node.edges[start + offset];
                out_edges
                    .add(offset)
                    .write(VtDictionaryEdge { label, node: child });
            }
            out_written.write(written);
            out_total.write(reported_total);
            return VtStatus::Ok.to_raw();
        }
    }

    let mut written = 0usize;
    for (label, child) in node.edges.iter().skip(start).take(capacity) {
        out_edges.add(written).write(VtDictionaryEdge {
            label: *label,
            node: *child,
        });
        written += 1;
    }
    out_written.write(written);
    out_total.write(total);
    VtStatus::Ok.to_raw()
}

static RESOURCE_VTABLE: VtResourceVTable = VtResourceVTable {
    struct_size: std::mem::size_of::<VtResourceVTable>(),
    abi_version: VT_ABI_VERSION,
    reserved: 0,
    retain: Some(retain),
    release: Some(release),
    query_interface: Some(query_interface),
};

static DICTIONARY_VTABLE: VtDictionaryVTable = VtDictionaryVTable {
    struct_size: std::mem::size_of::<VtDictionaryVTable>(),
    interface_version: VT_DICTIONARY_INTERFACE_VERSION,
    unit_domain: VtUnitDomain::UnicodeScalar,
    value_domain: VtValueDomain::Unit,
    flags: dictionary_flags::IMMUTABLE | dictionary_flags::PARALLEL_REENTRANT,
    snapshot: Some(snapshot),
    root: Some(root),
    len: Some(len),
    node_is_final: Some(node_is_final),
    // `node_value_u64` and `node_transition` are intentionally absent: leaving
    // `node_transition` unset routes every dictionary access through the
    // `node_edges` paging loop, which is exactly the surface finding F3 covers.
    node_value_u64: None,
    node_transition: None,
    node_edges: Some(node_edges),
};

/// Owning handle for an instrumented `vt.dictionary.v1` resource.
///
/// The handle keeps only an `Arc<Model>` (for metrics); it does not itself own
/// a resource retain. Every [`CountingDictionary::resource`] hands out a fresh
/// retain that the caller must release (directly, or by passing it to a
/// consumer that releases its captured snapshot on drop). The model — and thus
/// the metrics — outlives every context because each context clones the
/// `Arc<Model>`.
pub struct CountingDictionary {
    model: Arc<Model>,
}

impl CountingDictionary {
    fn from_model(model: Model) -> Self {
        Self {
            model: Arc::new(model),
        }
    }

    /// Build an honest trie over `terms` (Unicode scalar labels).
    pub fn from_terms(terms: &[&str]) -> Self {
        let mut nodes = vec![Node::default()];
        let mut len = 0usize;
        for term in terms {
            let mut current = 0usize;
            for scalar in term.chars() {
                let label = u64::from(scalar as u32);
                let existing = nodes[current]
                    .edges
                    .iter()
                    .find(|(edge_label, _)| *edge_label == label)
                    .map(|(_, child)| *child as usize);
                current = match existing {
                    Some(child) => child,
                    None => {
                        let child = nodes.len();
                        nodes[current].edges.push((label, child as u64));
                        nodes.push(Node::default());
                        child
                    }
                };
            }
            if !nodes[current].is_final {
                nodes[current].is_final = true;
                len += 1;
            }
        }
        for node in &mut nodes {
            node.edges.sort_by_key(|(label, _)| *label);
        }
        Self::from_model(Model {
            nodes,
            len,
            misbehavior: None,
            capture_failure: None,
            metrics: Metrics::default(),
        })
    }

    fn single_node_model(degree: usize, misbehavior: Option<Misbehavior>) -> Model {
        let mut nodes = vec![Node::default()];
        for index in 0..degree {
            let child = nodes.len() as u64;
            // Labels start at 0x100 so they stay distinct valid scalars well
            // below the surrogate range for the degrees these tests use.
            let label = 0x100u64 + index as u64;
            nodes[0].edges.push((label, child));
            nodes.push(Node {
                is_final: true,
                edges: vec![],
            });
        }
        Model {
            nodes,
            len: degree,
            misbehavior: misbehavior.map(|mode| (0, mode)),
            capture_failure: None,
            metrics: Metrics::default(),
        }
    }

    /// A single root of `degree` honest edges to distinct final leaves.
    ///
    /// A `degree` above [`vinary_tree_interop::VT_RECOMMENDED_EDGE_BATCH`]
    /// forces the consumer's `node_edges` paging loop across several batches.
    pub fn high_degree(degree: usize) -> Self {
        Self::from_model(Self::single_node_model(degree, None))
    }

    /// A single root of `degree` edges whose `node_edges` injects `misbehavior`.
    pub fn misbehaving(degree: usize, misbehavior: Misbehavior) -> Self {
        Self::from_model(Self::single_node_model(degree, Some(misbehavior)))
    }

    /// Fail one constructor callback without transferring a resource on failure.
    pub fn failing_capture(callback: CaptureCallback, status: VtStatus) -> Self {
        assert_ne!(status, VtStatus::Ok);
        let mut model = Self::single_node_model(1, None);
        model.capture_failure = Some((callback, status));
        Self::from_model(model)
    }

    /// Hand out a fresh owned resource retain. The caller must release it.
    pub fn resource(&self) -> VtResource {
        make_resource(Arc::clone(&self.model))
    }

    /// The label a single-node fixture assigns to its `index`-th edge, as a
    /// Unicode scalar, so a test can build the query that matches leaf `index`.
    pub fn high_degree_label(index: usize) -> char {
        char::from_u32(0x100 + index as u32).expect("0x100-based label is a scalar")
    }

    pub fn snapshot_calls(&self) -> usize {
        self.model.metrics.snapshot.load(Ordering::Relaxed)
    }
    pub fn root_calls(&self) -> usize {
        self.model.metrics.root.load(Ordering::Relaxed)
    }
    pub fn len_calls(&self) -> usize {
        self.model.metrics.len.load(Ordering::Relaxed)
    }
    pub fn is_final_calls(&self) -> usize {
        self.model.metrics.is_final.load(Ordering::Relaxed)
    }
    pub fn edges_calls(&self) -> usize {
        self.model.metrics.edges.load(Ordering::Relaxed)
    }
    pub fn handed_out_calls(&self) -> usize {
        self.model.metrics.into_raw.load(Ordering::Relaxed)
    }
    pub fn retain_calls(&self) -> usize {
        self.model.metrics.retain.load(Ordering::Relaxed)
    }
    pub fn release_calls(&self) -> usize {
        self.model.metrics.release.load(Ordering::Relaxed)
    }
    pub fn max_edge_page_capacity(&self) -> usize {
        self.model.metrics.max_capacity.load(Ordering::Relaxed)
    }

    /// Outstanding owned retains: `(handed_out + retain) - release`. Zero after
    /// a clean teardown; the retain/release ledger is balanced.
    pub fn outstanding_retains(&self) -> isize {
        let taken = self.handed_out_calls() + self.retain_calls();
        taken as isize - self.release_calls() as isize
    }
}
