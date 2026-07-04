use liblevenshtein::wallbreaker::WallBreakerResult;
use lling_llang::prelude::{StateId, TropicalWeight};
use rustc_hash::FxHashMap;

use crate::fx_hash_map_with_capacity;

pub(crate) const SUPER_START_RESULT_INDEX: u32 = u32::MAX;

/// Composite state key for WallBreaker.
///
/// States encode:
/// - `result_index`: Index into the results list
/// - `char_position`: Position within the result term
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct WallBreakerStateKey {
    /// Index into the results list.
    pub(crate) result_index: u32,
    /// Character position within the result term.
    pub(crate) char_position: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResultCharSpan {
    start: usize,
    len: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ResultCharArena {
    pub(crate) chars: Vec<char>,
    spans: Vec<ResultCharSpan>,
    pub(crate) state_count: usize,
    non_empty_terms: usize,
}

impl ResultCharArena {
    pub(crate) fn from_results(results: &[WallBreakerResult]) -> Self {
        let mut spans = Vec::with_capacity(results.len());
        let total_chars = results
            .iter()
            .map(|result| result.term.chars().count())
            .sum();
        let mut chars = Vec::with_capacity(total_chars);
        let mut state_count = 1usize;
        let mut non_empty_terms = 0usize;

        for result in results {
            let start = chars.len();
            chars.extend(result.term.chars());
            let len = chars.len() - start;
            state_count = state_count.saturating_add(len);
            non_empty_terms = non_empty_terms.saturating_add(usize::from(len > 0));
            spans.push(ResultCharSpan { start, len });
        }

        Self {
            chars,
            spans,
            state_count,
            non_empty_terms,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.spans.len()
    }

    pub(crate) fn term_len(&self, result_index: usize) -> Option<usize> {
        self.spans.get(result_index).map(|span| span.len)
    }

    pub(crate) fn first_char(&self, result_index: usize) -> Option<char> {
        self.char_at(result_index, 0)
    }

    pub(crate) fn char_at(&self, result_index: usize, char_position: usize) -> Option<char> {
        let span = self.spans.get(result_index)?;
        if char_position >= span.len {
            return None;
        }

        self.chars.get(span.start + char_position).copied()
    }

    pub(crate) fn state_count(&self) -> usize {
        self.state_count
    }

    pub(crate) fn start_transition_count(&self) -> usize {
        self.non_empty_terms
    }

    pub(crate) fn state_id_for_position(
        &self,
        result_index: usize,
        char_position: usize,
    ) -> Option<StateId> {
        next_wallbreaker_result_index(result_index)?;

        if char_position == 0 {
            return None;
        }

        let span = self.spans.get(result_index)?;
        if char_position > span.len {
            return None;
        }

        span.start
            .checked_add(char_position)
            .and_then(next_wallbreaker_state_id)
    }
}

pub(crate) fn normalize_wallbreaker_results<I>(
    results: I,
    max_distance: usize,
) -> Vec<WallBreakerResult>
where
    I: IntoIterator<Item = WallBreakerResult>,
{
    let results = results.into_iter();
    let reserve_hint = crate::capped_size_hint_capacity(results.size_hint().0);
    let mut normalized: Vec<WallBreakerResult> = Vec::with_capacity(reserve_hint);
    let mut term_to_index: FxHashMap<String, usize> = fx_hash_map_with_capacity(reserve_hint);

    for result in results {
        if result.distance > max_distance || distance_to_f64(result.distance).is_none() {
            continue;
        }

        if let Some(&existing_index) = term_to_index.get(&result.term) {
            if result.distance < normalized[existing_index].distance {
                normalized[existing_index].distance = result.distance;
            }
            continue;
        }

        term_to_index.insert(result.term.clone(), normalized.len());
        normalized.push(result);
    }

    normalized
}

pub(crate) fn wallbreaker_result_final_weights(
    results: &[WallBreakerResult],
) -> Vec<Option<TropicalWeight>> {
    results
        .iter()
        .map(|result| distance_to_f64(result.distance).map(TropicalWeight::new))
        .collect()
}

pub(crate) fn empty_wallbreaker_result_final_weight(
    results: &[WallBreakerResult],
    max_distance: usize,
) -> Option<TropicalWeight> {
    let mut best_weight = None;

    for result in results
        .iter()
        .filter(|result| result.term.is_empty() && result.distance <= max_distance)
    {
        if let Some(weight) = distance_to_f64(result.distance) {
            best_weight = Some(best_weight.map_or(weight, |best: f64| best.min(weight)));
        }
    }

    best_weight.map(TropicalWeight::new)
}

#[inline]
pub(crate) fn next_wallbreaker_state_id(len: usize) -> Option<StateId> {
    len.try_into().ok()
}

#[inline]
pub(crate) fn next_wallbreaker_result_index(index: usize) -> Option<u32> {
    let index = u32::try_from(index).ok()?;
    (index < SUPER_START_RESULT_INDEX).then_some(index)
}

#[inline]
pub(crate) fn next_wallbreaker_char_position(position: usize) -> Option<u32> {
    position.try_into().ok()
}

#[inline]
pub(crate) fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[inline]
pub(crate) fn usize_from_state_id(value: StateId) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[inline]
pub(crate) fn distance_to_f64(distance: usize) -> Option<f64> {
    crate::exact_usize_to_f64(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct InflatedResultHint {
        results: std::vec::IntoIter<WallBreakerResult>,
    }

    impl InflatedResultHint {
        fn new(results: Vec<WallBreakerResult>) -> Self {
            Self {
                results: results.into_iter(),
            }
        }
    }

    impl Iterator for InflatedResultHint {
        type Item = WallBreakerResult;

        fn next(&mut self) -> Option<Self::Item> {
            self.results.next()
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (usize::MAX, None)
        }
    }

    #[test]
    fn checked_index_helpers_reject_overflow() {
        let Ok(max_state_id) = usize::try_from(StateId::MAX) else {
            return;
        };
        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };

        assert_eq!(next_wallbreaker_state_id(max_state_id), Some(StateId::MAX));
        assert_eq!(next_wallbreaker_char_position(max_u32), Some(u32::MAX));
        assert_eq!(next_wallbreaker_result_index(max_u32), None);
        assert_eq!(
            next_wallbreaker_result_index(max_u32 - 1),
            Some(SUPER_START_RESULT_INDEX - 1)
        );

        if let Some(overflowing_len) = max_state_id.checked_add(1) {
            assert_eq!(next_wallbreaker_state_id(overflowing_len), None);
        }
        if let Some(overflowing_index) = max_u32.checked_add(1) {
            assert_eq!(next_wallbreaker_result_index(overflowing_index), None);
            assert_eq!(next_wallbreaker_char_position(overflowing_index), None);
        }
    }

    #[test]
    fn normalizes_results_to_bound_representable_unique_best_terms() {
        let mut raw_results = vec![
            WallBreakerResult::new("same".to_owned(), 3),
            WallBreakerResult::new("over-bound".to_owned(), 4),
            WallBreakerResult::new("same".to_owned(), 1),
            WallBreakerResult::new("other".to_owned(), 2),
        ];

        if let Some(unrepresentable_distance) = crate::max_exact_f64_usize().checked_add(1) {
            raw_results.push(WallBreakerResult::new(
                "unrepresentable".to_owned(),
                unrepresentable_distance,
            ));
        }

        let results = normalize_wallbreaker_results(raw_results, 3);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].term, "same");
        assert_eq!(results[0].distance, 1);
        assert_eq!(results[1].term, "other");
        assert_eq!(results[1].distance, 2);
    }

    #[test]
    fn normalizes_results_treats_size_hint_as_speculative() {
        let raw_results = InflatedResultHint::new(vec![
            WallBreakerResult::new("same".to_owned(), 2),
            WallBreakerResult::new("same".to_owned(), 1),
            WallBreakerResult::new("over-bound".to_owned(), 3),
        ]);

        let results = normalize_wallbreaker_results(raw_results, 2);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].term, "same");
        assert_eq!(results[0].distance, 1);
    }

    #[test]
    fn distance_to_f64_accepts_exact_integer_range() {
        assert_eq!(distance_to_f64(2), Some(2.0));

        let max_u32 = usize::try_from(u32::MAX).unwrap_or(usize::MAX);
        assert_eq!(distance_to_f64(max_u32), Some(f64::from(u32::MAX)));

        let max_exact = crate::max_exact_f64_usize();
        assert_eq!(distance_to_f64(max_exact), Some(max_exact as f64));

        if let Some(unrepresentable_distance) = max_exact.checked_add(1) {
            assert_eq!(distance_to_f64(unrepresentable_distance), None);
        }
    }

    #[test]
    fn precomputes_result_final_weights() {
        let results = vec![
            WallBreakerResult::new("a".to_owned(), 0),
            WallBreakerResult::new("b".to_owned(), 2),
        ];

        let weights = wallbreaker_result_final_weights(&results);

        assert_eq!(weights.len(), 2);
        assert_eq!(weights[0].map(TropicalWeight::value), Some(0.0));
        assert_eq!(weights[1].map(TropicalWeight::value), Some(2.0));
    }

    #[test]
    fn result_final_weights_reject_unrepresentable_distance() {
        let Some(unrepresentable_distance) = crate::max_exact_f64_usize().checked_add(1) else {
            return;
        };

        let results = vec![WallBreakerResult::new(
            "oversized".to_owned(),
            unrepresentable_distance,
        )];

        let weights = wallbreaker_result_final_weights(&results);

        assert_eq!(weights.len(), 1);
        assert!(weights[0].is_none());
    }

    #[test]
    fn empty_result_final_weight_respects_bound() {
        let results = vec![
            WallBreakerResult::new(String::new(), 3),
            WallBreakerResult::new(String::new(), 1),
            WallBreakerResult::new("x".to_owned(), 0),
        ];

        assert_eq!(
            empty_wallbreaker_result_final_weight(&results, 1)
                .expect("empty result within bound")
                .value(),
            1.0
        );
        assert!(empty_wallbreaker_result_final_weight(&results, 0).is_none());
    }

    #[test]
    fn empty_result_final_weight_rejects_unrepresentable_distance() {
        let Some(unrepresentable_distance) = crate::max_exact_f64_usize().checked_add(1) else {
            return;
        };

        let results = vec![WallBreakerResult::new(
            String::new(),
            unrepresentable_distance,
        )];

        assert!(
            empty_wallbreaker_result_final_weight(&results, unrepresentable_distance).is_none()
        );
    }

    #[test]
    fn result_char_arena_flattens_terms() {
        let results = vec![
            WallBreakerResult::new("é".to_owned(), 0),
            WallBreakerResult::new("ab".to_owned(), 1),
            WallBreakerResult::new(String::new(), 0),
        ];

        let arena = ResultCharArena::from_results(&results);

        assert_eq!(arena.len(), 3);
        assert_eq!(arena.chars, vec!['é', 'a', 'b']);
        assert_eq!(arena.term_len(0), Some(1));
        assert_eq!(arena.term_len(1), Some(2));
        assert_eq!(arena.term_len(2), Some(0));
        assert_eq!(arena.first_char(0), Some('é'));
        assert_eq!(arena.first_char(1), Some('a'));
        assert_eq!(arena.first_char(2), None);
        assert_eq!(arena.char_at(1, 1), Some('b'));
        assert_eq!(arena.state_count(), 4);
        assert_eq!(arena.start_transition_count(), 2);
        assert_eq!(arena.state_id_for_position(0, 1), Some(1));
        assert_eq!(arena.state_id_for_position(1, 1), Some(2));
        assert_eq!(arena.state_id_for_position(1, 2), Some(3));
        assert_eq!(arena.state_id_for_position(1, 0), None);
        assert_eq!(arena.state_id_for_position(2, 1), None);
    }
}
