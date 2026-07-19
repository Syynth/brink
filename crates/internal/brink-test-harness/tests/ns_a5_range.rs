//! NS-A5 range-value + inhabited-range-refinement integration tests
//! (issue #1111, `docs/stdlib-spec.md` §7 — F7/F8 ruled 2026-07-19;
//! `docs/stdlib-sequencing.md` §2 Wave A5).
//!
//! The wave's gate, end-to-end through the real compiler + runtime:
//!
//! - **Ranges are a real Value kind (F7)**: literals in both written
//!   forms, content equality (`1..=6 == 1..7`), the `0..10` display form,
//!   membership in the closed iterable set (`for i in 0..n`), and a
//!   durable wire form (a range in a global survives a serde `SaveState`
//!   round-trip — the FlowFrame-spill durability the ruling demanded).
//! - **The refinement (F8's template)**: under `types = strict`, `int(r)`
//!   demands `NonEmptyRange` evidence — a provably-empty literal is E117,
//!   provably-inhabited literals (CONST refs folded) coerce free, computed
//!   bounds must route through `non_empty(r)`. Under gradual the checks
//!   are INERT and the runtime fault (`EmptyRangeDraw`) is the residual.
//! - **The draw verbs**: `int(1..=6)` draws once, in-bounds, seeded-
//!   deterministic; `pick(range)` → `Option[int]`, empty → `none`.
//! - **Dialect gate**: range syntax is a brink extension — E051 under
//!   strict-ink.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::{DotNetRng, Line, RuntimeError, Story};
use std::collections::HashMap;
use std::sync::Arc;

fn compile_opts(
    source: &str,
    options: AnalysisOptions,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
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
        options,
    )
}

fn compile_brink(source: &str) -> brink_compiler::CompileOutput {
    compile_opts(
        source,
        AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        },
    )
    .expect("gradual brink compile must succeed")
}

fn compile_strict(
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    compile_opts(
        source,
        AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        },
    )
}

fn strict_diag_codes(source: &str) -> Vec<String> {
    match compile_strict(source) {
        Ok(_) => Vec::new(),
        Err(brink_compiler::CompileError::Diagnostics(diags)) => {
            diags.iter().map(|d| format!("{:?}", d.code)).collect()
        }
        Err(other) => panic!("unexpected compile error: {other:?}"),
    }
}

fn story_of(source: &str) -> Story<DotNetRng> {
    let output = compile_brink(source);
    let (program, tables) = brink_runtime::link(&output.data).expect("link");
    Story::<DotNetRng>::new(Arc::new(program), tables)
}

/// Run a straight-line story to `Done`/`End`, returning accumulated text.
fn run_to_end(story: &mut Story<DotNetRng>) -> String {
    let mut out = String::new();
    loop {
        match story.continue_single().expect("runtime error") {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Choices { .. } => panic!("straight-line story hit a choice"),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                return out;
            }
        }
    }
}

/// Run until the story faults, returning the error.
fn run_to_error(story: &mut Story<DotNetRng>) -> RuntimeError {
    loop {
        match story.continue_single() {
            Ok(Line::Text { .. }) => {}
            Ok(other) => panic!("expected a fault, story ended cleanly: {other:?}"),
            Err(e) => return e,
        }
    }
}

// ── F7: ranges are a real Value kind ─────────────────────────────────────

#[test]
fn range_display_preserves_the_written_form() {
    let mut story = story_of(
        "~ temp a = 0..10\n\
         ~ temp b = 1..=6\n\
         a: {string(a)}\n\
         b: {string(b)}\n\
         -> END\n",
    );
    assert_eq!(run_to_end(&mut story), "a: 0..10\nb: 1..=6\n");
}

#[test]
fn range_equality_is_content_equality() {
    // 1..=6 == 1..7 (same integer sequence, different spelling); every
    // empty range equals every other empty range; inequality otherwise.
    let mut story = story_of(
        "~ temp incl = 1..=6\n\
         ~ temp excl = 1..7\n\
         same: {incl == excl}\n\
         diff: {1..3 == 1..4}\n\
         empties: {0..0 == 5..5}\n\
         ne: {1..3 != 2..3}\n\
         -> END\n",
    );
    assert_eq!(
        run_to_end(&mut story),
        "same: true\ndiff: false\nempties: true\nne: true\n"
    );
}

