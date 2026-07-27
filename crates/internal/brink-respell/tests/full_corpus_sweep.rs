//! Full-corpus diagnostic sweep for issue #1335 (B0.8b): how much of the
//! real oracle corpus (`collect_oracle_cases` — every `tests/tier*/…`
//! directory with golden `.oracle.json` episodes, ~396 cases as of this
//! writing) does [`respell_ink_source`] currently handle, and what blocks
//! the rest?
//!
//! This is *not* the "full-corpus episode-identity differential"
//! `docs/b0-sequencing.md` names as the B0 ratification gate — it only
//! calls the emitter, so a case counted "OK" here has merely produced
//! `.brink` text, not been proven episode-identical (that's
//! `ink_corpus_convert.rs`'s job, one fixture at a time). It exists to
//! answer the recurring "how close are we, and to what specifically" B0.8b
//! question quantitatively instead of by hand-sampling, since `emit_file`
//! is all-or-nothing (issue #1335's own module-doc summary of *why* a
//! given construct is unsupported can go stale the moment a native-grammar
//! or emitter fix lands elsewhere; this sweep can't).
//!
//! `#[ignore]`d — it walks the whole corpus (hundreds of files) and its
//! value is the printed breakdown, not a pass/fail assertion (every
//! `EmitError` shape is either a real, currently-tracked gap or evidence
//! one just closed). Run with:
//!
//! ```sh
//! cargo test -p brink-respell --test full_corpus_sweep -- --ignored --nocapture
//! ```
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use brink_respell::respell_ink_source;
use brink_test_harness::corpus::collect_oracle_cases;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root must resolve")
}

#[test]
#[ignore = "diagnostic sweep over the whole corpus; not a pass/fail assertion, run manually"]
fn sweep() {
    let root = repo_root();
    let cases = collect_oracle_cases(&root.join("tests"));
    let mut ok = 0usize;
    let mut fail_by_reason: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

    for case in &cases {
        let ink_path = case.join("story.ink");
        let Ok(src) = std::fs::read_to_string(&ink_path) else {
            continue;
        };
        match respell_ink_source(&src) {
            Ok(_) => ok += 1,
            Err(e) => {
                // Bucket by the "what" portion of `EmitError::Unsupported`'s
                // `Display` (everything before the `(context)` suffix), so
                // e.g. every "temp declaration" refusal groups together
                // regardless of which knot triggered it.
                let reason = e.to_string();
                let bucket = reason
                    .split_once('(')
                    .map_or(reason.as_str(), |(head, _)| head.trim())
                    .to_string();
                let rel = case.strip_prefix(&root).unwrap_or(case).to_path_buf();
                fail_by_reason.entry(bucket).or_default().push(rel);
            }
        }
    }

    println!("oracle cases:  {}", cases.len());
    println!("respell OK:    {ok}");
    println!("respell FAIL:  {}", cases.len() - ok);
    println!();
    for (reason, examples) in &fail_by_reason {
        println!("  {:4}  {reason}", examples.len());
        for ex in examples.iter().take(3) {
            println!("        e.g. {}", ex.display());
        }
    }
}
