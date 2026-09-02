//! Lazy dictionary-path state source for exact fzf scores.

use std::sync::{Arc, RwLock};

use libdictenstein::{Dictionary, DictionaryNode};
use lling_llang::prelude::{
    ArcticWeight, ExpansionFailure, ExpansionRequest, Semiring, StateExpansion, StateId,
    StateSource, WeightedTransition,
};
use smallvec::SmallVec;

use crate::fzf_scorer::{FzfConfig, FzfError};
use crate::fzf_state_support::FzfStateRegistry;
use crate::fzf_support::FzfCore;
use crate::{fulfill_expansion_request, DirectStateSource};

type FzfTransitions = SmallVec<[WeightedTransition<char, ArcticWeight>; 4]>;
type ComputedFzfState = (bool, ArcticWeight, FzfTransitions);

/// Lazy WFST state producer over dictionary prefixes.
///
/// State IDs are path-sensitive. This remains correct for both tries and DAWGs:
/// two prefixes that reach a shared dictionary node retain distinct fzf DP
/// columns and therefore distinct WFST states.
#[derive(Clone)]
pub struct FzfStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    dictionary: D,
    core: Arc<FzfCore>,
    states: Arc<RwLock<FzfStateRegistry<D::Node>>>,
}

impl<D> FzfStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    /// Create a source using the default fzf configuration.
    pub fn new(dictionary: &D, query: &str) -> Result<Self, FzfError> {
        Self::with_config(dictionary, query, FzfConfig::default())
    }

    /// Create a source with explicit scoring and resource limits.
    pub fn with_config(dictionary: &D, query: &str, config: FzfConfig) -> Result<Self, FzfError> {
        let core = Arc::new(FzfCore::new(query, config)?);
        let states = FzfStateRegistry::new(dictionary.root(), core.initial_column());
        Ok(Self {
            dictionary: dictionary.clone(),
            core,
            states: Arc::new(RwLock::new(states)),
        })
    }

    /// Query scalar values after configured case folding.
    pub fn query(&self) -> &[char] {
        self.core.query()
    }

    /// Whether `state` has been registered by a reachable dictionary path.
    pub fn is_valid_state(&self, state: StateId) -> bool {
        crate::read_lock(&self.states).get(state).is_some()
    }

    /// Number of path-sensitive states registered so far.
    pub fn registered_states(&self) -> usize {
        crate::read_lock(&self.states).len()
    }

    fn compute_registered_state(&self, state: StateId) -> Option<ComputedFzfState> {
        let registered = crate::read_lock(&self.states).get(state)?.clone();
        let edge_capacity = crate::dictionary_edge_capacity(&registered.node, 1, 0);
        let mut pending = edge_capacity.map_or_else(Vec::new, Vec::with_capacity);

        for (unit, child) in registered.node.edges() {
            let label = unit.into();
            let column = self.core.advance(&registered.column, label);
            pending.push((label, child, column));
        }

        let mut transitions = edge_capacity.map_or_else(SmallVec::new, SmallVec::with_capacity);
        if !pending.is_empty() {
            let mut states = crate::write_lock(&self.states);
            for (label, child, column) in pending {
                let child_score = column.best_full_score().unwrap_or(0);
                let delta = child_score
                    .checked_sub(registered.path_score)
                    .expect("the best complete fzf score is prefix-monotone by construction");
                let Some(child_state) = states.register_child(state, label, child, column) else {
                    continue;
                };
                transitions.push(WeightedTransition::new(
                    state,
                    Some(label),
                    Some(label),
                    child_state,
                    ArcticWeight::new(f64::from(delta)),
                ));
            }
        }

        let is_final = registered.node.is_final() && registered.column.best_full_score().is_some();
        let final_weight = if is_final {
            ArcticWeight::one()
        } else {
            ArcticWeight::zero()
        };
        Some((is_final, final_weight, transitions))
    }
}

impl<D> DirectStateSource<char, ArcticWeight> for FzfStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    fn expand_state(&self, state: StateId) -> StateExpansion<char, ArcticWeight> {
        let Some((is_final, final_weight, transitions)) = self.compute_registered_state(state)
        else {
            return StateExpansion::failed(ExpansionFailure::invalid_state(state));
        };
        if is_final {
            StateExpansion::final_state(final_weight, transitions)
        } else {
            StateExpansion::non_final(transitions)
        }
    }
}

impl<D> StateSource<char, ArcticWeight> for FzfStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<char, ArcticWeight> {
        fulfill_expansion_request(self, request)
    }

    fn start(&self) -> StateId {
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        self.dictionary.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FzfScorer;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

    #[test]
    fn transition_weights_telescope_to_exact_score() {
        let dictionary = DynamicDawgChar::<()>::from_terms(["foo/bar/baz"]);
        let source = FzfStateSource::new(&dictionary, "fbb").expect("short query is valid");
        let mut state = source.start();
        let mut accumulated = ArcticWeight::one();
        for expected in "foo/bar/baz".chars() {
            let StateExpansion::Expanded { transitions, .. } = source.expand_state(state) else {
                panic!("state source computes eagerly");
            };
            let transition = transitions
                .iter()
                .find(|transition| transition.output == Some(expected))
                .expect("fixture path is present");
            accumulated = accumulated.times(&transition.weight);
            state = transition.to;
        }
        let StateExpansion::Expanded { is_final, .. } = source.expand_state(state) else {
            panic!("state source computes eagerly");
        };
        assert!(is_final);
        let expected = FzfScorer::new("fbb")
            .expect("short query is valid")
            .score("foo/bar/baz")
            .expect("candidate is bounded")
            .expect("candidate matches")
            .score;
        assert_eq!(accumulated, ArcticWeight::new(f64::from(expected)));
    }
}
