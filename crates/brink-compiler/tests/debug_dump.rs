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
    compare("I054", "tier1/diverts/I054-basic-tunnel");
}
