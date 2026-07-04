use std::sync::Arc;

use liblevenshtein::transducer::universal::{PositionVariant, UniversalState};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::fx_hash_map_with_capacity;

type UniversalStateKeyBytes = SmallVec<[u8; 96]>;

#[inline]
pub(crate) fn next_universal_registry_id(len: usize) -> Option<u32> {
    u32::try_from(len).ok()
}

#[inline]
pub(crate) fn universal_query_state_factor(query_len: usize) -> u32 {
    u32::try_from(query_len.saturating_add(1)).unwrap_or(u32::MAX)
}

#[inline]
pub(crate) fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[inline]
pub(crate) fn apply_i32_offset(base: usize, offset: i32) -> Option<usize> {
    if offset >= 0 {
        usize::try_from(offset)
            .ok()
            .and_then(|offset| base.checked_add(offset))
    } else {
        usize::try_from(offset.unsigned_abs())
            .ok()
            .and_then(|offset| base.checked_sub(offset))
    }
}

#[inline]
pub(crate) fn bounded_count_to_f64(value: usize) -> Option<f64> {
    crate::exact_usize_to_f64(value)
}

pub(crate) fn precompute_relevant_subwords(
    query_chars: &[char],
    max_distance: usize,
) -> Vec<String> {
    let cached_depths = query_chars.len().saturating_add(max_distance);
    (0..cached_depths)
        .map(|dict_depth| relevant_subword_at(query_chars, max_distance, dict_depth + 1))
        .collect()
}

pub(crate) fn relevant_subword_at(word: &[char], max_distance: usize, position: usize) -> String {
    let sentinel_count = max_distance.saturating_add(1).saturating_sub(position);
    let first_word_pos = position.saturating_sub(max_distance).max(1);
    let end_pos = position
        .saturating_add(max_distance)
        .saturating_add(1)
        .min(word.len());

    let char_capacity = sentinel_count.saturating_add(
        end_pos
            .checked_sub(first_word_pos)
            .map_or(0, |span| span.saturating_add(1)),
    );
    let mut result = String::with_capacity(char_capacity);

    for _ in 0..sentinel_count {
        result.push('$');
    }

    if first_word_pos <= end_pos {
        let start_idx = first_word_pos - 1;
        result.extend(word[start_idx..end_pos].iter().copied());
    }

    result
}

/// State registry for deduplicating Universal Levenshtein states.
///
/// Since `UniversalState<V>` is a complex set of positions, we assign each unique
/// `(universal state, consumed query-label position)` pair a sequential ID for
/// efficient WFST state encoding.
pub struct UniversalStateRegistry<V: PositionVariant> {
    /// Map from state to assigned ID.
    state_to_id: FxHashMap<UniversalStateKey, u32>,
    /// Map from ID back to state.
    id_to_state: Vec<RegisteredUniversalState<V>>,
}

/// Key for hashing a UniversalState plus the WFST query-label cursor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct UniversalStateKey {
    pub(crate) query_pos: usize,
    pub(crate) length_diff: i8,
    pub(crate) positions: UniversalStateKeyBytes,
}

#[derive(Clone)]
pub(crate) struct RegisteredUniversalState<V: PositionVariant> {
    pub(crate) state: Arc<UniversalState<V>>,
    pub(crate) query_pos: usize,
}

pub(crate) fn universal_state_key<V: PositionVariant>(
    state: &UniversalState<V>,
    query_pos: usize,
) -> UniversalStateKey {
    let positions = state.positions();
    let mut bytes = UniversalStateKeyBytes::with_capacity(
        crate::capped_size_hint_capacity(positions.size_hint().0).saturating_mul(1 + 4 + 1),
    );

    for pos in positions {
        let (pos_type, offset, errors) = if pos.is_i_type() {
            (0u8, pos.offset(), pos.errors())
        } else {
            (1u8, pos.offset(), pos.errors())
        };
        bytes.push(pos_type);
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.push(errors);
    }
    UniversalStateKey {
        query_pos,
        length_diff: state.length_diff(),
        positions: bytes,
    }
}

impl<V: PositionVariant> UniversalStateRegistry<V> {
    /// Create a new registry with the initial state.
    pub fn new(max_distance: u8) -> Self {
        let mut registry = Self {
            state_to_id: fx_hash_map_with_capacity(1),
            id_to_state: Vec::with_capacity(1),
        };

        // Register initial state as ID 0.
        let initial = UniversalState::initial(max_distance);
        let _ = registry.register_state_at(initial, 0);

        registry
    }

