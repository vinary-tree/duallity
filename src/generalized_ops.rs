use liblevenshtein::transducer::{OperationSet, OperationType};
use smallvec::SmallVec;

type LabelBuffer = SmallVec<[char; 4]>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperationApplicability {
    Match,
    UnrestrictedSubstitution,
    UnrestrictedTranspose,
    Restricted,
    Any,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedOperation {
    pub(crate) index: usize,
    pub(crate) consume_x: usize,
    pub(crate) consume_y: usize,
    pub(crate) weight: f64,
    pub(crate) applicability: OperationApplicability,
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
        && left.restriction() == right.restriction()
}

pub(crate) fn prepare_operations(operations: &OperationSet) -> Vec<PreparedOperation> {
    let mut prepared = Vec::with_capacity(operations.len());

    for (index, operation) in operations.operations().iter().enumerate() {
        prepared.push(PreparedOperation {
            index,
            consume_x: operation.consume_x(),
            consume_y: operation.consume_y(),
            weight: operation.weight(),
            applicability: classify_operation(operation),
        });
    }

    prepared
}

fn classify_operation(operation: &OperationType) -> OperationApplicability {
    if operation.name() == "transpose"
        && !operation.is_restricted()
        && operation.consume_x() == 2
        && operation.consume_y() == 2
    {
        OperationApplicability::UnrestrictedTranspose
    } else if operation.is_substitution() && !operation.is_restricted() {
        OperationApplicability::UnrestrictedSubstitution
    } else if operation.is_match() {
        OperationApplicability::Match
    } else if operation.is_restricted() {
        OperationApplicability::Restricted
    } else {
        OperationApplicability::Any
    }
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
    prepared: &PreparedOperation,
    op: &OperationType,
    dict_chars: &[char],
    dict_bytes: &[u8],
    query_chars: &[char],
    query_bytes: &[u8],
) -> bool {
    match prepared.applicability {
        OperationApplicability::UnrestrictedTranspose => {
            dict_chars.len() == 2
                && query_chars.len() == 2
                && dict_chars[0] == query_chars[1]
                && dict_chars[1] == query_chars[0]
                && dict_chars != query_chars
        }
        OperationApplicability::UnrestrictedSubstitution => dict_bytes != query_bytes,
        OperationApplicability::Match => dict_chars == query_chars,
        OperationApplicability::Restricted => op
            .restriction()
            .is_some_and(|restriction| restriction.contains_str(dict_bytes, query_bytes)),
        OperationApplicability::Any => true,
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
    use liblevenshtein::transducer::{OperationSetBuilder, OperationType};

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
    fn prepares_operation_applicability_once() {
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
        assert_eq!(prepared[0].applicability, OperationApplicability::Match);
        assert_eq!(
            prepared[1].applicability,
            OperationApplicability::UnrestrictedSubstitution
        );
        assert_eq!(
            prepared[2].applicability,
            OperationApplicability::UnrestrictedTranspose
        );
        assert_eq!(prepared[3].applicability, OperationApplicability::Any);
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
