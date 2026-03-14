#![allow(clippy::unwrap_used)]

use brink_intl::{ContentJson, LineJson, LinesJson, ScopeJson, regenerate_lines};

fn make_line(index: u16, hash: &str, content: Option<&str>, audio: Option<&str>) -> LineJson {
    LineJson {
        index,
        content: content.map(|s| ContentJson::Plain(s.to_string())),
        hash: hash.to_string(),
        audio: audio.map(str::to_string),
    }
}

fn make_scope(id: &str, name: Option<&str>, lines: Vec<LineJson>) -> ScopeJson {
    ScopeJson {
        name: name.map(str::to_string),
        id: id.to_string(),
        lines,
    }
}

fn make_lines_json(checksum: &str, scopes: Vec<ScopeJson>) -> LinesJson {
    LinesJson {
        version: 1,
        source_checksum: checksum.to_string(),
        scopes,
    }
}

#[test]
fn snapshot_duplicate_hashes() {
    // Same hash appears multiple times — alignment should handle positionally.
    let existing = make_lines_json(
        "0x00000001",
        vec![make_scope(
            "0x01",
            Some("root"),
            vec![
                make_line(0, "aaa", Some("[TR] first"), None),
                make_line(1, "aaa", Some("[TR] second"), None),
                make_line(2, "bbb", Some("[TR] third"), None),
            ],
        )],
    );

    let new_export = make_lines_json(
        "0x00000002",
        vec![make_scope(
            "0x01",
            Some("root"),
            vec![
                make_line(0, "aaa", Some("first (src)"), None),
                make_line(1, "aaa", Some("second (src)"), None),
                make_line(2, "bbb", Some("third (src)"), None),
                make_line(3, "aaa", Some("fourth (src)"), None),
            ],
        )],
    );

    let result = regenerate_lines(&new_export, &existing);
    insta::assert_json_snapshot!(result);
}

#[test]
fn snapshot_all_lines_changed() {
    // Every hash is different — all translations lost (inserted after removals).
    let existing = make_lines_json(
        "0x00000001",
        vec![make_scope(
            "0x01",
            Some("root"),
            vec![
                make_line(0, "aaa", Some("[TR] alpha"), None),
                make_line(1, "bbb", Some("[TR] beta"), None),
                make_line(2, "ccc", Some("[TR] gamma"), None),
            ],
        )],
    );

    let new_export = make_lines_json(
        "0x00000002",
        vec![make_scope(
            "0x01",
            Some("root"),
            vec![
                make_line(0, "xxx", Some("x (src)"), None),
                make_line(1, "yyy", Some("y (src)"), None),
                make_line(2, "zzz", Some("z (src)"), None),
            ],
        )],
    );

    let result = regenerate_lines(&new_export, &existing);
    insta::assert_json_snapshot!(result);
}

#[test]
fn snapshot_empty_scope_survives() {
    // A scope with no lines in both old and new should survive.
    let existing = make_lines_json(
        "0x00000001",
        vec![make_scope("0x01", Some("empty_knot"), vec![])],
    );

    let new_export = make_lines_json(
        "0x00000002",
        vec![make_scope("0x01", Some("empty_knot"), vec![])],
    );

    let result = regenerate_lines(&new_export, &existing);
    insta::assert_json_snapshot!(result);
}

#[test]
fn snapshot_insertion_and_deletion_net_zero() {
    // One line removed, one inserted — same count but shifted.
    let existing = make_lines_json(
        "0x00000001",
        vec![make_scope(
            "0x01",
            Some("root"),
            vec![
                make_line(0, "aaa", Some("[TR] first"), Some("audio/1.wav")),
                make_line(1, "bbb", Some("[TR] second"), None),
                make_line(2, "ccc", Some("[TR] third"), Some("audio/3.wav")),
            ],
        )],
    );

    // "bbb" removed, "ddd" inserted after "ccc".
    let new_export = make_lines_json(
        "0x00000002",
        vec![make_scope(
            "0x01",
            Some("root"),
            vec![
                make_line(0, "aaa", Some("first (src)"), None),
                make_line(1, "ccc", Some("third (src)"), None),
                make_line(2, "ddd", Some("fourth (src)"), None),
            ],
        )],
    );

    let result = regenerate_lines(&new_export, &existing);
    insta::assert_json_snapshot!(result);
}
