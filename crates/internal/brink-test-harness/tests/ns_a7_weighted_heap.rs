//! NS-A7 `Weighted[T]` + heap-verb integration tests (issue #1113,
//! `docs/stdlib-spec.md` §8 — Collections+ ruled 2026-07-18, F17 multiset
//! policy; `docs/stdlib-sequencing.md` §2 Wave A7).
//!
//! The wave's gate, end-to-end through the real compiler + runtime:
//!
//! - **Evidence-by-construction (the E078-style split)**: statically
//!   classifiable refusals — empty tables, dangling weights, literal
//!   zero/negative/non-int weights — are **E120 compile errors** in both
//!   type regimes (the check lives at the lowering, not the checker);
//!   computed weights carry the **construction fault** residual
//!   (`WeightedBadWeight`) at runtime, so `roll` over any table that
//!   exists is total.
//! - **F17 multiset policy**: duplicate weights are legal and meaningful;
//!   equality is order-insensitive multiset content.
//! - **`roll` is a draw**: seeded-deterministic through the one RNG cell,
//!   always lands on an entry.
//! - **The humble heap**: `heap_push`/`heap_pop`/`heap_peek` over an
//!   ordinary `[T]`, min-heap by the §4b doctrine order; empty pops are
//!   absence (`none`); `heap_push` carries the dev NaN entry fault / prod
//!   pinned placement (the `ExecMode` knob); the statement/expression
//!   split is enforced (E056 for `heap_push` in expression position, E055
//!   for a non-bare `heap_pop` receiver).
//! - **Wire durability**: a `Weighted` parked in a global survives a
//!   serde `SaveState` round-trip.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::{DotNetRng, ExecMode, Line, RuntimeError, Story};
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
            types: Some(TypePolicy::Gradual),
            ..AnalysisOptions::default()
        },
    )
    .expect("gradual brink compile must succeed")
}

