use libdictenstein::DictionaryNode;
use liblevenshtein::transducer::Algorithm;
use lling_llang::prelude::StateId;

use crate::state_encoding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AutomatonState {
    Normal { query_pos: u32, edit_cost: u32 },
    TransposeSecond { query_pos: u32, edit_cost: u32 },
    MergeSecond { query_pos: u32, edit_cost: u32 },
    SplitSecond { query_pos: u32, edit_cost: u32 },
}

#[derive(Clone)]
pub(crate) struct LevenshteinStateCodec {
    query_len: usize,
    max_distance: usize,
    algorithm: Algorithm,
    normal_automaton_states: u32,
    max_automaton_states: u32,
}

impl LevenshteinStateCodec {
    pub(crate) fn new(query_len: usize, max_distance: usize, algorithm: Algorithm) -> Self {
        let normal_automaton_states =
            state_encoding::bounded_levenshtein_states(query_len, max_distance);
        let max_automaton_states =
            normal_automaton_states.saturating_mul(1 + continuation_state_kinds(algorithm));

        Self {
            query_len,
            max_distance,
            algorithm,
            normal_automaton_states,
            max_automaton_states,
        }
    }

    #[inline]
    pub(crate) fn max_distance(&self) -> usize {
        self.max_distance
    }

    #[inline]
    pub(crate) fn max_distance_u32(&self) -> u32 {
        saturating_u32(self.max_distance)
    }

    #[inline]
    pub(crate) fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    #[inline]
    pub(crate) fn max_automaton_states(&self) -> u32 {
        self.max_automaton_states
    }

    #[cfg(test)]
    pub(crate) fn set_max_automaton_states_for_testing(&mut self, max_automaton_states: u32) {
        self.max_automaton_states = max_automaton_states;
    }

    #[inline]
    pub(crate) fn decode_product_state(&self, state: StateId) -> Option<(u32, u32)> {
        state_encoding::decode(state, self.max_automaton_states)
    }

    #[inline]
    pub(crate) fn encode_product_state(
        &self,
        dict_node_id: u32,
        automaton_state_id: u32,
    ) -> Option<StateId> {
        state_encoding::try_encode(dict_node_id, automaton_state_id, self.max_automaton_states)
    }

    #[inline]
    pub(crate) fn encode_transition_target(
        &self,
        dict_node_id: u32,
        automaton_state_id: Option<u32>,
    ) -> Option<StateId> {
        self.encode_product_state(dict_node_id, automaton_state_id?)
    }

    #[inline]
    pub(crate) fn encode_automaton_state(&self, query_pos: u32, edit_cost: u32) -> Option<u32> {
        let state = query_pos
            .checked_mul(self.automaton_stride())
            .and_then(|base| base.checked_add(edit_cost))?;

        (state < self.normal_automaton_states).then_some(state)
    }

    #[inline]
    pub(crate) fn encode_transpose_second_state(
        &self,
        query_pos: u32,
        edit_cost: u32,
    ) -> Option<u32> {
        let state = self
            .normal_automaton_states
            .checked_add(self.encode_automaton_state(query_pos, edit_cost)?)?;

        (state < self.max_automaton_states).then_some(state)
    }

    #[inline]
    pub(crate) fn encode_merge_second_state(&self, query_pos: u32, edit_cost: u32) -> Option<u32> {
        self.continuation_state(merge_second_slot(self.algorithm)?, query_pos, edit_cost)
    }

    #[inline]
    pub(crate) fn encode_split_second_state(&self, query_pos: u32, edit_cost: u32) -> Option<u32> {
        self.continuation_state(split_second_slot(self.algorithm)?, query_pos, edit_cost)
    }

    #[inline]
    pub(crate) fn decode_automaton_state(&self, automaton_state_id: u32) -> Option<AutomatonState> {
        if automaton_state_id >= self.max_automaton_states {
            return None;
        }

        let normal_state_id = automaton_state_id % self.normal_automaton_states;
        let stride = self.automaton_stride();
        let query_pos = normal_state_id / stride;
        let edit_cost = normal_state_id % stride;

        if usize_from_u32(query_pos) > self.query_len
            || usize_from_u32(edit_cost) > self.max_distance
        {
            return None;
        }

        let slot = automaton_state_id / self.normal_automaton_states;
        match slot {
            0 => Some(AutomatonState::Normal {
                query_pos,
                edit_cost,
            }),
            slot if transpose_second_slot(self.algorithm) == Some(slot) => {
                Some(AutomatonState::TransposeSecond {
                    query_pos,
                    edit_cost,
                })
            }
            slot if merge_second_slot(self.algorithm) == Some(slot) => {
                Some(AutomatonState::MergeSecond {
                    query_pos,
                    edit_cost,
                })
            }
            slot if split_second_slot(self.algorithm) == Some(slot) => {
                Some(AutomatonState::SplitSecond {
                    query_pos,
                    edit_cost,
                })
            }
            _ => None,
        }
    }

    #[inline]
    fn automaton_stride(&self) -> u32 {
        self.max_distance_u32().saturating_add(1).max(1)
    }

    #[inline]
    fn continuation_state(&self, slot: u32, query_pos: u32, edit_cost: u32) -> Option<u32> {
        let state = self
            .normal_automaton_states
            .checked_mul(slot)
            .and_then(|base| {
                base.checked_add(self.encode_automaton_state(query_pos, edit_cost)?)
            })?;

        (state < self.max_automaton_states).then_some(state)
    }
}

#[inline]
pub(crate) fn continuation_state_kinds(algorithm: Algorithm) -> u32 {
    let mut count = 0;
    if algorithm.supports_transposition() {
        count += 1;
    }
    if algorithm.supports_merge_split() {
        count += 2;
    }
    count
}

#[inline]
pub(crate) fn transpose_second_slot(algorithm: Algorithm) -> Option<u32> {
    algorithm.supports_transposition().then_some(1)
}

#[inline]
pub(crate) fn merge_second_slot(algorithm: Algorithm) -> Option<u32> {
    algorithm
        .supports_merge_split()
        .then(|| 1 + u32::from(algorithm.supports_transposition()))
}

#[inline]
pub(crate) fn split_second_slot(algorithm: Algorithm) -> Option<u32> {
    merge_second_slot(algorithm).map(|slot| slot + 1)
}

#[inline]
pub(crate) fn node_has_any_edge<N>(node: &N) -> bool
where
    N: DictionaryNode,
{
    node.edge_count()
        .map_or_else(|| node.edges().next().is_some(), |count| count > 0)
}

#[inline]
pub(crate) fn node_has_char_edge<N>(node: &N, label: char) -> bool
where
    N: DictionaryNode,
    N::Unit: TryFrom<char> + Copy,
{
    match N::Unit::try_from(label) {
        Ok(unit) => node.has_edge(unit),
        Err(_) => false,
    }
}

#[inline]
pub(crate) fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[inline]
pub(crate) fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[inline]
pub(crate) fn next_index(index: usize, len: usize) -> Option<usize> {
    index.checked_add(1).filter(|&next| next < len)
}

#[inline]
pub(crate) fn within_max_distance(edit_cost: u32, remaining: usize, max_distance: usize) -> bool {
    usize_from_u32(edit_cost)
        .checked_add(remaining)
        .is_some_and(|cost| cost <= max_distance)
}

#[inline]
pub(crate) fn remaining_query_chars(query_len: usize, query_pos: u32) -> Option<usize> {
    query_len.checked_sub(usize_from_u32(query_pos))
}

#[inline]
pub(crate) fn bounded_count_to_f64(value: usize) -> Option<f64> {
    crate::exact_usize_to_f64(value)
}
