//! Lazy arctic-weighted WFST over dictionary paths scored by fzf V2.

use libdictenstein::{Dictionary, DictionaryNode};
use lling_llang::prelude::{
    ArcticWeight, LazyWfst, LazyWfstWrapper, StateId, WeightedTransition, Wfst,
};
use lling_llang::wfst::CachePolicy;

use crate::{FzfConfig, FzfError, FzfStateSource};

/// A dictionary transducer whose accepting path weight is the exact fzf score.
///
/// Arc weights are score deltas. Their max-plus product telescopes to the
/// candidate's exact `FuzzyMatchV2` score; nonmatching final dictionary nodes
/// remain non-final in this transducer.
#[derive(Clone)]
pub struct FzfWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    inner: LazyWfstWrapper<FzfStateSource<D>, char, ArcticWeight>,
}

impl<D> FzfWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    /// Create a lazy WFST with the default fzf configuration.
    pub fn new(dictionary: &D, query: &str) -> Result<Self, FzfError> {
        Self::with_config(dictionary, query, FzfConfig::default())
    }

    /// Create a lazy WFST with explicit matching and resource limits.
    pub fn with_config(dictionary: &D, query: &str, config: FzfConfig) -> Result<Self, FzfError> {
        let source = FzfStateSource::with_config(dictionary, query, config)?;
        Ok(Self {
            inner: LazyWfstWrapper::new(source),
        })
    }

    /// Normalized query units retained by the state source.
    pub fn query(&self) -> &[char] {
        self.inner.source().query()
    }

    /// Consume the adapter and recover its state source.
    pub fn into_state_source(self) -> FzfStateSource<D> {
        self.inner.into_source()
    }
}

impl<D> Wfst<char, ArcticWeight> for FzfWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    fn start(&self) -> StateId {
        self.inner.start()
    }

    fn is_final(&self, state: StateId) -> bool {
        self.inner.is_final(state)
    }

    fn final_weight(&self, state: StateId) -> ArcticWeight {
        self.inner.final_weight(state)
    }

    fn transitions(&self, state: StateId) -> &[WeightedTransition<char, ArcticWeight>] {
        self.inner.transitions(state)
    }

    fn num_states(&self) -> usize {
        self.inner
            .num_states()
            .max(self.inner.source().registered_states())
    }

    fn is_valid_state(&self, state: StateId) -> bool {
        self.inner.source().is_valid_state(state)
    }

    fn total_transitions(&self) -> usize {
        self.inner.total_transitions()
    }
}

impl<D> LazyWfst<char, ArcticWeight> for FzfWfst<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + Copy + Send + Sync,
{
    fn is_expanded(&self, state: StateId) -> bool {
        self.inner.is_expanded(state)
    }

    fn expand(&mut self, state: StateId) {
        self.inner.expand(state);
    }

    fn transitions_lazy(&mut self, state: StateId) -> &[WeightedTransition<char, ArcticWeight>] {
        self.inner.transitions_lazy(state)
    }

    fn cache_policy(&self) -> CachePolicy {
        self.inner.cache_policy()
    }

    fn set_cache_policy(&mut self, policy: CachePolicy) {
        self.inner.set_cache_policy(policy);
    }

    fn computed_states(&self) -> usize {
        self.inner.computed_states()
    }

    fn clear_cache(&mut self) {
        self.inner.clear_cache();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use lling_llang::prelude::Semiring;

    #[test]
    fn lazy_wfst_accepts_matching_dictionary_path() {
        let dictionary = DynamicDawgChar::<()>::from_terms(["foo/bar", "other"]);
        let mut wfst = FzfWfst::new(&dictionary, "fb").expect("short query is valid");
        let mut state = wfst.start();
        let mut weight = ArcticWeight::one();
        for character in "foo/bar".chars() {
            let transition = wfst
                .transitions_lazy(state)
                .iter()
                .find(|transition| transition.output == Some(character))
                .expect("fixture path exists")
                .clone();
            weight = weight.times(&transition.weight);
            state = transition.to;
        }
        wfst.expand(state);
        assert!(wfst.is_final(state));
        assert!(weight.value() > 0.0);
    }
}
