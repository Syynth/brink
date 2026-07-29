#![allow(clippy::unwrap_used)]

use brink_intl::{
    ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson, compile_locale_xliff,
    generate_locale, lines_json_to_xliff, migrate_unit_ids, regenerate_locale, xliff_to_lines_json,
};
use xliff2::{
    Content, Document, ExtensionAttribute, Extensions, File, InlineElement, Segment, State,
    SubUnit, Unit,
};

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

// ── Inline markup spans through XLIFF (#1716) — documented v1 flattening ──

/// `PartJson::Span` (#1716) round-trips through XLIFF as flattened text: the
/// span's `children` splice into the surrounding element stream (no `<pc>`
/// paired inline code yet — "Translation, round 2",
/// `docs/prose-dialect-spec.md` §9), but every word and slot inside it
/// still survives the export → XML → parse → import round trip correctly.
/// This proves the *documented* v1 limitation is exactly what's documented
/// — the text is not lost, only the span boundary doesn't survive.
#[test]
fn span_flattens_through_xliff_but_loses_no_text_or_slots() {
    let lines = make_lines_json(vec![make_scope(
        "0x0100000000000001",
        Some("root"),
        vec![make_line(
            0,
            "aaaa",
            Some(ContentJson::Template {
                template: vec![
                    PartJson::Literal("He hands you ".to_string()),
                    PartJson::Span {
                        span: brink_intl::SpanJson {
                            name: "item".to_string(),
                            attrs: vec![brink_intl::AttrJson {
                                name: "id".to_string(),
                                value: "lantern".to_string(),
                            }],
                            children: vec![
                                PartJson::Literal("the ".to_string()),
                                PartJson::Slot { slot: 0 },
                                PartJson::Literal(" lantern".to_string()),
                            ],
                        },
                    },
                    PartJson::Literal(".".to_string()),
                ],
            }),
            None,
        )],
    )]);

    let doc = lines_json_to_xliff(&lines, "en", None);
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();
    let translated = fill_targets(parsed);
    let recovered = xliff_to_lines_json(&translated).unwrap();

    let Some(ContentJson::Template { template }) = &recovered.scopes[0].lines[0].content else {
        panic!(
            "expected a Template, got {:?}",
            recovered.scopes[0].lines[0].content
        );
    };
    // Flattened: no PartJson::Span survives the round trip (documented v1
    // limitation), but every literal and the slot are still present, in
    // order, with the span's own text intact.
    assert!(
        !template
            .iter()
            .any(|p| matches!(p, PartJson::Span { .. })),
        "span structure is not (yet) preserved through XLIFF: {template:?}"
    );
    // Adjacent literal parts may merge across the XML round trip (XML text
    // nodes don't preserve arbitrary split boundaries between adjacent
    // `InlineElement::Text`s) — what matters is the *concatenated* text is
    // unchanged, not the exact segmentation.
    let concatenated: String = template
        .iter()
        .filter_map(|p| match p {
            PartJson::Literal(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        concatenated, "He hands you the  lantern.",
        "no text lost by flattening"
    );
    assert!(
        template.iter().any(|p| matches!(p, PartJson::Slot { slot: 0 })),
        "the slot inside the span must survive"
    );
}

// ── generate_locale → fill targets → compile_locale_xliff ──

#[test]
fn generate_and_compile_xliff() {
    let src = include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink");
    let data = brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data;

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
    let src = include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink");
    let data = brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data;

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

// ── `brink migrate-xliff`'s actual code path, full XML round trip ──
//
// Exercises exactly what `run_migrate_xliff` (crates/brink-cli/src/main.rs)
// does: `xliff2::read::read_xliff` → `migrate_unit_ids` →
// `xliff2::write::to_string`, starting from a legacy display-name-id
// document serialized to XML text (not an in-memory `Document` built and
// consumed without ever touching the parser/serializer).

/// Build a legacy (pre-#1442) document exactly as `xliff2::write` would
/// serialize one exported by old brink: unit id built from the display
/// name, translated content, non-default state, and every `brink:*`
/// extension attribute this crate emits (`scope-id`, `hash`, `audio`).
fn make_legacy_document() -> Document {
    Document {
        version: "2.0".to_string(),
        src_lang: "en".to_string(),
        trg_lang: Some("es".to_string()),
        files: vec![File {
            id: "intro".to_string(),
            original: None,
            notes: Vec::new(),
            skeleton: None,
            groups: Vec::new(),
            units: vec![Unit {
                id: "intro:0".to_string(),
                name: Some("intro".to_string()),
                translate: None,
                notes: Vec::new(),
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: Some(State::Translated),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Hello".to_string())],
                    },
                    target: Some(Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Hola".to_string())],
                    }),
                })],
                original_data: None,
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![
                        ExtensionAttribute {
                            namespace: "brink".to_string(),
                            local_name: "hash".to_string(),
                            value: "aaa".to_string(),
                        },
                        ExtensionAttribute {
                            namespace: "brink".to_string(),
                            local_name: "audio".to_string(),
                            value: "audio/hi.wav".to_string(),
                        },
                    ],
                },
            }],
            extensions: Extensions {
                elements: Vec::new(),
                attributes: vec![ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "scope-id".to_string(),
                    value: "0x0100000000000001".to_string(),
                }],
            },
        }],
        extensions: Extensions {
            elements: Vec::new(),
            attributes: vec![
                ExtensionAttribute {
                    namespace: "xmlns".to_string(),
                    local_name: "brink".to_string(),
                    value: brink_intl::BRINK_NS.to_string(),
                },
                ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "checksum".to_string(),
                    value: "0xdeadbeef".to_string(),
                },
                ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "version".to_string(),
                    value: "1".to_string(),
                },
            ],
        },
    }
}

