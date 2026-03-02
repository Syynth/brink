use std::path::Path;

use brink_format::Opcode;
use brink_json::InkJson;

use crate::convert;

#[test]
fn convert_i001_minimal_story() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = convert(&story).unwrap();

    // Should have containers (root + sub-containers)
    assert!(!data.containers.is_empty(), "should produce containers");

    // At least one container should have a line table entry with "Hello, world!"
    let has_hello = data.containers.iter().any(|c| {
        c.line_table.iter().any(|entry| {
            matches!(
                &entry.content,
                brink_format::LineContent::Plain(s) if s == "Hello, world!"
            )
        })
    });
    assert!(has_hello, "should contain 'Hello, world!' in line table");
}

#[test]
fn convert_simple_divert() {
    let json_text = include_str!("../../../../tests/tier1/divert/simple-divert/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = convert(&story).unwrap();

    // Should have a container for "hurry_home"
    let has_hurry_home = data.containers.iter().any(|c| {
        c.line_table.iter().any(|entry| {
            matches!(
                &entry.content,
                brink_format::LineContent::Plain(s) if s.contains("hurried home")
            )
        })
    });
    assert!(
        has_hurry_home,
        "should contain 'hurried home' text in some container"
    );

    // Root sub-container should have a Divert opcode
    let has_divert = data.containers.iter().any(|c| {
        let mut offset = 0;
        while offset < c.bytecode.len() {
            if let Ok(op) = Opcode::decode(&c.bytecode, &mut offset) {
                if matches!(op, Opcode::Divert(_)) {
                    return true;
                }
            } else {
                break;
            }
        }
        false
    });
    assert!(has_divert, "should have a Divert opcode in bytecode");
}

fn collect_ink_json_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_ink_json_files(&path));
            } else if path.extension().is_some_and(|e| e == "json")
                && path.to_str().is_some_and(|s| s.contains(".ink.json"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn convert_all_test_corpus() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests");

    let files = collect_ink_json_files(&tests_dir);
    assert!(
        !files.is_empty(),
        "no .ink.json files found in {tests_dir:?}"
    );

    let mut failures = Vec::new();

    for path in &files {
        let json_text = std::fs::read_to_string(path).unwrap();
        let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(&json_text);

        let story: InkJson = match serde_json::from_str(json_text) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("PARSE {}: {e}", path.display()));
                continue;
            }
        };

        if let Err(e) = convert(&story) {
            failures.push(format!("CONVERT {}: {e}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed conversion:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
