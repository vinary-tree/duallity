//! Project-owned scalar-WFST resources over foreign dictionary snapshots.
//!
//! A constructor captures `vt.dictionary.v1` exactly once and then builds the
//! requested duallity adapter over that immutable revision. The resulting
//! `vt.scalar-wfst.1` resource may outlive the source dictionary handle. State
//! expansion clones only the lightweight WFST/cache shell; registries and the
//! dictionary snapshot remain shared, so independent calls are reentrant.

use crate::{FzfWfst, GeneralizedWfstBuilder, LevenshteinWfst, UniversalLevenshteinWfst};
use libdictenstein::{Dictionary, DictionaryNode, SnapshotTraversalCursor, SyncStrategy};
use liblevenshtein::transducer::universal::{MergeAndSplit, Standard, Transposition};
use liblevenshtein::transducer::Algorithm;
use lling_llang::bindings::{OwnedWfstResource, ScalarWfstProvider, ScalarWfstState};
use lling_llang::prelude::{ArcticWeight, LazyWfst, StateId, TropicalWeight, Wfst};
use std::ffi::c_void;
use std::fmt;
use std::sync::{Arc, Mutex};
use vinary_tree_interop::{
    dictionary_flags, VtDictionaryEdge, VtDictionaryVTable, VtResource, VtResourceVTable, VtStatus,
    VtUnitDomain, VtWeightDomain, VtWfstArc, VT_ABI_VERSION, VT_DICTIONARY_INTERFACE_ID,
    VT_DICTIONARY_INTERFACE_VERSION, VT_RECOMMENDED_EDGE_BATCH,
};

/// Which project-owned duallity WFST should be constructed.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WfstKind {
    /// Parameterized Levenshtein product using the selected edit algorithm.
    Levenshtein = 0,
    /// Universal standard Levenshtein automaton.
    UniversalStandard = 1,
    /// Universal adjacent-transposition automaton.
    UniversalTransposition = 2,
    /// Universal merge-and-split automaton.
    UniversalMergeAndSplit = 3,
    /// Generalized automaton with standard operations.
    GeneralizedStandard = 4,
    /// Generalized automaton with transposition operations.
    GeneralizedTransposition = 5,
    /// Generalized automaton with merge-and-split operations.
    GeneralizedMergeAndSplit = 6,
    /// Generalized automaton with phonetic digraph operations.
    GeneralizedPhonetic = 7,
    /// FZF V2 path scorer in the Arctic (max-plus) semiring.
    Fzf = 8,
}

/// Error while capturing a dictionary or constructing its duallity WFST.
#[derive(Clone, Debug, PartialEq)]
pub enum BindingError {
    /// One or both resource words were null.
    NullResource,
    /// Base resource vtable is incompatible.
    IncompatibleResourceAbi,
    /// Resource does not implement the dictionary interface.
    MissingDictionaryInterface,
    /// Dictionary vtable is incomplete or incompatible.
    IncompatibleDictionaryInterface,
    /// Duallity's text WFST adapters require Unicode scalar dictionary labels.
    UnitDomainMismatch(VtUnitDomain),
    /// Provider callback failed.
    Provider(VtStatus),
    /// Provider output violated the interface contract.
    InvalidProviderOutput(&'static str),
    /// Requested construction parameters are unsupported or out of range.
    InvalidArgument(String),
}

impl fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullResource => formatter.write_str("resource context or vtable is null"),
            Self::IncompatibleResourceAbi => formatter.write_str("incompatible resource ABI"),
            Self::MissingDictionaryInterface => {
                formatter.write_str("resource has no dictionary interface")
            }
            Self::IncompatibleDictionaryInterface => {
                formatter.write_str("incompatible dictionary interface")
            }
            Self::UnitDomainMismatch(domain) => write!(
                formatter,
                "expected Unicode scalar dictionary, got {domain:?}"
            ),
            Self::Provider(status) => write!(formatter, "dictionary provider returned {status:?}"),
            Self::InvalidProviderOutput(message) => write!(
                formatter,
                "dictionary provider returned invalid output: {message}"
            ),
            Self::InvalidArgument(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for BindingError {}

fn status(raw: u32) -> Result<(), BindingError> {
    // The wire carries a raw u32 (interop status rule): decode before any
    // enum-typed use; an out-of-range discriminant is provider misbehavior,
    // never undefined behavior (family hardening LLEV-B6).
    let Some(value) = VtStatus::from_raw(raw) else {
        return Err(BindingError::InvalidProviderOutput(
            "provider returned an out-of-range status code",
        ));
    };
    if value.is_ok() {
        Ok(())
    } else {
        Err(BindingError::Provider(value))
    }
}

struct OwnedRaw(VtResource);
impl Drop for OwnedRaw {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                if let Some(release) = (*self.0.vtable).release {
                    release(self.0.context);
                }
            }
        }
    }
}

