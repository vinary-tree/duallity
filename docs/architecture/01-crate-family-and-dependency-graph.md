# 01 · Crate family and dependency graph

> **Defines:** the four crates duallity bridges, the dependency cycle it avoids, and the migration
> path from liblevenshtein's old `wfst` module.

## 1. Four crates, four jobs

duallity is deliberately small: it is the *connective tissue* between three sibling crates, each with
a single responsibility.

| Crate | Responsibility | What duallity uses from it |
|-------|----------------|----------------------------|
| **liblevenshtein** | fuzzy matching / automata | `Algorithm`, the universal automaton (`PositionVariant`: `Standard`/`Transposition`/`MergeAndSplit`), `OperationSet`, `GeneralizedAutomaton`, the phonetic regex+NFA (`parse`, `compile`, `NFAChar`), and `wallbreaker`. |
| **libdictenstein** | dictionary containers | `Dictionary`, `DynamicDawgChar` (and other backends), `SubstringDictionary`, `Scdawg`/SCDAWG. |
| **lling-llang** | the WFST algebra | `Wfst` / `LazyWfst` / `StateSource` / `LatticeBackend` traits, `TropicalWeight`, `CachePolicy`, `compose`. |
| **duallity** | the WFST adapters (**this crate**) | wraps the first two to satisfy the third. |

## 2. Why duallity is a *separate* crate

These adapters used to live behind a `wfst` feature inside **liblevenshtein**. But **lling-llang
already depends on liblevenshtein**. A `wfst` feature that pulled lling-llang back *into*
liblevenshtein would therefore close a **dependency cycle**:

```
liblevenshtein ──(wfst feature)──► lling-llang ──(already depends on)──► liblevenshtein   ✗ cycle
```

Cargo forbids cyclic dependencies, and even if it did not, the cycle would entangle the two crates'
release cadences. The fix is to lift the adapters into a crate that sits **above both**:

<img src="../diagrams/crate-dependency-graph.svg" alt="duallity depends on all three siblings; the dashed red edge is the cycle it avoids" width="640"/>

duallity is the **only** place where liblevenshtein and lling-llang meet, so it is exactly the right
home for code that needs both. The resulting graph is acyclic, and each crate can version
independently.

## 3. Migrating from liblevenshtein's old `wfst` module

The types did not change — only their path did. If you used the adapters when they lived in
liblevenshtein:

```rust,ignore
// Old (liblevenshtein ≤ 0.8, behind the removed `wfst` feature)
use liblevenshtein::wfst::LevenshteinWfst;
use liblevenshtein::wfst::DictionaryBackend;

// New
use duallity::LevenshteinWfst;
use duallity::DictionaryBackend;
```

The dictionary types also moved out of liblevenshtein into **libdictenstein**:

```rust,ignore
// Old
use liblevenshtein::dictionary::dynamic_dawg_char::DynamicDawgChar;
// New (note the module path: dynamic_dawg::char, not dynamic_dawg_char)
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
```

| Old path | New path |
|----------|----------|
| `liblevenshtein::wfst::LevenshteinWfst` | `duallity::LevenshteinWfst` |
| `liblevenshtein::wfst::DictionaryBackend` | `duallity::DictionaryBackend` |
| `liblevenshtein::wfst::RewriteWfst` | `duallity::RewriteWfst` |
| `liblevenshtein::wfst::generalized_wfst::*` | `duallity::{GeneralizedWfst, GeneralizedWfstBuilder}` |
| `liblevenshtein::wfst::wallbreaker_wfst::WallBreakerWfst` | `duallity::WallBreakerWfst` |
| `liblevenshtein::dictionary::dynamic_dawg_char::DynamicDawgChar` | `libdictenstein::dynamic_dawg::char::DynamicDawgChar` |
| `liblevenshtein::dictionary::scdawg::Scdawg` | `libdictenstein::scdawg::Scdawg` |

## 4. Version matrix

duallity **0.2** is built against:

| Dependency | Version |
|------------|---------|
| liblevenshtein | 0.9 |
| lling-llang | 0.2 |
| libdictenstein | 0.2 |

These are the versions declared in `Cargo.toml`; the `phonetic-rules` feature additionally enables
`liblevenshtein/phonetic-rules`. See [guides/README · Feature flags](../guides/README.md) for what
each feature turns on.