#[test]
fn range_len_and_indexing() {
    let mut story = story_of(
        "~ temp r = 3..=7\n\
         len: {len(r)}\n\
         first: {r[0]}\n\
         last: {r[4]}\n\
         empty len: {len(9..9)}\n\
         -> END\n",
    );
    assert_eq!(
        run_to_end(&mut story),
        "len: 5\nfirst: 3\nlast: 7\nempty len: 0\n"
    );
}

#[test]
fn for_over_range_iterates_elements_and_empty_runs_zero_times() {
    let mut story = story_of(
        "~ temp acc = \"\"\n\
         ~ {\n\
         for i in 2..=5 {\n\
         acc = acc + \" \" + string(i)\n\
         }\n\
         for j in 0..0 {\n\
         acc = acc + \" never\"\n\
         }\n\
         for k in 0..3 {\n\
         acc = acc + \"|\" + string(k)\n\
         }\n\
         }\n\
         got:{acc}\n\
         -> END\n",
    );
    assert_eq!(run_to_end(&mut story), "got: 2 3 4 5|0|1|2\n");
}

#[test]
fn for_over_range_with_computed_bound() {
    // `for i in 0..n` with n = 0 runs zero times — emptiness is
    // load-bearing for iteration (the spec's own example).
    let mut story = story_of(
        "~ temp n = 0\n\
         ~ temp acc = \"x\"\n\
         ~ {\n\
         for i in 0..n {\n\
         acc = acc + string(i)\n\
         }\n\
         }\n\
         {acc}\n\
         -> END\n",
    );
    assert_eq!(run_to_end(&mut story), "x\n");
}

