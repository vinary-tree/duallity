use duallity::{
    CommonPhoneticRules, LazyState, LazyWfst, RewriteRule, RewriteWfst, Semiring, StateSource,
    TropicalWeight, Wfst,
};
use lling_llang::wfst::CachePolicy;

#[test]
fn rewrite_wfst_creation_starts_without_rules() {
    let wfst = RewriteWfst::new();

    assert_eq!(wfst.num_rules(), 0);
}

#[test]
fn rewrite_wfst_without_rules_and_identity_still_has_final_start_state() {
    let mut wfst = RewriteWfst::new();
    wfst.set_allow_identity(false);

    let start = Wfst::start(&wfst);

    assert_eq!(wfst.num_rules(), 0);
    assert_eq!(wfst.num_states(), 1);
    assert!(!wfst.is_empty());
    assert!(wfst.is_valid_state(start));
    assert!(wfst.is_final(start));
    assert_eq!(wfst.final_weight(start), TropicalWeight::one());
    assert!(wfst.transitions_lazy(start).is_empty());
}

#[test]
fn rewrite_wfst_add_rule_updates_rule_count() {
    let mut wfst = RewriteWfst::new();
    wfst.add_rule("ph", "f", 0.1).expect("valid rewrite rule");
    wfst.add_rule("c", "s", 0.2).expect("valid rewrite rule");

    assert_eq!(wfst.num_rules(), 2);
}

#[test]
fn rewrite_wfst_with_rules_prepares_rules() {
    let rules = vec![
        RewriteRule::with_cost("ph", "f", 0.1).expect("valid rewrite rule"),
        RewriteRule::with_cost("ck", "k", 0.1).expect("valid rewrite rule"),
    ];
    let wfst = RewriteWfst::with_rules(rules).expect("valid rewrite rules");

    assert_eq!(wfst.num_rules(), 2);
}

#[test]
fn rewrite_wfst_priority_order_is_used_when_expanding() {
    let rules = vec![
        RewriteRule::with_cost("a", "x", 0.1).expect("valid rewrite rule"),
        RewriteRule::with_cost("a", "y", 0.1)
            .expect("valid rewrite rule")
            .with_priority(10),
        RewriteRule::with_cost("a", "z", 0.1)
            .expect("valid rewrite rule")
            .with_priority(10),
    ];
    let mut wfst = RewriteWfst::with_rules(rules).expect("valid rewrite rules");
    wfst.set_allow_identity(false);
    wfst.expand(0);

    let outputs = wfst
        .transitions(0)
        .iter()
        .map(|transition| transition.output)
        .collect::<Vec<_>>();
    assert_eq!(outputs, vec![Some('y'), Some('z'), Some('x')]);
}

#[test]
fn rewrite_wfst_add_rewrite_rule_refreshes_prepared_metadata() {
    let mut wfst = RewriteWfst::with_rules(vec![
        RewriteRule::with_cost("a", "x", 0.1).expect("valid rewrite rule")
    ])
    .expect("valid rewrite rules");
    wfst.add_rewrite_rule(
        RewriteRule::with_cost("a", "y", 0.1)
            .expect("valid rewrite rule")
            .with_priority(10),
    )
    .expect("valid rewrite rule");
    wfst.set_allow_identity(false);
    wfst.expand(0);

    assert_eq!(
        wfst.transitions(0)
            .first()
            .map(|transition| transition.output),
        Some(Some('y'))
    );
}

