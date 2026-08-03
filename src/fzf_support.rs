//! Exact incremental state for fzf's `FuzzyMatchV2` score recurrence.
//!
//! The constants and recurrence follow fzf's upstream `src/algo/algo.go`.
//! Keeping the dynamic-programming state here lets both [`crate::FzfScorer`]
//! and [`crate::FzfStateSource`] use the same implementation.

use std::fmt;

pub(crate) const SCORE_MATCH: i32 = 16;
pub(crate) const SCORE_GAP_START: i32 = -3;
pub(crate) const SCORE_GAP_EXTENSION: i32 = -1;
pub(crate) const BONUS_BOUNDARY: i32 = SCORE_MATCH / 2;
pub(crate) const BONUS_NON_WORD: i32 = SCORE_MATCH / 2;
pub(crate) const BONUS_CAMEL_123: i32 = BONUS_BOUNDARY + SCORE_GAP_EXTENSION;
pub(crate) const BONUS_CONSECUTIVE: i32 = -(SCORE_GAP_START + SCORE_GAP_EXTENSION);
pub(crate) const BONUS_FIRST_CHAR_MULTIPLIER: i32 = 2;

/// Upstream fzf scoring scheme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FzfScheme {
    /// General text: whitespace has the strongest boundary bonus.
    #[default]
    Default,
    /// File paths: slash is the only delimiter and the initial class is a
    /// delimiter.
    Path,
    /// Shell history: whitespace and delimiters receive the same bonus.
    History,
}

/// Resource and matching configuration for [`crate::FzfScorer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FzfConfig {
    /// Compare Unicode scalar values without case folding.
    pub case_sensitive: bool,
    /// Upstream bonus-table scheme.
    pub scheme: FzfScheme,
    /// Number of exact scores retained before branch-and-bound can activate.
    /// Zero disables internal top-k threshold tracking.
    pub top_k: usize,
    /// Maximum accepted query length in Unicode scalar values.
    pub max_query_chars: usize,
    /// Maximum accepted candidate length in Unicode scalar values.
    pub max_candidate_chars: usize,
}

impl Default for FzfConfig {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            scheme: FzfScheme::Default,
            top_k: 0,
            max_query_chars: 1_000,
            max_candidate_chars: 1_000_000,
        }
    }
}

/// Invalid or resource-exhausting scorer input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FzfError {
    /// The configured query exceeds the caller-selected work limit.
    QueryTooLong { actual: usize, maximum: usize },
    /// The candidate exceeds the caller-selected work limit.
    CandidateTooLong { actual: usize, maximum: usize },
}

impl fmt::Display for FzfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueryTooLong { actual, maximum } => write!(
                formatter,
                "fzf query has {actual} characters, exceeding the configured maximum {maximum}"
            ),
            Self::CandidateTooLong { actual, maximum } => write!(
                formatter,
                "fzf candidate has {actual} characters, exceeding the configured maximum {maximum}"
            ),
        }
    }
}

impl std::error::Error for FzfError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharClass {
    White,
    NonWord,
    Delimiter,
    Lower,
    Upper,
    Letter,
    Number,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Cell {
    score: i32,
    consecutive: usize,
    first_bonus: i32,
    in_gap: bool,
    reachable: bool,
}

/// One immutable DP column for a candidate prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FzfColumn {
    cells: Vec<Cell>,
    previous_class: CharClass,
    depth: usize,
    best_full_score: Option<i32>,
}

impl FzfColumn {
    #[inline]
    pub(crate) fn best_full_score(&self) -> Option<i32> {
        self.best_full_score
    }
}

/// Immutable query and bonus table shared by scorers and WFST states.
#[derive(Clone, Debug)]
pub(crate) struct FzfCore {
    query: Vec<char>,
    config: FzfConfig,
    max_bonus: i32,
}

impl FzfCore {
    pub(crate) fn new(query: &str, config: FzfConfig) -> Result<Self, FzfError> {
        let query: Vec<char> = query
            .chars()
            .map(|character| comparable_char(character, config.case_sensitive))
            .collect();
        if query.len() > config.max_query_chars {
            return Err(FzfError::QueryTooLong {
                actual: query.len(),
                maximum: config.max_query_chars,
            });
        }

        let max_bonus = match config.scheme {
            FzfScheme::Default => BONUS_BOUNDARY + 2,
            FzfScheme::Path => BONUS_BOUNDARY + 1,
            FzfScheme::History => BONUS_BOUNDARY,
        };
        Ok(Self {
            query,
            config,
            max_bonus,
        })
    }

    #[inline]
    pub(crate) fn config(&self) -> FzfConfig {
        self.config
    }

    #[inline]
    pub(crate) fn query(&self) -> &[char] {
        &self.query
    }