enum Gate {
    Parallel,
    Serial(Mutex<()>),
}

struct DictionaryProvider {
    resource: OwnedRaw,
    table: *const VtDictionaryVTable,
    gate: Gate,
    root: u64,
    len: Option<usize>,
    fault: Mutex<Option<VtStatus>>,
}

unsafe impl Send for DictionaryProvider {}
unsafe impl Sync for DictionaryProvider {}

impl DictionaryProvider {
    unsafe fn capture(resource: VtResource) -> Result<Arc<Self>, BindingError> {
        let live = discover_dictionary(resource)?;
        let mut snapshot = VtResource::NULL;
        status((*live).snapshot.unwrap()(resource.context, &mut snapshot))?;
        if snapshot.is_null() {
            return Err(BindingError::InvalidProviderOutput(
                "snapshot returned null",
            ));
        }
        let snapshot = OwnedRaw(snapshot);
        let table = discover_dictionary(snapshot.0)?;
        if (*table).unit_domain != VtUnitDomain::UnicodeScalar {
            return Err(BindingError::UnitDomainMismatch((*table).unit_domain));
        }
        let gate = if (*table).flags & dictionary_flags::PARALLEL_REENTRANT != 0 {
            Gate::Parallel
        } else {
            Gate::Serial(Mutex::new(()))
        };
        let mut root = 0;
        let root_status = match &gate {
            Gate::Parallel => (*table).root.unwrap()(snapshot.0.context, &mut root),
            Gate::Serial(lock) => {
                let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                (*table).root.unwrap()(snapshot.0.context, &mut root)
            }
        };
        status(root_status)?;
        let len = if let Some(len_callback) = (*table).len {
            let mut len = 0;
            let mut known = 0;
            let len_status = match &gate {
                Gate::Parallel => len_callback(snapshot.0.context, &mut len, &mut known),
                Gate::Serial(lock) => {
                    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                    len_callback(snapshot.0.context, &mut len, &mut known)
                }
            };
            status(len_status)?;
            match known {
                0 => None,
                1 => Some(len),
                _ => {
                    return Err(BindingError::InvalidProviderOutput(
                        "len known flag was not zero or one",
                    ))
                }
            }
        } else {
            None
        };
        Ok(Arc::new(Self {
            resource: snapshot,
            table,
            gate,
            root,
            len,
            fault: Mutex::new(None),
        }))
    }

    fn call<T>(&self, operation: impl FnOnce() -> T) -> T {
        match &self.gate {
            Gate::Parallel => operation(),
            Gate::Serial(lock) => {
                let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                operation()
            }
        }
    }

    fn record(&self, status: VtStatus) {
        let mut fault = self.fault.lock().unwrap_or_else(|p| p.into_inner());
        if fault.is_none() {
            *fault = Some(status);
        }
    }

