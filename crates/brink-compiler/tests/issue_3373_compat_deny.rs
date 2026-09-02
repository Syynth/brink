//! Issue #3373 (RULED 2026-09-01): the compat-deny diagnostic tier and its
//! first member, `E194` — a knot's `~ temp` (native `~ let`) read from one
//! of that knot's stitches.
//!
//! Split out of `E193`'s former shape 4 during PR #3369's review: the
//! program plays correctly (`lir::lower::temps::alloc_temps` shares one
//! call frame across a knot and its stitches), but inklecate rejects the
//! identical program outright (`Unresolved variable`) rather than warning
//! on it — so it is not a dominance defect at all, it is brink accepting a
//! genuine superset of ink.
//!
//! This file pins, against real `.ink` source compiled and run through the
//! production pipeline:
//!
//! 1. **The admission invariant itself** — `docs/compiler-spec.md`
//!    "Compat-deny diagnostics" requires every member to have a fixture
//!    that compiles under `allow` and plays correctly. `E194`'s own fixture
//!    is here, both directions: default-`Error` refuses to compile, and
//!    `allow`/`warn` compile and play `Stitch sees 7.`.
//! 2. **`[lints]`/CLI reach**, through the real
//!    `AnalysisOptions::apply_lint_overrides` gate — `deny`/`warn`/`allow`
//!    all resolve the way `docs/compiler-spec.md`'s mechanics section says.
//! 3. **What does not fire** — a stitch's own declaration shadows the
//!    knot's, a stitch parameter is never mistaken for it, and reading the
//!    knot's own temp from its own root is untouched.
//! 4. **The native-surface message**, on a real `.brink` file compiled
//!    through `compile_path` — the same `is_native` wiring `E193`'s own
//!    coverage (`issue_3354_temp_dominance.rs`) already exercises.
//! 5. **Plain writes, not just reads** (PR #3369 review, second round) — a
//!    stitch's plain assignment (`~ n = 9`) to its knot root's temp must
//!    fire exactly like a read, on both surfaces, with its own message
//!    wording quoting inklecate's assignment-rejection text rather than its
//!    read-rejection text.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use brink_analyzer::LintLevel;
use brink_compiler::{AnalysisOptions, CompileError, DiagnosticCode, Dialect, Severity};
use brink_runtime::{DotNetRng, Step, Story};

/// The repro from issue #3373's own body: a knot declares `~ temp n`, its
/// root diverts into a stitch (running the declaration first), and the
/// stitch reads the name. Brink plays `Stitch sees 7.`; inklecate rejects
/// the program with `Unresolved variable: n`.
const ADMISSION_FIXTURE: &str =
    "-> k\n=== k ===\n~ temp n = 7\n-> s\n= s\nStitch sees {n}.\n-> END\n";

/// `Dialect::StrictInk` (types = `Gradual`), not `Dialect::Brink`: `E194` is
/// dialect- and type-policy-independent (it fires from the HIR block tree
/// alone), and several fixtures below use an untyped stitch parameter that
/// would otherwise trip TM-3's unrelated strict-mode `E065` escape check.
/// `is_native` (which the message's vocabulary keys on) comes from the
/// entry file's extension, not this enum, so switching it does not affect
/// which vocabulary a message uses.
fn compile_with(
    source: &str,
    overrides: &[(&str, LintLevel)],
) -> Result<brink_compiler::CompileOutput, CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let mut options = AnalysisOptions {
        dialect: Dialect::StrictInk,
        ..AnalysisOptions::default()
    };
    let map: BTreeMap<String, LintLevel> = overrides
        .iter()
        .map(|(code, level)| ((*code).to_owned(), *level))
        .collect();
    let warnings = options.apply_lint_overrides(&map, None);
    assert!(
        warnings.is_empty(),
        "an override this test named was rejected: {warnings:?}"
    );
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
        options,
    )
}

/// Every `E194` message a successful compile produced.
fn e194_messages(source: &str, overrides: &[(&str, LintLevel)]) -> Vec<String> {
    compile_with(source, overrides)
        .expect("compile should succeed with these overrides")
        .warnings
        .into_iter()
        .filter(|w| w.code == DiagnosticCode::E194)
        .map(|w| w.message)
        .collect()
}

