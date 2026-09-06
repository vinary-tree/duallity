//! Transaction-local identities and work accounting for generalized expansion.

use libdictenstein::DictionaryNode;
use lling_llang::prelude::{CancellationToken, ExpansionError, ExpansionFailure, StateId};
use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::generalized_limits::{
    GeneralizedWfstError, GeneralizedWfstLimits, GeneralizedWfstResource,
};
use crate::generalized_state_support::{
    next_state_id, EmissionChain, EmitState, LabelBuffer, PendingDictionaryNode, ProductState,
    RegisteredState, StateRegistry,
};

pub(crate) fn require_at_most(
    resource: GeneralizedWfstResource,
    required: usize,
    limit: usize,
) -> Result<(), GeneralizedWfstError> {
    if required <= limit {
        Ok(())
    } else {
        Err(GeneralizedWfstError::LimitExceeded {
            resource,
            limit,
            required,
        })
    }
}

pub(crate) fn expansion_failure(error: GeneralizedWfstError) -> ExpansionError {
    ExpansionError::Failure(ExpansionFailure::resource_exhausted(error.to_string()))
}

pub(crate) struct ExpansionBudget<'a> {
    paths: usize,
    work: usize,
    limits: GeneralizedWfstLimits,
    cancellation: Option<&'a CancellationToken>,
}

impl<'a> ExpansionBudget<'a> {
    pub(crate) fn new(
        limits: GeneralizedWfstLimits,
        cancellation: Option<&'a CancellationToken>,
    ) -> Self {
        Self {
            paths: 0,
            work: 0,
            limits,
            cancellation,
        }
    }

    pub(crate) fn check_cancelled(&self) -> Result<(), ExpansionError> {
        match self.cancellation.and_then(CancellationToken::reason) {
            Some(reason) => Err(ExpansionError::Cancelled(reason)),
            None => Ok(()),
        }
    }

    pub(crate) fn charge_work(&mut self, units: usize) -> Result<(), ExpansionError> {
        self.check_cancelled()?;
        self.work = self.work.checked_add(units).ok_or_else(|| {
            expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                "counting expansion work",
            ))
        })?;
        require_at_most(
            GeneralizedWfstResource::WorkUnitsPerExpansion,
            self.work,
            self.limits.max_work_units_per_expansion,
        )
        .map_err(expansion_failure)
    }

    pub(crate) fn charge_path(&mut self) -> Result<(), ExpansionError> {
        self.check_cancelled()?;
        self.paths = self.paths.checked_add(1).ok_or_else(|| {
            expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                "counting dictionary paths",
            ))
        })?;
        require_at_most(
            GeneralizedWfstResource::PathsPerExpansion,
            self.paths,
            self.limits.max_paths_per_expansion,
        )
        .map_err(expansion_failure)
    }
}

pub(crate) struct PendingOperationArc {
    pub(crate) target_node: PendingDictionaryNode,
    pub(crate) query_byte_pos: usize,
    pub(crate) cost: usize,
    pub(crate) input: LabelBuffer,
    pub(crate) output: LabelBuffer,
    pub(crate) weight: f64,
}

pub(crate) struct StagedDictionaryNode<N: DictionaryNode> {
    pub(crate) parent: PendingDictionaryNode,
    pub(crate) label: char,
    pub(crate) node: Arc<N>,
}

/// Staged IDs are local vector offsets and never escape a successful commit.
pub(crate) struct ExpansionStaging<N: DictionaryNode> {
    pub(crate) nodes: Vec<StagedDictionaryNode<N>>,
    node_by_step: FxHashMap<(PendingDictionaryNode, char), PendingDictionaryNode>,
}

impl<N: DictionaryNode> ExpansionStaging<N> {
    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_by_step: FxHashMap::default(),
        }
    }

    pub(crate) fn stage_child(
        &mut self,
        registered: Option<u32>,
        parent: PendingDictionaryNode,
        label: char,
        node: &N,
    ) -> PendingDictionaryNode {
        if let Some(id) = registered {
            return PendingDictionaryNode::Registered(id);
        }
        if let Some(reference) = self.node_by_step.get(&(parent, label)) {
            return *reference;
        }
        // Enumeration work bounds this vector. The retained-node limit is
        // checked only after concurrent aliases are reconciled at commit.
        let reference = PendingDictionaryNode::Staged(self.nodes.len());
        self.nodes.push(StagedDictionaryNode {
            parent,
            label,
            node: Arc::new(node.clone()),
        });
        self.node_by_step.insert((parent, label), reference);
        reference
    }
}

