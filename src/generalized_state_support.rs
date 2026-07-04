use std::sync::Arc;

use lling_llang::prelude::{StateId, TropicalWeight, WeightedTransition};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::fx_hash_map_with_capacity;
use crate::generalized_ops::canonical_cost;

pub(crate) type LabelBuffer = SmallVec<[char; 4]>;
pub(crate) type ByteBuffer = SmallVec<[u8; 8]>;

/// Product component of a generalized WFST state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProductState {
    /// Registered dictionary node.
    pub(crate) dict_node_id: u32,
    /// Byte offset in the query string.
    pub(crate) query_byte_pos: usize,
    /// Accumulated operation cost used for bounded pruning.
    pub(crate) cost: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProductStateKey {
    dict_node_id: u32,
    query_byte_pos: usize,
    cost_bits: u64,
}

impl From<ProductState> for ProductStateKey {
    fn from(state: ProductState) -> Self {
        Self {
            dict_node_id: state.dict_node_id,
            query_byte_pos: state.query_byte_pos,
            cost_bits: cost_bits(state.cost),
        }
    }
}

/// Continuation state for emitting a multi-symbol operation through a char WFST.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EmitState {
    pub(crate) input: Arc<[char]>,
    pub(crate) output: Arc<[char]>,
    pub(crate) input_pos: usize,
    pub(crate) output_pos: usize,
    pub(crate) target: StateId,
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

pub(crate) type QuerySegmentCache<'a> = SmallVec<[(usize, QuerySegment<'a>); 6]>;
pub(crate) type DictPaths = SmallVec<[DictPath; 4]>;
pub(crate) type DictPathCache = SmallVec<[(usize, DictPaths); 6]>;

#[derive(Clone, Debug)]
pub(crate) enum RegisteredState {
    Product(ProductState),
    Emit(Arc<EmitState>),
}

/// Registry for product and continuation states.
pub(crate) struct StateRegistry {
    pub(crate) product_to_id: FxHashMap<ProductStateKey, StateId>,
    pub(crate) emit_to_id: FxHashMap<Arc<EmitState>, StateId>,
    pub(crate) id_to_state: Vec<RegisteredState>,
}

impl StateRegistry {
    pub(crate) fn new() -> Self {
        let start = ProductState {
            dict_node_id: 0,
            query_byte_pos: 0,
            cost: 0.0,
        };
        let mut product_to_id = fx_hash_map_with_capacity(1);
        product_to_id.insert(ProductStateKey::from(start), 0);
        let id_to_state = vec![RegisteredState::Product(start)];

        Self {
            product_to_id,
            emit_to_id: FxHashMap::default(),
            id_to_state,
        }
    }

    pub(crate) fn get(&self, state: StateId) -> Option<&RegisteredState> {
        self.id_to_state.get(usize_from_state_id(state))
    }

    pub(crate) fn len(&self) -> usize {
        self.id_to_state.len()
    }

    pub(crate) fn register_product(&mut self, state: ProductState) -> Option<StateId> {
        let key = ProductStateKey::from(state);
        if let Some(&id) = self.product_to_id.get(&key) {
            return Some(id);
        }

        let id = next_state_id(self.id_to_state.len())?;
        self.id_to_state.push(RegisteredState::Product(state));
        self.product_to_id.insert(key, id);
        Some(id)
    }

    pub(crate) fn register_emit(&mut self, state: EmitState) -> Option<StateId> {
        if let Some(&id) = self.emit_to_id.get(&state) {
            return Some(id);
        }

        let id = next_state_id(self.id_to_state.len())?;
        let state = Arc::new(state);
        self.id_to_state
            .push(RegisteredState::Emit(Arc::clone(&state)));
        self.emit_to_id.insert(state, id);
        Some(id)
    }
}

#[derive(Clone)]
pub(crate) struct DictPath {
    pub(crate) target_node_id: u32,
    pub(crate) output: LabelBuffer,
    pub(crate) bytes: ByteBuffer,
}

pub(crate) fn reserve_operation_path_capacity(
    transitions: &mut SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    path_count: usize,
) {
    if path_count > 1 {
        transitions.reserve(path_count);
    }
}

pub(crate) fn next_label_pair(
    input: &[char],
    output: &[char],
    input_pos: usize,
    output_pos: usize,
) -> Option<(Option<char>, Option<char>, usize, usize)> {
    let input_label = input.get(input_pos).copied();
    let output_label = output.get(output_pos).copied();

    if input_label.is_none() && output_label.is_none() {
        return None;
    }

    let next_input_pos = if input_label.is_some() {
        input_pos.checked_add(1)?
    } else {
        input_pos
    };
    let next_output_pos = if output_label.is_some() {
        output_pos.checked_add(1)?
    } else {
        output_pos
    };

    Some((input_label, output_label, next_input_pos, next_output_pos))
}

#[inline]
fn cost_bits(cost: f64) -> u64 {
    canonical_cost(cost).to_bits()
}

#[inline]
pub(crate) fn next_state_id(len: usize) -> Option<StateId> {
    len.try_into().ok()
}

#[inline]
pub(crate) fn usize_from_state_id(id: StateId) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}
