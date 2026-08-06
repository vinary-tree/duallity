use duallity::{FzfConfig, FzfScorer};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use libdictenstein::Dictionary;
use liblevenshtein::transducer::SubsequenceQueryIterator;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dictionary = DynamicDawgChar::<()>::from_terms([
        "src/fzf_scorer.rs",
        "src/fzf_state_source.rs",
        "docs/design/fzf-wfst.md",
        "Cargo.toml",
    ]);
    let scorer = FzfScorer::with_config(
        "fzfs",
        FzfConfig {
            top_k: 3,
            ..FzfConfig::default()
        },
    )?;
    let mut matches: Vec<_> =
        SubsequenceQueryIterator::with_pruner(dictionary.root(), scorer.query_units(), scorer)
            .map(|matched| {
                (
                    matched.units.iter().collect::<String>(),
                    matched.score.expect("accepted fzf matches carry scores") as i32,
                )
            })
            .collect();
    matches.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    for (candidate, score) in matches.into_iter().take(3) {
        println!("{score:>4}  {candidate}");
    }
    Ok(())
}
