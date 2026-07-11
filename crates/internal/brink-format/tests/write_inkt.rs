#![cfg(feature = "inkt")]
#![allow(clippy::unwrap_used)]

use std::path::Path;

fn i001_data() -> brink_format::StoryData {
    // Compile from an in-memory string with a fixed entry name so the
    // embedded source path (and thus snapshots) stay machine-independent.
    let src = include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink");
    brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data
}

#[test]
fn snapshot_i001_minimal_story() {
    let data = i001_data();

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();

    insta::assert_snapshot!(buf);
}

#[test]
fn roundtrip_i001_minimal_story() {
    let data = i001_data();

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(data, recovered);
}

fn collect_story_ink_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_story_ink_files(&path));
            } else if path.file_name().is_some_and(|n| n == "story.ink") {
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

    let files = collect_story_ink_files(&tests_dir);
    assert!(
        !files.is_empty(),
        "no story.ink files found in {tests_dir:?}"
    );

    let mut failures = Vec::new();

    for path in &files {
        // Some corpus stories intentionally do not compile — skip those.
        let Ok(output) = brink_compiler::compile_path(path) else {
            continue;
        };
        let data = output.data;

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

#[test]
fn inkt_roundtrip_corpus_smoke() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests");

    let files = collect_story_ink_files(&tests_dir);
    assert!(
        !files.is_empty(),
        "no story.ink files found in {tests_dir:?}"
    );

    let mut failures = Vec::new();

    for path in &files {
        // Some corpus stories intentionally do not compile — skip those.
        let Ok(output) = brink_compiler::compile_path(path) else {
            continue;
        };
        let data = output.data;

        let mut buf = String::new();
        if brink_format::write_inkt(&data, &mut buf).is_err() {
            continue;
        }

        match brink_format::read_inkt(&buf) {
            Ok(recovered) => {
                if data != recovered {
                    failures.push(format!("MISMATCH {}", path.display()));
                }
            }
            Err(e) => {
                failures.push(format!("PARSE {}: {e}", path.display()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed inkt roundtrip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
