//! Issues #3354 / #3362 (RULED 2026-09-01, option C): a classic `~ temp`
//! used on a path its declaration does not dominate.
//!
//! Three halves, all pinned here against real `.ink` source compiled and run
//! through the production pipeline:
//!
//! 1. **`E193`** — the compile-time warning, naming the read site and the
//!    declaration, for each of the four shapes the ruling enumerates.
//! 2. **#3362 resolution** — a read written textually ahead of its
//!    declaration, and a stitch reading a temp declared at its knot's root,
//!    must lower to the temp's own slot. Before the fix they lowered to a
//!    phantom hashed `GetGlobal` that no link step could resolve, so the
//!    story died with `unresolved global: $02_…` at the first step.
//! 3. **Runtime fallback** — an uninitialized temp slot reads as ink's
//!    missing-variable default (`0`), with a `RuntimeWarning`, instead of a
//!    `Null` that faults on the next operator. The C# reference prints
//!    `Peek: 1.` here; so does brink now.
//!
//! The `.brink` (native-surface) half of the same coverage lives in
//! `docs/diagnostics/E193.md`'s fences, compiled by
//! `brink-test-harness`'s `diagnostic_docs_fences` gate.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use brink_compiler::{AnalysisOptions, DiagnosticCode, Dialect};
use brink_runtime::{DotNetRng, Step, Story};

fn compile(source: &str) -> brink_compiler::CompileOutput {
    compile_with_debug_info(source, false)
}

fn compile_with_debug_info(source: &str, emit_debug_info: bool) -> brink_compiler::CompileOutput {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        AnalysisOptions {
            dialect: Dialect::Brink,
            emit_debug_info,
            ..AnalysisOptions::default()
        },
    )
    .expect("compile should succeed — E193 is a warning, not an error")
}

/// The ink-compat dialect (`Dialect::StrictInk`, despite the name — its
/// `types` default is `Gradual`, see `resolve_type_policy`), for the one
/// shape `Dialect::Brink`'s typed mode statically rejects outright (`E067`,
/// void-assignment) but real `.ink` — and the C# reference — allow and play.
/// `strict::check` (E065-E067 among them) never runs at all here: its own
/// module doc gates every caller on `dialect = brink` first.
fn compile_ink(source: &str) -> brink_compiler::CompileOutput {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        AnalysisOptions {
            dialect: Dialect::StrictInk,
            ..AnalysisOptions::default()
        },
    )
    .expect("compile should succeed")
}

/// [`play`], against [`compile_ink`] instead of the Brink-dialect [`compile`].
fn play_ink(source: &str, picks: &[usize]) -> (Vec<String>, Vec<String>) {
    let output = compile_ink(source);
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut lines = Vec::new();
    let mut picks = picks.iter();
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(line) => lines.push(line.text),
            Step::Choices(_) => {
                let Some(&pick) = picks.next() else { break };
                story.choose(pick).expect("choice in range");
            }
            Step::Done | Step::End | Step::Suspended => break,
        }
    }
    let warnings = story
        .take_runtime_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    (lines, warnings)
}

/// Every `E193` message this compile produced.
fn e193(source: &str) -> Vec<String> {
    compile(source)
        .warnings
        .into_iter()
        .filter(|w| w.code == DiagnosticCode::E193)
        .map(|w| w.message)
        .collect()
}

/// Compile, link and play with a fixed choice script, returning the lines
/// the player sees plus every runtime warning the story emitted.
fn play(source: &str, picks: &[usize]) -> (Vec<String>, Vec<String>) {
    let output = compile(source);
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut lines = Vec::new();
    let mut picks = picks.iter();
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(line) => lines.push(line.text),
            Step::Choices(_) => {
                let Some(&pick) = picks.next() else { break };
                story.choose(pick).expect("choice in range");
            }
            Step::Done | Step::End | Step::Suspended => break,
        }
    }
    let warnings = story
        .take_runtime_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    (lines, warnings)
}

