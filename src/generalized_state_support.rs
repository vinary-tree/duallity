use std::sync::Arc;

use lling_llang::prelude::StateId;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::fx_hash_map_with_capacity;

pub(crate) type LabelBuffer = SmallVec<[char; 4]>;
pub(crate) type ByteBuffer = SmallVec<[u8; 8]>;

/// Exact product identity: dictionary path, query offset, and scaled cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProductState {
    pub(crate) dict_node_id: u32,
    pub(crate) query_byte_pos: usize,
    pub(crate) cost: usize,
}

/// One canonical multi-label operation. Interned once per chain, never once per
/// continuation position, so hashing remains linear in the label count.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EmissionChain {
    pub(crate) input: Arc<[char]>,
    pub(crate) output: Arc<[char]>,
    pub(crate) target: StateId,
}

#[derive(Clone, Debug)]
pub(crate) struct EmitState {
    pub(crate) chain: Arc<EmissionChain>,
    pub(crate) position: usize,
    pub(crate) next: StateId,
}

pub(crate) struct QuerySegment<'a> {
    pub(crate) chars: LabelBuffer,
    pub(crate) bytes: &'a [u8],
    pub(crate) end_byte_pos: usize,
}

impl<'a> QuerySegment<'a> {
    pub(crate) fn new(segment: &'a str, end_byte_pos: usize) -> Self {
        Self {
            chars: segment.chars().collect(),
            bytes: segment.as_bytes(),
            end_byte_pos,
        }
    }
}

/// Cache absence too: a query can be too short, or a dictionary path set empty.
#[derive(Default)]
pub(crate) enum WidthCacheEntry<T> {
    #[default]
    Uncomputed,
    Missing,
    Ready(T),
}

pub(crate) type QuerySegmentCache<'a> = SmallVec<[WidthCacheEntry<QuerySegment<'a>>; 6]>;
pub(crate) type DictPaths = SmallVec<[DictPath; 4]>;
pub(crate) type DictPathCache = SmallVec<[WidthCacheEntry<DictPaths>; 6]>;

#[derive(Clone, Debug)]
pub(crate) enum RegisteredState {
    Product(ProductState),
    Emit(EmitState),
}

/// Stable, shared identities. Evicting cached transitions does not evict IDs.
pub(crate) struct StateRegistry {
    product_to_id: FxHashMap<ProductState, StateId>,
    chain_to_id: FxHashMap<Arc<EmissionChain>, StateId>,
    id_to_state: Vec<RegisteredState>,
}

impl StateRegistry {
    pub(crate) fn new() -> Self {
        let start = ProductState {
            dict_node_id: 0,
            query_byte_pos: 0,
            cost: 0,
        };
        let mut product_to_id = fx_hash_map_with_capacity(1);
        product_to_id.insert(start, 0);
        Self {
            product_to_id,
            chain_to_id: FxHashMap::default(),
            id_to_state: vec![RegisteredState::Product(start)],
        }
    }

    pub(crate) fn get(&self, state: StateId) -> Option<&RegisteredState> {
        self.id_to_state.get(usize_from_state_id(state))
    }

    pub(crate) fn len(&self) -> usize {
        self.id_to_state.len()
    }

    pub(crate) fn product_id(&self, state: ProductState) -> Option<StateId> {
        self.product_to_id.get(&state).copied()
    }

    pub(crate) fn chain_id(&self, chain: &EmissionChain) -> Option<StateId> {
        self.chain_to_id.get(chain).copied()
    }

    pub(crate) fn try_reserve_additional(
        &mut self,
        product_count: usize,
        chain_count: usize,
        state_count: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        self.product_to_id.try_reserve(product_count)?;
        self.chain_to_id.try_reserve(chain_count)?;
        self.id_to_state.try_reserve(state_count)
    }

    /// Caller holds the shared write lock and has reserved all capacity.
    pub(crate) fn commit_prepared(&mut self, states: Vec<RegisteredState>) {
        for state in states {
            let id = next_state_id(self.len()).expect("preflight validated every state ID");
            match &state {
                RegisteredState::Product(product) => {
                    self.product_to_id.insert(*product, id);
                }
                RegisteredState::Emit(emit) if emit.position == 1 => {
                    self.chain_to_id.insert(Arc::clone(&emit.chain), id);
                }
                RegisteredState::Emit(_) => {}
            }
            self.id_to_state.push(state);
        }
    }
}

/// A node referenced by its committed ID or by a local staging offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PendingDictionaryNode {
    Registered(u32),
    Staged(usize),
}

#[derive(Clone)]
pub(crate) struct DictPath {
    pub(crate) target_node: PendingDictionaryNode,
    pub(crate) output: LabelBuffer,
    pub(crate) bytes: ByteBuffer,
}

#[inline]
pub(crate) fn next_state_id(len: usize) -> Option<StateId> {
    StateId::try_from(len)
        .ok()
        .filter(|id| *id != lling_llang::wfst::NO_STATE)
}

#[inline]
pub(crate) fn usize_from_state_id(id: StateId) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}
