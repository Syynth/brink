//! Law: **strict/gradual observational equivalence on error-free programs**
//! — issue #672 workstream B item 4.
//!
//! `docs/typed-mode-spec.md` §1 (companion to `docs/value-model-spec.md`,
//! see that doc's header): `types` is "a project-level policy over one
//! shared checker, not a second checker" — `gradual` (the default) lets
//! `Unknown` unify with anything and defer to runtime coercion; `strict`
//! makes an escaping `Unknown` a compile error and narrows the coercion
//! lattice, but is layered on the *same* inference/codegen pipeline.
//! `crates/internal/brink-analyzer/src/strict.rs` confirms the policy is
//! purely a diagnostic-severity knob (`effective_severity` — `E063`/`E040`
//! escalation only); it never changes what bytecode a definition lowers to.
//! `docs/value-model-spec.md` §9 ("the guarantees contract") is the
//! semantic backdrop this rests on: the elisions/codegen the compiler
//! performs are stable regardless of the annotation/diagnostic layer sitting
//! on top of them.
//!
//! So for a program with no `Unknown`/`Conflicted` escapes (fully inferred
//! from concrete literals, no host externals, no explicit annotations that
//! could mismatch inference) — an "error-free" program in the issue's
//! phrasing — compiling under `TypePolicy::Strict` and `TypePolicy::Gradual`
//! must both succeed *and* produce byte-identical observable behavior. This
//! suite generates small concrete programs mixing arithmetic, arrays, maps,
//! and structs and checks exactly that, without needing a third, independent
//! reference implementation — the two compiled policies are each other's
//! oracle.
//!
//! Deterministic seeds (house determinism rule): `ProptestConfig` fixes the
//! case count and reads no `PROPTEST_*` env override.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod law_support;

use brink_compiler::TypePolicy;
use law_support::{run_to_completion, try_compile};
use proptest::prelude::*;

const POINT_STRUCT: &str = "STRUCT Point = #{\n    x: int,\n    y: int,\n}\n";

/// The random terms [`program`] plugs into the generated source — bundled
/// into a struct (rather than 8 loose parameters) purely to keep clippy's
/// `too_many_arguments` happy; there is no semantic grouping beyond "the
/// knobs one generated case varies".
struct ProgramTerms {
    a: i32,
    b: i32,
    c: i32,
    arr0: i32,
    arr1: i32,
    arr2: i32,
    write: i32,
    m0: i32,
}

/// A concrete (no `Unknown` escapes) program mixing a struct, an array, and
/// a map — the three T1b/TM value shapes — so a policy-dependent codegen
/// divergence in any of them would show up.
fn program(t: &ProgramTerms) -> String {
    let ProgramTerms {
        a,
        b,
        c,
        arr0,
        arr1,
        arr2,
        write,
        m0,
    } = *t;
    format!(
        "{POINT_STRUCT}VAR p = 0\nVAR arr = 0\nVAR m = 0\nVAR total = 0\n~ {{\n    p = Point#{{x: {a}, y: {b}}}\n    p.x += {c}\n    arr = #[{arr0}, {arr1}, {arr2}]\n    arr[1] = {write}\n    m = #{{\"k\": {m0}}}\n    total = p.x + p.y + arr[0] + arr[1] + arr[2] + m[\"k\"]\n}}\n{{total}}\n-> DONE\n",
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Compiling and running the same error-free program under
    /// `TypePolicy::Strict` and `TypePolicy::Gradual` must succeed under
    /// both and produce identical output text.
    #[test]
    fn strict_and_gradual_agree_on_error_free_programs(
        a in -1000i32..1000,
        b in -1000i32..1000,
        c in -1000i32..1000,
        arr0 in -1000i32..1000,
        arr1 in -1000i32..1000,
        arr2 in -1000i32..1000,
        write in -1000i32..1000,
        m0 in -1000i32..1000,
    ) {
        let source = program(&ProgramTerms { a, b, c, arr0, arr1, arr2, write, m0 });

        let mut gradual_story = try_compile(&source, TypePolicy::Gradual)
            .unwrap_or_else(|e| panic!("Gradual compile error for:\n{source}\n{e}"));
        let mut strict_story = try_compile(&source, TypePolicy::Strict)
            .unwrap_or_else(|e| panic!("Strict compile error for:\n{source}\n{e}"));

        let gradual_out = run_to_completion(&mut gradual_story);
        let strict_out = run_to_completion(&mut strict_story);

        prop_assert_eq!(gradual_out, strict_out);
    }
}
