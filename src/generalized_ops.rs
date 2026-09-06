use liblevenshtein::transducer::{OperationApplicability, OperationSet, OperationType};
use smallvec::SmallVec;

type LabelBuffer = SmallVec<[char; 4]>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedOperation {
    pub(crate) index: usize,
    pub(crate) consume_x: usize,
    pub(crate) consume_y: usize,
    pub(crate) weight: f64,
}

pub(crate) fn bounded_operation_set(max_distance: u8, operations: OperationSet) -> OperationSet {
    let mut bounded = OperationSet::with_capacity(operations.len());

    for operation in operations.operations() {
        if operation_can_contribute_within_bound(operation, max_distance)
            && !contains_operation_with_same_wfst_semantics(&bounded, operation)
        {
            bounded.add(operation.clone());
        }
    }

    bounded
}

#[inline]
fn operation_can_contribute_within_bound(operation: &OperationType, max_distance: u8) -> bool {
    operation.weight().is_finite() && operation.weight() <= f64::from(max_distance) + f64::EPSILON
}

fn contains_operation_with_same_wfst_semantics(
    operations: &OperationSet,
    candidate: &OperationType,
) -> bool {
    operations
        .operations()
        .iter()
        .any(|operation| operations_have_same_wfst_semantics(operation, candidate))
}

fn operations_have_same_wfst_semantics(left: &OperationType, right: &OperationType) -> bool {
    left.consume_x() == right.consume_x()
        && left.consume_y() == right.consume_y()
        && left.weight() == right.weight()
        && left.applicability() == right.applicability()
}

pub(crate) fn prepare_operations(operations: &OperationSet) -> Vec<PreparedOperation> {
    let mut prepared = Vec::with_capacity(operations.len());

    for (index, operation) in operations.operations().iter().enumerate() {
        prepared.push(PreparedOperation {
            index,
            consume_x: operation.consume_x(),
            consume_y: operation.consume_y(),
            weight: operation.weight(),
        });
    }

    prepared
}

pub(crate) fn compute_query_only_costs(
    query: &str,
    operations: &[OperationType],
    prepared_operations: &[PreparedOperation],
) -> Vec<Option<f64>> {
    let mut costs = vec![None; query.len() + 1];
    costs[query.len()] = Some(0.0);

    for (start, _) in query.char_indices().rev() {
        let mut best: Option<f64> = None;

        for prepared in prepared_operations {
            let Some(op) = operations.get(prepared.index) else {
                continue;
            };
            let query_width = prepared.consume_y;
            if prepared.consume_x != 0 || query_width == 0 {
                continue;
            }

            let Some((segment, next)) = str_segment_by_char_width(query, start, query_width) else {
                continue;
            };
            let query_chars: LabelBuffer = segment.chars().collect();
            if !operation_applies(prepared, op, &[], &[], &query_chars, segment.as_bytes()) {
                continue;
            }

            if let Some(rest) = costs.get(next).copied().flatten() {
                let candidate = prepared.weight + rest;
                best = Some(best.map_or(candidate, |current| current.min(candidate)));
            }
        }

        costs[start] = best.map(canonical_cost);
    }

    costs
}

pub(crate) fn str_segment_by_char_width(
    input: &str,
    start: usize,
    char_len: usize,
) -> Option<(&str, usize)> {
    if start > input.len() || !input.is_char_boundary(start) {
        return None;
    }

    let mut end = start;
    let rest = input.get(start..)?;
    let mut chars = rest.char_indices();
    for _ in 0..char_len {
        let (offset, ch) = chars.next()?;
        end = start.checked_add(offset.checked_add(ch.len_utf8())?)?;
    }

    input.get(start..end).map(|segment| (segment, end))
}

pub(crate) fn operation_applies(
    _prepared: &PreparedOperation,
    op: &OperationType,
    dict_chars: &[char],
    dict_bytes: &[u8],
    query_chars: &[char],
    query_bytes: &[u8],
) -> bool {
    match op.applicability() {
        OperationApplicability::AdjacentTranspose => {
            dict_chars.len() == 2
                && query_chars.len() == 2
                && dict_chars[0] == query_chars[1]
                && dict_chars[1] == query_chars[0]
        }
        OperationApplicability::Any => true,
        OperationApplicability::Equal => dict_chars == query_chars,
        OperationApplicability::Listed(restriction) => {
            restriction.contains_str(dict_bytes, query_bytes)
        }
    }
}

