//! Handle explicit construction and expansion limits in a native Rust caller.

use duallity::{GeneralizedWfst, GeneralizedWfstLimits};
use libdictenstein::dynamic_dawg::char::DynamicDawgChar;
use liblevenshtein::transducer::OperationSet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dictionary = DynamicDawgChar::<()>::from_terms(["phone", "phony"]);
    let limits = GeneralizedWfstLimits {
        max_query_bytes: 128,
        max_query_scalars: 64,
        max_retained_dictionary_nodes: 10_000,
        max_retained_wfst_states: 20_000,
        max_paths_per_expansion: 1_000,
        max_work_units_per_expansion: 100_000,
        ..Default::default()
    };
    let mut wfst = GeneralizedWfst::try_new_with_limits(
        &dictionary,
        "fone",
        2,
        OperationSet::standard(),
        limits,
    )?;
    let arcs = wfst.try_transitions(0)?;
    assert!(!arcs.is_empty());
    Ok(())
}