// ─── E193: the four ruled shapes ────────────────────────────────────

/// Shape 3 — the read is written textually ahead of the declaration.
/// This is #3362's own repro.
#[test]
fn read_textually_before_declaration_is_warned() {
    let msgs = e193("-> k\n=== k ===\nBefore: {n}.\n~ temp n = 0\nAfter: {n}.\n-> END\n");
    assert_eq!(msgs.len(), 1, "exactly the `Before:` read: {msgs:?}");
    assert!(
        msgs[0].contains('`') && msgs[0].contains("temp n"),
        "{msgs:?}"
    );
    assert!(
        msgs[0].contains("knot `k`"),
        "names the declaration site: {msgs:?}"
    );
    assert!(
        msgs[0].contains("is written further down"),
        "names how the declaration is missed: {msgs:?}"
    );
}

/// Shape 1 — a sibling choice branch declares it. #3354's own repro.
#[test]
fn sibling_choice_branch_read_is_warned() {
    let msgs = e193(
        "-> k\n=== k ===\n- (top)\n+ [declare]\n    ~ temp n = 0\n    -> top\n\
         + [peek]\n    Peek: {n + 1}.\n    -> top\n+ [quit] -> END\n",
    );
    assert_eq!(msgs.len(), 1, "exactly the `peek` branch's read: {msgs:?}");
    assert!(
        msgs[0].contains("runs on a path this read does not pass through"),
        "{msgs:?}"
    );
}

/// Shape 2 — the gather is reached from a branch that did not declare it.
#[test]
fn gather_after_declaring_branch_is_warned() {
    let msgs = e193(
        "-> k\n=== k ===\n+ [declare]\n    ~ temp n = 1\n+ [skip]\n    Skipped.\n\
         - Gathered {n}.\n-> END\n",
    );
    assert_eq!(msgs.len(), 1, "exactly the gather's read: {msgs:?}");
}

/// Shape 4 — a stitch reads a temp declared at its knot's root.
#[test]
fn stitch_reading_knot_root_temp_is_warned() {
    let msgs = e193("-> k.s\n=== k ===\n~ temp n = 7\n-> END\n= s\nStitch sees {n}.\n-> END\n");
    assert_eq!(msgs.len(), 1, "exactly the stitch's read: {msgs:?}");
    assert!(msgs[0].contains("knot `k`"), "{msgs:?}");
}

/// A conditional branch declares it; the read after the conditional is not
/// dominated either — the same rule, a shape the ruling did not have to
/// enumerate separately.
#[test]
fn conditional_branch_declaration_is_warned() {
    let msgs =
        e193("VAR flag = true\n-> k\n=== k ===\n{flag:\n    ~ temp n = 1\n}\nSaw {n}.\n-> END\n");
    assert_eq!(msgs.len(), 1, "{msgs:?}");
}

// ─── E193: what must stay quiet ─────────────────────────────────────

/// The safe form — declare at the knot root, read after it — is the
/// overwhelmingly common shape and must never be warned.
#[test]
fn declaration_then_read_is_clean() {
    assert!(
        e193("-> k\n=== k ===\n~ temp n = 0\nSaw {n}.\n-> END\n").is_empty(),
        "the safe form must not warn"
    );
}

/// A declaration at the knot root dominates every nested branch below it,
/// including choice bodies and the gather.
#[test]
fn declaration_before_a_choice_set_dominates_every_branch() {
    assert!(
        e193(
            "-> k\n=== k ===\n~ temp n = 0\n+ [a]\n    Saw {n}.\n+ [b]\n    Also {n}.\n\
             - Gathered {n}.\n-> END\n"
        )
        .is_empty(),
        "a knot-root declaration dominates the whole weave below it"
    );
}

