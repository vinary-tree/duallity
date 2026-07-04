# Testing

duallity's tests fall into two layers: **per-module unit tests** (`#[cfg(test)] mod tests` inside each
`src/*.rs`) that pin the exact semantics of one component, and the **integration suite**
(`tests/wfst_integration.rs`) that drives the public API end to end.

## 1. Running the tests

```sh
cargo test                 # default-feature tests (Levenshtein, Universal, WallBreaker, Generalized, Rewrite)
cargo test --all-features  # adds the phonetic NFA / regex tests behind `phonetic-rules`
```

The phonetic NFA/regex suites live inside their feature-gated modules, so they run only under
`--all-features` (or `--features phonetic-rules`).

## 2. The integration suite (`tests/wfst_integration.rs`)

| Group | Representative tests | What they verify |
|-------|----------------------|------------------|
| Levenshtein | `test_levenshtein_wfst_basic`, `_start_state`, `_lazy_expansion`, `_clone` | construction, start state, lazy `expand`/`computed_states`, clone fidelity |
| Universal | `test_universal_wfst_tracks_exact_query_position` | exact query-label cursor tracking through wrapper and integration paths |
| State source | `test_levenshtein_state_source_basic`, `_compute_state`, `_with_lazy_wrapper` | `start() == 0`, `compute_state` is `Computed`, `LazyWfstWrapper` expansion |
| Backend | `test_dictionary_backend_basic`, `_lookup`, `_contains`, `_iter`, `_with_vocabulary` | interning ids, lookup, `contains` over cache ∪ dictionary, iteration, pre-interning |
| Weights | `test_tropical_weight_semantics` | `plus = min`, `times = +`, `zero().is_infinite()`, `one() == 0` |
| Algorithm | `test_wfst_transposition_algorithm`, `test_wfst_merge_and_split_algorithm` | `with_algorithm(...)` reports the variant, accepts adjacent swaps at cost `1`, and accepts merge/split arities at cost `1` |
| `generalized_tests` | creation, lazy expansion, `with_transposition`, builder, cache ops | builder happy/`Err` paths; `clear_cache` empties `computed_states` |
| `wallbreaker_tests` | creation, lazy expansion, `with_transposition`, builder, `num_results`, cache ops, state hint | eager query produces results; `LazyWfst` expansion; `num_states_hint` is `Some(>0)` |

## 3. The label-preservation tests (the semantic contract)

The most important unit tests pin the **transducer label orientation** (input = query side, output =
dictionary side, [theory/03](../theory/03-levenshtein-as-transducer.md)) — the contract that commit
`be3dc6a` established:

- `state_source.rs::test_state_source_transition_labels_preserve_transducer_sides` — substitution
  `(Some('b'), Some('c'), 1.0)`, insertion `(None, Some('c'), 1.0)`, deletion `(Some('c'), None, 1.0)`,
  identity `(Some('c'), Some('c'), 0.0)`;
- `universal_state_source.rs::test_universal_state_source_transition_labels_preserve_transducer_sides`
  — substitution and identity labels in the universal path, with zero local edge cost because the
  edit distance is carried by final weight;
- `universal_state_source.rs::test_universal_state_source_weights_paths_by_final_edit_distance` —
  one-edit standard cases have total path weight `1`;
- `universal_state_source.rs::test_universal_state_source_can_spell_full_label_pairs` — deletion and
  insertion cases still expose paths whose labels spell the whole query/output pair;
- `universal_state_source.rs::test_universal_state_source_tracks_exact_query_position` — the second
  dictionary edge consumes the second query label, proving the label cursor is not inferred from
  abstract universal offsets.

The phonetic char/ε-chain semantics (commit `314f285`) are pinned by:

- `phonetic_rewrite_wfst.rs::test_rewrite_wfst_many_to_one_input_chain` (`ph→f`: `p:f` then `h:ε`);
- `phonetic_rewrite_wfst.rs::test_rewrite_wfst_one_to_many_output_chain` (`f→ph`: `f:p` then `ε:h`);
- `phonetic_nfa_wfst.rs::test_phonetic_nfa_wfst_statesource_matches_lazy_expansion`
  (`StateSource` ≡ `LazyWfst` transitions);
- `composed_phonetic.rs::test_phonetic_match_ordering` (`PhoneticMatch` sorts by total cost).

When you change transition generation, **these are the tests that must still pass** — they are the
executable specification of the semantics documented in [theory/03](../theory/03-levenshtein-as-transducer.md)
and [design/phonetic-rewrite-wfst](../design/phonetic-rewrite-wfst.md).

## 4. Adding a test for a new variant

Follow the existing pattern (see `wallbreaker_tests`):

1. construct the WFST from a small, explicit dictionary and query;
2. assert the trivially-observable invariants (`query()`, `max_distance()`, `!is_empty()`);
3. assert **lazy** behaviour: `!is_expanded(start)` and `computed_states() == 0` before `expand`,
   then `is_expanded(start)` and `computed_states() >= 1` after;
4. if the variant has a documented **label semantics**, assert the produced
   `(input, output, weight)` triples directly — do not rely on path enumeration alone;
5. exercise `clear_cache()` and a non-default `CachePolicy`.

Keep dictionaries tiny and deterministic so a failure points at the exact transition, not at a search
heuristic.

## See also

- [theory/03 · The Levenshtein automaton as a transducer](../theory/03-levenshtein-as-transducer.md)
- [design/](../design/README.md) (each variant's semantics)