/// Diagnostic codes from a compile under the given policy (empty = clean).
fn diag_codes(source: &str, policy: TypePolicy) -> Vec<String> {
    match compile_opts(
        source,
        AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(policy),
            ..AnalysisOptions::default()
        },
    ) {
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

// ── Weighted[T]: construction, display, the F17 multiset ────────────────

#[test]
fn weighted_displays_as_the_construction_literal() {
    let mut story = story_of(
        "~ temp loot = weighted(3, \"sword\", 1, \"shield\")\n\
         table: {loot}\n\
         string: {string(loot)}\n\
         -> END\n",
    );
    assert_eq!(
        run_to_end(&mut story),
        "table: Weighted { 3: sword, 1: shield }\n\
         string: Weighted { 3: sword, 1: shield }\n"
    );
}

#[test]
fn weighted_equality_is_multiset_content() {
    // F17: same entries in a different order are the same table
    // (order-insensitive), duplicate entries are multiplicity-sensitive,
    // and a Weighted never equals a non-Weighted (no quiet coercion).
    let mut story = story_of(
        "~ temp a = weighted(3, \"a\", 1, \"b\")\n\
         ~ temp b = weighted(1, \"b\", 3, \"a\")\n\
         ~ temp dup = weighted(3, \"a\", 3, \"a\")\n\
         same: {a == b}\n\
         ne: {a != dup}\n\
         {a: truthy.}\n\
         -> END\n",
    );
    assert_eq!(run_to_end(&mut story), "same: true\nne: true\ntruthy.\n");

    // Cross-type equality is the TypeError fault, never a quiet `false`
    // (the Option/tower precedent — no coercion into or out of a table).
    let mut story = story_of("~ temp a = weighted(1, \"x\")\ncross: {a == 3}\n-> END\n");
    assert!(matches!(
        run_to_error(&mut story),
        RuntimeError::TypeError(_)
    ));
}

// ── The E078-style split: E120 statically, construction fault at run ────

#[test]
fn statically_malformed_weighted_tables_are_e120_in_both_regimes() {
    // Compile-classifiable refusals: empty, dangling weight, literal
    // zero/negative/float weights. The gate lives at the lowering, so the
    // type regime does not matter (unlike E117's strict-only refinement).
    for src in [
        "~ temp t = weighted()\n-> END\n",
        "~ temp t = weighted(3, \"a\", 2)\n-> END\n",
        "~ temp t = weighted(0, \"a\")\n-> END\n",
        "~ temp t = weighted(-3, \"a\")\n-> END\n",
        "~ temp t = weighted(1.5, \"a\")\n-> END\n",
        "~ temp t = weighted(true, \"a\")\n-> END\n",
        "~ temp t = weighted(\"w\", \"a\")\n-> END\n",
    ] {
        for policy in [TypePolicy::Strict, TypePolicy::Gradual] {
            let codes = diag_codes(src, policy);
            assert!(
                codes.contains(&"E120".to_string()),
                "expected E120 under {policy:?} for {src:?}, got {codes:?}"
            );
        }
    }
}

#[test]
fn computed_weights_carry_the_construction_fault_residual() {
    // A weight the checker cannot classify lowers through and faults at
    // construction when it turns out non-positive — the runtime half of
    // the split (gradual regime; strict has no evidence machinery for
    // weights v1, deliberately — computed weights are honest faults).
    let mut story = story_of(
        "~ temp w = 2 - 2\n\
         ~ temp t = weighted(w, \"a\")\n\
         unreachable {t}\n\
         -> END\n",
    );
    assert_eq!(
        run_to_error(&mut story),
        RuntimeError::WeightedBadWeight {
            found: "0".to_string()
        }
    );

    // A computed weight that turns out valid constructs fine.
    let mut story = story_of(
        "~ temp w = 2 + 1\n\
         ~ temp t = weighted(w, \"a\")\n\
         ok: {t}\n\
         -> END\n",
    );
    assert_eq!(run_to_end(&mut story), "ok: Weighted { 3: a }\n");
}

// ── roll: a seeded-deterministic draw, total over existing tables ───────

#[test]
fn roll_is_seeded_deterministic_and_always_lands_on_an_entry() {
    // (`||` cannot appear inside `{…}` interpolation — `|` is the
    // sequence separator there — so validity is computed in tilde lines.)
    let src = "~ seed(7)\n\
               ~ temp loot = weighted(3, \"sword\", 1, \"shield\")\n\
               ~ temp a = roll(loot)\n\
               ~ temp b = roll(loot)\n\
               ~ temp c = roll(loot)\n\
               ~ temp valid = (a == \"sword\" || a == \"shield\") && (b == \"sword\" || b == \"shield\") && (c == \"sword\" || c == \"shield\")\n\
               valid: {valid}\n\
               ~ seed(7)\n\
               ~ temp again = roll(loot) == a && roll(loot) == b && roll(loot) == c\n\
               replay: {again}\n\
               -> END\n";
    let mut story = story_of(src);
    assert_eq!(run_to_end(&mut story), "valid: true\nreplay: true\n");
}

#[test]
fn roll_on_a_non_weighted_operand_faults() {
    let mut story = story_of("~ temp x = roll(3)\n{x}\n-> END\n");
    assert_eq!(
        run_to_error(&mut story),
        RuntimeError::StdlibWrongType {
            verb: "roll",
            expected: "a weighted table",
            found: "int",
        }
    );
}

// ── The humble heap ─────────────────────────────────────────────────────

#[test]
fn heap_push_pop_peek_drain_ascending_end_to_end() {
    // Push a scrambled row into an empty heap, then drain with the
    // `while heap_pop(...) != none` shape — the §8 gate: out ascending.
    let mut story = story_of(
        "~ temp open = #[]\n\
         ~ {\n\
             temp xs = #[7, 3, 11, 3, 0, 42]\n\
             temp i = 0\n\
             while i < len(xs) {\n\
                 heap_push(open, xs[i])\n\
                 i = i + 1\n\
             }\n\
         }\n\
         peek: {heap_peek(open)}\n\
         ~ temp acc = \"\"\n\
         ~ {\n\
             temp nxt = heap_pop(open)\n\
             while nxt != none {\n\
                 acc = acc + string(nxt) + \" \"\n\
                 nxt = heap_pop(open)\n\
             }\n\
         }\n\
         drain: {acc}\n\
         empty peek: {heap_peek(open)}\n\
         -> END\n",
    );
    assert_eq!(
        run_to_end(&mut story),
        "peek: some(0)\n\
         drain: some(0) some(3) some(3) some(7) some(11) some(42)\n\
         empty peek: none\n"
    );
}

#[test]
fn heap_push_in_expression_position_is_e056_and_chained_heap_pop_is_e055() {
    let codes = diag_codes(
        "~ temp a = #[]\n~ temp x = heap_push(a, 1)\n-> END\n",
        TypePolicy::Gradual,
    );
    assert!(codes.contains(&"E056".to_string()), "got {codes:?}");

    let codes = diag_codes(
        "~ temp grid = #[#[3, 1]]\n~ temp x = heap_pop(grid[0])\n-> END\n",
        TypePolicy::Gradual,
    );
    assert!(codes.contains(&"E055".to_string()), "got {codes:?}");

    // Rvalue heap_push receiver is the E055 "bind it first" error.
    let codes = diag_codes("~ heap_push(#[1], 2)\n-> END\n", TypePolicy::Gradual);
    assert!(codes.contains(&"E055".to_string()), "got {codes:?}");
}

#[test]
fn heap_push_dev_faults_on_nan_prod_places_it_by_the_pinned_order() {
    // One compiled story, two modes — the §4b entry check at the heap
    // door. The NaN arrives computed (0.0 / 0.0): IEEE arithmetic is
    // frozen-total, the ordering context is where it stops.
    let src = "~ temp h = #[]\n\
               ~ heap_push(h, 1.0)\n\
               ~ heap_push(h, 0.0 / 0.0)\n\
               ~ heap_push(h, -2.0)\n\
               pop: {heap_pop(h)} {heap_pop(h)}\n\
               nan last: {heap_pop(h) != none}\n\
               -> END\n";
    let output = compile_brink(src);
    let (program, tables) = brink_runtime::link(&output.data).expect("link");
    let program = Arc::new(program);

    let mut dev = Story::<DotNetRng>::new(Arc::clone(&program), tables.clone());
    assert_eq!(dev.exec_mode(), ExecMode::Dev, "dev is the default");
    assert_eq!(
        run_to_error(&mut dev),
        RuntimeError::UnorderedComparand { verb: "heap_push" }
    );

    let mut prod = Story::<DotNetRng>::new(program, tables);
    prod.set_exec_mode(ExecMode::Prod);
    assert_eq!(
        run_to_end(&mut prod),
        "pop: some(-2) some(1)\nnan last: true\n"
    );
}

// ── Strict-mode typing ──────────────────────────────────────────────────

#[test]
fn strict_types_flow_through_weighted_roll_and_the_heap() {
    // `roll(Weighted[string])` is a string; `heap_pop([int])` is
    // `Option[int]` (compared against `some(...)`/`none`, never truthiness
    // — E116 territory otherwise). A clean strict compile is the check.
    let codes = diag_codes(
        "~ temp loot = weighted(3, \"sword\", 1, \"shield\")\n\
         ~ temp hit = roll(loot)\n\
         len of roll: {len(hit)}\n\
         ~ temp h = #[5, 9]\n\
         ~ heap_push(h, 3)\n\
         ~ temp top = heap_pop(h)\n\
         got three: {top == some(3)}\n\
         peek: {heap_peek(h) != none}\n\
         -> END\n",
        TypePolicy::Strict,
    );
    assert_eq!(codes, Vec::<String>::new(), "strict compile must be clean");
}

#[test]
fn strict_rejects_option_truthiness_on_heap_pop() {
    // `heap_pop` returns Option — condition position is E116 under
    // strict, exactly like every other Option-returning verb.
    let codes = diag_codes(
        "=== main ===\n\
         ~ temp h = #[1]\n\
         ~ temp x = heap_pop(h)\n\
         {x: yes.}\n\
         -> DONE\n",
        TypePolicy::Strict,
    );
    assert!(codes.contains(&"E116".to_string()), "got {codes:?}");
}

// ── Wire durability ─────────────────────────────────────────────────────

#[test]
fn weighted_survives_a_save_state_serde_round_trip() {
    // A Weighted parked in a global crosses a serde SaveState wire trip
    // and is still the same table afterwards (entries, order, equality).
    let src = "VAR loot = 0\n\
               ~ loot = weighted(3, \"sword\", 1, \"shield\")\n\
               before: {loot}\n\
               -> turn\n\
               === turn ===\n\
               + [go] -> after\n\
               === after ===\n\
               after: {loot} {loot == weighted(1, \"shield\", 3, \"sword\")}\n\
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
    assert_eq!(head, "before: Weighted { 3: sword, 1: shield }\n");

    let save = a.save_state();
    let json = serde_json::to_string(&save).expect("serialize");
    let restored: brink_format::SaveState = serde_json::from_str(&json).expect("deserialize");

    let mut b = Story::<DotNetRng>::new(program, tables);
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
            other => panic!("unexpected line {other:?}"),
        }
    }
    assert_eq!(tail, "after: Weighted { 3: sword, 1: shield } true\n");
}