    fn take_fault(&self) -> Option<VtStatus> {
        self.fault.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    fn table(&self) -> &VtDictionaryVTable {
        unsafe { &*self.table }
    }
}

unsafe fn discover_dictionary(
    resource: VtResource,
) -> Result<*const VtDictionaryVTable, BindingError> {
    if resource.is_null() {
        return Err(BindingError::NullResource);
    }
    let base = &*resource.vtable;
    if base.struct_size < std::mem::size_of::<VtResourceVTable>()
        || base.abi_version != VT_ABI_VERSION
        || base.retain.is_none()
        || base.release.is_none()
        || base.query_interface.is_none()
    {
        return Err(BindingError::IncompatibleResourceAbi);
    }
    let mut interface: *const c_void = std::ptr::null();
    let result = base.query_interface.unwrap()(
        resource.context,
        &VT_DICTIONARY_INTERFACE_ID,
        VT_DICTIONARY_INTERFACE_VERSION,
        &mut interface,
    );
    if VtStatus::from_raw(result) == Some(VtStatus::Unsupported) {
        return Err(BindingError::MissingDictionaryInterface);
    }
    status(result)?;
    if interface.is_null() {
        return Err(BindingError::InvalidProviderOutput(
            "query_interface returned null",
        ));
    }
    let table = interface.cast::<VtDictionaryVTable>();
    let value = &*table;
    if value.struct_size < std::mem::size_of::<VtDictionaryVTable>()
        || value.interface_version < VT_DICTIONARY_INTERFACE_VERSION
        || value.snapshot.is_none()
        || value.root.is_none()
        || value.node_is_final.is_none()
        || value.node_edges.is_none()
    {
        return Err(BindingError::IncompatibleDictionaryInterface);
    }
    Ok(table)
}

/// Immutable dictionary revision captured from a foreign provider.
#[derive(Clone)]
pub struct ResourceDictionary {
    provider: Arc<DictionaryProvider>,
}

impl ResourceDictionary {
    /// Capture the revision visible at this call.
    ///
    /// # Safety
    /// The borrowed resource and its vtables must obey the interop ABI for the
    /// duration of this call.
    pub unsafe fn capture(resource: VtResource) -> Result<Self, BindingError> {
        Ok(Self {
            provider: DictionaryProvider::capture(resource)?,
        })
    }

    fn take_fault(&self) -> Option<VtStatus> {
        self.provider.take_fault()
    }
}

#[derive(Clone)]
pub struct ResourceNode {
    provider: Arc<DictionaryProvider>,
    id: u64,
}

impl ResourceNode {
    fn fail<T>(&self, status: VtStatus, fallback: T) -> T {
        self.provider.record(status);
        fallback
    }

    fn expanded_edges(&self) -> Vec<(char, Self)> {
        let callback = self.provider.table().node_edges.unwrap();
        let mut result = Vec::with_capacity(VT_RECOMMENDED_EDGE_BATCH);
        let mut offset = 0;
        loop {
            let mut page = vec![VtDictionaryEdge::default(); VT_RECOMMENDED_EDGE_BATCH];
            let mut written = 0;
            let mut total = 0;
            let callback_status = self.provider.call(|| unsafe {
                callback(
                    self.provider.resource.0.context,
                    self.id,
                    offset,
                    page.as_mut_ptr(),
                    page.len(),
                    &mut written,
                    &mut total,
                )
            });
            let callback_status =
                VtStatus::from_raw(callback_status).unwrap_or(VtStatus::ProviderError);
            if !callback_status.is_ok() {
                return self.fail(callback_status, Vec::new());
            }
            // Paging acceptance harmonized to the single proven predicate
            // `ConsumerAcceptance.accepts_dec` (family finding F3 / LLEV-B8):
            // reject a reply unless every conjunct holds — `written <= capacity`
            // (over-fill), `offset <= total` and `offset + written <= total`
            // (past the end), and progress (an empty page is legal only when the
            // node is exhausted). The `offset > total` conjunct is stated
            // explicitly so this predicate is textually identical to the sibling
            // consumers (llev/lling `src/bindings.rs`) and each proven rejection
            // lemma maps to one conjunct; it is subsumed by the saturating
            // `offset + written > total` check but kept for that correspondence.
            // The claimed-total sanity bound (`total <= max_total`) is realized
            // STRUCTURALLY: neither `result` nor `page` is ever sized from the
            // provider-reported `total`, so an inflated total cannot drive an
            // allocation abort.
            if written > page.len()
                || offset > total
                || offset.saturating_add(written) > total
                || (written == 0 && offset < total)
            {
                return self.fail(VtStatus::ProviderError, Vec::new());
            }
            for edge in page.into_iter().take(written) {
                let Some(label) = u32::try_from(edge.label).ok().and_then(char::from_u32) else {
                    return self.fail(VtStatus::ProviderError, Vec::new());
                };
                result.push((
                    label,
                    Self {
                        provider: Arc::clone(&self.provider),
                        id: edge.node,
                    },
                ));
            }
            offset += written;
            if offset == total {
                return result;
            }
        }
    }
}

impl DictionaryNode for ResourceNode {
    type Unit = char;
    type SnapshotCursor = SnapshotTraversalCursor;
    type SnapshotGraphValueHandle = SnapshotTraversalCursor;

