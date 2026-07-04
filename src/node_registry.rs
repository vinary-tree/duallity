use libdictenstein::DictionaryNode;
use rustc_hash::FxHashMap;

use crate::fx_hash_map_with_capacity;
use crate::node_key::DictionaryNodeKey;

/// Registry for assigning stable dense IDs to dictionary nodes.
///
/// Dictionary node handles do not expose crate-wide integer identities, so
/// lazy WFSTs intern nodes by the exact path step that discovers them. The
/// registry stores the forward key-to-id map and the reverse id-to-node vector
/// used during later lazy expansion.
pub(crate) struct DictionaryNodeRegistry<N: DictionaryNode> {
    node_to_id: FxHashMap<DictionaryNodeKey, u32>,
    id_to_node: Vec<N>,
}

impl<N: DictionaryNode> DictionaryNodeRegistry<N> {
    pub(crate) fn new(root: N) -> Self {
        let mut registry = Self {
            node_to_id: fx_hash_map_with_capacity(1),
            id_to_node: Vec::with_capacity(1),
        };
        let _ = registry.register_node(root, DictionaryNodeKey::ROOT);
        registry
    }

    pub(crate) fn register_node(&mut self, node: N, key: DictionaryNodeKey) -> Option<u32> {
        if let Some(id) = self.get_id(key) {
            return Some(id);
        }

        let id = next_registry_id(self.id_to_node.len())?;
        self.node_to_id.insert(key, id);
        self.id_to_node.push(node);
        Some(id)
    }

    pub(crate) fn candidate_id(&self, key: DictionaryNodeKey) -> Option<u32> {
        self.get_id(key)
            .or_else(|| next_registry_id(self.id_to_node.len()))
    }

    pub(crate) fn get_node(&self, id: u32) -> Option<&N> {
        self.id_to_node.get(usize_from_u32(id))
    }

    pub(crate) fn get_id(&self, key: DictionaryNodeKey) -> Option<u32> {
        self.node_to_id.get(&key).copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.id_to_node.len()
    }

    #[cfg(test)]
    pub(crate) fn node_capacity(&self) -> usize {
        self.id_to_node.capacity()
    }

    #[cfg(test)]
    pub(crate) fn key_capacity(&self) -> usize {
        self.node_to_id.capacity()
    }
}

/// Dictionary-node registry variant that also tracks character depth.
pub(crate) struct DepthDictionaryNodeRegistry<N: DictionaryNode> {
    nodes: DictionaryNodeRegistry<N>,
    id_to_depth: Vec<usize>,
}

impl<N: DictionaryNode> DepthDictionaryNodeRegistry<N> {
    pub(crate) fn new(root: N) -> Self {
        Self {
            nodes: DictionaryNodeRegistry::new(root),
            id_to_depth: Vec::from([0]),
        }
    }

    pub(crate) fn register_node(
        &mut self,
        node: N,
        key: DictionaryNodeKey,
        depth: usize,
    ) -> Option<u32> {
        if let Some(id) = self.nodes.get_id(key) {
            return Some(id);
        }

        let id = self.nodes.register_node(node, key)?;
        self.id_to_depth.push(depth);
        Some(id)
    }

    pub(crate) fn candidate_id(&self, key: DictionaryNodeKey) -> Option<u32> {
        self.nodes.candidate_id(key)
    }

    pub(crate) fn get_node(&self, id: u32) -> Option<&N> {
        self.nodes.get_node(id)
    }

    pub(crate) fn get_depth(&self, id: u32) -> Option<usize> {
        self.id_to_depth.get(usize_from_u32(id)).copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }
}

#[inline]
pub(crate) fn next_registry_id(len: usize) -> Option<u32> {
    len.try_into().ok()
}

#[inline]
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::{next_registry_id, DepthDictionaryNodeRegistry, DictionaryNodeRegistry};
    use crate::node_key::DictionaryNodeKey;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use libdictenstein::Dictionary;

    #[test]
    fn dictionary_node_registry_preallocates_and_reuses_root_id() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let root = dict.root();
        let mut registry = DictionaryNodeRegistry::new(root.clone());

        assert!(registry.get_node(0).is_some());
        assert_eq!(registry.len(), 1);
        assert!(registry.node_capacity() >= 1);
        assert!(registry.key_capacity() >= 1);
        assert_eq!(
            registry.register_node(root, DictionaryNodeKey::ROOT),
            Some(0)
        );
    }

    #[test]
    fn depth_registry_reuses_existing_depth_for_existing_key() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let root = dict.root();
        let mut registry = DepthDictionaryNodeRegistry::new(root.clone());

        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.register_node(root, DictionaryNodeKey::ROOT, 99),
            Some(0)
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get_depth(0), Some(0));
    }

    #[test]
    fn next_registry_id_rejects_unrepresentable_len() {
        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };
        assert_eq!(next_registry_id(max_u32), Some(u32::MAX));

        if let Some(above_max_u32) = max_u32.checked_add(1) {
            assert_eq!(next_registry_id(above_max_u32), None);
        }
    }

    #[test]
    fn candidate_id_reuses_existing_or_reports_next_insert_id() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let root = dict.root();
        let mut registry = DictionaryNodeRegistry::new(root.clone());

        assert_eq!(registry.candidate_id(DictionaryNodeKey::ROOT), Some(0));
        assert_eq!(
            registry.candidate_id(DictionaryNodeKey::child(0, 'a')),
            Some(1)
        );

        assert_eq!(
            registry.register_node(root, DictionaryNodeKey::child(0, 'a')),
            Some(1)
        );
        assert_eq!(
            registry.candidate_id(DictionaryNodeKey::child(0, 'b')),
            Some(2)
        );
    }
}
