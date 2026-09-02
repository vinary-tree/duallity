//! StateSource implementation for Phonetic WFST composition.
//!
//! This module provides [`PhoneticStateSource`], which implements lling-llang's
//! [`StateSource`] trait for phonetic fuzzy matching. It composes a phonetic
//! NFA with a Levenshtein automaton for combined phonetic + edit distance matching.
//!
//! # Architecture
//!
//! The phonetic state source computes product states:
//! - **NFA states**: Active states in the phonetic NFA (after epsilon closure)
//! - **Edit distance**: Current edit distance consumed
//! - **Dictionary node**: Position in the dictionary trie
//!
//! The product state space is encoded into a single `StateId` for WFST composition.

use std::sync::Arc;

use lling_llang::prelude::{
    ExpansionFailure, ExpansionRequest, Semiring, StateExpansion, StateId, StateSource,
    TropicalWeight, WeightedTransition,
};
use smallvec::SmallVec;

use libdictenstein::{Dictionary, DictionaryNode};
#[cfg(feature = "phonetic-rules")]
use liblevenshtein::phonetic::nfa::{NFAChar, ProductAutomatonChar, ProductStateChar};

use crate::node_key::DictionaryNodeKey;
use crate::node_registry::DictionaryNodeRegistry;
#[cfg(feature = "phonetic-rules")]
use crate::phonetic_state_support::{estimated_phonetic_product_states, ProductStateRegistry};
use crate::state_encoding;
use crate::{fulfill_expansion_request, DirectStateSource};

/// State source for phonetic WFST composition.
///
/// This implements lling-llang's [`StateSource`] trait using the phonetic
/// product automaton (NFA × Levenshtein). States are computed on-demand as
/// the composed transducer is traversed.
///
/// # Product State Representation
///
/// Each WFST state represents a tuple:
/// - `dictionary_node_id`: Position in the dictionary trie
/// - `product_state_id`: ID of the phonetic product state (NFA × Levenshtein)
///
/// # Example
///
/// ```rust,no_run
/// use duallity::PhoneticStateSource;
/// use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
/// use liblevenshtein::phonetic::nfa::compile;
/// use liblevenshtein::phonetic::regex::parse;
/// use lling_llang::prelude::*;
///
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["phone", "fone", "phon"]);
/// let nfa = compile(&parse("(ph|f)one").expect("valid pattern")).expect("compiles");
/// let source = PhoneticStateSource::new(&dict, nfa, 2);
///
/// // Use with LazyWfstWrapper for composition
/// let lazy_wfst = LazyWfstWrapper::new(source);
/// ```
#[cfg(feature = "phonetic-rules")]
#[derive(Clone)]
pub struct PhoneticStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// The dictionary to search
    dictionary: D,
    /// Product automaton (NFA × Levenshtein)
    product: Arc<ProductAutomatonChar>,
    /// Maximum edit distance
    max_distance: u8,
    /// Phonetic weight (cost for phonetic transformations)
    phonetic_weight: f64,
    /// Edit distance weight multiplier applied to accepting final weights.
    edit_weight: f64,
    /// Maximum product states for encoding
    max_product_states: u32,
    /// Node registry: maps exact path keys to node IDs
    node_registry: Arc<std::sync::RwLock<DictionaryNodeRegistry<D::Node>>>,
    /// Product frontier registry: maps product-state frontiers to IDs.
    product_state_registry: Arc<std::sync::RwLock<ProductStateRegistry>>,
}