    pub(crate) fn initial_column(&self) -> FzfColumn {
        FzfColumn {
            cells: vec![Cell::default(); self.query.len()],
            previous_class: self.initial_class(),
            depth: 0,
            best_full_score: self.query.is_empty().then_some(0),
        }
    }

    pub(crate) fn advance(&self, previous: &FzfColumn, candidate: char) -> FzfColumn {
        debug_assert_eq!(previous.cells.len(), self.query.len());
        let class = self.classify(candidate);
        let position_bonus = self.bonus_for(previous.previous_class, class);
        let candidate = comparable_char(candidate, self.config.case_sensitive);
        let mut cells = Vec::with_capacity(self.query.len());

        if let Some(&first_query) = self.query.first() {
            let previous_first = previous.cells[0];
            let first = if candidate == first_query {
                Cell {
                    score: SCORE_MATCH
                        .saturating_add(position_bonus.saturating_mul(BONUS_FIRST_CHAR_MULTIPLIER)),
                    consecutive: 1,
                    first_bonus: position_bonus,
                    in_gap: false,
                    reachable: true,
                }
            } else if previous_first.reachable {
                Cell {
                    score: previous_first
                        .score
                        .saturating_add(if previous_first.in_gap {
                            SCORE_GAP_EXTENSION
                        } else {
                            SCORE_GAP_START
                        })
                        .max(0),
                    consecutive: 0,
                    first_bonus: 0,
                    in_gap: true,
                    reachable: true,
                }
            } else {
                Cell::default()
            };
            cells.push(first);

            for query_index in 1..self.query.len() {
                let left = previous.cells[query_index];
                let diagonal = previous.cells[query_index - 1];
                let gap_score = left.reachable.then(|| {
                    left.score.saturating_add(if left.in_gap {
                        SCORE_GAP_EXTENSION
                    } else {
                        SCORE_GAP_START
                    })
                });

                let matched = candidate == self.query[query_index] && diagonal.reachable;
                let mut consecutive = 0;
                let mut first_bonus = 0;
                let match_score = matched.then(|| {
                    let mut applied_bonus = position_bonus;
                    consecutive = diagonal.consecutive.saturating_add(1);
                    first_bonus = if diagonal.consecutive == 0 {
                        position_bonus
                    } else {
                        diagonal.first_bonus
                    };

                    if consecutive > 1 {
                        if position_bonus >= BONUS_BOUNDARY && position_bonus > diagonal.first_bonus
                        {
                            consecutive = 1;
                            first_bonus = position_bonus;
                        } else {
                            applied_bonus = applied_bonus
                                .max(BONUS_CONSECUTIVE)
                                .max(diagonal.first_bonus);
                        }
                    }
                    diagonal
                        .score
                        .saturating_add(SCORE_MATCH)
                        .saturating_add(applied_bonus)
                });

                let reachable = gap_score.is_some() || match_score.is_some();
                if !reachable {
                    cells.push(Cell::default());
                    continue;
                }

                let match_value = match_score.unwrap_or(i32::MIN / 4);
                let gap_value = gap_score.unwrap_or(i32::MIN / 4);
                let match_wins = match_value >= gap_value;
                if !match_wins {
                    consecutive = 0;
                    first_bonus = 0;
                }
                cells.push(Cell {
                    score: match_value.max(gap_value).max(0),
                    consecutive,
                    first_bonus,
                    in_gap: match_value < gap_value,
                    reachable,
                });
            }
        }

        let current_full_score = cells
            .last()
            .filter(|cell| cell.reachable)
            .map(|cell| cell.score);
        let best_full_score = match (previous.best_full_score, current_full_score) {
            (Some(previous), Some(current)) => Some(previous.max(current)),
            (previous, current) => previous.or(current),
        };

        FzfColumn {
            cells,
            previous_class: class,
            depth: previous.depth.saturating_add(1),
            best_full_score,
        }
    }

    pub(crate) fn score_chars<I>(&self, candidate: I) -> Result<Option<i32>, FzfError>
    where
        I: IntoIterator<Item = char>,
    {
        let mut column = self.initial_column();
        for character in candidate {
            if column.depth >= self.config.max_candidate_chars {
                return Err(FzfError::CandidateTooLong {
                    actual: column.depth.saturating_add(1),
                    maximum: self.config.max_candidate_chars,
                });
            }
            column = self.advance(&column, character);
        }
        Ok(column.best_full_score)
    }