/// A parameter is bound at call time and shares the same slot as a
/// same-named `~ temp` (`alloc_temps` inserts the parameter first), so a
/// read of it is never a definite-assignment defect.
#[test]
fn parameter_shadowed_by_a_later_temp_is_clean() {
    assert!(
        e193("-> k(3)\n=== k(n) ===\nSaw {n}.\n~ temp n = 0\nNow {n}.\n-> END\n").is_empty(),
        "a parameter read is always initialized"
    );
}

/// A write is not a read: `~ n = 1` ahead of the declaration assigns the
/// slot, and reporting it as an uninitialized *read* would be wrong.
#[test]
fn plain_assignment_target_is_not_a_read() {
    let msgs = e193("-> k\n=== k ===\n~ n = 1\n~ temp n = 0\nSaw {n}.\n-> END\n");
    assert!(msgs.is_empty(), "an assignment target is a write: {msgs:?}");
}

/// A temp in another knot is another call frame — a same-named read here
/// resolves to whatever this frame has, never to that declaration.
#[test]
fn temp_in_another_knot_is_not_this_frames_business() {
    assert!(
        e193(
            "VAR n = 5\n-> k\n=== k ===\nSaw {n}.\n-> END\n\
             === other ===\n~ temp n = 1\nOther {n}.\n-> END\n"
        )
        .is_empty(),
        "a global read in a knot with no temp of that name must stay quiet"
    );
}

// ─── #3362: resolution, not a phantom global ────────────────────────

/// The #3362 repro: before the fix this compiled clean and then died at the
/// first step with `unresolved global: $02_f575fae22fdf81`.
#[test]
fn textually_early_read_resolves_to_the_temp_slot_and_plays() {
    let (lines, warnings) = play(
        "-> k\n=== k ===\nBefore: {n}.\n~ temp n = 0\nAfter: {n}.\n-> END\n",
        &[],
    );
    assert_eq!(
        lines.join(""),
        "Before: 0.\nAfter: 0.\n",
        "the early read must play as ink's missing-variable default"
    );
    assert_eq!(warnings.len(), 1, "one runtime warning: {warnings:?}");
    assert!(
        warnings[0].contains("temp slot 0"),
        "with no DebugInfo section the slot is all the artifact carries: {warnings:?}"
    );
}

/// With debug info emitted — what the studio and the language server both
/// compile with — the warning names the variable the author wrote, matching
/// the C# reference's own `Variable not found: 'n'.` wording.
#[test]
fn the_runtime_warning_names_the_variable_when_debug_info_is_emitted() {
    let output = compile_with_debug_info(
        "-> k\n=== k ===\nBefore: {n}.\n~ temp n = 0\n-> END\n",
        true,
    );
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    while let Ok(Step::Line(_)) = story.continue_single() {}
    let warnings: Vec<String> = story
        .take_runtime_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("Variable not found: 'n'."),
        "{warnings:?}"
    );
}

