//! Score-for-score differential tests against an independent batch port of
//! junegunn/fzf's `FuzzyMatchV2` recurrence.
//!
//! The oracle deliberately does not use duallity's incremental DP state.  It
//! is a test-only, full-candidate implementation whose constants and fixtures
//! are traceable to `src/algo/algo.go` and `src/algo/algo_test.go` in fzf.

use duallity::{FzfConfig, FzfScheme, FzfScorer};

const MATCH: i32 = 16;
const GAP_START: i32 = -3;
const GAP_EXTENSION: i32 = -1;
const BOUNDARY: i32 = MATCH / 2;
const NON_WORD: i32 = MATCH / 2;
const CAMEL_123: i32 = BOUNDARY + GAP_EXTENSION;
const CONSECUTIVE: i32 = -(GAP_START + GAP_EXTENSION);
const FIRST_MULTIPLIER: i32 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    White,
    NonWord,
    Delimiter,
    Lower,
    Upper,
    Letter,
    Number,
}

#[derive(Clone, Copy, Default)]
struct Cell {
    score: i32,
    consecutive: usize,
    first_bonus: i32,
    gap: bool,
    live: bool,
}

fn comparable(character: char, sensitive: bool) -> char {
    if sensitive {
        character
    } else if character.is_ascii() {
        character.to_ascii_lowercase()
    } else {
        character.to_lowercase().next().unwrap_or(character)
    }
}

fn classify(character: char, scheme: FzfScheme) -> Class {
    if character.is_lowercase() {
        Class::Lower
    } else if character.is_uppercase() {
        Class::Upper
    } else if character.is_numeric() {
        Class::Number
    } else if character.is_alphabetic() {
        Class::Letter
    } else if character.is_whitespace() || matches!(character, '\u{85}' | '\u{a0}') {
        Class::White
    } else if match scheme {
        FzfScheme::Path => character == '/' || character == std::path::MAIN_SEPARATOR,
        FzfScheme::Default | FzfScheme::History => "/,:;|".contains(character),
    } {
        Class::Delimiter
    } else {
        Class::NonWord
    }
}

fn bonus(previous: Class, current: Class, scheme: FzfScheme) -> i32 {
    if current != Class::White {
        match previous {
            Class::White => {
                return match scheme {
                    FzfScheme::Default => BOUNDARY + 2,
                    FzfScheme::Path | FzfScheme::History => BOUNDARY,
                };
            }
            Class::Delimiter => {
                return match scheme {
                    FzfScheme::Default | FzfScheme::Path => BOUNDARY + 1,
                    FzfScheme::History => BOUNDARY,
                };
            }
            Class::NonWord => return BOUNDARY,
            Class::Lower | Class::Upper | Class::Letter | Class::Number => {}
        }
    }
    if (previous == Class::Lower && current == Class::Upper)
        || (previous != Class::Number && current == Class::Number)
    {
        return CAMEL_123;
    }
    match current {
        Class::NonWord | Class::Delimiter => NON_WORD,
        Class::White => match scheme {
            FzfScheme::Default => BOUNDARY + 2,
            FzfScheme::Path | FzfScheme::History => BOUNDARY,
        },
        Class::Lower | Class::Upper | Class::Letter | Class::Number => 0,
    }
}