    /// A capacity-sensitive upper bound for every accepted descendant.
    ///
    /// Each term corresponds to one state of fzf's local-alignment recurrence:
    /// a match already completed in this prefix, a reachable query cell that
    /// still has enough candidate characters to finish, or an alignment that
    /// has not started yet. The latter is included only while the remaining
    /// candidate budget can still contain the complete query. `None` means
    /// that no completion fits inside [`FzfConfig::max_candidate_chars`].
    pub(crate) fn upper_bound(&self, column: &FzfColumn) -> Option<i32> {
        let available = self.config.max_candidate_chars.saturating_sub(column.depth);
        let per_match = SCORE_MATCH.saturating_add(self.max_bonus);
        let mut bound = column.best_full_score;

        for (index, cell) in column
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.reachable)
        {
            let remaining = self.query.len().saturating_sub(index.saturating_add(1));
            if remaining <= available {
                let optimistic = cell.score.saturating_add(
                    i32::try_from(remaining)
                        .unwrap_or(i32::MAX)
                        .saturating_mul(per_match),
                );
                bound = Some(bound.map_or(optimistic, |current| current.max(optimistic)));
            }
        }

        if self.query.len() <= available {
            let unstarted = self.maximum_score();
            bound = Some(bound.map_or(unstarted, |current| current.max(unstarted)));
        }
        bound
    }

    pub(crate) fn maximum_score(&self) -> i32 {
        let Some(remaining) = self.query.len().checked_sub(1) else {
            return 0;
        };
        let remaining = i32::try_from(remaining).unwrap_or(i32::MAX);
        SCORE_MATCH
            .saturating_add(self.max_bonus.saturating_mul(BONUS_FIRST_CHAR_MULTIPLIER))
            .saturating_add(remaining.saturating_mul(SCORE_MATCH.saturating_add(self.max_bonus)))
    }

    #[inline]
    pub(crate) fn matches_query_unit(&self, candidate: char, query: char) -> bool {
        comparable_char(candidate, self.config.case_sensitive)
            == comparable_char(query, self.config.case_sensitive)
    }

    fn initial_class(&self) -> CharClass {
        match self.config.scheme {
            FzfScheme::Path => CharClass::Delimiter,
            FzfScheme::Default | FzfScheme::History => CharClass::White,
        }
    }

    fn classify(&self, character: char) -> CharClass {
        if character.is_lowercase() {
            CharClass::Lower
        } else if character.is_uppercase() {
            CharClass::Upper
        } else if character.is_numeric() {
            CharClass::Number
        } else if character.is_alphabetic() {
            CharClass::Letter
        } else if character.is_whitespace() || matches!(character, '\u{85}' | '\u{a0}') {
            CharClass::White
        } else if self.is_delimiter(character) {
            CharClass::Delimiter
        } else {
            CharClass::NonWord
        }
    }

    fn is_delimiter(&self, character: char) -> bool {
        match self.config.scheme {
            FzfScheme::Path => character == '/' || character == std::path::MAIN_SEPARATOR,
            FzfScheme::Default | FzfScheme::History => "/,:;|".contains(character),
        }
    }

    fn bonus_for(&self, previous: CharClass, current: CharClass) -> i32 {
        if current != CharClass::White {
            match previous {
                CharClass::White => {
                    return match self.config.scheme {
                        FzfScheme::Default => BONUS_BOUNDARY + 2,
                        FzfScheme::Path | FzfScheme::History => BONUS_BOUNDARY,
                    };
                }
                CharClass::Delimiter => {
                    return match self.config.scheme {
                        FzfScheme::Default | FzfScheme::Path => BONUS_BOUNDARY + 1,
                        FzfScheme::History => BONUS_BOUNDARY,
                    };
                }
                CharClass::NonWord => return BONUS_BOUNDARY,
                CharClass::Lower | CharClass::Upper | CharClass::Letter | CharClass::Number => {}
            }
        }

        if (previous == CharClass::Lower && current == CharClass::Upper)
            || (previous != CharClass::Number && current == CharClass::Number)
        {
            return BONUS_CAMEL_123;
        }

        match current {
            CharClass::NonWord | CharClass::Delimiter => BONUS_NON_WORD,
            CharClass::White => match self.config.scheme {
                FzfScheme::Default => BONUS_BOUNDARY + 2,
                FzfScheme::Path | FzfScheme::History => BONUS_BOUNDARY,
            },
            CharClass::Lower | CharClass::Upper | CharClass::Letter | CharClass::Number => 0,
        }
    }
}

