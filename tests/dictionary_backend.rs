use duallity::{DictionaryBackend, VocabId};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use lling_llang::backend::LatticeBackend;

#[test]
fn dictionary_backend_interns_unicode_terms_in_insertion_order() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["hello", "éclair", "世界"]);
    let mut backend = DictionaryBackend::new(dict);

    let hello = backend.intern("hello");
    let eclair = backend.intern("éclair");
    let world = backend.intern("世界");

    assert_eq!(hello, 0);
    assert_eq!(eclair, 1);
    assert_eq!(world, 2);
    assert_eq!(backend.intern("hello"), hello);
    assert_eq!(backend.vocab_size(), 3);
    assert_eq!(
        backend.iter().collect::<Vec<_>>(),
        vec![(0, "hello"), (1, "éclair"), (2, "世界")]
    );
}

#[test]
fn dictionary_backend_contains_cache_union_underlying_dictionary() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["known"]);
    let mut backend = DictionaryBackend::new(dict);

    assert!(backend.contains("known"));
    assert!(!backend.contains("ad-hoc"));
    assert_eq!(backend.get_id("known"), None);

    let ad_hoc = backend.intern("ad-hoc");

    assert!(backend.contains("ad-hoc"));
    assert_eq!(backend.get_id("ad-hoc"), Some(ad_hoc));
    assert_eq!(backend.lookup(ad_hoc), Some("ad-hoc"));
}

#[test]
fn dictionary_backend_reserved_sentinel_is_never_a_lookup_result() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["term"]);
    let mut backend = DictionaryBackend::new(dict);
    let id = backend.intern("term");

    assert_ne!(
        id,
        DictionaryBackend::<DynamicDawgChar<()>>::VOCAB_ID_EXHAUSTED
    );
    assert_eq!(
        DictionaryBackend::<DynamicDawgChar<()>>::VOCAB_ID_EXHAUSTED,
        VocabId::MAX
    );
    assert_eq!(
        backend.lookup(DictionaryBackend::<DynamicDawgChar<()>>::VOCAB_ID_EXHAUSTED),
        None
    );
}

#[test]
fn dictionary_backend_with_vocabulary_deduplicates_terms_and_preserves_first_ids() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a", "b", "é"]);
    let backend = DictionaryBackend::with_vocabulary(
        dict,
        ["b", "a", "b", "é"].into_iter().map(str::to_owned),
    );

    assert_eq!(backend.vocab_size(), 3);
    assert_eq!(backend.get_id("b"), Some(0));
    assert_eq!(backend.get_id("a"), Some(1));
    assert_eq!(backend.get_id("é"), Some(2));
    assert_eq!(
        backend.iter().collect::<Vec<_>>(),
        vec![(0, "b"), (1, "a"), (2, "é")]
    );
}

#[test]
fn dictionary_backend_clone_has_independent_lazy_vocabulary() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["a", "b", "c"]);
    let mut original = DictionaryBackend::new(dict);
    let a = original.intern("a");
    let mut cloned = original.clone();

    let original_b = original.intern("b");
    let cloned_c = cloned.intern("c");

    assert_eq!(a, 0);
    assert_eq!(original_b, 1);
    assert_eq!(cloned_c, 1);
    assert_eq!(original.get_id("c"), None);
    assert_eq!(cloned.get_id("b"), None);
    assert_eq!(original.lookup(original_b), Some("b"));
    assert_eq!(cloned.lookup(cloned_c), Some("c"));
}
