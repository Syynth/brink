//! T1c-3 property tests (`docs/t1c-spec.md` §5/§9, issue #701).
//!
//! The function-value **display form** (`fn heal(ref hp = player_hp, amount)`)
//! and **structural equality** (`same fn token + equal bound rows`) are
//! permanently observable surfaces, so they get property coverage, not just
//! example cases. The law under test: a bound-arg prefix produced by chaining
//! `bind(_, v)` one arg at a time is indistinguishable — through both `==` and
//! `string()` — from the same prefix bound all at once at the `#fn(name, …)`
//! creation site. This is the sharing-unobservable / structural-equality law
//! of spec §5 exercised end-to-end through real production codegen, and it
//! simultaneously pins the display form's exact rendering as stable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Line, Story};
use proptest::prelude::*;

/// Compile+run a choice-free brink program to completion, returning its text.
fn run_brink(source: &str) -> String {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let compile_msg = format!("compile error for:\n{source}");
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_string()), options)
            .expect(&compile_msg);
    let link_msg = format!("link error for:\n{source}");
    let (program, line_tables) = brink_runtime::link(&output.data).expect(&link_msg);
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let step_msg = format!("runtime error for:\n{source}");
    let mut out = String::new();
    loop {
        match story.continue_single().expect(&step_msg) {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => panic!("unexpected choices for:\n{source}"),
        }
    }
    out
}

/// The four param names of the `add4` target, in declared order.
const PARAMS: [&str; 4] = ["a", "b", "c", "d"];

/// The authoritative display form (spec §5) for `add4` with `bound` as its
/// bound `val` prefix — the exact string `string(f)`/`{f}` must produce.
fn expected_display(bound: &[i32]) -> String {
    let parts: Vec<String> = PARAMS
        .iter()
        .enumerate()
        .map(|(i, name)| match bound.get(i) {
            Some(v) => format!("{name} = {v}"),
            None => (*name).to_owned(),
        })
        .collect();
    format!("fn add4({})", parts.join(", "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// bind-chain ≡ direct-creation, observed through `==` and `string()`:
    /// binding a `val` prefix one arg at a time via `bind` produces a fn value
    /// structurally equal to (and displaying identically to) the same prefix
    /// bound all at once at the `#fn` creation site — and both render the
    /// stable display form (spec §5).
    #[test]
    fn bind_chain_equals_direct_creation_and_display_is_stable(
        bound in prop::collection::vec(-9i32..=9, 0..=4),
    ) {
        // Direct creation: `#fn(add4, v0, v1, …)` binds the whole prefix.
        let direct = if bound.is_empty() {
            "#fn(add4)".to_owned()
        } else {
            let vals: Vec<String> = bound.iter().map(i32::to_string).collect();
            format!("#fn(add4, {})", vals.join(", "))
        };
        // Chain: start from `#fn(add4)` and `bind` one value at a time.
        let mut chained = "#fn(add4)".to_owned();
        for v in &bound {
            chained = format!("bind({chained}, {v})");
        }

        let source = format!(
            "~ temp direct = {direct}\n\
             ~ temp chained = {chained}\n\
             EQ:{{direct == chained}}\n\
             D:{{direct}}\n\
             C:{{chained}}\n\
             -> END\n\n\
             === function add4(a, b, c, d) ===\n\
             ~ return a + b + c + d\n"
        );
        let out = run_brink(&source);
        let want = format!(
            "EQ:true\nD:{disp}\nC:{disp}\n",
            disp = expected_display(&bound),
        );
        prop_assert_eq!(out, want, "bound = {:?}", bound);
    }
}
