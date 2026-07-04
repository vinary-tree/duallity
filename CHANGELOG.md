# Changelog

All notable changes to `duallity` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-07-04

This release consolidates a large architecture, correctness, and efficiency pass over
the WFST adapters. The public-API changes below are the only source-breaking changes;
each is a deliberate correctness or safety improvement. Downstream crates that consume
`duallity` through the standard wrapper surface (constructing a WFST and querying it)
require no changes.

### Breaking Changes (migration guide)

- **`state_encoding::encode` → `state_encoding::try_encode`.** The old infallible
  `encode(dict_node, automaton_state, max_automaton_states) -> StateId` silently
  overflowed the `u32` `StateId` space for large product states. It is replaced by
  `try_encode(..) -> Option<StateId>`, which returns `None` when the product state
  cannot be represented.
  *Migrate:* replace `encode(a, b, m)` with `try_encode(a, b, m)` and handle `None`.

- **`state_encoding::decode` now returns `Option`.** `decode(state_id, max_automaton_states)`
  previously returned `(u32, u32)` and panicked (divide-by-zero) when
  `max_automaton_states == 0`. It now returns `Option<(u32, u32)>`, yielding `None`
  for a zero-width product space.
  *Migrate:* handle the `Option` (e.g. `decode(id, m)?`).

- **`LevenshteinWfst::query()` and `UniversalLevenshteinWfst::query()` now return `&str`
  instead of `String`.** They borrow the stored query text instead of allocating a fresh
  `String` on every call, matching the existing `GeneralizedWfst`/`WallBreakerWfst`
  accessors.
  *Migrate:* call `.query().to_string()` where an owned `String` is required. The
  underlying state sources additionally expose `query_str() -> &str` and retain
  `query() -> String` for owned access.

- **`PhoneticPipelineBuilder` and `PhoneticWfstBuilder` weight setters now return `Result`.**
  `phonetic_weight`, `edit_weight`, `add_rewrite_rule`, and `add_rewrite_rules` now return
  `Result<Self, InvalidWeightError>` and reject non-finite or negative weights instead of
  silently accepting them.
  *Migrate:* chain with `?` (or `.expect(..)`), e.g.
  `builder.phonetic_weight(0.4)?.edit_weight(3.0)?`.

### Added

- `InvalidWeightError` — public error type surfaced by the validating builder setters and
  rewrite-rule constructors.
- `state_encoding::try_encode` and the `Option`-returning `state_encoding::decode` checked
  encoders (see Breaking Changes).
- `DictionaryBackend::try_intern(word) -> Option<VocabId>` — fallible interning that reports
  vocabulary-ID exhaustion; the infallible `LatticeBackend::intern` trait method is retained.
- `query_str() -> &str` borrowing accessors on the standard and universal state sources.

### Changed

- Internal architecture: the monolithic WFST modules were decomposed into focused
  support / builder / ops submodules (lazy cache, node registry and key, generalized ops
  and builder, WallBreaker results and builder, phonetic NFA / rewrite / state support,
  phonetic anchors, and per-variant state-source support), reducing module size and
  duplication while keeping the public export surface stable.
- Numeric hardening throughout: saturating `usize → u32/f64` conversions, overflow-checked
  state encoders, and poison-recovering `RwLock` accessors.
- Expanded regression coverage: dedicated integration suites for the generalized,
  phonetic-NFA, phonetic-rewrite, phonetic-state-source, WallBreaker, algorithm-acceptance,
  dictionary-backend, cache-policy, and phonetic-weight surfaces.

### Fixed

- Levenshtein WFST label semantics, phonetic rewrite / state-source semantics, and phonetic
  product-frontier semantics (query-input / dictionary-output orientation).
- Silent `u32` overflow in product-state encoding and divide-by-zero in decoding (see
  Breaking Changes).
- Standard Levenshtein final-weight arithmetic no longer undercounts remaining query
  suffixes for very large queries.
- Phonetic product frontiers are canonicalized (sorted and deduplicated) before registry
  interning, so duplicate-equivalent frontiers no longer receive distinct state IDs.
- Dead-code warnings in the default-feature (non-`phonetic-rules`) library build: the
  phonetic-only helpers are now correctly feature-gated.

## [0.2.0] - 2026-06-15

- WFST semantic fixes over 0.1.0: Levenshtein label semantics, phonetic rewrite and
  state-source semantics, and phonetic frontier semantics.

## [0.1.0] - 2026-06-10

- Initial release: Levenshtein automata as lling-llang WFSTs, with composition adapters
  bridging liblevenshtein fuzzy matching and the lling-llang weighted-transducer framework.
