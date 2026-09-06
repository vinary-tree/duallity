use liblevenshtein::cost::{CostScale, ScaleError};
use liblevenshtein::transducer::{OperationApplicability, OperationSet, OperationType};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedOperation {
    pub(crate) index: usize,
    pub(crate) consume_x: usize,
    pub(crate) consume_y: usize,
    /// Compact constructor-assigned indexes, independent of raw widths.
    pub(crate) source_width_slot: usize,
    pub(crate) query_width_slot: usize,
    /// Exact cost in the WFST's shared fixed-point domain.
    pub(crate) scaled_weight: usize,
    /// Presentation weight retained only for emitted tropical-f64 arcs.
    pub(crate) weight: f64,
}

pub(crate) fn bounded_operation_set(
    scale: CostScale,
    max_cost: usize,
    operations: OperationSet,
) -> Result<OperationSet, ScaleError> {
    // Scale every original rule before filtering. An unrepresentable rule is a
    // configuration error even when its nominal f64 value exceeds the budget.
    let scaled_weights = operations
        .operations()
        .iter()
        .map(|operation| scale.to_scaled(operation.weight()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut bounded = OperationSet::with_capacity(operations.len());

    for (operation, scaled_weight) in operations.operations().iter().zip(scaled_weights) {
        if scaled_weight <= max_cost
            && !contains_operation_with_same_wfst_semantics(&bounded, operation)
        {
            bounded.add(operation.clone());
        }
    }

    Ok(bounded)
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

pub(crate) fn prepare_operations(
    operations: &OperationSet,
    scale: CostScale,
) -> Result<Vec<PreparedOperation>, ScaleError> {
    let mut prepared = Vec::with_capacity(operations.len());
    // The caller validates the native aggregate-consumption ceiling before
    // preparation, so each temporary table has at most 4097 scalar entries.
    let mut source_slots = vec![
        usize::MAX;
        operations
            .operations()
            .iter()
            .map(OperationType::consume_x)
            .max()
            .unwrap_or(0)
            + 1
    ];
    let mut query_slots = vec![
        usize::MAX;
        operations
            .operations()
            .iter()
            .map(OperationType::consume_y)
            .max()
            .unwrap_or(0)
            + 1
    ];
    let mut source_count = 0;
    let mut query_count = 0;

    for (index, operation) in operations.operations().iter().enumerate() {
        let source_slot = &mut source_slots[operation.consume_x()];
        if *source_slot == usize::MAX {
            *source_slot = source_count;
            source_count += 1;
        }
        let query_slot = &mut query_slots[operation.consume_y()];
        if *query_slot == usize::MAX {
            *query_slot = query_count;
            query_count += 1;
        }
        prepared.push(PreparedOperation {
            index,
            consume_x: operation.consume_x(),
            consume_y: operation.consume_y(),
            source_width_slot: *source_slot,
            query_width_slot: *query_slot,
            scaled_weight: scale.to_scaled(operation.weight())?,
            weight: operation.weight(),
        });
    }

    Ok(prepared)
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

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::SmallVec;
    type LabelBuffer = SmallVec<[char; 4]>;
    use liblevenshtein::transducer::{
        OperationApplicability as NativeOperationApplicability, OperationSetBuilder, OperationType,
        SubstitutionSet,
    };

    fn applies(operation: OperationType, dictionary: &str, query: &str) -> bool {
        let operations = OperationSetBuilder::new().with_operation(operation).build();
        let scale = CostScale::for_operations(&operations).expect("test costs are exact decimals");
        let prepared = prepare_operations(&operations, scale).expect("test costs scale exactly");
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
            .build();

        let scale = CostScale::for_operations(&operations).expect("test costs are exact decimals");
        let bounded = bounded_operation_set(scale, 0, operations).expect("weights scale exactly");

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

        let scale = CostScale::for_operations(&operations).expect("test costs are exact decimals");
        let bounded = bounded_operation_set(scale, scale.scale_budget(1).unwrap(), operations)
            .expect("weights scale exactly");

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
            let scale =
                CostScale::for_operations(&operations).expect("test costs are exact decimals");
            let bounded = bounded_operation_set(scale, scale.scale_budget(1).unwrap(), operations)
                .expect("weights scale exactly");
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

        let scale = CostScale::for_operations(&operations).expect("test costs are exact decimals");
        let bounded = bounded_operation_set(scale, scale.scale_budget(1).unwrap(), operations)
            .expect("weights scale exactly");

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

        let scale = CostScale::for_operations(&operations).expect("test costs are exact decimals");
        let prepared = prepare_operations(&operations, scale).expect("weights scale exactly");

        assert_eq!(prepared.len(), 4);
        assert_eq!(prepared[0].consume_x, 1);
        assert_eq!(prepared[0].consume_y, 1);
        assert_eq!(prepared[0].scaled_weight, 0);
        assert_eq!(prepared[0].weight, 0.0);
        assert_eq!(prepared[1].scaled_weight, 1);
        assert_eq!(prepared[1].weight, 1.0);
        assert_eq!(prepared[2].consume_x, 2);
        assert_eq!(prepared[2].consume_y, 2);
        assert_eq!(prepared[3].consume_x, 2);
        assert_eq!(prepared[3].consume_y, 1);
        assert_eq!(
            prepared
                .iter()
                .map(|op| op.source_width_slot)
                .collect::<Vec<_>>(),
            [0, 0, 1, 1]
        );
        assert_eq!(
            prepared
                .iter()
                .map(|op| op.query_width_slot)
                .collect::<Vec<_>>(),
            [0, 0, 1, 0]
        );
    }

    #[test]
    fn maximum_width_uses_one_compact_slot_per_side() {
        for (source, query) in [(4096, 0), (0, 4096)] {
            let operations = OperationSetBuilder::new()
                .with_operation(OperationType::new(source, query, 1.0, "wide"))
                .build();
            operations.validate().expect("native width ceiling");
            let scale = CostScale::for_operations(&operations).expect("integer cost");
            let prepared = prepare_operations(&operations, scale).expect("prepare");
            assert_eq!(prepared[0].source_width_slot, 0);
            assert_eq!(prepared[0].query_width_slot, 0);
        }
    }

    #[test]
    fn filtering_reports_an_unrepresentable_over_budget_rule() {
        let operations = OperationSetBuilder::new()
            .with_operation(OperationType::new(1, 1, 1.0e-100, "unrepresentable"))
            .build();

        assert_eq!(
            CostScale::for_operations(&operations),
            Err(ScaleError::DenominatorOverflow)
        );
    }
}