fn play(source: &str, overrides: &[(&str, LintLevel)]) -> (Vec<String>, Vec<String>) {
    let output = compile_with(source, overrides).expect("compile should succeed");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut lines = Vec::new();
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(line) => lines.push(line.text),
            Step::Done => {}
            Step::Choices(_) | Step::End | Step::Suspended => break,
        }
    }
    let warnings = story
        .take_runtime_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    (lines, warnings)
}

// ─── The admission invariant ─────────────────────────────────────────

/// Default severity: `Error`. By default the fixture does not produce a
/// program at all — matching inklecate's own rejection.
#[test]
fn default_severity_refuses_to_compile() {
    match compile_with(ADMISSION_FIXTURE, &[]) {
        Err(CompileError::Diagnostics(diags)) => {
            let e194s: Vec<_> = diags
                .iter()
                .filter(|d| d.code == DiagnosticCode::E194)
                .collect();
            assert_eq!(e194s.len(), 1, "{diags:?}");
            assert_eq!(
                e194s[0].severity,
                Severity::Error,
                "must be reported at Error, matching inklecate's own rejection"
            );
        }
        other => panic!("expected CompileError::Diagnostics with E194, got {other:?}"),
    }
}

/// The admission invariant's own proof: downgraded to `allow`, the fixture
/// compiles AND plays correctly — the exact repro from the issue body.
#[test]
fn downgraded_to_allow_compiles_and_plays_correctly() {
    let (lines, warnings) = play(ADMISSION_FIXTURE, &[("E194", LintLevel::Allow)]);
    assert_eq!(lines.join(""), "Stitch sees 7.\n");
    assert!(
        warnings.is_empty(),
        "the declaration already ran before the divert reached the stitch — \
         no runtime fallback should fire: {warnings:?}"
    );
}

