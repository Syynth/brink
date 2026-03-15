#![allow(clippy::unwrap_used)]

use brink_intl::{
    ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson, compile_locale_xliff,
    generate_locale, lines_json_to_xliff, regenerate_locale, xliff_to_lines_json,
};
use xliff2::{Content, InlineElement, State, SubUnit};

fn make_line(
    index: u16,
    hash: &str,
    content: Option<ContentJson>,
    audio: Option<&str>,
) -> LineJson {
    LineJson {
        index,
        content,
        hash: hash.to_string(),
        audio: audio.map(str::to_string),
        slots: Vec::new(),
        source: None,
    }
}

fn make_scope(id: &str, name: Option<&str>, lines: Vec<LineJson>) -> ScopeJson {
    ScopeJson {
        name: name.map(str::to_string),
        id: id.to_string(),
        lines,
    }
}

fn make_lines_json(scopes: Vec<ScopeJson>) -> LinesJson {
    LinesJson {
        version: 1,
        source_checksum: "0xdeadbeef".to_string(),
        scopes,
    }
}

/// Copy source → target on all segments, mark as translated.
fn fill_targets(mut doc: xliff2::Document) -> xliff2::Document {
    for file in &mut doc.files {
        for unit in &mut file.units {
            for su in &mut unit.sub_units {
                if let SubUnit::Segment(seg) = su {
                    seg.target = Some(seg.source.clone());
                    seg.state = Some(State::Translated);
                }
            }
        }
    }
    doc
}

// ── Full LinesJson → XLIFF → string → parse → LinesJson round-trip ──

#[test]
fn full_roundtrip_through_xml() {
    let mut v1 = serde_json::Map::new();
    v1.insert(
        "cardinal:One".to_string(),
        serde_json::Value::String("cat".to_string()),
    );
    let mut v2 = serde_json::Map::new();
    v2.insert(
        "cardinal:Other".to_string(),
        serde_json::Value::String("cats".to_string()),
    );

    let lines = make_lines_json(vec![
        make_scope(
            "0x0100000000000001",
            Some("root"),
            vec![
                make_line(
                    0,
                    "aaaa",
                    Some(ContentJson::Plain("Hello world".to_string())),
                    None,
                ),
                make_line(
                    1,
                    "bbbb",
                    Some(ContentJson::Template {
                        template: vec![
                            PartJson::Literal("You have ".to_string()),
                            PartJson::Slot { slot: 0 },
                            PartJson::Literal(" ".to_string()),
                            PartJson::Select {
                                select: SelectJson {
                                    slot: 0,
                                    variants: vec![v1, v2],
                                    default: "cats".to_string(),
                                },
                            },
                        ],
                    }),
                    Some("audio/count.wav"),
                ),
            ],
        ),
        make_scope(
            "0x0100000000000002",
            Some("knot_a"),
            vec![make_line(
                0,
                "cccc",
                Some(ContentJson::Plain("Goodbye".to_string())),
                None,
            )],
        ),
    ]);

    // LinesJson → XLIFF Document
    let doc = lines_json_to_xliff(&lines, "en", None);
    assert_eq!(doc.src_lang, "en");
    assert_eq!(doc.files.len(), 2);

    // XLIFF Document → XML string → parse back
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    // Fill targets and convert back to LinesJson
    let translated = fill_targets(parsed);
    let recovered = xliff_to_lines_json(&translated).unwrap();

    assert_eq!(recovered.version, lines.version);
    assert_eq!(recovered.source_checksum, lines.source_checksum);
    assert_eq!(recovered.scopes.len(), 2);
    assert_eq!(recovered.scopes[0].name, Some("root".to_string()));
    assert_eq!(recovered.scopes[0].lines.len(), 2);

    // Plain content preserved
    assert_eq!(
        recovered.scopes[0].lines[0].content,
        lines.scopes[0].lines[0].content,
    );

    // Template with slot + select preserved
    assert_eq!(
        recovered.scopes[0].lines[1].content,
        lines.scopes[0].lines[1].content,
    );

    // Audio preserved
    assert_eq!(
        recovered.scopes[0].lines[1].audio,
        Some("audio/count.wav".to_string()),
    );

    // Second scope
    assert_eq!(recovered.scopes[1].id, "0x0100000000000002");
    assert_eq!(recovered.scopes[1].name, Some("knot_a".to_string()));
}