    fn is_final(&self) -> bool {
        let mut result = 0;
        let callback = self.provider.table().node_is_final.unwrap();
        let callback_status = self
            .provider
            .call(|| unsafe { callback(self.provider.resource.0.context, self.id, &mut result) });
        let callback_status =
            VtStatus::from_raw(callback_status).unwrap_or(VtStatus::ProviderError);
        if !callback_status.is_ok() {
            return self.fail(callback_status, false);
        }
        match result {
            0 => false,
            1 => true,
            _ => self.fail(VtStatus::ProviderError, false),
        }
    }

    fn transition(&self, label: char) -> Option<Self> {
        let Some(callback) = self.provider.table().node_transition else {
            return self
                .expanded_edges()
                .into_iter()
                .find_map(|(edge, node)| (edge == label).then_some(node));
        };
        let mut child = 0;
        let mut found = 0;
        let callback_status = self.provider.call(|| unsafe {
            callback(
                self.provider.resource.0.context,
                self.id,
                u64::from(u32::from(label)),
                &mut child,
                &mut found,
            )
        });
        let callback_status =
            VtStatus::from_raw(callback_status).unwrap_or(VtStatus::ProviderError);
        if !callback_status.is_ok() {
            return self.fail(callback_status, None);
        }
        match found {
            0 => None,
            1 => Some(Self {
                provider: Arc::clone(&self.provider),
                id: child,
            }),
            _ => self.fail(VtStatus::ProviderError, None),
        }
    }

    fn edges(&self) -> Box<dyn Iterator<Item = (char, Self)> + '_> {
        Box::new(self.expanded_edges().into_iter())
    }
}

impl Dictionary for ResourceDictionary {
    type Node = ResourceNode;
    fn root(&self) -> Self::Node {
        ResourceNode {
            provider: Arc::clone(&self.provider),
            id: self.provider.root,
        }
    }
    fn len(&self) -> Option<usize> {
        self.provider.len
    }
    fn sync_strategy(&self) -> SyncStrategy {
        SyncStrategy::Persistent
    }
}

enum Adapter {
    Levenshtein(LevenshteinWfst<ResourceDictionary>),
    UniversalStandard(UniversalLevenshteinWfst<Standard, ResourceDictionary>),
    UniversalTransposition(UniversalLevenshteinWfst<Transposition, ResourceDictionary>),
    UniversalMergeAndSplit(UniversalLevenshteinWfst<MergeAndSplit, ResourceDictionary>),
    Generalized(crate::GeneralizedWfst<ResourceDictionary>),
    Fzf(FzfWfst<ResourceDictionary>),
}

struct AdapterProvider {
    adapter: Adapter,
    dictionary: ResourceDictionary,
}

fn tropical_state<W>(
    mut wfst: W,
    dictionary: &ResourceDictionary,
    state: u64,
) -> Result<ScalarWfstState, VtStatus>
where
    W: LazyWfst<char, TropicalWeight>,
{
    let Ok(state) = StateId::try_from(state) else {
        return Ok(ScalarWfstState {
            valid: false,
            is_final: false,
            final_weight: f64::INFINITY,
            arcs: vec![],
        });
    };
    if !wfst.is_valid_state(state) {
        return Ok(ScalarWfstState {
            valid: false,
            is_final: false,
            final_weight: f64::INFINITY,
            arcs: vec![],
        });
    }
    let arcs = wfst
        .transitions_lazy(state)
        .iter()
        .map(|arc| VtWfstArc {
            input_label: arc.input.map_or(0, |label| u64::from(u32::from(label))),
            output_label: arc.output.map_or(0, |label| u64::from(u32::from(label))),
            target_state: u64::from(arc.to),
            weight: arc.weight.value(),
            has_input: u8::from(arc.input.is_some()),
            has_output: u8::from(arc.output.is_some()),
            reserved: [0; 6],
        })
        .collect();
    if let Some(status) = dictionary.take_fault() {
        return Err(status);
    }
    Ok(ScalarWfstState {
        valid: true,
        is_final: wfst.is_final(state),
        final_weight: wfst.final_weight(state).value(),
        arcs,
    })
}

