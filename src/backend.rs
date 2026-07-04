//! Dictionary to LatticeBackend adapter.
//!
//! This module provides [`DictionaryBackend`], which adapts a liblevenshtein
//! dictionary to lling-llang's [`LatticeBackend`] trait.

use std::sync::Arc;

use lling_llang::backend::{LatticeBackend, VocabId};
use rustc_hash::FxHashMap;

use libdictenstein::Dictionary;

use crate::fx_hash_map_with_capacity;

const VOCAB_ID_EXHAUSTED: VocabId = VocabId::MAX;

#[inline]
fn next_vocab_id(len: usize) -> Option<VocabId> {
    let id = VocabId::try_from(len).ok()?;
    (id < VOCAB_ID_EXHAUSTED).then_some(id)
}

#[inline]
fn usize_from_vocab_id(id: VocabId) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}

#[inline]
fn vocab_capacity_limit() -> usize {
    usize_from_vocab_id(VOCAB_ID_EXHAUSTED)
}

/// Adapter that exposes a liblevenshtein dictionary as a lling-llang `LatticeBackend`.
///
/// This allows liblevenshtein's efficient dictionary structures (DoubleArrayTrie,
/// DynamicDawg, etc.) to be used directly with lling-llang's lattice infrastructure.
///
/// # Vocabulary Management
///
/// The adapter maintains a bidirectional mapping between:
/// - lling-llang's `VocabId` (sequential u32 indices)
/// - Dictionary terms (strings in the dictionary)
///
/// Terms are interned lazily as they are accessed.
///
/// # Example
///
/// ```rust,no_run
/// use duallity::DictionaryBackend;
/// use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
/// use lling_llang::prelude::*;
///
/// // Create a dictionary
/// let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "help", "world"]);
///
/// // Wrap as LatticeBackend
/// let backend = DictionaryBackend::new(dict);
///
/// // Use with lling-llang's LatticeBuilder
/// let mut builder = LatticeBuilder::<TropicalWeight, _>::new(backend);
/// ```
#[derive(Clone)]
pub struct DictionaryBackend<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
{
    /// The underlying dictionary
    dictionary: D,
    /// Forward mapping: word -> VocabId
    word_to_id: FxHashMap<Arc<str>, VocabId>,
    /// Reverse mapping: VocabId -> word
    id_to_word: Vec<Arc<str>>,
}

impl<D> DictionaryBackend<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
{
    /// `VocabId` returned by the infallible [`LatticeBackend::intern`] adapter
    /// when the u32 vocabulary space is exhausted.
    ///
    /// The backend never assigns this value to a word, so
    /// [`LatticeBackend::lookup`] returns `None` for it. Prefer
    /// [`Self::try_intern`] when callers need to distinguish exhaustion from a
    /// successfully interned term.
    pub const VOCAB_ID_EXHAUSTED: VocabId = VOCAB_ID_EXHAUSTED;

    /// Create a new dictionary backend from an existing dictionary.
    ///
    /// The vocabulary is initially empty and will be populated lazily
    /// as terms are interned.
    pub fn new(dictionary: D) -> Self {
        Self::with_vocabulary_capacity(dictionary, 0)
    }

    fn with_vocabulary_capacity(dictionary: D, capacity: usize) -> Self {
        let capacity = capacity.min(vocab_capacity_limit());
        Self {
            dictionary,
            word_to_id: fx_hash_map_with_capacity(capacity),
            id_to_word: Vec::with_capacity(capacity),
        }
    }

    /// Create a dictionary backend with pre-populated vocabulary.
    ///
    /// This iterates over all terms in the dictionary and interns them
    /// upfront. Useful when you need stable VocabIds from the start.
    ///
    /// # Note
    ///
    /// This requires iterating over the entire dictionary, which may be
    /// expensive for large dictionaries. Use `new()` for lazy population. If
    /// the vocabulary space is exhausted, population stops before assigning the
    /// reserved [`Self::VOCAB_ID_EXHAUSTED`] sentinel.
    pub fn with_vocabulary<I>(dictionary: D, terms: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let terms = terms.into_iter();
        let (lower_bound, upper_bound) = terms.size_hint();
        let reserve_hint = crate::capped_size_hint_capacity(upper_bound.unwrap_or(lower_bound));
        let mut backend = Self::with_vocabulary_capacity(dictionary, reserve_hint);

        for term in terms {
            if backend.try_intern(&term).is_none() {
                break;
            }
        }
        backend
    }