#[test]
fn migrate_unit_ids_survives_full_xml_round_trip() {
    let legacy_doc = make_legacy_document();

    // Serialize to XML text, exactly as an archived `.xlf` on disk.
    let legacy_xml = xliff2::write::to_string(&legacy_doc).unwrap();

    // `run_migrate_xliff`'s actual path: parse XML → migrate → serialize XML.
    let mut parsed = xliff2::read::read_xliff(&legacy_xml).unwrap();
    let changed = migrate_unit_ids(&mut parsed).unwrap();
    assert_eq!(changed, 1);
    let migrated_xml = xliff2::write::to_string(&parsed).unwrap();

    // Read the migrated XML back once more and assert everything the CLI
    // promises survives the full round trip.
    let result = xliff2::read::read_xliff(&migrated_xml).unwrap();
    let unit = &result.files[0].units[0];

    // Unit id was rewritten to the scope-id-based scheme.
    assert_eq!(unit.id, "0x0100000000000001:0");
    // `name` (display-name metadata) untouched.
    assert_eq!(unit.name, Some("intro".to_string()));

    // Segment state and target content untouched.
    let SubUnit::Segment(seg) = &unit.sub_units[0] else {
        unreachable!()
    };
    assert_eq!(seg.state, Some(State::Translated));
    assert_eq!(
        seg.target.as_ref().unwrap().elements,
        vec![InlineElement::Text("Hola".to_string())]
    );

    // Every brink:* extension attribute on the unit untouched.
    let ext = |local_name: &str| {
        unit.extensions
            .attributes
            .iter()
            .find(|a| a.namespace == "brink" && a.local_name == local_name)
            .map(|a| a.value.as_str())
    };
    assert_eq!(ext("hash"), Some("aaa"));
    assert_eq!(ext("audio"), Some("audio/hi.wav"));

    // The file's brink:scope-id extension untouched.
    let file_ext = |local_name: &str| {
        result.files[0]
            .extensions
            .attributes
            .iter()
            .find(|a| a.namespace == "brink" && a.local_name == local_name)
            .map(|a| a.value.as_str())
    };
    assert_eq!(file_ext("scope-id"), Some("0x0100000000000001"));

    // Document-level brink:* extensions (checksum, version) untouched.
    let doc_ext = |local_name: &str| {
        result
            .extensions
            .attributes
            .iter()
            .find(|a| a.namespace == "brink" && a.local_name == local_name)
            .map(|a| a.value.as_str())
    };
    assert_eq!(doc_ext("checksum"), Some("0xdeadbeef"));
    assert_eq!(doc_ext("version"), Some("1"));

    // Idempotent: migrating the already-migrated document is a no-op.
    let mut already_migrated = result;
    let second_pass = migrate_unit_ids(&mut already_migrated).unwrap();
    assert_eq!(second_pass, 0);
}