#[inline]
fn comparable_char(character: char, case_sensitive: bool) -> char {
    if case_sensitive {
        character
    } else if character.is_ascii() {
        character.to_ascii_lowercase()
    } else {
        character.to_lowercase().next().unwrap_or(character)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn score(input: &str, pattern: &str, case_sensitive: bool) -> Option<i32> {
        FzfCore::new(
            pattern,
            FzfConfig {
                case_sensitive,
                ..FzfConfig::default()
            },
        )
        .expect("official fzf fixture query is valid")
        .score_chars(input.chars())
        .expect("official fzf fixture candidate is valid")
    }

    #[test]
    fn official_fuzzy_match_v2_scores_agree() {
        // Shared directly with junegunn/fzf src/algo/algo_test.go.
        let cases = [
            ("fooBarbaz1", "oBZ", false, 48 + 7 - 3 - 3),
            ("foo bar baz", "fbb", false, 48 + 20 + 20 - 6 - 4),
            ("/AutomatorDocument.icns", "rdoc", false, 64 + 7 + 8),
            ("/man1/zshcompctl.1", "zshc", false, 64 + 18 + 27),
            ("/.oh-my-zsh/cache", "zshc", false, 64 + 16 + 16 - 3 + 9),
            (".vimrc", ".vimrc", false, 96 + 70),
            ("/.vimrc", ".vimrc", false, 96 + 63),
            ("a.vimrc", ".vimrc", false, 96 + 56),
            ("ab0123 456", "12356", false, 80 + 12 - 3 - 1),
            ("abc123 456", "12356", false, 80 + 14 + 14 + 4 - 3 - 1),
            ("foo/bar/baz", "fbb", false, 48 + 20 + 18 - 6 - 4),
            ("fooBarBaz", "fbb", false, 48 + 20 + 14 - 6 - 2),
            ("fooBarbaz", "oBz", true, 48 + 7 - 3 - 3),
            ("Foo/Bar/Baz", "FBB", true, 48 + 20 + 18 - 6 - 4),
            ("foo-bar", "o-ba", true, 64 + 24),
        ];
        for (candidate, query, case_sensitive, expected) in cases {
            assert_eq!(
                score(candidate, query, case_sensitive),
                Some(expected),
                "candidate={candidate:?}, query={query:?}"
            );
        }
    }

    #[test]
    fn non_subsequence_and_empty_query_boundaries() {
        assert_eq!(score("fooBarbaz", "oBZ", true), None);
        assert_eq!(score("foo", "", true), Some(0));
    }

    #[test]
    fn unstarted_alternative_prevents_unsound_prefix_pruning() {
        let candidate = "xxxxxxxx/foo/bar";
        let core = FzfCore::new(
            "fb",
            FzfConfig {
                max_candidate_chars: candidate.chars().count(),
                ..FzfConfig::default()
            },
        )
        .expect("short query is valid");
        let mut column = core.initial_column();
        for character in "xxxxxxxx/".chars() {
            column = core.advance(&column, character);
        }
        let descendant_score = core
            .score_chars(candidate.chars())
            .expect("candidate is bounded")
            .expect("descendant contains the query");
        assert!(core
            .upper_bound(&column)
            .is_some_and(|bound| bound >= descendant_score));
    }

    #[test]
    fn exhausted_capacity_has_no_completion_bound() {
        let core = FzfCore::new(
            "abc",
            FzfConfig {
                max_candidate_chars: 2,
                ..FzfConfig::default()
            },
        )
        .expect("short query is valid");
        let column = core.advance(&core.initial_column(), 'x');
        assert_eq!(core.upper_bound(&column), None);
    }

    proptest! {
        /// Executable counterpart of the branch-and-bound soundness theorem:
        /// every completed descendant score is below the bound computed at
        /// the chosen trie prefix.
        #[test]
        fn prefix_upper_bound_dominates_every_generated_descendant(
            query in "[A-Za-z0-9/_ -]{0,7}",
            prefix in "[A-Za-z0-9/_ -]{0,12}",
            suffix in "[A-Za-z0-9/_ -]{0,12}",
            case_sensitive in any::<bool>(),
        ) {
            let capacity = prefix.chars().count().saturating_add(suffix.chars().count());
            let core = FzfCore::new(
                &query,
                FzfConfig {
                    case_sensitive,
                    max_candidate_chars: capacity,
                    ..FzfConfig::default()
                },
            ).expect("generated query is below the configured limit");
            let mut prefix_column = core.initial_column();
            for character in prefix.chars() {
                prefix_column = core.advance(&prefix_column, character);
            }
            let prefix_bound = core.upper_bound(&prefix_column);
            let candidate = format!("{prefix}{suffix}");
            if let Some(score) = core
                .score_chars(candidate.chars())
                .expect("generated candidate is below the configured limit")
            {
                prop_assert!(
                    prefix_bound.is_some_and(|bound| score <= bound),
                    "query={query:?}, prefix={prefix:?}, suffix={suffix:?}, score={score}, bound={prefix_bound:?}",
                );
            }

            if let Some(first) = suffix.chars().next() {
                let child = core.advance(&prefix_column, first);
                if let (Some(parent_bound), Some(child_bound)) =
                    (prefix_bound, core.upper_bound(&child))
                {
                    prop_assert!(child_bound <= parent_bound,
                        "query={query:?}, prefix={prefix:?}, next={first:?}, parent={parent_bound}, child={child_bound}");
                }
            }
        }
    }
}
