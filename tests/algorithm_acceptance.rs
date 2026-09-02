use std::collections::HashSet;

use duallity::{
    GeneralizedWfstBuilder, LazyWfst, LevenshteinWfst, TropicalWeight, UniversalLevenshteinWfst,
    Wfst,
};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::transducer::universal::Standard;
use liblevenshtein::transducer::Algorithm;

fn accepts_pair<W>(wfst: &mut W, input: &str, output: &str, max_cost: f64) -> bool
where
    W: LazyWfst<char, TropicalWeight> + Wfst<char, TropicalWeight>,
{
    let input: Vec<_> = input.chars().collect();
    let output: Vec<_> = output.chars().collect();
    let mut stack = vec![(Wfst::start(wfst), 0usize, 0usize, 0.0f64)];
    let mut seen = HashSet::new();

    while let Some((state, input_pos, output_pos, cost)) = stack.pop() {
        if !seen.insert((state, input_pos, output_pos, cost.to_bits())) {
            continue;
        }

        if seen.len() > 50_000 {
            return false;
        }

        wfst.expand(state).expect("reachable state expands");

        if input_pos == input.len()
            && output_pos == output.len()
            && Wfst::is_final(wfst, state)
            && cost + Wfst::final_weight(wfst, state).value() <= max_cost + f64::EPSILON
        {
            return true;
        }

        for transition in wfst.transitions_lazy(state) {
            let Some(next_input_pos) = advance(transition.input, &input, input_pos) else {
                continue;
            };
            let Some(next_output_pos) = advance(transition.output, &output, output_pos) else {
                continue;
            };

            let next_cost = cost + transition.weight.value();
            if next_cost.is_finite() && next_cost <= max_cost + f64::EPSILON {
                stack.push((transition.to, next_input_pos, next_output_pos, next_cost));
            }
        }
    }

    false
}

fn advance(label: Option<char>, tape: &[char], pos: usize) -> Option<usize> {
    match label {
        Some(label) if tape.get(pos).copied() == Some(label) => pos.checked_add(1),
        Some(_) => None,
        None => Some(pos),
    }
}

#[test]
fn standard_variants_accept_exact_and_one_substitution_paths() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "hullo"]);

    let mut levenshtein = LevenshteinWfst::new(&dict, "hello", 1);
    assert!(accepts_pair(&mut levenshtein, "hello", "hello", 0.0));
    assert!(accepts_pair(&mut levenshtein, "hello", "hullo", 1.0));

    let mut generalized = GeneralizedWfstBuilder::new(&dict)
        .query("hello")
        .max_distance(1)
        .with_standard_ops()
        .build()
        .expect("standard generalized WFST should build");
    assert!(accepts_pair(&mut generalized, "hello", "hello", 0.0));
    assert!(accepts_pair(&mut generalized, "hello", "hullo", 1.0));

    let mut universal = UniversalLevenshteinWfst::<Standard, _>::new(&dict, "hello", 1);
    assert!(accepts_pair(&mut universal, "hello", "hello", 0.0));
    assert!(accepts_pair(&mut universal, "hello", "hullo", 1.0));
}

#[test]
fn universal_variant_accepts_one_insertion_and_deletion_paths() {
    let insertion_dict = DynamicDawgChar::<()>::from_terms(vec!["cat"]);
    let mut insertion = UniversalLevenshteinWfst::<Standard, _>::new(&insertion_dict, "at", 1);
    assert!(accepts_pair(&mut insertion, "at", "cat", 1.0));

    let deletion_dict = DynamicDawgChar::<()>::from_terms(vec!["at"]);
    let mut deletion = UniversalLevenshteinWfst::<Standard, _>::new(&deletion_dict, "cat", 1);
    assert!(accepts_pair(&mut deletion, "cat", "at", 1.0));
}

#[test]
fn transposition_variants_accept_swapped_adjacent_pair() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);

    let mut levenshtein = LevenshteinWfst::with_algorithm(&dict, "ba", 1, Algorithm::Transposition);
    assert!(accepts_pair(&mut levenshtein, "ba", "ab", 1.0));

    let mut generalized = GeneralizedWfstBuilder::new(&dict)
        .query("ba")
        .max_distance(1)
        .with_transposition()
        .build()
        .expect("transposition generalized WFST should build");
    assert!(accepts_pair(&mut generalized, "ba", "ab", 1.0));
}
