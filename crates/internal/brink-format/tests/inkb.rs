#![allow(clippy::unwrap_used)]

use std::path::Path;

use brink_format::{
    DecodeError, SectionKind, assemble_inkb, read_inkb, read_inkb_index, read_section_containers,
    read_section_externals, read_section_labels, read_section_line_tables, read_section_list_defs,
    read_section_list_items, read_section_name_table, read_section_variables, write_inkb,
    write_section_containers, write_section_externals, write_section_labels,
    write_section_line_tables, write_section_list_defs, write_section_list_items,
    write_section_name_table, write_section_variables,
};
use brink_json::InkJson;

#[test]
fn roundtrip_i001_minimal_story() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let recovered = read_inkb(&buf).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn snapshot_i001_inkb_bytes() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

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
        write_inkb(&data, &mut buf);

        match read_inkb(&buf) {
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

// ── New tests for sectioned header ──────────────────────────────────────────

fn make_test_data() -> brink_format::StoryData {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    brink_converter::convert(&story).unwrap()
}

#[test]
fn index_parsing() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert_eq!(index.version, 1);
    assert_eq!(index.file_size as usize, buf.len());
    assert_eq!(index.sections.len(), 8);

    // Sections are in canonical order.
    assert_eq!(index.sections[0].kind, SectionKind::NameTable);
    assert_eq!(index.sections[1].kind, SectionKind::Variables);
    assert_eq!(index.sections[2].kind, SectionKind::ListDefs);
    assert_eq!(index.sections[3].kind, SectionKind::ListItems);
    assert_eq!(index.sections[4].kind, SectionKind::Externals);
    assert_eq!(index.sections[5].kind, SectionKind::Containers);
    assert_eq!(index.sections[6].kind, SectionKind::LineTables);
    assert_eq!(index.sections[7].kind, SectionKind::Labels);

    // Header size is 16 + 8*8 = 80.
    assert_eq!(index.header_size(), 80);

    // First section starts right after header.
    assert_eq!(index.sections[0].offset as usize, index.header_size());

    // Offsets are monotonically increasing.
    for w in index.sections.windows(2) {
        assert!(
            w[0].offset < w[1].offset,
            "section {:?} offset {} >= {:?} offset {}",
            w[0].kind,
            w[0].offset,
            w[1].kind,
            w[1].offset
        );
    }
}

#[test]
fn section_ranges() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();

    // All section ranges should cover the entire post-header region with no gaps.
    let mut covered = index.header_size();
    for entry in &index.sections {
        let range = index.section_range(entry.kind).unwrap();
        assert_eq!(range.start, covered, "gap before section {:?}", entry.kind);
        covered = range.end;
    }
    assert_eq!(covered, index.file_size as usize);
}

#[test]
fn section_level_roundtrip() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();

    let names = read_section_name_table(&buf, &index).unwrap();
    assert_eq!(names, data.name_table);

    let vars = read_section_variables(&buf, &index).unwrap();
    assert_eq!(vars, data.variables);

    let list_defs = read_section_list_defs(&buf, &index).unwrap();
    assert_eq!(list_defs, data.list_defs);

    let list_items = read_section_list_items(&buf, &index).unwrap();
    assert_eq!(list_items, data.list_items);

    let exts = read_section_externals(&buf, &index).unwrap();
    assert_eq!(exts, data.externals);

    let containers = read_section_containers(&buf, &index).unwrap();
    assert_eq!(containers, data.containers);

    let line_tables = read_section_line_tables(&buf, &index).unwrap();
    assert_eq!(line_tables, data.line_tables);

    let labels = read_section_labels(&buf, &index).unwrap();
    assert_eq!(labels, data.labels);
}

#[test]
fn checksum_validation() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    // Corrupt a byte in the section data region.
    let last = buf.len() - 1;
    buf[last] ^= 0xFF;

    let err = read_inkb(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::ChecksumMismatch { .. }),
        "expected ChecksumMismatch, got {err:?}"
    );
}

#[test]
fn assemble_inkb_equivalence() {
    let data = make_test_data();

    // Write via write_inkb.
    let mut direct = Vec::new();
    write_inkb(&data, &mut direct);

    // Write via individual section writers + assemble_inkb.
    let mut name_buf = Vec::new();
    write_section_name_table(&data.name_table, &mut name_buf);

    let mut var_buf = Vec::new();
    write_section_variables(&data.variables, &mut var_buf);

    let mut ld_buf = Vec::new();
    write_section_list_defs(&data.list_defs, &mut ld_buf);

    let mut list_item_buf = Vec::new();
    write_section_list_items(&data.list_items, &mut list_item_buf);

    let mut ext_buf = Vec::new();
    write_section_externals(&data.externals, &mut ext_buf);

    let mut cont_buf = Vec::new();
    write_section_containers(&data.containers, &mut cont_buf);

    let mut line_table_buf = Vec::new();
    write_section_line_tables(&data.line_tables, &mut line_table_buf);

    let mut label_buf = Vec::new();
    write_section_labels(&data.labels, &mut label_buf);

    let mut assembled = Vec::new();
    assemble_inkb(
        &[
            (SectionKind::NameTable, &name_buf),
            (SectionKind::Variables, &var_buf),
            (SectionKind::ListDefs, &ld_buf),
            (SectionKind::ListItems, &list_item_buf),
            (SectionKind::Externals, &ext_buf),
            (SectionKind::Containers, &cont_buf),
            (SectionKind::LineTables, &line_table_buf),
            (SectionKind::Labels, &label_buf),
        ],
        &mut assembled,
    );

    assert_eq!(
        direct, assembled,
        "write_inkb and assemble_inkb should produce identical output"
    );

    // Also verify the assembled version can be read back.
    let recovered = read_inkb(&assembled).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn file_size_mismatch_detected() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    // Truncate the buffer — the file_size in the header will be larger than actual.
    buf.truncate(buf.len() - 1);

    let err = read_inkb_index(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::FileSizeMismatch { .. }),
        "expected FileSizeMismatch, got {err:?}"
    );
}

#[test]
fn bad_magic_detected() {
    let mut buf = vec![0x00; 64];
    buf[0..4].copy_from_slice(b"XYZW");

    let err = read_inkb_index(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::BadMagic(..)),
        "expected BadMagic, got {err:?}"
    );
}