#[test]
fn rewrite_rule_rejects_negative_cost() {
    let error = match RewriteRule::with_cost("ph", "f", -0.1) {
        Ok(_) => std::panic::panic_any("negative rewrite cost should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.name(), "rewrite rule cost");
    assert_eq!(error.value(), -0.1);
}

#[test]
fn rewrite_wfst_rejects_manually_constructed_nan_cost() {
    let rule = RewriteRule {
        input: "ph".to_string(),
        output: "f".to_string(),
        cost: f64::NAN,
        priority: 0,
    };

    let error = match RewriteWfst::with_rules(vec![rule]) {
        Ok(_) => std::panic::panic_any("NaN rewrite cost should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.name(), "rewrite rule cost");
    assert!(error.value().is_nan());
}

#[test]
fn rewrite_wfst_start_state_is_zero() {
    let wfst = RewriteWfst::new();

    assert_eq!(Wfst::start(&wfst), 0);
}

#[test]
fn rewrite_wfst_expand_materializes_start_state() {
    let mut wfst = RewriteWfst::new();
    wfst.add_rule("a", "b", 0.1).expect("valid rewrite rule");

    assert!(!wfst.is_expanded(0));
    wfst.expand(0);
    assert!(wfst.is_expanded(0));
}

#[test]
fn rewrite_wfst_rejects_invalid_state_without_caching() {
    let mut wfst = RewriteWfst::new();
    let invalid_state = 1;

    assert!(!wfst.is_valid_state(invalid_state));
    wfst.expand(invalid_state);

    assert_eq!(wfst.computed_states(), 0);
    assert!(wfst.transitions_lazy(invalid_state).is_empty());
    assert_eq!(wfst.computed_states(), 0);
}

#[test]
fn rewrite_wfst_statesource_rejects_invalid_state() {
    let source = RewriteWfst::new();

    match StateSource::<char, TropicalWeight>::compute_state(&source, 1) {
        LazyState::Computed {
            is_final,
            final_weight,
            transitions,
        } => {
            assert!(!is_final);
            assert_eq!(final_weight, TropicalWeight::zero());
            assert!(transitions.is_empty());
        }
        LazyState::Pending => {
            std::panic::panic_any("RewriteWfst StateSource should compute eagerly")
        }
    }
}

#[test]
fn rewrite_wfst_no_cache_policy_uses_scratch_only() {
    let mut wfst = RewriteWfst::new();
    wfst.add_rule("a", "b", 0.1).expect("valid rewrite rule");
    wfst.set_cache_policy(CachePolicy::NoCache);

    let transition_count = wfst.transitions_lazy(0).len();

    assert!(transition_count > 0);
    assert_eq!(wfst.computed_states(), 0);
    assert!(!wfst.is_expanded(0));
    assert_eq!(wfst.transitions(0).len(), transition_count);
    assert_eq!(wfst.total_transitions(), transition_count);
}

#[test]
fn rewrite_wfst_lru_policy_evicts_least_recently_used_state() {
    let mut wfst = RewriteWfst::new();
    wfst.set_allow_identity(false);
    wfst.add_rule("ab", "xy", 0.1).expect("valid rewrite rule");
    wfst.set_cache_policy(CachePolicy::Lru { max_states: 1 });

    let next = wfst
        .transitions_lazy(0)
        .first()
        .expect("expected first rewrite transition")
        .to;

    assert!(wfst.is_expanded(0));
    assert_eq!(wfst.computed_states(), 1);

    wfst.expand(next);

    assert_eq!(wfst.computed_states(), 1);
    assert!(!wfst.is_expanded(0));
    assert!(wfst.is_expanded(next));
}

#[test]
fn rewrite_wfst_transitions_include_rewrite_edges() {
    let mut wfst = RewriteWfst::new();
    wfst.add_rule("a", "b", 0.1).expect("valid rewrite rule");
    wfst.expand(0);

    let transition = wfst
        .transitions(0)
        .iter()
        .find(|transition| transition.input == Some('a'))
        .expect("expected rewrite transition for a");
    assert_eq!(transition.output, Some('b'));
}

#[test]
fn rewrite_wfst_one_to_many_output_chain_emits_continuation() {
    let mut wfst = RewriteWfst::new();
    wfst.set_allow_identity(false);
    wfst.add_rule("f", "ph", 0.1).expect("valid rewrite rule");
    wfst.expand(0);

    let first = wfst
        .transitions(0)
        .iter()
        .find(|transition| transition.input == Some('f') && transition.output == Some('p'))
        .expect("expected f -> p first transition")
        .clone();
    assert_ne!(first.to, 0);
    assert_eq!(first.weight.value(), 0.1);

    wfst.expand(first.to);
    let continuation = wfst
        .transitions(first.to)
        .iter()
        .find(|transition| transition.input.is_none() && transition.output == Some('h'))
        .expect("expected epsilon -> h continuation");
    assert_eq!(continuation.to, 0);
    assert_eq!(continuation.weight, TropicalWeight::one());
}

#[test]
fn rewrite_wfst_many_to_one_input_chain_consumes_continuation() {
    let mut wfst = RewriteWfst::new();
    wfst.set_allow_identity(false);
    wfst.add_rule("ph", "f", 0.1).expect("valid rewrite rule");
    wfst.expand(0);

    let first = wfst
        .transitions(0)
        .iter()
        .find(|transition| transition.input == Some('p') && transition.output == Some('f'))
        .expect("expected p -> f first transition")
        .clone();
    assert_ne!(first.to, 0);

    wfst.expand(first.to);
    let continuation = wfst
        .transitions(first.to)
        .iter()
        .find(|transition| transition.input == Some('h') && transition.output.is_none())
        .expect("expected h -> epsilon continuation");
    assert_eq!(continuation.to, 0);
}

#[test]
fn rewrite_wfst_identity_transitions_passthrough_printable_symbols() {
    let mut wfst = RewriteWfst::new();
    wfst.set_allow_identity(true);
    wfst.expand(0);

    for ch in [' ', '?', 'z', '~'] {
        assert!(
            wfst.transitions(0).iter().any(|transition| {
                transition.input == Some(ch)
                    && transition.output == Some(ch)
                    && transition.to == 0
                    && transition.weight.value() == 0.0
            }),
            "expected printable ASCII identity transition for {ch:?}"
        );
    }

    assert!(!wfst
        .transitions(0)
        .iter()
        .any(|transition| { transition.input == Some('\n') && transition.output == Some('\n') }));
}

#[test]
fn rewrite_wfst_prunes_dominated_identity_equivalent_rewrite() {
    let mut wfst = RewriteWfst::new();
    wfst.set_allow_identity(true);
    wfst.add_rule("a", "a", 0.2).expect("valid rewrite rule");
    wfst.expand(0);

    let matching = wfst
        .transitions(0)
        .iter()
        .filter(|transition| {
            transition.input == Some('a') && transition.output == Some('a') && transition.to == 0
        })
        .collect::<Vec<_>>();

    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].weight, TropicalWeight::one());
}

#[test]
fn rewrite_wfst_statesource_computes_start_state() {
    let mut lazy = RewriteWfst::new();
    lazy.set_allow_identity(false);
    lazy.add_rule("ph", "f", 0.1).expect("valid rewrite rule");
    lazy.expand(0);

    let mut source = RewriteWfst::with_rules(vec![
        RewriteRule::with_cost("ph", "f", 0.1).expect("valid rewrite rule")
    ])
    .expect("valid rewrite rules");
    source.set_allow_identity(false);
    let state = StateSource::<char, TropicalWeight>::compute_state(&source, 0);

    assert!(matches!(state, LazyState::Computed { is_final: true, .. }));
    assert_eq!(
        state.transitions().expect("computed transitions"),
        lazy.transitions(0)
    );
    assert_eq!(
        StateSource::<char, TropicalWeight>::num_states_hint(&source),
        Some(2)
    );
}

#[test]
fn common_english_rules_include_ph_to_f() {
    let rules = CommonPhoneticRules::english();
    assert!(!rules.is_empty());

    let rule = rules
        .iter()
        .find(|rule| rule.input == "ph")
        .expect("expected ph rewrite rule");
    assert_eq!(rule.output, "f");
}

#[test]
fn common_german_rules_include_sch_to_sh() {
    let rules = CommonPhoneticRules::german();
    assert!(!rules.is_empty());

    let rule = rules
        .iter()
        .find(|rule| rule.input == "sch")
        .expect("expected sch rewrite rule");
    assert_eq!(rule.output, "sh");
}

#[test]
fn rewrite_rule_priority_is_configurable() {
    let rule = RewriteRule::new("ph", "f").with_priority(10);

    assert_eq!(rule.priority, 10);
}