pub(crate) fn resolved_node(reference: PendingDictionaryNode, ids: &[u32]) -> u32 {
    match reference {
        PendingDictionaryNode::Registered(id) => id,
        PendingDictionaryNode::Staged(index) => ids[index],
    }
}

/// Validate the last ID of a nonempty batch, excluding lling-llang's sentinel.
fn checked_state_count(required: usize, limit: usize) -> Result<StateId, ExpansionError> {
    require_at_most(GeneralizedWfstResource::RetainedWfstStates, required, limit)
        .map_err(expansion_failure)?;
    required
        .checked_sub(1)
        .and_then(next_state_id)
        .ok_or_else(|| {
            expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                "encoding a WFST state ID without using NO_STATE",
            ))
        })
}

/// A transaction-local extension, without copying any existing registry entry.
pub(crate) struct StateBatch {
    pub(crate) states: Vec<RegisteredState>,
    products: FxHashMap<ProductState, StateId>,
    chains: FxHashMap<Arc<EmissionChain>, StateId>,
}

impl StateBatch {
    pub(crate) fn new() -> Self {
        Self {
            states: Vec::new(),
            products: FxHashMap::default(),
            chains: FxHashMap::default(),
        }
    }

    fn next_id(&self, registry: &StateRegistry, limit: usize) -> Result<StateId, ExpansionError> {
        let index = registry
            .len()
            .checked_add(self.states.len())
            .ok_or_else(|| {
                expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                    "assigning a WFST state ID",
                ))
            })?;
        let required = index.checked_add(1).ok_or_else(|| {
            expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                "counting retained WFST states",
            ))
        })?;
        checked_state_count(required, limit)
    }

    pub(crate) fn product(
        &mut self,
        registry: &StateRegistry,
        state: ProductState,
        limit: usize,
    ) -> Result<StateId, ExpansionError> {
        if let Some(id) = registry
            .product_id(state)
            .or_else(|| self.products.get(&state).copied())
        {
            return Ok(id);
        }
        let id = self.next_id(registry, limit)?;
        self.products.insert(state, id);
        self.states.push(RegisteredState::Product(state));
        Ok(id)
    }

    pub(crate) fn chain(
        &mut self,
        registry: &StateRegistry,
        chain: EmissionChain,
        limit: usize,
    ) -> Result<StateId, ExpansionError> {
        let width = chain.input.len().max(chain.output.len());
        if width <= 1 {
            return Ok(chain.target);
        }
        if let Some(id) = registry
            .chain_id(&chain)
            .or_else(|| self.chains.get(&chain).copied())
        {
            return Ok(id);
        }
        let first = self.next_id(registry, limit)?;
        let count = width - 1;
        let required = registry
            .len()
            .checked_add(self.states.len())
            .and_then(|base| base.checked_add(count))
            .ok_or_else(|| {
                expansion_failure(GeneralizedWfstError::ArithmeticOverflow(
                    "counting a complete emission chain",
                ))
            })?;
        checked_state_count(required, limit)?;
        self.states.try_reserve(count).map_err(|_| {
            expansion_failure(GeneralizedWfstError::AllocationFailed(
                "staging an emission chain",
            ))
        })?;
        let chain = Arc::new(chain);
        self.chains.insert(Arc::clone(&chain), first);
        for position in 1..width {
            let next = if position + 1 == width {
                chain.target
            } else {
                first + StateId::try_from(position).expect("preflight checked chain width")
            };
            self.states.push(RegisteredState::Emit(EmitState {
                chain: Arc::clone(&chain),
                position,
                next,
            }));
        }
        Ok(first)
    }

    pub(crate) fn prepare_registry(
        &self,
        registry: &mut StateRegistry,
    ) -> Result<(), ExpansionError> {
        registry
            .try_reserve_additional(self.products.len(), self.chains.len(), self.states.len())
            .map_err(|_| {
                expansion_failure(GeneralizedWfstError::AllocationFailed(
                    "reserving WFST identities",
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_and_chain_preflight_never_allocate_the_no_state_sentinel() {
        let sentinel = usize::try_from(lling_llang::wfst::NO_STATE).expect("state ID fits usize");
        assert_eq!(
            next_state_id(sentinel - 1),
            Some(lling_llang::wfst::NO_STATE - 1)
        );
        assert_eq!(next_state_id(sentinel), None);
        assert_eq!(
            checked_state_count(sentinel, usize::MAX).expect("last valid chain endpoint"),
            lling_llang::wfst::NO_STATE - 1
        );
        if let Some(includes_sentinel) = sentinel.checked_add(1) {
            assert!(checked_state_count(includes_sentinel, usize::MAX).is_err());
        }
        assert!(checked_state_count(2, 1).is_err());
    }
}
