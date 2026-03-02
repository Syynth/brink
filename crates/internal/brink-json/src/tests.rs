use std::collections::HashMap;
use std::path::Path;

use super::*;

#[test]
fn load_basic_text() {
    let json_text = include_str!("../inkfiles/basictext/oneline.ink.json");

    let parsed: InkJson = serde_json::from_str(json_text).unwrap();

    let actual = InkJson {
        ink_version: 21,
        root: Container {
            flags: None,
            name: None,
            named_content: HashMap::new(),
            contents: vec![
                Element::Container(Container {
                    flags: None,
                    name: None,
                    named_content: HashMap::new(),
                    contents: vec![
                        Element::Value(InkValue::String("Line.".to_string())),
                        Element::Value(InkValue::String("\n".to_string())),
                        Element::Container(Container {
                            flags: None,
                            name: Some("g-0".to_string()),
                            named_content: HashMap::new(),
                            contents: vec![Element::ControlCommand(ControlCommand::Done)],
                        }),
                    ],
                }),
                Element::ControlCommand(ControlCommand::Done),
            ],
        },
    };

    assert_eq!(parsed, actual);
}

#[test]
fn round_trip_basic_text() {
    let json_text = include_str!("../inkfiles/basictext/oneline.ink.json");
    let parsed: InkJson = serde_json::from_str(json_text).unwrap();
    let serialized = serde_json::to_string(&parsed).unwrap();
    let reparsed: InkJson = serde_json::from_str(&serialized).unwrap();
    assert_eq!(parsed, reparsed);
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
fn round_trip_all_test_corpus() {
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

        let parsed: InkJson = match serde_json::from_str(json_text) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("DESERIALIZE {}: {e}", path.display()));
                continue;
            }
        };

        let serialized = match serde_json::to_string(&parsed) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("SERIALIZE {}: {e}", path.display()));
                continue;
            }
        };

        let reparsed: InkJson = match serde_json::from_str(&serialized) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("RE-DESERIALIZE {}: {e}", path.display()));
                continue;
            }
        };

        if parsed != reparsed {
            failures.push(format!("MISMATCH {}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed round-trip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