    /// Create a dictionary backend with pre-populated vocabulary, reporting
    /// exhaustion instead of silently returning the sentinel ID.
    pub fn try_with_vocabulary<I>(dictionary: D, terms: I) -> Option<Self>
    where
        I: IntoIterator<Item = String>,
    {
        let terms = terms.into_iter();
        let (lower_bound, upper_bound) = terms.size_hint();
        let reserve_hint = crate::capped_size_hint_capacity(upper_bound.unwrap_or(lower_bound));
        let mut backend = Self::with_vocabulary_capacity(dictionary, reserve_hint);

        for term in terms {
            backend.try_intern(&term)?;
        }
        Some(backend)
    }

    /// Get the underlying dictionary.
    pub fn dictionary(&self) -> &D {
        &self.dictionary
    }

    /// Get mutable access to the underlying dictionary.
    ///
    /// Note: Modifying the dictionary after vocabulary has been built
    /// may cause inconsistencies if terms are removed.
    pub fn dictionary_mut(&mut self) -> &mut D {
        &mut self.dictionary
    }

    /// Take ownership of the underlying dictionary.
    pub fn into_dictionary(self) -> D {
        self.dictionary
    }

    /// Intern a word, returning `None` if the backend has exhausted its
    /// representable vocabulary IDs.
    pub fn try_intern(&mut self, word: &str) -> Option<VocabId> {
        if let Some(&id) = self.word_to_id.get(word) {
            return Some(id);
        }

        let id = next_vocab_id(self.id_to_word.len())?;
        let word_arc: Arc<str> = word.into();

        self.word_to_id.insert(word_arc.clone(), id);
        self.id_to_word.push(word_arc);

        Some(id)
    }

    /// Return whether a new word can still receive a representable `VocabId`.
    pub fn has_vocab_capacity(&self) -> bool {
        next_vocab_id(self.id_to_word.len()).is_some()
    }
}

impl<D> LatticeBackend for DictionaryBackend<D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: Send + Sync,
{
    fn intern(&mut self, word: &str) -> VocabId {
        self.try_intern(word).unwrap_or(Self::VOCAB_ID_EXHAUSTED)
    }

    fn lookup(&self, id: VocabId) -> Option<&str> {
        self.id_to_word
            .get(usize_from_vocab_id(id))
            .map(|s| s.as_ref())
    }

    fn vocab_size(&self) -> usize {
        self.id_to_word.len()
    }

    fn contains(&self, word: &str) -> bool {
        // Check both our cache and the dictionary
        self.word_to_id.contains_key(word) || self.dictionary.contains(word)
    }

    fn get_id(&self, word: &str) -> Option<VocabId> {
        self.word_to_id.get(word).copied()
    }

    fn iter(&self) -> impl Iterator<Item = (VocabId, &str)> {
        self.id_to_word
            .iter()
            .enumerate()
            .filter_map(|(i, s)| next_vocab_id(i).map(|id| (id, s.as_ref())))
    }

    fn supports_sharing(&self) -> bool {
        // Dictionary backends can support structural sharing
        // depending on the underlying dictionary type
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libdictenstein::dynamic_dawg::char::DynamicDawgChar;

    struct InflatedTermHint {
        terms: std::vec::IntoIter<String>,
    }

    impl InflatedTermHint {
        fn new(terms: Vec<String>) -> Self {
            Self {
                terms: terms.into_iter(),
            }
        }
    }

    impl Iterator for InflatedTermHint {
        type Item = String;

        fn next(&mut self) -> Option<Self::Item> {
            self.terms.next()
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (usize::MAX, None)
        }
    }

    #[test]
    fn test_next_vocab_id_reserves_exhaustion_sentinel() {
        assert_eq!(next_vocab_id(0), Some(0));

        let Ok(max_vocab_id) = usize::try_from(VocabId::MAX) else {
            return;
        };
        assert_eq!(next_vocab_id(max_vocab_id), None);

        if let Some(last_assignable) = max_vocab_id.checked_sub(1) {
            assert_eq!(next_vocab_id(last_assignable), Some(VocabId::MAX - 1));
        }
    }

    #[test]
    fn test_dictionary_backend_new() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let backend = DictionaryBackend::new(dict);

        assert_eq!(backend.vocab_size(), 0); // Lazy - nothing interned yet
        assert!(backend.has_vocab_capacity());
    }