// ── #1442: a declared `#@was` rename rebinds through the alias table ──
//
// The whole `brink regenerate-xliff` path, over real XML: export the
// pre-rename story, translate it, serialize, rename the knot with `#@was`,
// recompile, and regenerate. Before alias-awareness the renamed knot's
// `<file>` came back with every `<target>` gone.

/// A knot whose translations must survive the rename below.
const RENAME_BEFORE: &str = "\
== hub ==
Welcome to the hub.
-> END
";

/// [`RENAME_BEFORE`] with the knot renamed and the rename declared.
const RENAME_AFTER: &str = "\
== plaza ==
#@was(hub)
Welcome to the hub.
-> END
";

/// `#@was` is a brink-dialect extension (`E051` under strict ink).
fn compile_brink(src: &str) -> brink_format::StoryData {
    let options = brink_compiler::AnalysisOptions {
        dialect: brink_compiler::Dialect::Brink,
        ..brink_compiler::AnalysisOptions::default()
    };
    brink_compiler::compile_with_options("story.ink", |_p| Ok(src.to_owned()), options)
        .unwrap()
        .data
}

/// The `brink:scope-id` of the exported scope with this display name.
fn exported_scope_id(story: &brink_format::StoryData, name: &str) -> String {
    brink_intl::export_lines(story, 0)
        .scopes
        .into_iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap()
        .id
}

/// Every `<target>` text in the `<file>` carrying this `brink:scope-id`.
fn targets_of_scope(doc: &Document, scope_id: &str) -> Vec<String> {
    doc.files
        .iter()
        .filter(|f| {
            f.extensions.attributes.iter().any(|a| {
                a.namespace == "brink" && a.local_name == "scope-id" && a.value == scope_id
            })
        })
        .flat_map(|f| &f.units)
        .flat_map(|u| &u.sub_units)
        .filter_map(|su| match su {
            SubUnit::Segment(seg) => seg.target.as_ref(),
            SubUnit::Ignorable(_) => None,
        })
        .flat_map(|c| &c.elements)
        .filter_map(|el| match el {
            InlineElement::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn regenerate_locale_rebinds_a_declared_rename_through_real_xml() {
    let before = compile_brink(RENAME_BEFORE);
    let after = compile_brink(RENAME_AFTER);
    assert!(
        !after.alias_table.is_empty(),
        "`#@was(hub)` must mint the alias edge this test rebinds through"
    );

    let mut doc = generate_locale(&before, 0x1111, "en", Some("es"));
    for file in &mut doc.files {
        for unit in &mut file.units {
            for su in &mut unit.sub_units {
                if let SubUnit::Segment(seg) = su {
                    seg.target = Some(Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Bienvenido".to_string())],
                    });
                    seg.state = Some(State::Translated);
                }
            }
        }
    }
    // Through real XLIFF XML, as `brink regenerate-xliff` reads it.
    let xml = xliff2::write::to_string(&doc).unwrap();
    let existing = xliff2::read::read_xliff(&xml).unwrap();
    let old_knot = exported_scope_id(&before, "hub");
    assert_eq!(targets_of_scope(&existing, &old_knot), ["Bienvenido"]);

    let result = regenerate_locale(&after, 0x2222, "en", &existing).unwrap();

    let new_knot = exported_scope_id(&after, "plaza");
    assert_ne!(old_knot, new_knot, "the rename must move the scope id");
    assert_eq!(
        targets_of_scope(&result, &new_knot),
        ["Bienvenido"],
        "the declared rename must carry the translation onto the new scope id"
    );

    // The state survives too: the prose is byte-identical, so the line hash
    // is unchanged and the segment must not be reset to `initial`.
    let states: Vec<Option<State>> = result
        .files
        .iter()
        .flat_map(|f| &f.units)
        .flat_map(|u| &u.sub_units)
        .filter_map(|su| match su {
            SubUnit::Segment(seg) => Some(seg.state),
            SubUnit::Ignorable(_) => None,
        })
        .collect();
    assert_eq!(states, [Some(State::Translated)]);
}
