//! #3274 (stage-2 flip) regression suite: variant-claimed lines compile
//! through the whole real pipeline — source → normalize (no cartesian
//! lift) → stamp → LIR `EmitLineVariants` → codegen → link → run — and
//! produce ink's documented shared-alternative semantics.
//!
//! The stage-1 sibling (`variant_groups_e2e.rs`) drives hand-built LIR;
//! this suite drives authored source, so it also covers the
//! normalize/stamp/lowering agreement that stage 2 added. The two closed
//! bugs each get a permanent case: #3271 (two stateful alternatives on
//! one line advanced independently per clone) and #3272 (a labeled choice
//! cloned by the cartesian lift → E060 internal error on legal source).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

fn compile(src: &str) -> brink_compiler::CompileOutput {
    brink_compiler::compile("t.ink", |p| {
        assert_eq!(p, "t.ink", "single-file fixture");
        Ok(src.to_string())
    })
    .expect("fixture compiles")
}

/// Native-surface compile via a scratch directory — same helper shape as
/// `debug_control_3186.rs` (native discovery always walks a real
/// filesystem).
fn compile_native(
    src: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("brink-variant-flip-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let entry = dir.join("main.brink");
    std::fs::write(&entry, src).expect("write scratch fixture");
    let result = brink_compiler::compile_path(&entry);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run_to_end(data: &brink_format::StoryData) -> Vec<String> {
    let (program, line_tables) = brink_runtime::link(data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut lines = Vec::new();
    loop {
        match story.continue_single().expect("runtime") {
            Step::Line(line) => lines.push(line.text.trim_end().to_string()),
            Step::Done | Step::End | Step::Suspended => break,
            Step::Choices(_) => panic!("fixture has no choices"),
        }
    }
    lines
}

/// #3271's exact repro. Ink's documented semantics: a stopping sequence
/// shows its next item **each time it is viewed**; both alternatives on
/// the line are viewed once per line evaluation, so both advance
/// together — `a x` / `b y` / `b y`. The pre-flip cartesian clone gave
/// the second alternative one clone per branch of the first, each with
/// its own visit count, producing `b x` on the second view.
#[test]
fn two_stateful_alternatives_advance_together() {
    let src = "VAR n = 0\n\
               -> loop\n\
               === loop ===\n\
               ~ n = n + 1\n\
               Line: {a|b} {x|y}\n\
               { n < 3: -> loop }\n\
               -> END\n";
    let lines = run_to_end(&compile(src).data);
    assert_eq!(
        lines,
        vec!["Line: a x", "Line: b y", "Line: b y"],
        "both stopping alternatives advance on every view (ink semantics, #3271)"
    );
}

/// The same line under `&` (cycle) — both alternatives wrap together.
#[test]
fn two_cycles_wrap_together() {
    let src = "VAR n = 0\n\
               -> loop\n\
               === loop ===\n\
               ~ n = n + 1\n\
               Line: {&a|b} {&x|y}\n\
               { n < 3: -> loop }\n\
               -> END\n";
    let lines = run_to_end(&compile(src).data);
    assert_eq!(
        lines,
        vec!["Line: a x", "Line: b y", "Line: a x"],
        "cycles wrap in step (#3274)"
    );
}

/// A `once` alternative exhausts to its empty rendering while the
/// stopping alternative beside it keeps its last branch.
#[test]
fn once_exhausts_beside_stopping() {
    let src = "VAR n = 0\n\
               -> loop\n\
               === loop ===\n\
               ~ n = n + 1\n\
               Line: {!a|b} {x|y}.\n\
               { n < 3: -> loop }\n\
               -> END\n";
    let lines = run_to_end(&compile(src).data);
    assert_eq!(
        lines,
        vec!["Line: a x.", "Line: b y.", "Line: y."],
        "once shows its exhausted (empty) variant on view three (#3274)"
    );
}

/// #3272's fixture (native surface): a labeled choice inside an inline
/// `{if …}` on a line that also carries a stateful alternative. The
/// cartesian lift cloned the conditional — one label id stamped onto two
/// containers, refused by codegen's #1673 guard as the internal-error
/// E060. Under the flip the label carrier lifts first and is never
/// cloned: the program is legal and compiles.
#[test]
fn labeled_choice_beside_alternative_compiles() {
    let src = "var flag = true\n\
               \n\
               flow start() {\n\
               \x20\x20Pre {~ one|two} mid {if flag {\n\
               \x20\x20\x20\x20{?\n\
               \x20\x20\x20\x20\x20\x20* (dup) Pick me -> the_end\n\
               \x20\x20\x20\x20}\n\
               \x20\x20}} post.\n\
               \x20\x20-> the_end\n\
               }\n\
               \n\
               flow the_end() {\n\
               \x20\x20Done.\n\
               \x20\x20-> END\n\
               }\n";
    let out = compile_native(src);
    match out {
        Ok(_) => {}
        Err(brink_compiler::CompileError::Diagnostics(diags)) => {
            panic!("#3272 fixture must compile clean, got diagnostics: {diags:?}")
        }
        Err(e) => panic!("#3272 fixture must compile clean, got: {e}"),
    }
}

/// The enumerated group is real in the artifact: one `LineVariantGroup`
/// covering `dims.product()` consecutive whole-line entries, each with
/// its own distinct `source_hash` (the translation/VO contract — every
/// variant is its own line entry, never a fragment).
#[test]
fn line_table_carries_the_variant_group() {
    let src = "Line: {a|b} {x|y|z}\n-> END\n";
    let data = compile(src).data;
    assert_eq!(
        data.line_variant_groups.len(),
        1,
        "one authored variant line, one group"
    );
    let group = &data.line_variant_groups[0];
    assert_eq!(group.dims, vec![2, 3], "authored branch counts, in order");
    let scope = data
        .line_tables
        .iter()
        .find(|s| s.scope_id == group.scope_id)
        .expect("group's scope exists");
    let base = usize::try_from(group.base).unwrap();
    let entries = &scope.lines[base..base + 6];
    let hashes: std::collections::BTreeSet<u64> = entries.iter().map(|e| e.source_hash).collect();
    assert_eq!(
        hashes.len(),
        6,
        "every variant is its own whole-line entry with a distinct source_hash"
    );
}

/// The cap breach is a worded E191, never a silent fallback: 2^6 = 64
/// variants over the 32 cap.
#[test]
fn cap_breach_is_a_worded_error() {
    let src = "Line: {a|b} {c|d} {e|f} {g|h} {i|j} {k|l}\n-> END\n";
    let err = brink_compiler::compile("t.ink", |_| Ok(src.to_string()))
        .expect_err("64 variants must not compile silently");
    let brink_compiler::CompileError::Diagnostics(diags) = err else {
        panic!("expected diagnostics, got: {err}");
    };
    assert!(
        diags.iter().any(|d| d.code.as_str() == "E191"),
        "the breach is E191, worded: {diags:?}"
    );
}

/// A single stateful alternative — the overwhelmingly common shape — still
/// behaves exactly as before the flip (it now routes through the variant
/// model, whose arithmetic is `emit_sequence`'s byte-for-byte).
#[test]
fn single_alternative_unchanged() {
    let src = "VAR n = 0\n\
               -> loop\n\
               === loop ===\n\
               ~ n = n + 1\n\
               It's {a fine|a good|an average} day.\n\
               { n < 4: -> loop }\n\
               -> END\n";
    let lines = run_to_end(&compile(src).data);
    assert_eq!(
        lines,
        vec![
            "It's a fine day.",
            "It's a good day.",
            "It's an average day.",
            "It's an average day.",
        ],
        "stopping semantics unchanged for the single-alternative shape"
    );
}

/// #3275 (stage 3b) — the pinned mixed-line ruling (2026-08-29): a
/// stateful alternative on a line with an inline conditional advances
/// once per line VIEW, whichever conditional branch ran. Delivered by
/// stamping before the lift: the conditional's branches each carry a
/// clone of `{&p|q}`, and every clone keeps the one stamped container id,
/// so all branches advance the same visit count. Pre-3a each clone had
/// its own container: visit 2 took the "late" branch's clone on ITS
/// first view and printed `p` again.
#[test]
fn mixed_line_alternative_advances_once_per_view() {
    let out = compile(
        "VAR n = 0\n\
         -> loop\n\
         === loop ===\n\
         ~ n = n + 1\n\
         Line: {n > 1: late|early} {&p|q}\n\
         { n < 3: -> loop }\n\
         -> END\n",
    );
    let lines = run_to_end(&out.data);
    assert_eq!(
        lines,
        vec!["Line: early p", "Line: late q", "Line: late p"],
        "the cycle advances p→q→p across views even as the conditional \
         switches branches — shared container state, ink's semantics"
    );
}