#[cfg(feature = "phonetic-rules")]
impl<D> PhoneticStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    /// Create a new phonetic state source.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `nfa`: The phonetic NFA pattern
    /// - `max_distance`: Maximum edit distance for matches
    pub fn new(dictionary: &D, nfa: NFAChar, max_distance: u8) -> Self {
        Self::from_validated_weights(dictionary, nfa, max_distance, 0.0, 1.0)
    }

    /// Create a new phonetic state source with custom phonetic weight.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `nfa`: The phonetic NFA pattern
    /// - `max_distance`: Maximum edit distance for matches
    /// - `phonetic_weight`: Cost for phonetic transformations
    pub fn with_phonetic_weight(
        dictionary: &D,
        nfa: NFAChar,
        max_distance: u8,
        phonetic_weight: f64,
    ) -> Result<Self, crate::InvalidWeightError> {
        Self::with_weights(dictionary, nfa, max_distance, phonetic_weight, 1.0)
    }

    /// Create a new phonetic state source with custom phonetic and edit weights.
    ///
    /// `phonetic_weight` is charged on each consumed dictionary/NFA edge, matching
    /// [`crate::PhoneticNfaWfst`]. `edit_weight` scales the accepting edit-distance
    /// final weight reported by the product automaton.
    ///
    /// # Arguments
    ///
    /// - `dictionary`: The dictionary to search
    /// - `nfa`: The phonetic NFA pattern
    /// - `max_distance`: Maximum edit distance for matches, before weighting
    /// - `phonetic_weight`: Cost for each consumed phonetic transition
    /// - `edit_weight`: Multiplier applied to accepted edit distance
    pub fn with_weights(
        dictionary: &D,
        nfa: NFAChar,
        max_distance: u8,
        phonetic_weight: f64,
        edit_weight: f64,
    ) -> Result<Self, crate::InvalidWeightError> {
        let phonetic_weight =
            crate::validate_finite_nonnegative_weight("phonetic_weight", phonetic_weight)?;
        let edit_weight = crate::validate_finite_nonnegative_weight("edit_weight", edit_weight)?;

        Ok(Self::from_validated_weights(
            dictionary,
            nfa,
            max_distance,
            phonetic_weight,
            edit_weight,
        ))
    }

    fn from_validated_weights(
        dictionary: &D,
        nfa: NFAChar,
        max_distance: u8,
        phonetic_weight: f64,
        edit_weight: f64,
    ) -> Self {
        let nfa = crate::phonetic_anchors::normalize_full_string_anchors(nfa);
        let product =
            ProductAutomatonChar::with_phonetic_weight(nfa, max_distance, phonetic_weight);
        let initial_frontier = product.initial_frontier();

        // Estimate max product states based on NFA size and max distance
        let max_product_states = estimated_phonetic_product_states(max_distance);

        let root = dictionary.root();
        let node_registry = DictionaryNodeRegistry::new(root);
        let product_state_registry = ProductStateRegistry::new(initial_frontier);

        Self {
            dictionary: dictionary.clone(),
            product: Arc::new(product),
            max_distance,
            phonetic_weight,
            edit_weight,
            max_product_states,
            node_registry: Arc::new(std::sync::RwLock::new(node_registry)),
            product_state_registry: Arc::new(std::sync::RwLock::new(product_state_registry)),
        }
    }

    /// Get the maximum edit distance.
    pub fn max_distance(&self) -> u8 {
        self.max_distance
    }

    /// Get the phonetic weight.
    pub fn phonetic_weight(&self) -> f64 {
        self.phonetic_weight
    }

    /// Get the edit distance weight multiplier.
    pub fn edit_weight(&self) -> f64 {
        self.edit_weight
    }

    pub(crate) fn is_valid_product_state(&self, state: StateId) -> bool {
        let Some((dict_node_id, product_state_id)) =
            state_encoding::decode(state, self.max_product_states)
        else {
            return false;
        };

        let node_is_registered = {
            let registry = crate::read_lock(&self.node_registry);
            registry.get_node(dict_node_id).is_some()
        };
        let product_state_is_registered = {
            let registry = crate::read_lock(&self.product_state_registry);
            registry.get_state(product_state_id).is_some()
        };

        node_is_registered && product_state_is_registered
    }

    pub(crate) fn final_weight_for_state(&self, state: StateId) -> Option<TropicalWeight> {
        let (dict_node_id, product_state_id) =
            state_encoding::decode(state, self.max_product_states)?;
        let node_registry = crate::read_lock(&self.node_registry);
        let product_registry = crate::read_lock(&self.product_state_registry);

        let dict_node = node_registry.get_node(dict_node_id)?;
        let product_frontier = product_registry.get_state(product_state_id)?;

        self.product_final_weight(dict_node, product_frontier.as_ref())
    }

    pub(crate) fn registered_state_id_span(&self) -> usize {
        let node_registry = crate::read_lock(&self.node_registry);
        let product_registry = crate::read_lock(&self.product_state_registry);
        crate::registered_product_state_id_span(
            node_registry.len(),
            usize::try_from(self.max_product_states).unwrap_or(usize::MAX),
            product_registry.len(),
        )
    }

    #[inline]
    fn encode_product_state(&self, dict_node_id: u32, product_state_id: u32) -> Option<StateId> {
        state_encoding::try_encode(dict_node_id, product_state_id, self.max_product_states)
    }

    /// Compute transitions for a product state.
    fn compute_transitions(
        &self,
        dict_node_id: u32,
        product_state_id: u32,
    ) -> (
        bool,
        TropicalWeight,
        SmallVec<[WeightedTransition<char, TropicalWeight>; 4]>,
    ) {
        let node_registry = crate::read_lock(&self.node_registry);
        let product_registry = crate::read_lock(&self.product_state_registry);

        let dict_node = match node_registry.get_node(dict_node_id) {
            Some(node) => node.clone(),
            None => {
                return (false, TropicalWeight::zero(), SmallVec::new());
            }
        };

        let product_frontier = match product_registry.get_state(product_state_id) {
            Some(frontier) => frontier,
            None => {
                return (false, TropicalWeight::zero(), SmallVec::new());
            }
        };

        drop(node_registry);
        drop(product_registry);

        let edge_capacity = crate::dictionary_edge_capacity(&dict_node, 1, 0);
        let mut transitions = edge_capacity.map_or_else(SmallVec::new, SmallVec::with_capacity);
        let Some(from_state) = self.encode_product_state(dict_node_id, product_state_id) else {
            return (false, TropicalWeight::zero(), transitions);
        };
        let mut pending_edges = edge_capacity.map_or_else(Vec::new, Vec::with_capacity);

        // For each dictionary edge, compute product transitions
        for (unit, child_node) in dict_node.edges() {
            let dict_char: char = unit.into();

            // Compute product automaton successors for this character using the same
            // deletion-closed frontier semantics as native ProductAutomatonChar trie
            // traversal. A single ProductStateChar misses deletion-closure states.
            let successors = self
                .product
                .transition_frontier(product_frontier.as_ref(), dict_char);

            if successors.is_empty() {
                continue;
            }

            let child_key = DictionaryNodeKey::child(dict_node_id, dict_char);
            let Some((successors, successor_key)) = ProductStateRegistry::prepare_state(successors)
            else {
                continue;
            };
            pending_edges.push((dict_char, child_node, child_key, successors, successor_key));
        }

        if !pending_edges.is_empty() {
            let mut node_registry = crate::write_lock(&self.node_registry);
            let mut product_registry = crate::write_lock(&self.product_state_registry);

            for (dict_char, child_node, child_key, successors, successor_key) in pending_edges {
                let Some(candidate_child_id) = node_registry.candidate_id(child_key) else {
                    continue;
                };
                let Some(candidate_successor_id) =
                    product_registry.candidate_state_id_for_key(&successor_key)
                else {
                    continue;
                };
                if self
                    .encode_product_state(candidate_child_id, candidate_successor_id)
                    .is_none()
                {
                    continue;
                }

                let Some(child_node_id) = node_registry.register_node(child_node, child_key) else {
                    continue;
                };
                let Some(successor_id) =
                    product_registry.register_prepared_state_with_key(successors, successor_key)
                else {
                    continue;
                };

                if let Some(target_state) = self.encode_product_state(child_node_id, successor_id) {
                    transitions.push(WeightedTransition::new(
                        from_state,
                        Some(dict_char),
                        Some(dict_char),
                        target_state,
                        TropicalWeight::new(self.phonetic_weight),
                    ));
                }
            }
        }

        let accepted_final_weight =
            self.product_final_weight(&dict_node, product_frontier.as_ref());
        let is_final = accepted_final_weight.is_some();
        let final_weight = accepted_final_weight.unwrap_or_else(TropicalWeight::zero);

        (is_final, final_weight, transitions)
    }

    fn product_final_weight(
        &self,
        dict_node: &D::Node,
        product_frontier: &[ProductStateChar],
    ) -> Option<TropicalWeight> {
        if !dict_node.is_final() {
            return None;
        }

        self.product
            .min_accepting_distance(product_frontier)
            .map(|distance| TropicalWeight::new(f64::from(distance) * self.edit_weight))
    }
}