/// `warn` is also an admissible downgrade (the ruling: "warn, or all the
/// way to allow") — the program compiles, carries the warning, and still
/// plays correctly.
#[test]
fn downgraded_to_warn_compiles_carries_the_warning_and_plays_correctly() {
    let msgs = e194_messages(ADMISSION_FIXTURE, &[("E194", LintLevel::Warn)]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("knot `k`"), "{msgs:?}");
    assert!(
        msgs[0].contains("Unresolved variable"),
        "names what inklecate itself would say: {msgs:?}"
    );

    let (lines, warnings) = play(ADMISSION_FIXTURE, &[("E194", LintLevel::Warn)]);
    assert_eq!(lines.join(""), "Stitch sees 7.\n");
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// `deny` is a no-op restatement of the default — still `Error`, still
/// refuses to compile.
#[test]
fn denied_explicitly_still_refuses_to_compile() {
    match compile_with(ADMISSION_FIXTURE, &[("E194", LintLevel::Deny)]) {
        Err(CompileError::Diagnostics(diags)) => {
            assert!(diags.iter().any(|d| d.code == DiagnosticCode::E194));
        }
        other => panic!("expected CompileError::Diagnostics, got {other:?}"),
    }
}

// ─── Plain writes fire too (PR #3369 review) ──────────────────────────
//
// `ReadCollector::enter_stmt` discounts a plain (`~ n = …`) assignment
// target's range from `E193`'s dominance question — correctly, since `E193`
// is about reads. `check_knot` used to inherit that same discount wholesale,
// so `~ n = 9` in a stitch, assigning to a name only the knot's root
// declares, compiled with zero diagnostics on either road — even though
// assigning still has to resolve `n` to a slot, and inklecate rejects that
// resolution too (`Variable could not be found to assign to: 'n'`,
// `ParsedHierarchy/VariableAssignment.cs`). The exact repro from the
// review, verified against origin/main pre-fix as a loud runtime failure
// (`unresolved global: $07_c97cb87a72101c` at play time) rather than a
// caught compile-time diagnostic.

/// The reviewer's own repro: a plain write, not a read, to a knot-root temp
/// from inside a stitch. Refuses to compile by default, exactly like the
/// read shape.
const WRITE_FIXTURE: &str =
    "-> k\n=== k ===\n~ temp n = 7\n-> s\n= s\n~ n = 9\nKnot temp is now assigned.\n-> END\n";

#[test]
fn a_plain_write_to_the_knot_root_temp_is_e194() {
    match compile_with(WRITE_FIXTURE, &[]) {
        Err(CompileError::Diagnostics(diags)) => {
            let e194s: Vec<_> = diags
                .iter()
                .filter(|d| d.code == DiagnosticCode::E194)
                .collect();
            assert_eq!(e194s.len(), 1, "{diags:?}");
            assert_eq!(e194s[0].severity, Severity::Error, "{diags:?}");
        }
        other => panic!("expected CompileError::Diagnostics with E194, got {other:?}"),
    }
}

/// The message must say "written", not "read" (a write is not a read), and
/// must quote inklecate's OWN assignment-rejection wording — different
/// from the read shape's `Unresolved variable`.
#[test]
fn a_plain_write_message_says_written_and_quotes_the_assignment_rejection() {
    let msgs = e194_messages(WRITE_FIXTURE, &[("E194", LintLevel::Warn)]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        msgs[0].contains("`n` is written here"),
        "must say written, not read: {msgs:?}"
    );
    assert!(
        msgs[0].contains("Variable could not be found to assign to: 'n'"),
        "must quote inklecate's own assignment rejection, not its read rejection: {msgs:?}"
    );
}

/// Downgraded, the write compiles and plays exactly as the review verified
/// on the built CLI: `Knot temp is now assigned.`
#[test]
fn a_plain_write_downgraded_to_allow_compiles_and_plays_correctly() {
    let (lines, warnings) = play(WRITE_FIXTURE, &[("E194", LintLevel::Allow)]);
    assert_eq!(lines.join(""), "Knot temp is now assigned.\n");
    assert!(
        warnings.is_empty(),
        "the write lands in the knot's own shared frame slot cleanly: {warnings:?}"
    );
}

/// Native surface: the same write shape, spelled `~ let`/`flow`, must also
/// fire — not just the read shape native coverage already pins.
#[test]
fn native_surface_plain_write_is_e194() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-e194-native-write-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "flow main() {\n  ~ let n = 7\n  flow s() {\n    ~ n = 9\n    Knot temp is now assigned.\n    -> END\n  }\n  -> s\n}\n",
    )
    .unwrap();
    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();
    let diags = match result {
        Err(CompileError::Diagnostics(diags)) => diags,
        other => panic!("expected CompileError::Diagnostics, got {other:?}"),
    };
    let msgs: Vec<&String> = diags
        .iter()
        .filter(|w| w.code == DiagnosticCode::E194)
        .map(|w| &w.message)
        .collect();
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        msgs[0].contains("`n` is written here"),
        "must say written, not read: {msgs:?}"
    );
    assert!(
        msgs[0].contains("`~ let n`") && msgs[0].contains("flow `main`"),
        "must name the declaration/enclosing definition the author wrote: {msgs:?}"
    );
}

// ─── What does not fire ───────────────────────────────────────────────

/// A stitch that declares its own `~ temp` of the same name shadows the
/// knot's for its own reads — that is `E193`'s dominance question, not
/// this one.
#[test]
fn a_stitch_declaring_its_own_temp_of_the_same_name_is_not_e194() {
    let msgs = e194_messages(
        "-> k\n=== k ===\n~ temp n = 7\n-> s\n= s\n~ temp n = 9\nStitch sees {n}.\n-> END\n",
        &[],
    );
    assert!(
        msgs.is_empty(),
        "the stitch's own declaration shadows the knot's: {msgs:?}"
    );
}

/// A stitch parameter of the same name as the knot's temp is bound at call
/// time, not read from the knot's frame.
#[test]
fn a_stitch_parameter_of_the_same_name_is_not_e194() {
    let msgs = e194_messages(
        "-> k\n=== k ===\n~ temp n = 7\n-> s(1)\n= s(n)\nStitch sees {n}.\n-> END\n",
        &[],
    );
    assert!(msgs.is_empty(), "a parameter read is never this: {msgs:?}");
}

/// Reading the knot's own root temp from the knot's own root body — no
/// stitch involved at all — is untouched.
#[test]
fn reading_the_knot_root_temp_from_the_knot_root_is_not_e194() {
    let msgs = e194_messages("-> k\n=== k ===\n~ temp n = 7\nSaw {n}.\n-> END\n", &[]);
    assert!(msgs.is_empty(), "{msgs:?}");
}

