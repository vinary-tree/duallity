//! Exact fzf V2 scoring and prefix-shared dictionary traversal.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;

use liblevenshtein::transducer::PrefixPruner;

use crate::fzf_support::{FzfColumn, FzfCore};
pub use crate::fzf_support::{FzfConfig, FzfError, FzfScheme};

/// Work counters for incremental fzf scoring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FzfStats {
    /// DP columns constructed. A trie-shared traversal constructs one per
    /// visited dictionary edge rather than one per character of every term.
    pub columns_computed: usize,
    /// Complete candidates scored.
    pub candidates_scored: usize,
    /// Total prefixes rejected by either resource or score bounds.
    pub prefixes_pruned: usize,
    /// Prefixes rejected because no completion can reach the kth exact score.
    pub score_bound_prefixes_pruned: usize,
    /// Prefixes rejected because they exceed the candidate-length limit.
    pub length_prefixes_pruned: usize,
    /// Capacity-sensitive score bounds evaluated after the length check.
    pub upper_bounds_computed: usize,
}

/// An exact `FuzzyMatchV2` score.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FzfMatch {
    /// Higher is better.
    pub score: i32,
}

/// Incremental fzf scorer and balanced liblevenshtein prefix visitor.
///
/// The scorer retains one DP column per active DFS depth. [`PrefixPruner::enter`]
/// pushes a column and [`PrefixPruner::leave`] pops exactly that column.
/// `top_k == 0` computes scores without maintaining an internal threshold.
#[derive(Clone, Debug)]
pub struct FzfScorer {
    core: Arc<FzfCore>,
    columns: Vec<FzfColumn>,
    top_scores: BinaryHeap<Reverse<i32>>,
    stats: FzfStats,
}

impl FzfScorer {
    /// Create a case-insensitive default-scheme scorer.
    pub fn new(query: &str) -> Result<Self, FzfError> {
        Self::with_config(query, FzfConfig::default())
    }

    /// Create a scorer with explicit matching and resource limits.
    pub fn with_config(query: &str, config: FzfConfig) -> Result<Self, FzfError> {
        let core = Arc::new(FzfCore::new(query, config)?);
        let columns = vec![core.initial_column()];
        Ok(Self {
            core,
            columns,
            top_scores: BinaryHeap::with_capacity(config.top_k),
            stats: FzfStats::default(),
        })
    }

    /// Score one candidate independently.
    pub fn score(&self, candidate: &str) -> Result<Option<FzfMatch>, FzfError> {
        Ok(self
            .core
            .score_chars(candidate.chars())?
            .map(|score| FzfMatch { score }))
    }

    /// Query characters for [`liblevenshtein::transducer::SubsequenceQueryIterator`].
    ///
    /// The visitor owns the comparison relation, so the returned lowercase
    /// units remain correct for both case-sensitive and insensitive modes.
    pub fn query_units(&self) -> Vec<char> {
        self.core.query().to_vec()
    }

    /// Current kth score, once `top_k` exact candidates have been seen.
    pub fn cutoff(&self) -> Option<i32> {
        let top_k = self.core.config().top_k;
        (top_k > 0 && self.top_scores.len() == top_k).then(|| {
            self.top_scores
                .peek()
                .expect("a full top-k heap is nonempty")
                .0
        })
    }

    /// Sound upper bound for every accepted descendant of the current DFS prefix.
    ///
    /// `None` means that the configured candidate budget cannot contain a
    /// completion from any live local-alignment state.
    pub fn current_upper_bound(&self) -> Option<i32> {
        self.core.upper_bound(
            self.columns
                .last()
                .expect("the root fzf column is never removed"),
        )
    }

    /// Maximum score any candidate can attain for this query and scheme.
    pub fn maximum_score(&self) -> i32 {
        self.core.maximum_score()
    }

    /// Snapshot work counters.
    pub fn stats(&self) -> FzfStats {
        self.stats
    }

    /// Clear accumulated top-k scores and counters while retaining the query.
    pub fn reset_observations(&mut self) {
        self.top_scores.clear();
        self.stats = FzfStats::default();
    }

    fn observe_score(&mut self, score: i32) {
        let top_k = self.core.config().top_k;
        if top_k == 0 {
            return;
        }
        if self.top_scores.len() < top_k {
            self.top_scores.push(Reverse(score));
            return;
        }
        let threshold = self
            .top_scores
            .peek()
            .expect("a full top-k heap is nonempty")
            .0;
        if score > threshold {
            let _ = self.top_scores.pop();
            self.top_scores.push(Reverse(score));
        }
    }
}

impl PrefixPruner<char> for FzfScorer {
    fn matches_query_unit(&self, candidate: char, query: char) -> bool {
        self.core.matches_query_unit(candidate, query)
    }