#[cfg(feature = "phonetic-rules")]
impl<D> DirectStateSource<char, TropicalWeight> for PhoneticStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    fn expand_state(&self, state: StateId) -> StateExpansion<char, TropicalWeight> {
        let Some((dict_node_id, product_state_id)) =
            state_encoding::decode(state, self.max_product_states)
        else {
            return StateExpansion::failed(ExpansionFailure::invalid_state(state));
        };

        let (is_final, final_weight, transitions) =
            self.compute_transitions(dict_node_id, product_state_id);

        if is_final {
            StateExpansion::final_state(final_weight, transitions)
        } else {
            StateExpansion::non_final(transitions)
        }
    }
}

#[cfg(feature = "phonetic-rules")]
impl<D> StateSource<char, TropicalWeight> for PhoneticStateSource<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
    <D::Node as DictionaryNode>::Unit: Into<char> + TryFrom<char> + Copy + Send + Sync,
{
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<char, TropicalWeight> {
        fulfill_expansion_request(self, request)
    }

    fn start(&self) -> StateId {
        // Start state is (root_node=0, initial_product_state=0).
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        let node_registry = crate::read_lock(&self.node_registry);
        let product_registry = crate::read_lock(&self.product_state_registry);
        let states_per_dictionary_node =
            usize::try_from(self.max_product_states).unwrap_or(usize::MAX);
        let live_state_id_span = crate::registered_product_state_id_span(
            node_registry.len(),
            states_per_dictionary_node,
            product_registry.len(),
        );

        crate::dictionary_product_states_hint(
            self.dictionary
                .len()
                .map(|dict_size| dict_size.max(node_registry.len())),
            states_per_dictionary_node,
        )
        .map(|estimated_state_count| estimated_state_count.max(live_state_id_span))
    }
}

