#![allow(clippy::unwrap_used)]

use std::path::Path;

use brink_json::InkJson;

#[test]
fn snapshot_i001_minimal_story() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();

    insta::assert_snapshot!(buf);
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
fn write_inkt_corpus_smoke() {
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

        let Ok(story): Result<InkJson, _> = serde_json::from_str(json_text) else {
            continue;
        };

        let Ok(data) = brink_converter::convert(&story) else {
            continue;
        };

        let mut buf = String::new();
        if let Err(e) = brink_format::write_inkt(&data, &mut buf) {
            failures.push(format!("WRITE_INKT {}: {e}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed write_inkt:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