/// Independent batch oracle. It owns only two rows, never duallity columns.
fn upstream_reference(candidate: &str, query: &str, config: FzfConfig) -> Option<i32> {
    let query: Vec<_> = query
        .chars()
        .map(|character| comparable(character, config.case_sensitive))
        .collect();
    if query.is_empty() {
        return Some(0);
    }

    let mut previous = vec![Cell::default(); query.len()];
    let mut previous_class = match config.scheme {
        FzfScheme::Path => Class::Delimiter,
        FzfScheme::Default | FzfScheme::History => Class::White,
    };
    let mut best = None;

    for raw in candidate.chars() {
        let class = classify(raw, config.scheme);
        let position_bonus = bonus(previous_class, class, config.scheme);
        let character = comparable(raw, config.case_sensitive);
        let mut current = vec![Cell::default(); query.len()];

        current[0] = if character == query[0] {
            Cell {
                score: MATCH + position_bonus * FIRST_MULTIPLIER,
                consecutive: 1,
                first_bonus: position_bonus,
                gap: false,
                live: true,
            }
        } else if previous[0].live {
            Cell {
                score: (previous[0].score
                    + if previous[0].gap {
                        GAP_EXTENSION
                    } else {
                        GAP_START
                    })
                .max(0),
                consecutive: 0,
                first_bonus: 0,
                gap: true,
                live: true,
            }
        } else {
            Cell::default()
        };

        for index in 1..query.len() {
            let left = previous[index];
            let diagonal = previous[index - 1];
            let gap = left
                .live
                .then(|| left.score + if left.gap { GAP_EXTENSION } else { GAP_START });

            let mut consecutive = 0;
            let mut first_bonus = 0;
            let matched = character == query[index] && diagonal.live;
            let matched_score = matched.then(|| {
                let mut applied = position_bonus;
                consecutive = diagonal.consecutive + 1;
                first_bonus = if diagonal.consecutive == 0 {
                    position_bonus
                } else {
                    diagonal.first_bonus
                };
                if consecutive > 1 {
                    if position_bonus >= BOUNDARY && position_bonus > diagonal.first_bonus {
                        consecutive = 1;
                        first_bonus = position_bonus;
                    } else {
                        applied = applied.max(CONSECUTIVE).max(diagonal.first_bonus);
                    }
                }
                diagonal.score + MATCH + applied
            });

            if gap.is_none() && matched_score.is_none() {
                continue;
            }
            let gap_score = gap.unwrap_or(i32::MIN / 4);
            let match_score = matched_score.unwrap_or(i32::MIN / 4);
            if match_score < gap_score {
                consecutive = 0;
                first_bonus = 0;
            }
            current[index] = Cell {
                score: match_score.max(gap_score).max(0),
                consecutive,
                first_bonus,
                gap: match_score < gap_score,
                live: true,
            };
        }

        if current[query.len() - 1].live {
            let score = current[query.len() - 1].score;
            best = Some(best.map_or(score, |prior: i32| prior.max(score)));
        }
        previous = current;
        previous_class = class;
    }
    best
}

fn production(candidate: &str, query: &str, config: FzfConfig) -> Option<i32> {
    FzfScorer::with_config(query, config)
        .expect("fixture query is bounded")
        .score(candidate)
        .expect("fixture candidate is bounded")
        .map(|matched| matched.score)
}

#[test]
fn upstream_published_score_fixtures_match_both_engines() {
    let fixtures = [
        ("fooBarbaz1", "oBZ", false, 49),
        ("foo bar baz", "fbb", false, 78),
        ("/AutomatorDocument.icns", "rdoc", false, 79),
        ("/man1/zshcompctl.1", "zshc", false, 109),
        ("/.oh-my-zsh/cache", "zshc", false, 102),
        (".vimrc", ".vimrc", false, 166),
        ("/.vimrc", ".vimrc", false, 159),
        ("a.vimrc", ".vimrc", false, 152),
        ("ab0123 456", "12356", false, 88),
        ("abc123 456", "12356", false, 108),
        ("foo/bar/baz", "fbb", false, 76),
        ("fooBarBaz", "fbb", false, 74),
        ("fooBarbaz", "oBz", true, 49),
        ("Foo/Bar/Baz", "FBB", true, 76),
        ("foo-bar", "o-ba", true, 88),
    ];
    for (candidate, query, case_sensitive, expected) in fixtures {
        let config = FzfConfig {
            case_sensitive,
            max_candidate_chars: candidate.chars().count(),
            ..FzfConfig::default()
        };
        assert_eq!(upstream_reference(candidate, query, config), Some(expected));
        assert_eq!(production(candidate, query, config), Some(expected));
    }
}

#[test]
fn real_repository_path_corpus_is_score_for_score_equal() {
    let paths = include_str!("fixtures/fzf_real_paths.txt");
    let queries = [
        "fzf", "wfst", "formal", "doc", "test", "src", "weight", "dyck",
    ];
    for candidate in paths.lines().filter(|line| !line.is_empty()) {
        for query in queries {
            let config = FzfConfig {
                scheme: FzfScheme::Path,
                max_candidate_chars: candidate.chars().count(),
                ..FzfConfig::default()
            };
            assert_eq!(
                production(candidate, query, config),
                upstream_reference(candidate, query, config),
                "candidate={candidate:?}, query={query:?}",
            );
        }
    }
}