/// A knot with no stitches at all is never a candidate.
#[test]
fn a_knot_with_no_stitches_is_not_e194() {
    let msgs = e194_messages("-> k\n=== k ===\n~ temp n = 7\nSaw {n}.\n-> END\n", &[]);
    assert!(msgs.is_empty(), "{msgs:?}");
}

/// A knot that HAS stitches, but whose root declares no `~ temp` at all,
/// hits `check_knot`'s OTHER early return (`knot_decls.is_empty()`) — a
/// different code path than `a_knot_with_no_stitches_is_not_e194`'s
/// `hir.knots`/`knot.stitches.is_empty()` guard in `check`.
#[test]
fn a_knot_with_stitches_but_no_root_temp_is_not_e194() {
    let msgs = e194_messages("-> k\n=== k ===\n-> s\n= s\nSaw the stitch.\n-> END\n", &[]);
    assert!(
        msgs.is_empty(),
        "no root-level `~ temp` means nothing for a stitch to shadow: {msgs:?}"
    );
}

/// A stitch reading a name that is a genuine project `VAR`, not a knot
/// temp of the same name, is untouched — `E194` is specifically about a
/// knot's OWN `~ temp`.
#[test]
fn a_stitch_reading_an_unrelated_var_is_not_e194() {
    let msgs = e194_messages(
        "VAR n = 5\n-> k\n=== k ===\n-> s\n= s\nStitch sees {n}.\n-> END\n",
        &[],
    );
    assert!(
        msgs.is_empty(),
        "a VAR read is not this diagnostic's subject: {msgs:?}"
    );
}

/// Entering the stitch directly (`-> k.s`, never running the knot root's
/// declaration) fires exactly the same as diverting through the root —
/// `E194` fires unconditionally, dominance aside, because ink's own
/// compiler never extends a knot's `~ temp` visibility into its stitches
/// regardless of the runtime path.
#[test]
fn firing_is_unconditional_on_the_runtime_path() {
    // Both refuse to compile at all by default, so read the messages via a
    // `warn` downgrade instead — the point here is that both fire exactly
    // once, not what the default severity does (that is
    // `default_severity_refuses_to_compile`'s job).
    let via_root = e194_messages(ADMISSION_FIXTURE, &[("E194", LintLevel::Warn)]);
    let direct = e194_messages(
        "-> k.s\n=== k ===\n~ temp n = 7\n-> END\n= s\nStitch sees {n}.\n-> END\n",
        &[("E194", LintLevel::Warn)],
    );
    assert_eq!(via_root.len(), 1, "{via_root:?}");
    assert_eq!(direct.len(), 1, "{direct:?}");
}

// ─── Native surface ─────────────────────────────────────────────────

/// The native surface spells the declaration `~ let` inside a nested
/// stitch-shaped `flow`, and the message must name it that way — not ink's
/// `~ temp`/`knot` vocabulary — mirroring `E193`'s own native-vocabulary
/// coverage (`issue_3354_temp_dominance.rs`'s
/// `native_surface_message_says_let_and_flow_not_temp_and_knot`).
#[test]
fn native_surface_message_says_let_and_flow_not_temp_and_knot() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-e194-native-vocabulary-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "flow main() {\n  ~ let n = 7\n  flow s() {\n    Stitch sees {n}.\n    -> END\n  }\n  -> s\n}\n",
    )
    .unwrap();
    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();
    // Default severity is Error, so this is expected to refuse to compile —
    // the diagnostic still needs to be findable in that refusal.
    let diags = match result {
        Err(CompileError::Diagnostics(diags)) => diags,
        other => panic!("expected CompileError::Diagnostics, got {other:?}"),
    };
    let msgs: Vec<&String> = diags
        .iter()
        .filter(|w| w.code == DiagnosticCode::E194)
        .map(|w| &w.message)
        .collect();
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        msgs[0].contains("`~ let n`"),
        "must name the declaration the author actually wrote: {msgs:?}"
    );
    assert!(
        msgs[0].contains("flow `main`"),
        "must name the enclosing definition kind the author actually wrote: {msgs:?}"
    );
    assert!(
        !msgs[0].contains("~ temp") && !msgs[0].contains("knot `main`"),
        "must not speak ink vocabulary on the native surface: {msgs:?}"
    );
}