#[test]
fn range_survives_a_save_state_serde_round_trip() {
    // The F7 durability gate: a range value parked in a global crosses a
    // serde SaveState wire trip bit-for-bit (written form included) and
    // is still the same value afterwards.
    let src = "VAR r = 0\n\
               ~ r = 2..=9\n\
               before: {string(r)}\n\
               -> turn\n\
               === turn ===\n\
               + [go] -> after\n\
               === after ===\n\
               after: {string(r)} {len(r)} {r == 2..10}\n\
               -> END\n";
    let output = compile_brink(src);
    let (program, tables) = brink_runtime::link(&output.data).expect("link");
    let program = Arc::new(program);

    let mut a = Story::<DotNetRng>::new(Arc::clone(&program), tables.clone());
    let mut head = String::new();
    loop {
        match a.continue_single().expect("runtime error") {
            Line::Text { text, .. } => head.push_str(&text),
            Line::Choices { text, .. } => {
                head.push_str(&text);
                break;
            }
            other => panic!("expected a choice, got {other:?}"),
        }
    }
    assert_eq!(head, "before: 2..=9\n");

    let save = a.save_state();
    let json = serde_json::to_string(&save).expect("serialize");
    let restored: brink_format::SaveState = serde_json::from_str(&json).expect("deserialize");

    let mut b = Story::<DotNetRng>::new(program, tables);
    // Advance B to the same choice point, then load the wire-tripped save.
    loop {
        match b.continue_single().expect("runtime error") {
            Line::Choices { .. } => break,
            Line::Text { .. } => {}
            other => panic!("expected a choice, got {other:?}"),
        }
    }
    let report = b.load_state(&restored);
    assert!(
        report.unknown_globals.is_empty(),
        "clean load, got {report:?}"
    );
    b.choose(0).expect("choose");
    let mut tail = String::new();
    loop {
        match b.continue_single().expect("runtime error") {
            Line::Text { text, .. } => tail.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } => {
                tail.push_str(&text);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(tail, "after: 2..=9 8 true\n");
}

// ── The draw verbs: `int(range)` + pick's range leg ──────────────────────

#[test]
fn int_over_a_range_draws_seeded_deterministic_and_in_bounds() {
    let src = "~ seed(7)\n\
               ~ temp acc = \"\"\n\
               ~ {\n\
               for i in 0..20 {\n\
               acc = acc + \" \" + string(int(1..=6))\n\
               }\n\
               }\n\
               rolls:{acc}\n\
               excl: {int(0..3) < 3}\n\
               single: {int(5..=5)}\n\
               -> END\n";
    let mut a = story_of(src);
    let mut b = story_of(src);
    let out_a = run_to_end(&mut a);
    assert_eq!(out_a, run_to_end(&mut b), "seeded replay must be identical");
    // Every roll of 1..=6 lands in bounds.
    let rolls_line = out_a.lines().next().expect("rolls line");
    let rolls: Vec<i32> = rolls_line
        .trim_start_matches("rolls:")
        .split_whitespace()
        .map(|t| t.parse().expect("int roll"))
        .collect();
    assert_eq!(rolls.len(), 20);
    assert!(rolls.iter().all(|r| (1..=6).contains(r)), "{rolls:?}");
    assert!(out_a.contains("excl: true"));
    assert!(out_a.contains("single: 5"));
}

#[test]
fn int_conversion_leg_is_untouched() {
    // `int(x)` over a non-range operand keeps its TM-3 conversion
    // semantics — one verb, two legs, dispatch on the value.
    let mut story = story_of(
        "conv: {int(\"41\") + int(1.9)}\n\
         -> END\n",
    );
    assert_eq!(run_to_end(&mut story), "conv: 42\n");
}

#[test]
fn pick_over_a_range_is_option_int_and_empty_is_none() {
    // NOTE: `||` can't appear inside a content interpolation (`{a|b}` is
    // ink's alternatives construct), so the membership test computes in a
    // `~` temp first.
    let mut story = story_of(
        "~ seed(3)\n\
         ~ temp p = pick(10..13)\n\
         ~ temp ok = p == some(10) || p == some(11) || p == some(12)\n\
         picked in bounds: {ok}\n\
         empty: {string(pick(4..4))}\n\
         -> END\n",
    );
    assert_eq!(
        run_to_end(&mut story),
        "picked in bounds: true\nempty: none\n"
    );
}

#[test]
fn non_empty_validator_mints_some_or_none() {
    let mut story = story_of(
        "~ temp lo = 1\n\
         ~ temp hi = 6\n\
         ok: {string(non_empty(lo..=hi))}\n\
         empty: {string(non_empty(hi..lo))}\n\
         -> END\n",
    );
    assert_eq!(run_to_end(&mut story), "ok: some(1..=6)\nempty: none\n");
}

// ── F8: the gradual-mode residual ────────────────────────────────────────

#[test]
fn gradual_empty_range_draw_is_a_turn_terminating_fault() {
    // Under gradual the refinement is INERT — `int(0..0)` compiles — and
    // the runtime fault is the residual. THE template for every future
    // refinement (F8's general rule).
    let mut story = story_of("~ temp x = int(0..0)\n{x}\n-> END\n");
    let err = run_to_error(&mut story);
    assert!(
        matches!(err, RuntimeError::EmptyRangeDraw { ref range } if range == "0..0"),
        "expected EmptyRangeDraw, got {err:?}"
    );
    // Backwards inclusive form faults too, with its own written form.
    let mut story = story_of("~ temp x = int(9..=2)\n{x}\n-> END\n");
    let err = run_to_error(&mut story);
    assert!(
        matches!(err, RuntimeError::EmptyRangeDraw { ref range } if range == "9..=2"),
        "expected EmptyRangeDraw, got {err:?}"
    );
}

#[test]
fn gradual_range_make_faults_on_non_int_bounds() {
    let mut story = story_of("~ temp r = 1..\"six\"\nnever: {string(r)}\n-> END\n");
    let err = run_to_error(&mut story);
    assert!(
        matches!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "range",
                found: "string",
                ..
            }
        ),
        "expected range-bound fault, got {err:?}"
    );
}

