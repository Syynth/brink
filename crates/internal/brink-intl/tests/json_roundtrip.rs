#![allow(clippy::unwrap_used)]

use brink_intl::{ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson};

fn roundtrip(lines: &LinesJson) -> LinesJson {
    let json_str = serde_json::to_string(lines).unwrap();
    serde_json::from_str(&json_str).unwrap()
}

fn make_line(index: u16, content: ContentJson, audio: Option<String>) -> LineJson {
    LineJson {
        index,
        content: Some(content),
        hash: "0000000000000000".to_string(),
        audio,
        slots: Vec::new(),
        source: None,
    }
}

fn wrap_in_lines_json(lines: Vec<LineJson>) -> LinesJson {
    LinesJson {
        version: 1,
        source_checksum: "0x00000000".to_string(),
        scopes: vec![ScopeJson {
            name: Some("test".to_string()),
            id: "0x0100000000000001".to_string(),
            lines,
        }],
    }
}

#[test]
fn plain_content_roundtrip() {
    let lines = wrap_in_lines_json(vec![make_line(
        0,
        ContentJson::Plain("hello".to_string()),
        None,
    )]);
    let recovered = roundtrip(&lines);
    assert_eq!(lines, recovered);
}

#[test]
fn template_literal_slot_roundtrip() {
    let lines = wrap_in_lines_json(vec![make_line(
        0,
        ContentJson::Template {
            template: vec![
                PartJson::Literal("Hello, ".to_string()),
                PartJson::Slot { slot: 0 },
                PartJson::Literal("!".to_string()),
            ],
        },
        None,
    )]);
    let recovered = roundtrip(&lines);
    assert_eq!(lines, recovered);
}

#[test]
fn template_select_roundtrip() {
    let mut variant_map = serde_json::Map::new();
    variant_map.insert(
        "cardinal:One".to_string(),
        serde_json::Value::String("item".to_string()),
    );
    let mut variant_map2 = serde_json::Map::new();
    variant_map2.insert(
        "cardinal:Other".to_string(),
        serde_json::Value::String("items".to_string()),
    );

    let lines = wrap_in_lines_json(vec![make_line(
        0,
        ContentJson::Template {
            template: vec![PartJson::Select {
                select: SelectJson {
                    slot: 0,
                    variants: vec![variant_map, variant_map2],
                    default: "items".to_string(),
                },
            }],
        },
        None,
    )]);
    let recovered = roundtrip(&lines);
    assert_eq!(lines, recovered);
}

#[test]
fn audio_present_roundtrip() {
    let lines = wrap_in_lines_json(vec![make_line(
        0,
        ContentJson::Plain("hello".to_string()),
        Some("audio/hello.wav".to_string()),
    )]);

    let json_str = serde_json::to_string(&lines).unwrap();
    // Verify the "audio" key is present in serialized JSON
    assert!(
        json_str.contains("\"audio\""),
        "expected 'audio' key in JSON"
    );

    let recovered: LinesJson = serde_json::from_str(&json_str).unwrap();
    assert_eq!(lines, recovered);
}

#[test]
fn audio_absent_not_in_json() {
    let lines = wrap_in_lines_json(vec![make_line(
        0,
        ContentJson::Plain("hello".to_string()),
        None,
    )]);

    let json_str = serde_json::to_string(&lines).unwrap();
    // When audio is None, the key should be absent from the JSON
    // (serde skip_serializing_if = "Option::is_none")
    assert!(
        !json_str.contains("\"audio\""),
        "expected no 'audio' key in JSON when None, got: {json_str}"
    );
}

#[test]
fn full_lines_json_roundtrip() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(json_text);
    let story: brink_json::InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    let exported = brink_intl::export_lines(&data, 0x1234);
    let json_str = serde_json::to_string_pretty(&exported).unwrap();
    let recovered: LinesJson = serde_json::from_str(&json_str).unwrap();
    assert_eq!(exported, recovered);
}
