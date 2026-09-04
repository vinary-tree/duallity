//! FFI constructor benchmarks (wave W8).
//!
//! Evidence base for the capture-once construction claim: `duallity_wfst_new`
//! captures the source dictionary's snapshot exactly once and returns a lazy
//! WFST without scanning the dictionary, so its cost must be FLAT in the
//! dictionary size (the automaton is expanded lazily during traversal, not at
//! construction). This benchmark builds `DynamicDawgBinding` dictionaries of
//! geometrically increasing size and measures the constructor across them;
//! a flat curve supports the O(1)-construction contract. Record the complete
//! host, affinity, governor, load, git ref, and raw samples with every result;
//! repository evidence must not depend on a machine-local path.
//!
//! Run: `cargo bench --features ffi --bench ffi_constructor_benchmarks`
//! (pin with taskset + the performance governor for stable numbers).

use std::hint::black_box;
use std::ptr;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use duallity::ffi::{duallity_wfst_free, duallity_wfst_new, DuallityStatus, DuallityWfst};
use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};

/// Build a Unicode-scalar dictionary of `n` distinct terms.
fn build_dict(n: usize) -> DynamicDawgBinding {
    let dictionary = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
    for index in 0..n {
        let term = format!("term{index:06}");
        dictionary
            .insert_text(term.as_bytes(), None)
            .expect("insert_text succeeds");
    }
    dictionary
}

/// Constructing a Levenshtein WFST should cost the same whether the dictionary
/// has sixteen terms or sixty-five thousand: the constructor captures one
/// snapshot and defers all expansion to traversal.
fn bench_constructor_vs_dict_size(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("wfst_new_vs_dict_size");
    for &n in &[16usize, 256, 4096, 65_536] {
        let dictionary = build_dict(n);
        let source = dictionary.resource();
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut wfst: *mut DuallityWfst = ptr::null_mut();
                let status = duallity_wfst_new(
                    black_box(source.as_raw()),
                    b"term000000".as_ptr(),
                    10,
                    2,
                    0, // algorithm: Standard
                    0, // kind: Levenshtein
                    &mut wfst,
                );
                assert_eq!(status, DuallityStatus::Ok);
                unsafe { duallity_wfst_free(wfst) };
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_constructor_vs_dict_size);
criterion_main!(benches);