    /// Register a state and return its ID.
    pub fn register_state(&mut self, state: UniversalState<V>) -> Option<u32> {
        self.register_state_at(state, 0)
    }

    /// Register a state at an exact consumed query position and return its ID.
    pub(crate) fn register_state_at(
        &mut self,
        state: UniversalState<V>,
        query_pos: usize,
    ) -> Option<u32> {
        let key = self.state_to_key(&state, query_pos);
        if let Some(&id) = self.state_to_id.get(&key) {
            return Some(id);
        }

        let id = next_universal_registry_id(self.id_to_state.len())?;
        self.state_to_id.insert(key, id);
        self.id_to_state.push(RegisteredUniversalState {
            state: Arc::new(state),
            query_pos,
        });
        Some(id)
    }

    pub(crate) fn candidate_state_id_at(
        &self,
        state: &UniversalState<V>,
        query_pos: usize,
    ) -> Option<u32> {
        let key = self.state_to_key(state, query_pos);
        self.candidate_state_id_for_key(&key)
    }

    pub(crate) fn candidate_state_id_for_key(&self, key: &UniversalStateKey) -> Option<u32> {
        self.state_to_id
            .get(key)
            .copied()
            .or_else(|| next_universal_registry_id(self.id_to_state.len()))
    }

    pub(crate) fn register_state_with_key(
        &mut self,
        state: UniversalState<V>,
        query_pos: usize,
        key: UniversalStateKey,
    ) -> Option<u32> {
        if let Some(&id) = self.state_to_id.get(&key) {
            return Some(id);
        }

        let id = next_universal_registry_id(self.id_to_state.len())?;
        self.state_to_id.insert(key, id);
        self.id_to_state.push(RegisteredUniversalState {
            state: Arc::new(state),
            query_pos,
        });
        Some(id)
    }

    pub(crate) fn register_shared_state_at(
        &mut self,
        state: Arc<UniversalState<V>>,
        query_pos: usize,
    ) -> Option<u32> {
        let key = self.state_to_key(state.as_ref(), query_pos);
        if let Some(&id) = self.state_to_id.get(&key) {
            return Some(id);
        }

        let id = next_universal_registry_id(self.id_to_state.len())?;
        self.state_to_id.insert(key, id);
        self.id_to_state
            .push(RegisteredUniversalState { state, query_pos });
        Some(id)
    }

    /// Get a state by ID.
    pub fn get_state(&self, id: u32) -> Option<&UniversalState<V>> {
        self.id_to_state
            .get(usize_from_u32(id))
            .map(|registered| registered.state.as_ref())
    }

    pub(crate) fn get_registered_state(&self, id: u32) -> Option<&RegisteredUniversalState<V>> {
        self.id_to_state.get(usize_from_u32(id))
    }

    /// Convert a state to a hashable key.
    pub(crate) fn state_to_key(
        &self,
        state: &UniversalState<V>,
        query_pos: usize,
    ) -> UniversalStateKey {
        universal_state_key(state, query_pos)
    }

    /// Number of registered states.
    pub fn len(&self) -> usize {
        self.id_to_state.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.id_to_state.is_empty()
    }
}

pub(crate) fn universal_accepting_weight<V>(
    state: &UniversalState<V>,
    fixed_word_len: usize,
    processed_input_len: usize,
) -> Option<f64>
where
    V: PositionVariant,
{
    let max_distance = state.max_distance();

    state
        .positions()
        .filter_map(|pos| {
            if pos.is_m_type() {
                (pos.offset() <= 0 && pos.errors() <= max_distance).then(|| f64::from(pos.errors()))
            } else {
                let current_word_pos = apply_i32_offset(processed_input_len, pos.offset())?;
                let remaining_chars = fixed_word_len.checked_sub(current_word_pos)?;
                let remaining_errors = usize::from(max_distance.checked_sub(pos.errors())?);

                if remaining_chars <= remaining_errors {
                    Some(f64::from(pos.errors()) + bounded_count_to_f64(remaining_chars)?)
                } else {
                    None
                }
            }
        })
        .min_by(|left, right| left.total_cmp(right))
}