#[cfg(test)]
#[cfg(feature = "phonetic-rules")]
mod tests {
    use super::*;
    use crate::phonetic_state_support::{next_phonetic_registry_id, ProductStateKeyBytes};
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
    use liblevenshtein::phonetic::nfa::compiler::compile;
    use liblevenshtein::phonetic::regex::parse;

    #[test]
    fn test_phonetic_registry_id_helper_rejects_unrepresentable_len() {
        assert_eq!(next_phonetic_registry_id(0), Some(0));
        let Ok(max_u32) = usize::try_from(u32::MAX) else {
            return;
        };
        assert_eq!(next_phonetic_registry_id(max_u32), Some(u32::MAX));

        if let Some(overflowing_len) = max_u32.checked_add(1) {
            assert_eq!(next_phonetic_registry_id(overflowing_len), None);
        }
    }

    #[test]
    fn test_phonetic_product_state_estimate_has_floor_and_scales() {
        assert_eq!(estimated_phonetic_product_states(0), 10_000);
        assert_eq!(estimated_phonetic_product_states(9), 10_000);
        assert_eq!(estimated_phonetic_product_states(10), 11_000);
        assert_eq!(estimated_phonetic_product_states(u8::MAX), 256_000);
    }

    #[test]
    fn test_product_state_key_uses_product_cost_bucket() {
        let same_bucket = vec![ProductStateChar {
            nfa_states: vec![1],
            accumulated_cost: 0.500_000_499_90,
        }];
        let same_bucket_roundoff = vec![ProductStateChar {
            nfa_states: vec![1],
            accumulated_cost: 0.500_000_499_91,
        }];
        let adjacent_bucket = vec![ProductStateChar {
            nfa_states: vec![1],
            accumulated_cost: 0.500_000_500_40,
        }];

        assert!(
            ProductStateRegistry::state_to_key(&same_bucket)
                == ProductStateRegistry::state_to_key(&same_bucket_roundoff)
        );
        assert!(
            ProductStateRegistry::state_to_key(&same_bucket)
                != ProductStateRegistry::state_to_key(&adjacent_bucket)
        );
    }

    #[test]
    fn test_product_state_key_fast_paths_empty_and_singleton_frontiers() {
        let empty_key =
            ProductStateRegistry::state_to_key(&[]).expect("empty frontier should encode");
        assert!(empty_key.0.is_empty());

        let singleton = vec![ProductStateChar {
            nfa_states: vec![7, 11],
            accumulated_cost: 0.75,
        }];
        let singleton_key =
            ProductStateRegistry::state_to_key(&singleton).expect("singleton should encode");

        let mut expected = ProductStateKeyBytes::new();
        expected.extend_from_slice(&2u32.to_le_bytes());
        expected.extend_from_slice(&7u32.to_le_bytes());
        expected.extend_from_slice(&11u32.to_le_bytes());
        expected.extend_from_slice(&750_000i64.to_le_bytes());

        assert_eq!(singleton_key.0, expected);
        assert!(!singleton_key.0.spilled());
    }

