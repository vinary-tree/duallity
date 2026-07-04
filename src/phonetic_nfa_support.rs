use std::sync::Arc;

use liblevenshtein::phonetic::nfa::{StateSet, TransitionLabelChar};
use lling_llang::prelude::StateId;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{fx_hash_map_with_capacity, fx_hash_set_with_capacity};

type StateSetKeyBuffer = SmallVec<[u32; 8]>;
type LabelSuccessorSlots = SmallVec<[(char, StateSet); 8]>;
type CandidateCharBuffer = SmallVec<[char; 64]>;

/// Key for hashing NFA state sets.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct StateSetKey(pub(crate) StateSetKeyBuffer);

const MAX_EXACT_CLASS_RANGE_CHARS: usize = 256;
const CANDIDATE_INLINE_CAPACITY: usize = 64;
const CANDIDATE_HASH_THRESHOLD: usize = 16;

/// Convert a StateSet to a hashable key.
pub(crate) fn state_set_to_key(state_set: &StateSet) -> StateSetKey {
    let mut states = StateSetKeyBuffer::with_capacity(state_set.len());
    states.extend(state_set.iter());
    states.sort_unstable();
    StateSetKey(states)
}

#[inline]
pub(crate) fn next_nfa_state_id(len: usize) -> Option<StateId> {
    len.try_into().ok()
}

#[inline]
pub(crate) fn usize_from_state_id(id: StateId) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}

pub(crate) struct CandidateChars {
    ordered: CandidateCharBuffer,
    seen: Option<FxHashSet<char>>,
    reserve_hint: usize,
}

impl CandidateChars {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            ordered: CandidateCharBuffer::with_capacity(capacity.min(CANDIDATE_INLINE_CAPACITY)),
            seen: None,
            reserve_hint: crate::capped_size_hint_capacity(capacity),
        }
    }

    pub(crate) fn push_unique(&mut self, candidate: char) {
        if let Some(seen) = &mut self.seen {
            if seen.insert(candidate) {
                self.ordered.push(candidate);
            }
            return;
        }

        if self.ordered.contains(&candidate) {
            return;
        }

        if self.ordered.len() >= CANDIDATE_HASH_THRESHOLD {
            self.materialize_seen();
            if self
                .seen
                .as_mut()
                .is_some_and(|seen| seen.insert(candidate))
            {
                self.ordered.push(candidate);
            }
        } else {
            self.ordered.push(candidate);
        }
    }

    pub(crate) fn as_slice(&self) -> &[char] {
        self.ordered.as_slice()
    }

    fn materialize_seen(&mut self) {
        let mut seen = fx_hash_set_with_capacity(
            crate::capped_size_hint_capacity(self.reserve_hint)
                .max(self.ordered.len().saturating_add(1)),
        );
        seen.extend(self.ordered.iter().copied());
        self.seen = Some(seen);
    }

    fn reserve_additional(&mut self, additional: usize) {
        if additional == 0 {
            return;
        }

        self.reserve_hint = self
            .reserve_hint
            .max(self.ordered.len().saturating_add(additional))
            .min(crate::MAX_SPECULATIVE_PREALLOCATION);
        self.ordered.reserve(additional);

        if let Some(seen) = &mut self.seen {
            seen.reserve(additional);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.ordered.len()
    }
}

pub(crate) struct LabelSuccessors {
    slots: LabelSuccessorSlots,
    index_by_char: Option<FxHashMap<char, usize>>,
}

impl LabelSuccessors {
    pub(crate) fn new(candidates: &[char]) -> Self {
        let slots = candidates
            .iter()
            .copied()
            .map(|candidate| (candidate, StateSet::new()))
            .collect();
        let index_by_char = if candidates.len() >= CANDIDATE_HASH_THRESHOLD {
            let mut index = fx_hash_map_with_capacity(candidates.len());
            for (slot, &candidate) in candidates.iter().enumerate() {
                index.insert(candidate, slot);
            }
            Some(index)
        } else {
            None
        };

        Self {
            slots,
            index_by_char,
        }
    }

