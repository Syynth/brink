#![allow(clippy::unwrap_used)]

use std::path::Path;

use brink_json::InkJson;

#[test]
fn roundtrip_i001_minimal_story() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    let mut buf = Vec::new();
    brink_format::write_inkb(&data, &mut buf);

    let recovered = brink_format::read_inkb(&buf).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn snapshot_i001_inkb_bytes() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    let mut buf = Vec::new();
    brink_format::write_inkb(&data, &mut buf);

    insta::assert_snapshot!(format_hex(&buf));
}

fn format_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        write!(out, "{:08x}  ", i * 16).unwrap();
        for (j, byte) in chunk.iter().enumerate() {
            if j == 8 {
                out.push(' ');
            }
            write!(out, "{byte:02x} ").unwrap();
        }
        // Pad to fixed width
        let padding = 16 - chunk.len();
        for j in 0..padding {
            if chunk.len() + j == 8 {
                out.push(' ');
            }
            out.push_str("   ");
        }
        out.push(' ');
        out.push('|');
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                out.push(*byte as char);
            } else {
                out.push('.');
            }
        }
        out.push('|');
        out.push('\n');
    }
    out
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
fn inkb_roundtrip_corpus_smoke() {
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

        let mut buf = Vec::new();
        brink_format::write_inkb(&data, &mut buf);

        match brink_format::read_inkb(&buf) {
            Ok(recovered) => {
                if data != recovered {
                    failures.push(format!("MISMATCH {}", path.display()));
                }
            }
            Err(e) => {
                failures.push(format!("DECODE {}: {e}", path.display()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed inkb roundtrip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
