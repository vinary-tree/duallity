//! Path-sensitive state registry shared by the lazy fzf WFST adapter.

use libdictenstein::DictionaryNode;
use lling_llang::prelude::StateId;
use rustc_hash::FxHashMap;

use crate::fzf_support::FzfColumn;

#[derive(Clone)]
pub(crate) struct RegisteredFzfState<N: DictionaryNode> {
    pub(crate) node: N,
    pub(crate) column: FzfColumn,
    pub(crate) path_score: i32,
}

pub(crate) struct FzfStateRegistry<N: DictionaryNode> {
    child_to_id: FxHashMap<(StateId, char), StateId>,
    states: Vec<RegisteredFzfState<N>>,
}

impl<N: DictionaryNode> FzfStateRegistry<N> {
    pub(crate) fn new(root: N, root_column: FzfColumn) -> Self {
        Self {
            child_to_id: FxHashMap::default(),
            states: vec![RegisteredFzfState {
                node: root,
                path_score: root_column.best_full_score().unwrap_or(0),
                column: root_column,
            }],
        }
    }

    pub(crate) fn get(&self, state: StateId) -> Option<&RegisteredFzfState<N>> {
        self.states
            .get(usize::try_from(state).unwrap_or(usize::MAX))
    }

    pub(crate) fn register_child(
        &mut self,
        parent: StateId,
        label: char,
        node: N,
        column: FzfColumn,
    ) -> Option<StateId> {
        if let Some(state) = self.child_to_id.get(&(parent, label)).copied() {
            return Some(state);
        }
        let state = StateId::try_from(self.states.len()).ok()?;
        let path_score = column.best_full_score().unwrap_or(0);
        self.child_to_id.insert((parent, label), state);
        self.states.push(RegisteredFzfState {
            node,
            column,
            path_score,
        });
        Some(state)
    }

    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fzf_support::{FzfConfig, FzfCore};
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use libdictenstein::Dictionary;

    #[test]
    fn registry_keys_children_by_full_parent_path_state() {
        let dictionary = DynamicDawgChar::<()>::from_terms(["a"]);
        let core = FzfCore::new("a", FzfConfig::default()).expect("short query is valid");
        let root = dictionary.root();
        let child = root.edges().next().expect("fixture has one edge").1;
        let column = core.advance(&core.initial_column(), 'a');
        let mut registry = FzfStateRegistry::new(root, core.initial_column());
        assert_eq!(
            registry.register_child(0, 'a', child.clone(), column.clone()),
            Some(1)
        );
        assert_eq!(registry.register_child(0, 'a', child, column), Some(1));
        assert_eq!(registry.len(), 2);
    }
}