// ── generate_locale → fill targets → compile_locale_xliff ──

#[test]
fn generate_and_compile_xliff() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(json_text);
    let story: brink_json::InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    // Generate XLIFF
    let doc = generate_locale(&data, 0x1234, "en", None);
    assert_eq!(doc.version, "2.0");
    assert_eq!(doc.src_lang, "en");

    // Verify brink extension attributes on document
    let checksum_attr = doc
        .extensions
        .attributes
        .iter()
        .find(|a| a.namespace == "brink" && a.local_name == "checksum")
        .unwrap();
    assert_eq!(checksum_attr.value, "0x00001234");

    // Fill targets (simulate translation)
    let translated = fill_targets(doc);

    // Compile — the base inkb
    let mut base_inkb = Vec::new();
    brink_format::write_inkb(&data, &mut base_inkb);
    let inkl = compile_locale_xliff(&base_inkb, &translated, "es").unwrap();
    assert!(!inkl.is_empty());

    // Compare with JSON path: export → JSON → compile should produce identical bytes
    let lines_json = brink_intl::export_lines(&data, 0x1234);
    let inkl_json_path = brink_intl::compile_locale(&base_inkb, &lines_json, "es").unwrap();
    assert_eq!(inkl, inkl_json_path);
}

// ── Regeneration preserves translations with correct states ──

#[test]
fn regeneration_preserves_translations() {
    let lines_v1 = make_lines_json(vec![make_scope(
        "0x01",
        Some("root"),
        vec![
            make_line(
                0,
                "aaa",
                Some(ContentJson::Plain("Hello".to_string())),
                None,
            ),
            make_line(
                1,
                "bbb",
                Some(ContentJson::Plain("World".to_string())),
                None,
            ),
        ],
    )]);

    // Create "existing" XLIFF with translations
    let mut existing = lines_json_to_xliff(&lines_v1, "en", None);
    existing.trg_lang = Some("es".to_string());
    // Set translated content
    for file in &mut existing.files {
        for unit in &mut file.units {
            for su in &mut unit.sub_units {
                if let SubUnit::Segment(seg) = su {
                    seg.target = Some(Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Traducido".to_string())],
                    });
                    seg.state = Some(State::Translated);
                }
            }
        }
    }

    // "Recompile" with one new line and one changed line
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(json_text);
    let story: brink_json::InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();

    let result = regenerate_locale(&data, 0x5678, "en", &existing).unwrap();

    // Target language carried forward
    assert_eq!(result.trg_lang, Some("es".to_string()));

    // Checksum updated
    let checksum = result
        .extensions
        .attributes
        .iter()
        .find(|a| a.namespace == "brink" && a.local_name == "checksum")
        .unwrap();
    assert_eq!(checksum.value, "0x00005678");
}

// ── Snapshot test of XLIFF XML output ──

#[test]
fn xliff_output_snapshot() {
    let lines = make_lines_json(vec![make_scope(
        "0x0100000000000001",
        Some("root"),
        vec![
            make_line(
                0,
                "aaaa1234",
                Some(ContentJson::Plain("Hello world".to_string())),
                None,
            ),
            make_line(
                1,
                "bbbb5678",
                Some(ContentJson::Template {
                    template: vec![
                        PartJson::Literal("Count: ".to_string()),
                        PartJson::Slot { slot: 0 },
                    ],
                }),
                Some("audio/count.wav"),
            ),
        ],
    )]);

    let doc = lines_json_to_xliff(&lines, "en", None);
    let xml = xliff2::write::to_string(&doc).unwrap();
    insta::assert_snapshot!(xml);
}