    fn enter(&mut self, unit: char, depth: usize) -> bool {
        debug_assert_eq!(depth, self.columns.len());
        let next = self.core.advance(
            self.columns
                .last()
                .expect("the root fzf column is never removed"),
            unit,
        );
        self.columns.push(next);
        self.stats.columns_computed = self.stats.columns_computed.saturating_add(1);

        if depth > self.core.config().max_candidate_chars {
            self.stats.length_prefixes_pruned = self.stats.length_prefixes_pruned.saturating_add(1);
            self.stats.prefixes_pruned = self.stats.prefixes_pruned.saturating_add(1);
            return false;
        }

        self.stats.upper_bounds_computed = self.stats.upper_bounds_computed.saturating_add(1);
        let bound = self.current_upper_bound();
        let score_can_survive = match (bound, self.cutoff()) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(upper), Some(threshold)) => upper >= threshold,
        };
        if !score_can_survive {
            self.stats.score_bound_prefixes_pruned =
                self.stats.score_bound_prefixes_pruned.saturating_add(1);
            self.stats.prefixes_pruned = self.stats.prefixes_pruned.saturating_add(1);
        }
        score_can_survive
    }

    fn leave(&mut self, _unit: char, depth: usize) {
        debug_assert_eq!(depth.saturating_add(1), self.columns.len());
        let _ = self.columns.pop();
        debug_assert!(!self.columns.is_empty());
    }

    fn permits_accept(&mut self, _prefix: &[char]) -> bool {
        self.columns
            .last()
            .expect("the root fzf column is never removed")
            .best_full_score()
            .is_some()
    }

    fn accept(&mut self, _prefix: &[char]) -> Option<f64> {
        let score = self
            .columns
            .last()
            .expect("the root fzf column is never removed")
            .best_full_score()?;
        self.stats.candidates_scored = self.stats.candidates_scored.saturating_add(1);
        self.observe_score(score);
        Some(f64::from(score))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use libdictenstein::Dictionary;
    use liblevenshtein::transducer::SubsequenceQueryIterator;
    use proptest::prelude::*;

    #[test]
    fn trie_scoring_matches_independent_scoring() {
        let terms = ["FooBarBaz", "foo/bar/baz", "far", "src/fzf_scorer.rs"];
        let dictionary = DynamicDawgChar::<()>::from_terms(terms);
        let scorer = FzfScorer::new("fbb").expect("short query is valid");
        let query_units = scorer.query_units();
        let mut walker =
            SubsequenceQueryIterator::with_pruner(dictionary.root(), query_units, scorer.clone());
        let mut trie_scores: Vec<_> = walker
            .by_ref()
            .map(|item| {
                (
                    item.units.iter().collect::<String>(),
                    item.score.expect("fzf accepts every structural match") as i32,
                )
            })
            .collect();
        trie_scores.sort();

        let mut flat_scores: Vec<_> = terms
            .iter()
            .filter_map(|term| {
                scorer
                    .score(term)
                    .expect("fixture term is bounded")
                    .map(|matched| ((*term).to_owned(), matched.score))
            })
            .collect();
        flat_scores.sort();
        assert_eq!(trie_scores, flat_scores);
    }

    fn top_k(mut entries: Vec<(String, i32)>, k: usize) -> Vec<(String, i32)> {
        entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        entries.truncate(k);
        entries
    }

    proptest! {
        #[test]
        fn trie_top_k_equals_brute_force(
            query in "[a-c]{0,4}",
            terms in prop::collection::btree_set("[a-c/_A-C]{0,10}", 0..30),
            k in 1usize..8,
        ) {
            let terms: Vec<_> = terms.into_iter().collect();
            let maximum_candidate_chars = terms
                .iter()
                .map(|term| term.chars().count())
                .max()
                .unwrap_or(0);
            let dictionary = DynamicDawgChar::<()>::from_terms(terms.iter().map(String::as_str));
            let scorer = FzfScorer::with_config(
                &query,
                FzfConfig {
                    top_k: k,
                    max_candidate_chars: maximum_candidate_chars,
                    ..FzfConfig::default()
                },
            ).expect("generated query is bounded");
            let query_units = scorer.query_units();
            let trie = SubsequenceQueryIterator::with_pruner(
                dictionary.root(),
                query_units,
                scorer.clone(),
            )
            .map(|item| (
                item.units.iter().collect::<String>(),
                item.score.expect("structural and score acceptance agree") as i32,
            ))
            .collect();

            let flat = terms.iter().filter_map(|term| {
                scorer.score(term).expect("generated candidate is bounded")
                    .map(|matched| (term.clone(), matched.score))
            }).collect();
            prop_assert_eq!(top_k(trie, k), top_k(flat, k));
        }
    }

    #[test]
    fn score_bound_pruning_is_observably_non_vacuous() {
        let terms = ["abc", "azzzzz", "azzzyz", "azzyzz"];
        let dictionary = DynamicDawgChar::<()>::from_terms(terms);
        let config = FzfConfig {
            top_k: 1,
            max_candidate_chars: 6,
            ..FzfConfig::default()
        };
        let scorer = FzfScorer::with_config("abc", config).expect("fixture query is valid");
        let mut walker =
            SubsequenceQueryIterator::with_pruner(dictionary.root(), scorer.query_units(), scorer);
        let results: Vec<_> = walker.by_ref().collect();
        let stats = walker.pruner().stats();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].units.iter().collect::<String>(), "abc");
        assert!(stats.score_bound_prefixes_pruned > 0, "stats={stats:?}");
        assert_eq!(stats.length_prefixes_pruned, 0, "stats={stats:?}");
        assert_eq!(
            stats.prefixes_pruned,
            stats.score_bound_prefixes_pruned + stats.length_prefixes_pruned,
        );
    }
}
