//! Resource and error contracts for generalized WFST construction and expansion.

use liblevenshtein::cost::ScaleError;
use liblevenshtein::transducer::OperationSetValidationError;
use std::fmt;

/// Resource controlled by [`GeneralizedWfstLimits`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneralizedWfstResource {
    /// UTF-8 bytes retained by the owned query.
    QueryBytes,
    /// Unicode scalar values retained by the owned query.
    QueryScalars,
    /// Dictionary scalars consumed by one operation.
    OperationSourceScalars,
    /// Query scalars consumed by one operation.
    OperationQueryScalars,
    /// Dictionary-node identities retained across expansions.
    RetainedDictionaryNodes,
    /// Product and multi-label continuation identities retained across expansions.
    RetainedWfstStates,
    /// Complete dictionary paths materialized during one state expansion.
    PathsPerExpansion,
    /// Aggregate traversal, predicate, and label-copy work in one expansion.
    WorkUnitsPerExpansion,
}

impl fmt::Display for GeneralizedWfstResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::QueryBytes => "query bytes",
            Self::QueryScalars => "query scalars",
            Self::OperationSourceScalars => "operation source scalars",
            Self::OperationQueryScalars => "operation query scalars",
            Self::RetainedDictionaryNodes => "retained dictionary nodes",
            Self::RetainedWfstStates => "retained WFST states",
            Self::PathsPerExpansion => "paths per expansion",
            Self::WorkUnitsPerExpansion => "work units per expansion",
        };
        formatter.write_str(name)
    }
}

/// Inclusive resource ceilings for a generalized dictionary-product WFST.
///
/// A zero query limit accepts only an empty query. A zero operation-width
/// limit accepts only operations that consume zero scalars on that side. The
/// retained-node and retained-state limits must each be at least one because
/// construction installs the dictionary root and product start state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneralizedWfstLimits {
    /// Maximum UTF-8 byte length of the owned query.
    pub max_query_bytes: usize,
    /// Maximum Unicode-scalar length of the owned query.
    pub max_query_scalars: usize,
    /// Maximum dictionary-scalar width of one operation.
    pub max_operation_source_scalars: usize,
    /// Maximum query-scalar width of one operation.
    pub max_operation_query_scalars: usize,
    /// Maximum shared dictionary-node identities, including the root.
    pub max_retained_dictionary_nodes: usize,
    /// Maximum shared product and continuation states, including the start.
    pub max_retained_wfst_states: usize,
    /// Maximum complete dictionary paths considered by one expansion.
    pub max_paths_per_expansion: usize,
    /// Maximum aggregate charged work units for one expansion.
    pub max_work_units_per_expansion: usize,
}

impl Default for GeneralizedWfstLimits {
    fn default() -> Self {
        Self {
            max_query_bytes: 1 << 20,
            max_query_scalars: 1 << 18,
            max_operation_source_scalars: 4_096,
            max_operation_query_scalars: 4_096,
            max_retained_dictionary_nodes: 1_000_000,
            max_retained_wfst_states: 1_000_000,
            max_paths_per_expansion: 262_144,
            max_work_units_per_expansion: 4_000_000,
        }
    }
}

/// Construction or bounded-expansion failure for [`crate::GeneralizedWfst`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneralizedWfstError {
    /// No query was supplied to the builder.
    MissingQuery,
    /// The operation catalog violates the native generalized grammar contract.
    InvalidOperations(OperationSetValidationError),
    /// Decimal costs cannot be represented exactly in a bounded integer scale.
    CostScale(ScaleError),
    /// An inclusive configured ceiling was exceeded.
    LimitExceeded {
        /// Resource whose bound was exceeded.
        resource: GeneralizedWfstResource,
        /// Configured inclusive ceiling.
        limit: usize,
        /// Minimum required amount when known.
        required: usize,
    },
    /// Checked internal cost or identifier arithmetic overflowed.
    ArithmeticOverflow(&'static str),
    /// A fallible reservation could not allocate the requested capacity.
    AllocationFailed(&'static str),
}

impl GeneralizedWfstError {
    pub(crate) fn limit(resource: GeneralizedWfstResource, limit: usize, required: usize) -> Self {
        Self::LimitExceeded {
            resource,
            limit,
            required,
        }
    }
}

impl fmt::Display for GeneralizedWfstError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingQuery => formatter.write_str("Query not set"),
            Self::InvalidOperations(error) => write!(formatter, "invalid operation set: {error}"),
            Self::CostScale(error) => write!(formatter, "invalid exact cost scale: {error}"),
            Self::LimitExceeded {
                resource,
                limit,
                required,
            } => write!(
                formatter,
                "{resource} limit exceeded: requires {required}, configured limit is {limit}"
            ),
            Self::ArithmeticOverflow(context) => {
                write!(
                    formatter,
                    "generalized WFST arithmetic overflow while {context}"
                )
            }
            Self::AllocationFailed(context) => {
                write!(
                    formatter,
                    "generalized WFST allocation failed while {context}"
                )
            }
        }
    }
}

impl std::error::Error for GeneralizedWfstError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidOperations(error) => Some(error),
            Self::CostScale(error) => Some(error),
            _ => None,
        }
    }
}

impl From<OperationSetValidationError> for GeneralizedWfstError {
    fn from(error: OperationSetValidationError) -> Self {
        Self::InvalidOperations(error)
    }
}

impl From<ScaleError> for GeneralizedWfstError {
    fn from(error: ScaleError) -> Self {
        Self::CostScale(error)
    }
}