    #[test]
    fn test_dictionary_backend_intern() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let mut backend = DictionaryBackend::new(dict);

        let id1 = backend.intern("hello");
        let id2 = backend.intern("world");
        let id3 = backend.intern("hello"); // duplicate

        assert_eq!(id1, id3); // Same word, same ID
        assert_ne!(id1, id2); // Different words, different IDs
        assert_eq!(backend.vocab_size(), 2);
        assert_eq!(backend.try_intern("hello"), Some(id1));
    }

    #[test]
    fn test_dictionary_backend_lookup() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let mut backend = DictionaryBackend::new(dict);

        let id = backend.intern("hello");
        assert_eq!(backend.lookup(id), Some("hello"));
        assert_eq!(backend.lookup(999), None);
    }

    #[test]
    fn test_dictionary_backend_contains() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let backend = DictionaryBackend::new(dict);

        // Contains checks the underlying dictionary
        assert!(backend.contains("hello"));
        assert!(backend.contains("world"));
        assert!(!backend.contains("missing"));
    }

    #[test]
    fn test_dictionary_backend_get_id() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let mut backend = DictionaryBackend::new(dict);

        // Not interned yet
        assert_eq!(backend.get_id("hello"), None);

        // After interning
        let id = backend.intern("hello");
        assert_eq!(backend.get_id("hello"), Some(id));
    }

    #[test]
    fn test_dictionary_backend_iter() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let mut backend = DictionaryBackend::new(dict);

        backend.intern("hello");
        backend.intern("world");

        let entries: Vec<_> = backend.iter().collect();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_dictionary_backend_with_vocabulary() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world", "test"]);
        let terms = vec!["hello".to_string(), "world".to_string()];
        let backend = DictionaryBackend::with_vocabulary(dict, terms);

        assert_eq!(backend.vocab_size(), 2);
        assert!(backend.get_id("hello").is_some());
        assert!(backend.get_id("world").is_some());
        assert_eq!(backend.get_id("test"), None); // Not pre-populated
    }

    #[test]
    fn test_dictionary_backend_try_with_vocabulary() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let terms = vec!["hello".to_string(), "world".to_string()];
        let backend = DictionaryBackend::try_with_vocabulary(dict, terms)
            .expect("small vocabulary should fit");

        assert_eq!(backend.vocab_size(), 2);
        assert_ne!(
            backend.get_id("hello"),
            Some(DictionaryBackend::<DynamicDawgChar<()>>::VOCAB_ID_EXHAUSTED)
        );
    }

    #[test]
    fn test_dictionary_backend_treats_size_hint_as_speculative() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let terms = InflatedTermHint::new(vec!["hello".to_string(), "world".to_string()]);
        let backend = DictionaryBackend::with_vocabulary(dict, terms);

        assert_eq!(backend.vocab_size(), 2);
        assert!(backend.get_id("hello").is_some());
        assert!(backend.get_id("world").is_some());

        let dict = DynamicDawgChar::<()>::from_terms(vec!["again"]);
        let terms = InflatedTermHint::new(vec!["again".to_string()]);
        let backend = DictionaryBackend::try_with_vocabulary(dict, terms)
            .expect("inflated hint should not prevent a representable vocabulary");

        assert_eq!(backend.vocab_size(), 1);
        assert!(backend.get_id("again").is_some());
    }

    #[test]
    fn test_dictionary_backend_clone() {
        let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "world"]);
        let mut backend = DictionaryBackend::new(dict);
        backend.intern("hello");

        let cloned = backend.clone();
        assert_eq!(cloned.vocab_size(), 1);
        assert_eq!(cloned.get_id("hello"), backend.get_id("hello"));
    }
}
