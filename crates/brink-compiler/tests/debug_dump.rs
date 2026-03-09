#![allow(clippy::unwrap_used, clippy::panic, clippy::print_stderr)]

use serde_json::Value;
use std::path::Path;

fn compare(name: &str, ink_rel: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let ink_path = root.join(ink_rel).join("story.ink");
    let json_path = root.join(ink_rel).join("story.ink.json");

    let our_json = match brink_compiler::compile_to_json(ink_path.to_str().unwrap(), |p| {
        std::fs::read_to_string(p).map_err(|e| std::io::Error::new(e.kind(), format!("{p}: {e}")))
    }) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("=== {name}: COMPILE ERROR: {e}");
            return;
        }
    };

    let our_value: Value = serde_json::to_value(&our_json).unwrap();
    let ref_text = std::fs::read_to_string(&json_path).unwrap();
    let ref_value: Value = serde_json::from_str(&ref_text).unwrap();

    if our_value == ref_value {
        eprintln!("=== {name}: PASS");
    } else {
        std::fs::write(
            format!("/tmp/brink_{name}_ours.json"),
            serde_json::to_string_pretty(&our_value).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("/tmp/brink_{name}_ref.json"),
            serde_json::to_string_pretty(&ref_value).unwrap(),
        )
        .unwrap();
        eprintln!("=== {name}: MISMATCH (see /tmp/brink_{name}_{{ours,ref}}.json)");
    }
}

#[test]
fn dump_cases() {
    compare("I044", "tier1/glue/I044-implicit-inline-glue-c");
    compare("I046", "tier1/glue/I046-left-right-glue-matching");
    compare("I055", "tier1/diverts/I055-same-line-divert-is-inline");
    compare("I086", "tier1/choices/I086-default-simple-gather");
    compare("I092", "tier1/choices/I092-should-not-gather-due-to-choice");
    compare("simple_divert", "tier1/divert/simple-divert");
    compare("sticky_choice", "tier1/choices/sticky-choice");
    compare("one_choice", "tier1/choices/one");
    compare("cond_choice", "tier1/choices/conditional-choice");
    compare(
        "I079",
        "tier1/choices/I079-once-only-choices-can-link-back-to-self",
    );
    compare("I081", "tier1/choices/I081-gather-choice-same-line");
}

fn try_compile(name: &str, ink_rel: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let ink_path = root.join(ink_rel).join("story.ink");
    match brink_compiler::compile_to_json(ink_path.to_str().unwrap(), |p| {
        std::fs::read_to_string(p).map_err(|e| std::io::Error::new(e.kind(), format!("{p}: {e}")))
    }) {
        Ok(_) => eprintln!("=== {name}: OK (no error)"),
        Err(brink_compiler::CompileError::Diagnostics(diags)) => {
            eprintln!("=== {name}: {} diagnostic(s):", diags.len());
            for d in &diags {
                eprintln!("    [{:?}] {}", d.code, d.message);
            }
        }
        Err(e) => eprintln!("=== {name}: {e}"),
    }
}

#[test]
fn dump_errors() {
    try_compile(
        "I077-fallback-choice-on-thread",
        "tier1/choices/I077-fallback-choice-on-thread",
    );
    try_compile(
        "I079-once-only-choices-can-link-back-to-self",
        "tier1/choices/I079-once-only-choices-can-link-back-to-self",
    );
    try_compile(
        "I083-choice-thread-forking",
        "tier1/choices/I083-choice-thread-forking",
    );
    try_compile(
        "I090-various-default-choices",
        "tier1/choices/I090-various-default-choices",
    );
    try_compile("I091-choice-count", "tier1/choices/I091-choice-count");
    try_compile("I093-default-choices", "tier1/choices/I093-default-choices");
    try_compile("choice-count", "tier1/choices/choice-count");
    try_compile(
        "choice-thread-forking",
        "tier1/choices/choice-thread-forking",
    );
    try_compile(
        "conditional-choice-in-weave",
        "tier1/choices/conditional-choice-in-weave",
    );
    try_compile("default-choices", "tier1/choices/default-choices");
    try_compile(
        "fallback-choice-on-thread",
        "tier1/choices/fallback-choice-on-thread",
    );
    try_compile("label-scope", "tier1/choices/label-scope");
    try_compile(
        "once-only-choices-can-link-back-to-self",
        "tier1/choices/once-only-choices-can-link-back-to-self",
    );
    try_compile(
        "I056-divert-targets-with-parameters",
        "tier1/diverts/I056-divert-targets-with-parameters",
    );
    try_compile(
        "I060-tunnel-onwards-divert-after-with-arg",
        "tier1/diverts/I060-tunnel-onwards-divert-after-with-arg",
    );
    try_compile("I062-complex-tunnels", "tier1/diverts/I062-complex-tunnels");
    try_compile(
        "I065-tunnel-onwards-with-param-default-choice",
        "tier1/diverts/I065-tunnel-onwards-with-param-default-choice",
    );
    try_compile("I003-tunnel-to-death", "tier1/basics/I003-tunnel-to-death");
    try_compile(
        "I004-print-number-as-english",
        "tier1/basics/I004-print-number-as-english",
    );
}
