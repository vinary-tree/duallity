use duallity::{GeneralizedWfst, LazyWfst, WallBreakerWfst, Wfst};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use libdictenstein::scdawg::Scdawg;
use liblevenshtein::transducer::OperationSet;
use lling_llang::wfst::CachePolicy;

#[test]
fn generalized_lru_zero_uses_tunable_default_bound() {
    let dict = DynamicDawgChar::<()>::from_terms(vec!["ab"]);
    let mut wfst = GeneralizedWfst::new(&dict, "ab", 1, OperationSet::standard());
    wfst.set_max_cache_size(2);
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 0 });

    let start = Wfst::start(&wfst);
    let next = wfst
        .transitions_lazy(start)
        .first()
        .expect("expected generalized start transition")
        .to;
    wfst.expand(next).expect("valid state expands");

    assert_eq!(wfst.computed_states(), 2);

    wfst.set_max_cache_size(1);

    assert_eq!(wfst.computed_states(), 1);
}

#[test]
fn wallbreaker_lru_zero_uses_tunable_default_bound() {
    let dict = Scdawg::<()>::from_terms(vec!["ab"]);
    let mut wfst = WallBreakerWfst::new(&dict, "ab", 0);
    wfst.set_max_cache_size(2);
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 0 });

    let start = Wfst::start(&wfst);
    let next = wfst
        .transitions_lazy(start)
        .first()
        .expect("expected WallBreaker start transition")
        .to;
    wfst.expand(next).expect("valid state expands");

    assert_eq!(wfst.computed_states(), 2);

    wfst.set_max_cache_size(1);

    assert_eq!(wfst.computed_states(), 1);
}