fn arctic_state<W>(
    mut wfst: W,
    dictionary: &ResourceDictionary,
    state: u64,
) -> Result<ScalarWfstState, VtStatus>
where
    W: LazyWfst<char, ArcticWeight>,
{
    let Ok(state) = StateId::try_from(state) else {
        return Ok(ScalarWfstState {
            valid: false,
            is_final: false,
            final_weight: f64::NEG_INFINITY,
            arcs: vec![],
        });
    };
    if !wfst.is_valid_state(state) {
        return Ok(ScalarWfstState {
            valid: false,
            is_final: false,
            final_weight: f64::NEG_INFINITY,
            arcs: vec![],
        });
    }
    let arcs = wfst
        .transitions_lazy(state)
        .iter()
        .map(|arc| VtWfstArc {
            input_label: arc.input.map_or(0, |label| u64::from(u32::from(label))),
            output_label: arc.output.map_or(0, |label| u64::from(u32::from(label))),
            target_state: u64::from(arc.to),
            weight: arc.weight.value(),
            has_input: u8::from(arc.input.is_some()),
            has_output: u8::from(arc.output.is_some()),
            reserved: [0; 6],
        })
        .collect();
    if let Some(status) = dictionary.take_fault() {
        return Err(status);
    }
    Ok(ScalarWfstState {
        valid: true,
        is_final: wfst.is_final(state),
        final_weight: wfst.final_weight(state).value(),
        arcs,
    })
}

impl ScalarWfstProvider for AdapterProvider {
    fn weight_domain(&self) -> VtWeightDomain {
        if matches!(self.adapter, Adapter::Fzf(_)) {
            VtWeightDomain::ArcticF64
        } else {
            VtWeightDomain::TropicalF64
        }
    }

    fn start(&self) -> Result<u64, VtStatus> {
        let start = match &self.adapter {
            Adapter::Levenshtein(wfst) => wfst.start(),
            Adapter::UniversalStandard(wfst) => wfst.start(),
            Adapter::UniversalTransposition(wfst) => wfst.start(),
            Adapter::UniversalMergeAndSplit(wfst) => wfst.start(),
            Adapter::Generalized(wfst) => wfst.start(),
            Adapter::Fzf(wfst) => wfst.start(),
        };
        Ok(u64::from(start))
    }

    fn num_states(&self) -> Result<Option<usize>, VtStatus> {
        Ok(None)
    }

    fn state(&self, state: u64) -> Result<ScalarWfstState, VtStatus> {
        match &self.adapter {
            Adapter::Levenshtein(wfst) => tropical_state(wfst.clone(), &self.dictionary, state),
            Adapter::UniversalStandard(wfst) => {
                tropical_state(wfst.clone(), &self.dictionary, state)
            }
            Adapter::UniversalTransposition(wfst) => {
                tropical_state(wfst.clone(), &self.dictionary, state)
            }
            Adapter::UniversalMergeAndSplit(wfst) => {
                tropical_state(wfst.clone(), &self.dictionary, state)
            }
            Adapter::Generalized(wfst) => tropical_state(wfst.clone(), &self.dictionary, state),
            Adapter::Fzf(wfst) => arctic_state(wfst.clone(), &self.dictionary, state),
        }
    }
}