    #[test]
    fn test_product_state_key_canonicalizes_public_nfa_state_vectors() {
        let noncanonical = vec![ProductStateChar {
            nfa_states: vec![2, 1, 2],
            accumulated_cost: 0.25,
        }];
        let canonical = vec![ProductStateChar {
            nfa_states: vec![1, 2],
            accumulated_cost: 0.25,
        }];

        assert_eq!(
            ProductStateRegistry::state_to_key(&noncanonical),
            ProductStateRegistry::state_to_key(&canonical)
        );

        let mut registry = ProductStateRegistry::new(noncanonical);
        assert_eq!(registry.register_state(canonical), Some(0));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_product_state_key_deduplicates_equivalent_frontier_entries() {
        let duplicate_frontier = vec![
            ProductStateChar {
                nfa_states: vec![2, 1, 2],
                accumulated_cost: 0.500_000_499_90,
            },
            ProductStateChar {
                nfa_states: vec![1, 2],
                accumulated_cost: 0.500_000_499_91,
            },
            ProductStateChar {
                nfa_states: vec![4],
                accumulated_cost: 1.0,
            },
        ];
        let canonical_frontier = vec![
            ProductStateChar {
                nfa_states: vec![1, 2],
                accumulated_cost: 0.500_000_499_90,
            },
            ProductStateChar {
                nfa_states: vec![4],
                accumulated_cost: 1.0,
            },
        ];

        let duplicate_key =
            ProductStateRegistry::state_to_key(&duplicate_frontier).expect("duplicates encode");
        let canonical_key =
            ProductStateRegistry::state_to_key(&canonical_frontier).expect("canonical encodes");

        assert_eq!(duplicate_key, canonical_key);
        assert!(!duplicate_key.0.spilled());

        let mut registry = ProductStateRegistry::new(duplicate_frontier);
        assert_eq!(registry.register_state(canonical_frontier), Some(0));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_product_state_key_is_order_independent_and_inline_for_small_frontiers() {
        let ordered = vec![
            ProductStateChar {
                nfa_states: vec![2],
                accumulated_cost: 0.5,
            },
            ProductStateChar {
                nfa_states: vec![1, 3],
                accumulated_cost: 0.25,
            },
        ];
        let reversed = vec![ordered[1].clone(), ordered[0].clone()];

        let ordered_key =
            ProductStateRegistry::state_to_key(&ordered).expect("ordered key should encode");
        let reversed_key =
            ProductStateRegistry::state_to_key(&reversed).expect("reversed key should encode");

        assert_eq!(ordered_key, reversed_key);
        assert!(!ordered_key.0.spilled());
    }

    #[test]
    fn test_product_state_key_order_uses_product_cost_bucket() {
        let left = vec![
            ProductStateChar {
                nfa_states: vec![2],
                accumulated_cost: 0.500_000_499_91,
            },
            ProductStateChar {
                nfa_states: vec![1],
                accumulated_cost: 0.500_000_499_90,
            },
        ];
        let right = vec![
            ProductStateChar {
                nfa_states: vec![1],
                accumulated_cost: 0.500_000_499_91,
            },
            ProductStateChar {
                nfa_states: vec![2],
                accumulated_cost: 0.500_000_499_90,
            },
        ];

        assert_eq!(
            ProductStateRegistry::state_to_key(&left),
            ProductStateRegistry::state_to_key(&right)
        );
    }

    #[test]
    fn test_product_state_registry_reuses_shared_frontier_storage() {
        let frontier = vec![ProductStateChar {
            nfa_states: vec![1, 2],
            accumulated_cost: 0.25,
        }];
        let registry = ProductStateRegistry::new(frontier);

        let first = registry
            .get_state(0)
            .expect("initial frontier should be registered");
        let second = registry
            .get_state(0)
            .expect("initial frontier should still be registered");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first[0].nfa_states, vec![1, 2]);
        assert_eq!(first[0].accumulated_cost, 0.25);
    }

    #[test]
    fn test_product_state_registry_stores_canonical_frontier() {
        let frontier = vec![
            ProductStateChar {
                nfa_states: vec![2, 1, 2],
                accumulated_cost: 0.500_000_499_91,
            },
            ProductStateChar {
                nfa_states: vec![4],
                accumulated_cost: 1.0,
            },
            ProductStateChar {
                nfa_states: vec![1, 2],
                accumulated_cost: 0.500_000_499_90,
            },
        ];
        let registry = ProductStateRegistry::new(frontier);

        let stored = registry
            .get_state(0)
            .expect("initial frontier should be registered");

        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].nfa_states, vec![1, 2]);
        assert_eq!(stored[0].accumulated_cost, 0.500_000_499_90);
        assert_eq!(stored[1].nfa_states, vec![4]);
        assert_eq!(stored[1].accumulated_cost, 1.0);
    }

    #[test]
    fn test_product_state_registry_with_key_registration_canonicalizes_frontier() {
        let mut registry = ProductStateRegistry::new(vec![ProductStateChar {
            nfa_states: vec![0],
            accumulated_cost: 0.0,
        }]);
        let frontier = vec![
            ProductStateChar {
                nfa_states: vec![3, 1, 3],
                accumulated_cost: 0.25,
            },
            ProductStateChar {
                nfa_states: vec![1, 3],
                accumulated_cost: 0.25,
            },
        ];
        let key = ProductStateRegistry::state_to_key(&frontier).expect("frontier should encode");

        let id = registry
            .register_state_with_key(frontier, key)
            .expect("frontier should register");
        let stored = registry
            .get_state(id)
            .expect("registered frontier should be retrievable");

        assert_eq!(id, 1);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].nfa_states, vec![1, 3]);
        assert_eq!(stored[0].accumulated_cost, 0.25);
    }

    #[test]
    fn test_phonetic_state_source_hint_covers_registered_state_id_span() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["abcd"]);
        let nfa = compile(&parse("a").expect("parse")).expect("compile");
        let source = PhoneticStateSource::new(&dict, nfa, 0);

        assert_eq!(
            crate::registered_dictionary_product_states_hint(
                Some(1),
                2,
                crate::MAX_NUM_STATES_HINT + 1
            ),
            Some(2usize.saturating_mul(crate::MAX_NUM_STATES_HINT + 1))
        );

        let _ = source.expand_state(source.start());

        let registered_nodes = crate::read_lock(&source.node_registry).len();
        let states_per_dictionary_node =
            usize::try_from(source.max_product_states).unwrap_or(usize::MAX);
        let live_state_id_span = source.registered_state_id_span();
        let expected_hint = crate::dictionary_product_states_hint(
            Some(dict.len().unwrap_or(0).max(registered_nodes)),
            states_per_dictionary_node,
        )
        .map(|state_count| state_count.max(live_state_id_span));

        assert!(registered_nodes > dict.len().unwrap_or(0));
        assert!(live_state_id_span >= registered_nodes);
        assert_eq!(
            StateSource::<char, TropicalWeight>::num_states_hint(&source),
            expected_hint
        );
    }

    #[test]
    fn test_phonetic_state_source_start_state_encoding() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["test"]);
        let nfa = compile(&parse("test").expect("parse")).expect("compile");
        let source = PhoneticStateSource::new(&dict, nfa, 1);

        let start = source.start();
        let (dict_node, product_state) =
            state_encoding::decode(start, source.max_product_states).expect("start state decodes");
        assert_eq!(dict_node, 0);
        assert_eq!(product_state, 0);
    }

    #[test]
    fn test_phonetic_state_source_prunes_unencodable_targets() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["a"]);
        let nfa = compile(&parse("a").expect("parse")).expect("compile");
        let mut source = PhoneticStateSource::new(&dict, nfa, 1);
        source.max_product_states = 1;

        assert_eq!(registered_dictionary_node_count(&source), 1);
        assert_eq!(crate::read_lock(&source.product_state_registry).len(), 1);

        match source.expand_state(source.start()) {
            StateExpansion::Expanded { transitions, .. } => {
                assert!(transitions.is_empty());
            }
            StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
            StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
        }

        assert_eq!(registered_dictionary_node_count(&source), 1);
        assert_eq!(crate::read_lock(&source.product_state_registry).len(), 1);
    }

    #[test]
    fn test_phonetic_state_source_does_not_register_pruned_dictionary_edges() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["b"]);
        let nfa = compile(&parse("a").expect("parse")).expect("compile");
        let source = PhoneticStateSource::new(&dict, nfa, 0);

        assert_eq!(registered_dictionary_node_count(&source), 1);

        match source.expand_state(source.start()) {
            StateExpansion::Expanded { transitions, .. } => {
                assert!(
                    !transitions
                        .iter()
                        .any(|transition| transition.output == Some('b')),
                    "mismatched dictionary edge must be pruned at distance zero"
                );
            }
            StateExpansion::Failed(failure) => panic!("state expansion failed: {failure}"),
            StateExpansion::Cancelled(reason) => panic!("state expansion cancelled: {reason:?}"),
        }

        assert_eq!(registered_dictionary_node_count(&source), 1);
    }

    fn registered_dictionary_node_count(
        source: &PhoneticStateSource<DynamicDawgChar<()>>,
    ) -> usize {
        let registry = crate::read_lock(&source.node_registry);
        let mut count = 0usize;
        while registry.get_node(count as u32).is_some() {
            count += 1;
        }
        count
    }
}