// ── E117: the strict-mode evidence machinery ─────────────────────────────

#[test]
fn strict_provably_empty_literal_is_e117() {
    for src in [
        "~ temp x = int(0..0)\n{x}\n-> END\n",
        "~ temp x = int(5..=2)\n{x}\n-> END\n",
        // CONST refs fold (the F7 evidence rule): LO..LO is provably empty.
        "CONST LO = 3\n~ temp x = int(LO..LO)\n{x}\n-> END\n",
    ] {
        let codes = strict_diag_codes(src);
        assert!(
            codes.iter().any(|c| c == "E117"),
            "expected E117 for {src:?}, got {codes:?}"
        );
    }
}

#[test]
fn strict_provably_inhabited_literals_coerce_free() {
    for src in [
        "~ temp x = int(1..=6)\n{x}\n-> END\n",
        "~ temp x = int(5..=5)\n{x}\n-> END\n",
        "~ temp x = int(-3..3)\n{x}\n-> END\n",
        // CONST refs fold: 1..=SIDES is provably inhabited.
        "CONST SIDES = 6\n~ temp x = int(1..=SIDES)\n{x}\n-> END\n",
        // Evidence flows through a temp whose initializer is a
        // provably-inhabited literal (inside a knot, where the inference
        // substrate can classify the temp).
        "-> play\n=== play ===\n~ temp die = 1..=6\n~ temp x = int(die)\n{x}\n-> END\n",
    ] {
        let codes = strict_diag_codes(src);
        assert!(
            !codes.iter().any(|c| c == "E117"),
            "unexpected E117 for {src:?}: {codes:?}"
        );
    }
}

#[test]
fn strict_unproven_bounds_are_e117() {
    for src in [
        // Computed bounds written literally in position — no evidence.
        "~ temp n = 6\n~ temp x = int(1..n)\n{x}\n-> END\n",
        // A possibly-empty range-typed temp — no evidence on its type.
        // (Inside a knot: the inference substrate finalizes locals per
        // def, the same classification scope `conversions`' E078 uses.)
        "-> play\n=== play ===\n~ temp n = 0\n~ temp r = 1..n\n~ temp x = int(r)\n{x}\n-> END\n",
    ] {
        let codes = strict_diag_codes(src);
        assert!(
            codes.iter().any(|c| c == "E117"),
            "expected E117 for {src:?}, got {codes:?}"
        );
    }
}

#[test]
fn strict_author_shadowed_int_is_not_checked() {
    // An author-defined `int` function shadows the builtin — an ordinary
    // call, never E117 (the same shadow-fallback discipline every stdlib
    // intrinsic follows).
    let src = "~ temp x = int(0..0)\nresult: {x}\n-> END\n\
               === function int(r) ===\n\
               ~ return 99\n";
    let codes = strict_diag_codes(src);
    assert!(
        !codes.iter().any(|c| c == "E117"),
        "shadowed int must not be refinement-checked, got {codes:?}"
    );
}

#[test]
fn gradual_never_emits_e117() {
    // F8: refinements are inert in gradual mode — even the provably-empty
    // literal compiles (and faults at runtime instead).
    let output = compile_brink("~ temp x = int(0..0)\n{x}\n-> END\n");
    drop(output); // compiling at all is the assertion
}

// ── E051: the dialect gate ───────────────────────────────────────────────

#[test]
fn strict_ink_rejects_range_syntax_with_e051() {
    let src = "~ temp r = 0\n~ r = 1..6\n{r}\n-> END\n";
    let err = compile_opts(
        src,
        AnalysisOptions {
            dialect: Dialect::StrictInk,
            ..AnalysisOptions::default()
        },
    )
    .expect_err("strict-ink must reject range syntax");
    let brink_compiler::CompileError::Diagnostics(diags) = err else {
        panic!("expected diagnostics");
    };
    assert!(
        diags.iter().any(|d| format!("{:?}", d.code) == "E051"),
        "expected E051, got {diags:?}"
    );
}