/// The stitch half of the same hole — a knot-root temp read from a stitch
/// entered directly. Also a phantom global before the fix.
#[test]
fn stitch_read_of_a_knot_root_temp_resolves_to_the_temp_slot_and_plays() {
    let (lines, warnings) = play(
        "-> k.s\n=== k ===\n~ temp n = 7\n-> END\n= s\nStitch sees {n}.\n-> END\n",
        &[],
    );
    assert_eq!(lines.join(""), "Stitch sees 0.\n");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

/// The write half of the same hole (`lower_assign_target`): `~ n = 1`
/// ahead of the declaration named the same phantom global and died at link
/// the same way. It must write the frame's slot.
#[test]
fn textually_early_assignment_writes_the_temp_slot_and_plays() {
    let (lines, warnings) = play(
        "-> k\n=== k ===\n~ n = 3\nSaw {n}.\n~ temp n = 0\nThen {n}.\n-> END\n",
        &[],
    );
    assert_eq!(
        lines.join(""),
        "Saw 3.\nThen 0.\n",
        "the early write lands in the slot the declaration later resets"
    );
    assert!(
        warnings.is_empty(),
        "the slot was written before it was read: {warnings:?}"
    );
}

/// The `ref`-argument half (`lower_ref_path_call_arg`): passing a
/// not-yet-declared temp by `ref` emitted `RefGlobal` against the same
/// unregistered id.
#[test]
fn textually_early_ref_argument_binds_the_temp_slot_and_plays() {
    let (lines, _) = play(
        "-> k\n=== k ===\n~ bump(n)\nSaw {n}.\n~ temp n = 0\n-> END\n\
         === function bump(ref x) ===\n~ x = 9\n",
        &[],
    );
    assert_eq!(
        lines.join(""),
        "Saw 9.\n",
        "the callee wrote through to this frame's slot"
    );
}

// ─── The runtime fallback ───────────────────────────────────────────

/// #3354's own repro, played the way the reporter did: pick `peek` first.
/// The C# runtime prints `Peek: 1.` plus a missing-variable warning; brink
/// used to die with `type error: cannot apply Add to Null and Int`.
#[test]
fn sibling_branch_read_plays_the_way_inky_plays_it() {
    let (lines, warnings) = play(
        "-> k\n=== k ===\n- (top)\n+ [declare]\n    ~ temp n = 0\n    -> top\n\
         + [peek]\n    Peek: {n + 1}.\n    -> top\n+ [quit] -> END\n",
        &[1, 2],
    );
    assert!(
        lines.iter().any(|l| l.contains("Peek: 1.")),
        "the story keeps going, exactly as Inky plays it: {lines:?}"
    );
    assert!(!warnings.is_empty(), "and says so: {warnings:?}");
    assert!(
        warnings[0].contains("default value of 0"),
        "the warning states the substituted default: {warnings:?}"
    );
}

/// A declared-then-read temp is initialized, so no runtime warning rides
/// along with it — the fallback must not fire on the ordinary path.
#[test]
fn an_initialized_temp_read_emits_no_runtime_warning() {
    let (lines, warnings) = play("-> k\n=== k ===\n~ temp n = 4\nSaw {n}.\n-> END\n", &[]);
    assert_eq!(lines.join(""), "Saw 4.\n");
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// BLOCKER fix: `Opcode::GetTemp`'s "was this ever written" check must key
/// on `CallFrame::is_temp_written`, not on the stored value being
/// `Value::Null` — a `~ temp x = f()` whose `f` falls off its end without
/// `~ return` legitimately stores `Value::Null` into a slot `DeclareTemp`
/// really did write. Keying on the value alone misclassifies that as
/// "never written" and both fires a false `Variable not found` warning and
/// substitutes `0` for the void return — flipping `{x == 0: ZERO|NOTZERO}`
/// from `ZERO` to `NOTZERO` and changing what plays. Matches the C#
/// reference, which prints the void return as empty text with no warning.
#[test]
fn temp_declared_from_a_void_returning_function_is_not_misread_as_uninitialized() {
    let (lines, warnings) = play_ink(
        "-> k\n=== k ===\n~ temp x = f()\nGot [{x}].\n-> END\n\
         === function f() ===\n~ return\n",
        &[],
    );
    assert_eq!(
        lines.join(""),
        "Got [].\n",
        "a void return plays as empty text, exactly like main, no substituted `0`"
    );
    assert!(
        warnings.is_empty(),
        "the declaration DID run — 'Variable not found' would be false: {warnings:?}"
    );
}

/// Same shape, checked through a comparison rather than interpolation: the
/// void-returned `Value::Null` must still compare as `Null`, not as the
/// substituted `Int(0)` the uninitialized-read fallback pushes.
#[test]
fn temp_declared_from_a_void_returning_function_does_not_compare_as_zero() {
    let (lines, warnings) = play_ink(
        "-> k\n=== k ===\n~ temp x = f()\n{x == 0: ZERO|NOTZERO}\n-> END\n\
         === function f() ===\n~ return\n",
        &[],
    );
    assert_eq!(
        lines.join(""),
        "NOTZERO\n",
        "Null does not equal 0 — matching main's behavior"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}