    fn slot_mut(&mut self, candidate: char) -> Option<&mut StateSet> {
        if let Some(index) = self
            .index_by_char
            .as_ref()
            .and_then(|index| index.get(&candidate).copied())
        {
            return self.slots.get_mut(index).map(|(_, successors)| successors);
        }

        self.slots
            .iter_mut()
            .find_map(|(successor, successors)| (*successor == candidate).then_some(successors))
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &(char, StateSet)> {
        self.slots.iter()
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut (char, StateSet)> {
        self.slots.iter_mut()
    }

    pub(crate) fn into_slots(self) -> LabelSuccessorSlots {
        self.slots
    }
}

pub(crate) fn collect_label_successors(
    label: &TransitionLabelChar,
    target: u32,
    candidates: &[char],
    successors_by_char: &mut LabelSuccessors,
) {
    match label {
        TransitionLabelChar::Char(candidate) => {
            if let Some(successors) = successors_by_char.slot_mut(*candidate) {
                successors.insert(target);
            }
        }
        TransitionLabelChar::CharClass(class) => {
            debug_assert_eq!(successors_by_char.len(), candidates.len());
            for (candidate, successors) in successors_by_char.iter_mut() {
                if class.matches(*candidate) {
                    successors.insert(target);
                }
            }
        }
        TransitionLabelChar::Any => {
            debug_assert_eq!(successors_by_char.len(), candidates.len());
            for (_, successors) in successors_by_char.iter_mut() {
                successors.insert(target);
            }
        }
        _ => {}
    }
}

pub(crate) fn push_bounded_char_range(chars: &mut CandidateChars, start: char, end: char) {
    let Some(width) = char_range_width(start, end) else {
        return;
    };
    if width > MAX_EXACT_CLASS_RANGE_CHARS {
        return;
    }

    chars.reserve_additional(width);

    let mut codepoint = u32::from(start);
    let end = u32::from(end);
    while codepoint <= end {
        if let Some(candidate) = char::from_u32(codepoint) {
            chars.push_unique(candidate);
        }
        codepoint += 1;
    }
}

fn char_range_width(start: char, end: char) -> Option<usize> {
    let start = u32::from(start);
    let end = u32::from(end);
    if start > end {
        return None;
    }

    usize::try_from(end - start + 1).ok()
}

pub(crate) fn normalize_alphabet<I>(alphabet: I) -> Arc<[char]>
where
    I: IntoIterator<Item = char>,
{
    let alphabet = alphabet.into_iter();
    let reserve_hint =
        crate::capped_size_hint_capacity(alphabet.size_hint().0).min(MAX_EXACT_CLASS_RANGE_CHARS);
    let mut seen = fx_hash_set_with_capacity(reserve_hint);
    let mut deduped = Vec::with_capacity(reserve_hint);
    for candidate in alphabet {
        if seen.insert(candidate) {
            deduped.push(candidate);
        }
    }
    Arc::from(deduped)
}

pub(crate) fn default_phonetic_nfa_alphabet() -> impl Iterator<Item = char> {
    (b' '..=b'~').map(char::from)
}

#[inline]
pub(crate) fn phonetic_nfa_state_hint(nfa_state_count: usize, registered_states: usize) -> usize {
    nfa_state_count.saturating_mul(10).max(registered_states)
}

#[cfg(test)]
#[inline]
fn push_unique_char(chars: &mut SmallVec<[char; 64]>, candidate: char) {
    if !chars.contains(&candidate) {
        chars.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liblevenshtein::phonetic::nfa::CharClassChar;

    struct InflatedAlphabetHint {
        chars: std::vec::IntoIter<char>,
    }

    impl InflatedAlphabetHint {
        fn new(chars: Vec<char>) -> Self {
            Self {
                chars: chars.into_iter(),
            }
        }
    }

    impl Iterator for InflatedAlphabetHint {
        type Item = char;

        fn next(&mut self) -> Option<Self::Item> {
            self.chars.next()
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (usize::MAX, None)
        }
    }

    #[test]
    fn state_set_key_sorts_states_and_stays_inline_for_small_closures() {
        let mut state_set = StateSet::new();
        state_set.insert(7);
        state_set.insert(1);
        state_set.insert(3);

        let key = state_set_to_key(&state_set);

        assert_eq!(key.0.as_slice(), &[1, 3, 7]);
        assert!(!key.0.spilled());
    }

    #[test]
    fn next_nfa_state_id_rejects_unrepresentable_len() {
        let Ok(max_state_id) = usize::try_from(StateId::MAX) else {
            return;
        };
        assert_eq!(next_nfa_state_id(max_state_id), Some(StateId::MAX));

        if let Some(overflowing_len) = max_state_id.checked_add(1) {
            assert_eq!(next_nfa_state_id(overflowing_len), None);
        }
    }

    #[test]
    fn push_unique_char_preserves_first_seen_order() {
        let mut chars = SmallVec::<[char; 64]>::new();

        push_unique_char(&mut chars, 'b');
        push_unique_char(&mut chars, 'a');
        push_unique_char(&mut chars, 'b');

        assert_eq!(chars.as_slice(), &['b', 'a']);
        assert!(!chars.spilled());
    }

    #[test]
    fn candidate_chars_preserve_order_without_hashing_small_sets() {
        let mut chars = CandidateChars::with_capacity(3);

        chars.push_unique('b');
        chars.push_unique('a');
        chars.push_unique('b');

        assert_eq!(chars.as_slice(), &['b', 'a']);
        assert_eq!(chars.len(), 2);
        assert!(chars.seen.is_none());
        assert!(!chars.ordered.spilled());
    }

    #[test]
    fn candidate_chars_materialize_hash_membership_for_large_sets() {
        let mut chars = CandidateChars::with_capacity(32);

        for candidate in 'a'..='z' {
            chars.push_unique(candidate);
        }
        chars.push_unique('m');

        assert_eq!(chars.len(), 26);
        assert!(chars.seen.is_some());
        assert_eq!(chars.as_slice().first(), Some(&'a'));
        assert_eq!(chars.as_slice().last(), Some(&'z'));
    }

    #[test]
    fn candidate_chars_treat_initial_capacity_as_speculative() {
        let mut chars = CandidateChars::with_capacity(usize::MAX);

        assert_eq!(chars.reserve_hint, crate::MAX_SPECULATIVE_PREALLOCATION);

        for candidate in 'a'..='q' {
            chars.push_unique(candidate);
        }

        assert_eq!(chars.len(), 17);
        assert!(chars.seen.is_some());
        assert!(chars
            .seen
            .as_ref()
            .is_some_and(|seen| seen.capacity() < 100_000));
    }

    #[test]
    fn collect_label_successors_groups_targets_by_candidate() {
        let candidates = SmallVec::<[char; 64]>::from_slice(&['b', 'a']);
        let mut successors = LabelSuccessors::new(&candidates);
        assert!(successors.index_by_char.is_none());

        collect_label_successors(
            &TransitionLabelChar::Char('a'),
            3,
            &candidates,
            &mut successors,
        );
        collect_label_successors(
            &TransitionLabelChar::CharClass(CharClassChar::from_range('b', 'b')),
            4,
            &candidates,
            &mut successors,
        );
        collect_label_successors(&TransitionLabelChar::Any, 5, &candidates, &mut successors);
        collect_label_successors(
            &TransitionLabelChar::Epsilon,
            6,
            &candidates,
            &mut successors,
        );

        let a_successors = successors
            .iter()
            .find_map(|(candidate, successors)| (*candidate == 'a').then_some(successors))
            .expect("a successors should exist");
        let b_successors = successors
            .iter()
            .find_map(|(candidate, successors)| (*candidate == 'b').then_some(successors))
            .expect("b successors should exist");

        assert_eq!(
            successors
                .iter()
                .map(|(candidate, _)| *candidate)
                .collect::<Vec<_>>(),
            vec!['b', 'a']
        );
        assert!(a_successors.contains(&3));
        assert!(a_successors.contains(&5));
        assert!(!a_successors.contains(&6));
        assert!(b_successors.contains(&4));
        assert!(b_successors.contains(&5));
        assert!(!b_successors.contains(&6));
    }

    #[test]
    fn label_successors_index_large_candidate_sets() {
        let candidates = ('a'..='z').collect::<SmallVec<[char; 64]>>();
        let mut successors = LabelSuccessors::new(&candidates);

        assert!(successors.index_by_char.is_some());

        collect_label_successors(
            &TransitionLabelChar::Char('z'),
            11,
            &candidates,
            &mut successors,
        );
        collect_label_successors(
            &TransitionLabelChar::Char('a'),
            13,
            &candidates,
            &mut successors,
        );

        let slots = successors.into_slots();
        assert_eq!(slots.first().map(|(candidate, _)| *candidate), Some('a'));
        assert_eq!(slots.last().map(|(candidate, _)| *candidate), Some('z'));
        assert!(slots[0].1.contains(&13));
        assert!(slots[25].1.contains(&11));
    }

    #[test]
    fn push_bounded_char_range_enumerates_small_positive_classes() {
        let mut chars = CandidateChars::with_capacity(3);

        push_bounded_char_range(&mut chars, 'a', 'c');
        push_bounded_char_range(&mut chars, 'b', 'd');

        assert_eq!(chars.as_slice(), &['a', 'b', 'c', 'd']);
    }

    #[test]
    fn push_bounded_char_range_reserves_large_exact_ranges() {
        let mut chars = CandidateChars::with_capacity(0);

        push_bounded_char_range(&mut chars, 'a', 'z');

        assert_eq!(chars.len(), 26);
        assert!(chars.ordered.capacity() >= 26);
        assert!(chars
            .seen
            .as_ref()
            .is_some_and(|seen| seen.capacity() >= 26));
    }

    #[test]
    fn push_bounded_char_range_skips_large_ranges() {
        let mut chars = CandidateChars::with_capacity(0);

        push_bounded_char_range(&mut chars, '\u{0}', char::MAX);

        assert_eq!(chars.len(), 0);
    }

    #[test]
    fn normalize_alphabet_treats_size_hint_as_speculative() {
        let alphabet = normalize_alphabet(InflatedAlphabetHint::new(vec!['b', 'a', 'b']));

        assert_eq!(alphabet.as_ref(), &['b', 'a']);
    }
}