/// Capture a dictionary and construct a lazy duallity WFST resource.
///
/// Construction is O(1) in dictionary size; no terms are serialized or copied.
/// State expansion remains lazy after the resource crosses a language boundary.
///
/// # Safety
/// `dictionary` must be a live `vt.dictionary.v1` resource whose base and
/// dictionary vtables remain valid for the duration of this call. A successful
/// snapshot callback transfers an independent retain to the returned resource.
pub unsafe fn create_wfst(
    dictionary: VtResource,
    query: &str,
    maximum_distance: usize,
    algorithm: Algorithm,
    kind: WfstKind,
) -> Result<OwnedWfstResource, BindingError> {
    let dictionary = ResourceDictionary::capture(dictionary)?;
    let adapter = match kind {
        WfstKind::Levenshtein => Adapter::Levenshtein(LevenshteinWfst::with_algorithm(
            &dictionary,
            query,
            maximum_distance,
            algorithm,
        )),
        WfstKind::UniversalStandard
        | WfstKind::UniversalTransposition
        | WfstKind::UniversalMergeAndSplit => {
            let distance = u8::try_from(maximum_distance).map_err(|_| {
                BindingError::InvalidArgument("universal maximum distance must fit u8".into())
            })?;
            match kind {
                WfstKind::UniversalStandard => Adapter::UniversalStandard(
                    UniversalLevenshteinWfst::new(&dictionary, query, distance),
                ),
                WfstKind::UniversalTransposition => Adapter::UniversalTransposition(
                    UniversalLevenshteinWfst::new(&dictionary, query, distance),
                ),
                WfstKind::UniversalMergeAndSplit => Adapter::UniversalMergeAndSplit(
                    UniversalLevenshteinWfst::new(&dictionary, query, distance),
                ),
                _ => unreachable!(),
            }
        }
        WfstKind::GeneralizedStandard
        | WfstKind::GeneralizedTransposition
        | WfstKind::GeneralizedMergeAndSplit
        | WfstKind::GeneralizedPhonetic => {
            let distance = u8::try_from(maximum_distance).map_err(|_| {
                BindingError::InvalidArgument("generalized maximum distance must fit u8".into())
            })?;
            let builder = GeneralizedWfstBuilder::new(&dictionary)
                .query(query)
                .max_distance(distance);
            let builder = match kind {
                WfstKind::GeneralizedStandard => builder.with_standard_ops(),
                WfstKind::GeneralizedTransposition => builder.with_transposition(),
                WfstKind::GeneralizedMergeAndSplit => builder.with_merge_split(),
                WfstKind::GeneralizedPhonetic => builder.with_phonetic_digraphs(),
                _ => unreachable!(),
            };
            Adapter::Generalized(builder.build().map_err(BindingError::InvalidArgument)?)
        }
        WfstKind::Fzf => Adapter::Fzf(
            FzfWfst::new(&dictionary, query)
                .map_err(|error| BindingError::InvalidArgument(error.to_string()))?,
        ),
    };
    if let Some(status) = dictionary.take_fault() {
        return Err(BindingError::Provider(status));
    }
    Ok(OwnedWfstResource::from_provider(Arc::new(
        AdapterProvider {
            adapter,
            dictionary,
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
    use lling_llang::bindings::OwnedWfstResource as LlingWfstResource;
    use lling_llang::semiring::TropicalWeight;
    use lling_llang::wfst::{MutableWfst, VectorWfst};
    use std::collections::HashSet;
    use vinary_tree_interop::{VtWfstVTable, VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION};

    #[test]
    fn captured_dictionary_revision_survives_mutation_and_handle_drop() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"cat", None).unwrap();
        let source = dictionary.resource();
        let wfst = unsafe {
            create_wfst(
                source.as_raw(),
                "cat",
                1,
                Algorithm::Standard,
                WfstKind::Levenshtein,
            )
        }
        .unwrap();
        dictionary.remove_text(b"cat").unwrap();
        dictionary.insert_text(b"dog", None).unwrap();
        drop(source);
        drop(dictionary);

        unsafe {
            let resource = wfst.as_raw();
            let mut interface: *const c_void = std::ptr::null();
            assert_eq!(
                (*resource.vtable).query_interface.unwrap()(
                    resource.context,
                    &VT_WFST_INTERFACE_ID,
                    VT_WFST_INTERFACE_VERSION,
                    &mut interface
                ),
                VtStatus::Ok.to_raw()
            );
            let table = &*interface.cast::<VtWfstVTable>();
            assert_eq!(table.weight_domain, VtWeightDomain::TropicalF64);
            let mut seen = HashSet::new();
            let mut frontier = vec![0u64];
            let mut found_final = false;
            while let Some(state) = frontier.pop() {
                if !seen.insert(state) {
                    continue;
                }
                let mut valid = 0;
                let mut final_state = 0;
                let mut weight = 0.0;
                assert_eq!(
                    table.state_info.unwrap()(
                        resource.context,
                        state,
                        &mut valid,
                        &mut final_state,
                        &mut weight
                    ),
                    VtStatus::Ok.to_raw()
                );
                found_final |= final_state == 1;
                let mut arcs = [VtWfstArc::default(); 64];
                let mut written = 0;
                let mut total = 0;
                assert_eq!(
                    table.state_arcs.unwrap()(
                        resource.context,
                        state,
                        0,
                        arcs.as_mut_ptr(),
                        arcs.len(),
                        &mut written,
                        &mut total
                    ),
                    VtStatus::Ok.to_raw()
                );
                assert_eq!(written, total);
                frontier.extend(arcs[..written].iter().map(|arc| arc.target_state));
            }
            assert!(found_final);
        }
    }

    #[test]
    fn retained_duallity_snapshot_composes_after_all_source_handles_are_dropped() {
        let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
        dictionary.insert_text(b"cat", Some(1)).unwrap();
        let source = dictionary.resource();
        let edit = unsafe {
            create_wfst(
                source.as_raw(),
                "cat",
                1,
                Algorithm::Standard,
                WfstKind::Levenshtein,
            )
        }
        .unwrap();
        dictionary.clear();
        drop(source);
        drop(dictionary);

        let mut uppercase = VectorWfst::new();
        let states = [
            uppercase.add_state(),
            uppercase.add_state(),
            uppercase.add_state(),
            uppercase.add_state(),
        ];
        uppercase.set_start(states[0]);
        uppercase.set_final(states[3], TropicalWeight::new(0.0));
        for (index, (input, output)) in [('c', 'C'), ('a', 'A'), ('t', 'T')].into_iter().enumerate()
        {
            uppercase.add_arc(
                states[index],
                Some(input),
                Some(output),
                states[index + 1],
                TropicalWeight::new(0.0),
            );
        }
        let uppercase = LlingWfstResource::from_wfst(uppercase);
        let composed = LlingWfstResource::compose(edit.as_raw(), uppercase.as_raw()).unwrap();
        drop(edit);
        drop(uppercase);

        unsafe {
            let resource = composed.as_raw();
            let mut interface: *const c_void = std::ptr::null();
            assert_eq!(
                (*resource.vtable).query_interface.unwrap()(
                    resource.context,
                    &VT_WFST_INTERFACE_ID,
                    VT_WFST_INTERFACE_VERSION,
                    &mut interface,
                ),
                VtStatus::Ok.to_raw(),
            );
            let table = &*interface.cast::<VtWfstVTable>();
            let mut start = 0;
            assert_eq!(
                table.start.unwrap()(resource.context, &mut start),
                VtStatus::Ok.to_raw(),
            );
            let mut seen = HashSet::new();
            let mut frontier = vec![(start, String::new())];
            let mut accepted = false;
            while let Some((state, output)) = frontier.pop() {
                if !seen.insert((state, output.clone())) {
                    continue;
                }
                let mut valid = 0;
                let mut final_state = 0;
                let mut final_weight = 0.0;
                assert_eq!(
                    table.state_info.unwrap()(
                        resource.context,
                        state,
                        &mut valid,
                        &mut final_state,
                        &mut final_weight,
                    ),
                    VtStatus::Ok.to_raw(),
                );
                accepted |= final_state == 1 && output == "CAT";
                let mut arcs = [VtWfstArc::default(); 128];
                let mut written = 0;
                let mut total = 0;
                assert_eq!(
                    table.state_arcs.unwrap()(
                        resource.context,
                        state,
                        0,
                        arcs.as_mut_ptr(),
                        arcs.len(),
                        &mut written,
                        &mut total,
                    ),
                    VtStatus::Ok.to_raw(),
                );
                assert_eq!(written, total);
                for arc in &arcs[..written] {
                    let mut next_output = output.clone();
                    if arc.has_output == 1 {
                        next_output.push(char::from_u32(arc.output_label as u32).unwrap());
                    }
                    frontier.push((arc.target_state, next_output));
                }
            }
            assert!(accepted);
        }
    }
}
