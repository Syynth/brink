//! NS-A4 dev/prod [`ExecMode`] end to end (`docs/stdlib-spec.md` §4b, issue
//! #1110): one compiled story, two modes — DEV (the default) faults on a
//! NaN comparand in an ordering context; PROD keeps moving with the pinned
//! non-fabricating total order (NaN greatest). The mode changes WHERE
//! execution stops, never WHAT values are fabricated: the clean-prefix
//! output is identical in both runs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, ExecMode, RuntimeError, Step, Story};

/// A brink story whose first line is NaN-free (the modes-agree prefix) and
/// whose second line sorts an array containing NaN (`0.0 / 0.0` — float
/// division is IEEE-total, so the NaN flows freely through arithmetic and
/// stops at the ordering context, exactly the doctrine's shape).
const SOURCE: &str = "\
~ temp clean = #[3.0, 1.0, 2.0]\n\
~ sort(clean)\n\
clean: {clean}.\n\
~ temp dirty = #[1.0, 0.0 / 0.0, -1.0]\n\
~ sort(dirty)\n\
dirty: {dirty}.\n\
-> END\n";

fn build_story() -> Story<DotNetRng> {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let files = std::collections::HashMap::from([("main.ink", SOURCE)]);
    let output = brink_compiler::compile_with_options(
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
    .expect("story must compile — rows are mode-independent, no static rejection");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    Story::new(Arc::new(program), line_tables)
}

/// Drive to completion, collecting text; `Err` propagates.
fn run(story: &mut Story<DotNetRng>) -> Result<String, RuntimeError> {
    let mut out = String::new();
    loop {
        match story.continue_single()? {
            Step::Line(line) => out.push_str(&line.text),
            Step::Done | Step::End | Step::Suspended => {
                return Ok(out);
            }
            Step::Choices(_) => panic!("no choices in this story"),
        }
    }
}

#[test]
fn dev_mode_is_the_default_and_faults_on_the_nan_comparand() {
    let mut story = build_story();
    assert_eq!(story.exec_mode(), ExecMode::Dev, "dev is the default");

    let mut out = String::new();
    let err = loop {
        match story.continue_single() {
            Ok(Step::Line(line)) => out.push_str(&line.text),
            Ok(_) => panic!("expected the NaN fault before any terminal line"),
            Err(e) => break e,
        }
    };
    // The fault surfaces at the ordering consumption. (No assertion on
    // `out`: `continue_single` looks ahead past the first line's newline
    // to classify it, so the dirty sort faults before the clean line is
    // handed back — the clean-prefix/modes-agree property is pinned by
    // the prod test below, which runs the identical bytecode.)
    let _ = out;
    assert!(
        matches!(err, RuntimeError::UnorderedComparand { verb: "sort" }),
        "{err:?}"
    );
}

/// Build and run an arbitrary brink-dialect source under gradual types
/// (the comparator-misbehavior cases are exactly what gradual defers to
/// the runtime).
fn run_gradual(source: &str) -> Result<String, RuntimeError> {
    use brink_compiler::TypePolicy;
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        ..AnalysisOptions::default()
    };
    let files = std::collections::HashMap::from([("main.ink", source)]);
    let output = brink_compiler::compile_with_options(
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
    .expect("source must compile under gradual types");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story: Story<DotNetRng> = Story::new(Arc::new(program), line_tables);
    run(&mut story)
}

/// F0's comparator contract at the runtime residual: a non-int return is
/// the `ComparatorReturnType` turn-terminating fault (never a silent
/// coercion), a non-function comparator is `ComparatorNotAFunction`.
#[test]
fn comparator_misbehavior_faults_at_the_op() {
    let bad_return = "\
~ temp a = #[2, 1]\n\
~ sort_by(a, #fn(words))\n\
{a}\n\
-> END\n\
\n\
=== function words(x, y) ===\n\
~ return \"smaller\"\n";
    let err = run_gradual(bad_return).unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::ComparatorReturnType {
                verb: "sort_by",
                found: "string",
            }
        ),
        "{err:?}"
    );

    let not_a_fn = "~ temp a = #[2, 1]\n~ sort_by(a, 5)\n{a}\n-> END\n";
    let err = run_gradual(not_a_fn).unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::ComparatorNotAFunction {
                verb: "sort_by",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn prod_mode_places_nan_by_the_pinned_order_and_keeps_moving() {
    let mut story = build_story();
    story.set_exec_mode(ExecMode::Prod);
    assert_eq!(story.exec_mode(), ExecMode::Prod);

    let out = run(&mut story).expect("prod mode keeps moving");
    // Same clean prefix as dev (the modes agree on clean data), then the
    // pinned order: NaN greatest, every element preserved — placement,
    // never fabrication.
    assert_eq!(out, "clean: [1, 2, 3].\ndirty: [-1, 1, NaN].\n");
}