#[inline]
pub(crate) fn canonical_cost(cost: f64) -> f64 {
    if cost == 0.0 {
        0.0
    } else {
        cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liblevenshtein::transducer::{
        OperationApplicability as NativeOperationApplicability, OperationSetBuilder, OperationType,
        SubstitutionSet,
    };

    fn applies(operation: OperationType, dictionary: &str, query: &str) -> bool {
        let operations = OperationSetBuilder::new().with_operation(operation).build();
        let prepared = prepare_operations(&operations);
        let dictionary_chars = dictionary.chars().collect::<LabelBuffer>();
        let query_chars = query.chars().collect::<LabelBuffer>();

        operation_applies(
            &prepared[0],
            &operations.operations()[0],
            &dictionary_chars,
            dictionary.as_bytes(),
            &query_chars,
            query.as_bytes(),
        )
    }

    #[test]
    fn filters_operations_that_cannot_contribute_within_bound() {
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 0.0, "match"))
            .with_operation(OperationType::new(1, 1, 1.0, "over_budget"))
            .with_operation(OperationType::new(1, 1, f64::INFINITY, "infinite"))
            .build();

        let bounded = bounded_operation_set(0, operations);

        assert_eq!(bounded.len(), 1);
        assert_eq!(bounded.operations()[0].name(), "match");
    }

    #[test]
    fn bounded_operations_deduplicate_wfst_equivalent_operations() {
        let mut left_only = liblevenshtein::transducer::SubstitutionSet::new();
        left_only.allow('a', 'b');
        let mut right_only = liblevenshtein::transducer::SubstitutionSet::new();
        right_only.allow('a', 'c');
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 0.0, "match"))
            .with_operation(OperationType::new(1, 1, 0.0, "match_alias"))
            .with_operation(OperationType::new(1, 1, 1.0, "substitute"))
            .with_operation(OperationType::new(1, 1, 1.0, "substitute_alias"))
            .with_operation(OperationType::with_restriction(
                1,
                1,
                0.25,
                left_only.clone(),
                "left",
            ))
            .with_operation(OperationType::with_restriction(
                1,
                1,
                0.25,
                left_only,
                "left_alias",
            ))
            .with_operation(OperationType::with_restriction(
                1, 1, 0.25, right_only, "right",
            ))
            .build();

        let bounded = bounded_operation_set(1, operations);

        assert_eq!(
            bounded
                .operations()
                .iter()
                .map(OperationType::name)
                .collect::<Vec<_>>(),
            vec!["match", "substitute", "left", "right"]
        );
    }

    #[test]
    fn any_applicability_includes_equal_and_unequal_slices() {
        assert!(applies(OperationType::new(1, 1, 1.0, "any"), "a", "a"));
        assert!(applies(OperationType::new(1, 1, 1.0, "any"), "a", "b"));
        assert!(applies(
            OperationType::with_applicability(
                1,
                1,
                0.0,
                NativeOperationApplicability::Any,
                "zero_cost_any",
            ),
            "a",
            "b",
        ));
    }

    #[test]
    fn equal_applicability_is_independent_of_weight_and_supports_unicode_widths() {
        let equal = || {
            OperationType::with_applicability(
                2,
                2,
                0.25,
                NativeOperationApplicability::Equal,
                "positive_equal",
            )
        };

        assert!(applies(equal(), "éa", "éa"));
        assert!(!applies(equal(), "éa", "éb"));
    }

    #[test]
    fn adjacent_transpose_is_tagged_not_named_and_allows_repeated_scalars() {
        let transpose = || OperationType::adjacent_transposition(1.0, "renamed");
        assert!(applies(transpose(), "ab", "ba"));
        assert!(applies(transpose(), "aa", "aa"));
        assert!(!applies(transpose(), "ab", "cd"));

        let misleading_name = OperationType::new(2, 2, 1.0, "transpose");
        assert!(applies(misleading_name, "ab", "cd"));
    }

    #[test]
    fn listed_applicability_is_directional_and_empty_lists_apply_nowhere() {
        let mut directed = SubstitutionSet::new();
        directed.allow_str("ph", "f");
        let listed = || OperationType::with_restriction(2, 1, 0.25, directed.clone(), "directed");

        assert!(applies(listed(), "ph", "f"));
        assert!(!applies(listed(), "f", "ph"));

        let empty = OperationType::with_restriction(1, 1, 0.25, SubstitutionSet::new(), "empty");
        assert!(!applies(empty, "a", "b"));

        let mut zero_cost_pairs = SubstitutionSet::new();
        zero_cost_pairs.allow('a', 'b');
        let zero_cost_listed =
            OperationType::with_restriction(1, 1, 0.0, zero_cost_pairs, "zero_cost_listed");
        assert!(applies(zero_cost_listed, "a", "b"));
    }

    #[test]
    fn semantic_deduplication_preserves_distinct_applicability_in_either_order() {
        let equal = || {
            OperationType::with_applicability(
                1,
                1,
                1.0,
                NativeOperationApplicability::Equal,
                "equal",
            )
        };
        let any = || OperationType::new(1, 1, 1.0, "any");

        for operations in [
            OperationSetBuilder::new()
                .with_operation(equal())
                .with_operation(any())
                .build(),
            OperationSetBuilder::new()
                .with_operation(any())
                .with_operation(equal())
                .build(),
        ] {
            let bounded = bounded_operation_set(1, operations);
            assert_eq!(bounded.len(), 2);
        }
    }

    #[test]
    fn semantic_deduplication_ignores_names_and_list_insertion_order() {
        let mut forward = SubstitutionSet::new();
        forward.allow('a', 'b');
        forward.allow('c', 'd');
        let mut reverse = SubstitutionSet::new();
        reverse.allow('c', 'd');
        reverse.allow('a', 'b');
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::with_restriction(
                1, 1, 0.25, forward, "first",
            ))
            .with_operation(OperationType::with_restriction(
                1, 1, 0.25, reverse, "renamed",
            ))
            .with_operation(OperationType::adjacent_transposition(1.0, "transpose_one"))
            .with_operation(OperationType::adjacent_transposition(1.0, "transpose_two"))
            .build();

        let bounded = bounded_operation_set(1, operations);

        assert_eq!(bounded.len(), 2);
        assert_eq!(bounded.operations()[0].name(), "first");
        assert_eq!(bounded.operations()[1].name(), "transpose_one");
    }

    #[test]
    fn prepares_operation_dimensions_and_weights_once() {
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 0.0, "match"))
            .with_operation(OperationType::new(1, 1, 1.0, "substitute"))
            .with_operation(OperationType::new(2, 2, 1.0, "transpose"))
            .with_operation(OperationType::new(2, 1, 1.0, "merge"))
            .build();

        let prepared = prepare_operations(&operations);

        assert_eq!(prepared.len(), 4);
        assert_eq!(prepared[0].consume_x, 1);
        assert_eq!(prepared[0].consume_y, 1);
        assert_eq!(prepared[0].weight, 0.0);
        assert_eq!(prepared[1].weight, 1.0);
        assert_eq!(prepared[2].consume_x, 2);
        assert_eq!(prepared[2].consume_y, 2);
        assert_eq!(prepared[3].consume_x, 2);
        assert_eq!(prepared[3].consume_y, 1);
    }

    #[test]
    fn query_only_costs_skip_unrepresentable_operation_widths() {
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(0, usize::MAX, 1.0, "too_wide"))
            .with_operation(OperationType::new(0, 1, 1.0, "insert"))
            .build();
        let prepared = prepare_operations(&operations);

        let costs = compute_query_only_costs("a", operations.operations(), &prepared);

        assert_eq!(costs[0], Some(1.0));
        assert_eq!(costs[1], Some(0.0));
    }

    #[test]
    fn query_only_costs_count_unicode_chars() {
        let operations = OperationSet::standard();
        let prepared = prepare_operations(&operations);
        let costs = compute_query_only_costs("é", operations.operations(), &prepared);

        assert_eq!(costs[0], Some(1.0));
        assert_eq!(costs[1], None);
        assert_eq!(costs[2], Some(0.0));
    }
}
