# Rank a dictionary with fzf V2 scores

Use `FzfScorer` when candidates already live in a `libdictenstein` character
dictionary and the query should be an ordered subsequence.

```rust
use duallity::{FzfConfig, FzfScorer};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use libdictenstein::Dictionary;
use liblevenshtein::transducer::SubsequenceQueryIterator;

fn example() -> Result<(), duallity::FzfError> {
let dictionary = DynamicDawgChar::<()>::from_terms([
    "src/fzf_scorer.rs", "src/state_source.rs", "Cargo.toml",
]);
let scorer = FzfScorer::with_config(
    "fzfs",
    FzfConfig { top_k: 10, ..FzfConfig::default() },
)?;
let mut matches: Vec<_> = SubsequenceQueryIterator::with_pruner(
    dictionary.root(), scorer.query_units(), scorer,
)
.map(|item| (
    item.units.iter().collect::<String>(),
    item.score.expect("fzf accepts with an exact score") as i32,
))
.collect();
matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
Ok(())
}
```

Set case mode, scoring scheme, and resource ceilings explicitly for a public
service. `top_k` maintains a pruning cutoff but does not truncate the iterator.
Use `FzfWfst` when the score must participate in weighted composition. See the
[design and proof rationale](../design/fzf-wfst.md).
