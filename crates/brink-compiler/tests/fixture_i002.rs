#![allow(clippy::unwrap_used, clippy::panic)]

//! Focused fixture test: compile I002 and diff against reference.

use serde_json::Value;
use std::path::Path;

#[test]
fn i002_choice_structure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let ink_path = root.join("tier1/basics/I002-fogg-comforts-passepartout/story.ink");
    let json_path = root.join("tier1/basics/I002-fogg-comforts-passepartout/story.ink.json");

    let our_json = brink_compiler::compile_to_json(ink_path.to_string_lossy().as_ref(), |p| {
        std::fs::read_to_string(p).map_err(|e| std::io::Error::new(e.kind(), format!("{p}: {e}")))
    })
    .expect("should compile");

    let our_value: Value = serde_json::to_value(&our_json).unwrap();
    let ref_text = std::fs::read_to_string(&json_path).unwrap();
    let ref_value: Value = serde_json::from_str(&ref_text).unwrap();

    if our_value != ref_value {
        let our_pretty = serde_json::to_string_pretty(&our_value).unwrap();
        let ref_pretty = serde_json::to_string_pretty(&ref_value).unwrap();

        // Write to temp files for easy diffing
        std::fs::write("/tmp/brink_i002_ours.json", &our_pretty).unwrap();
        std::fs::write("/tmp/brink_i002_ref.json", &ref_pretty).unwrap();

        // Print structural diff
        let mut diffs = Vec::new();
        collect_diffs(&ref_value, &our_value, "", &mut diffs);
        let diff_text = diffs.join("\n");

        panic!(
            "I002 mismatch ({} diffs).\n\
             Files written to /tmp/brink_i002_{{ours,ref}}.json\n\n\
             {diff_text}",
            diffs.len()
        );
    }
}

fn collect_diffs(expected: &Value, actual: &Value, path: &str, out: &mut Vec<String>) {
    if expected == actual {
        return;
    }
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for key in e
                .keys()
                .chain(a.keys())
                .collect::<std::collections::BTreeSet<_>>()
            {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (e.get(key), a.get(key)) {
                    (Some(ev), Some(av)) => collect_diffs(ev, av, &child_path, out),
                    (Some(ev), None) => {
                        out.push(format!("  MISSING {child_path}: expected {ev}"));
                    }
                    (None, Some(av)) => {
                        out.push(format!("  EXTRA   {child_path}: {av}"));
                    }
                    (None, None) => {}
                }
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            let max_len = e.len().max(a.len());
            for i in 0..max_len {
                let child_path = format!("{path}[{i}]");
                match (e.get(i), a.get(i)) {
                    (Some(ev), Some(av)) => collect_diffs(ev, av, &child_path, out),
                    (Some(ev), None) => {
                        out.push(format!("  MISSING {child_path}: expected {ev}"));
                    }
                    (None, Some(av)) => {
                        out.push(format!("  EXTRA   {child_path}: {av}"));
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {
            out.push(format!(
                "  DIFF    {path}: expected {expected}, got {actual}"
            ));
        }
    }
}
