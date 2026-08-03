//! `bench-counters`-gated proof that `push`/`insert`/… on a single-level
//! struct-field projection (`push(a.items, v)`) amortizes to O(1) per call
//! instead of paying a fresh `Arc::make_mut` COW copy on *every* call
//! (issue #2123 — the loop-append cliff #576 closed at the root, still
//! present one field deeper).
//!
//! Runs in CI's required "Test (bench-counters)" job
//! (`cargo test -p brink-runtime --features bench-counters`, see
//! `docs/runtime-bench.md`), same as `bench_counters::tests`. Without the
//! fix, 2,000 sequential `push(a.items, i)` calls pay 2,000 COW copies
//! (one per call — O(n²) total); with it, the field's `Arc` becomes the
//! sole owner after the first (a one-time literal-pool-sharing fork, the
//! same shape `loop_append_bench`'s bare-variable case pays), so the
//! count stays a small constant regardless of the loop bound.
#![cfg(feature = "bench-counters")]

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::bench_counters;
use brink_runtime::{DotNetRng, Step, Story};

/// Loose enough to tolerate the one-time literal-pool-sharing fork
/// `loop_append_bench`'s own doc describes (a `benches/runtime.rs` divan
/// bench, not linkable from a `tests/` target — hence plain text, not an
/// intra-doc link), but two orders of magnitude below the loop bound —
/// nowhere near enough headroom for the O(n²) pre-fix behavior (2,000
/// copies) to slip through.
const MAX_EXPECTED_COW_COPIES: u64 = 4;

#[test]
fn field_push_loop_amortizes_to_o1_cow_copies() {
    let src = "STRUCT Bag = #{\n    items: Array<int>,\n}\nVAR a = 0\nVAR total = 0\n~ {\n    a = Bag#{items: #[]}\n    temp i = 0\n    while i < 2000 {\n        push(a.items, i)\n        i = i + 1\n    }\n    total = len(a.items)\n}\n{total}\n-> END\n";
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        ..AnalysisOptions::default()
    };
    let data = brink_compiler::compile_with_options("main.ink", |_p| Ok(src.to_owned()), options)
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    bench_counters::reset();
    let mut out = String::new();
    while let Step::Line(line) = story.continue_single().unwrap() {
        out.push_str(&line.text);
    }
    assert_eq!(
        out.trim(),
        "2000",
        "the loop must still push all 2000 elements"
    );

    let snap = bench_counters::snapshot();
    assert!(
        snap.cow_copies <= MAX_EXPECTED_COW_COPIES,
        "field-mutator loop-append paid {} COW copies over 2000 pushes \
         (expected <= {MAX_EXPECTED_COW_COPIES}) — the O(n²) cliff (issue \
         #2123) is back: `push(a.items, i)` is re-sharing the field's Arc \
         on every call instead of mutating in place",
        snap.cow_copies
    );
}
