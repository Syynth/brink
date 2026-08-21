#![allow(clippy::unwrap_used, clippy::panic)]

use brink_intl::{
    ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson, SpanJson,
    compile_locale_xliff, generate_locale, lines_json_to_xliff, migrate_unit_ids,
    regenerate_locale, xliff_to_lines_json,
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

// ── Inline markup spans through XLIFF (#1716, real mapping #1734) ──

/// A non-empty [`PartJson::Span`] round-trips through XLIFF as a real paired
/// `<pc>` inline code — `name`/`attrs`/`children` (including the slot inside
/// it) all survive the export → XML → parse → import round trip, not merely
/// the flattened text.
#[test]
fn span_with_children_roundtrips_as_pc_through_xliff() {
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

    // The exported XML must actually contain a real `<pc>` element, not
    // just round-trip correctly through the in-memory Document — otherwise
    // a bug that only shows up in the XML writer/reader wouldn't be caught.
    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(
        xml.contains("<pc "),
        "expected a <pc> paired inline code in the exported XML, got:\n{xml}"
    );

    let parsed = xliff2::read::read_xliff(&xml).unwrap();
    let translated = fill_targets(parsed);
    let recovered = xliff_to_lines_json(&translated).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content, lines.scopes[0].lines[0].content,
        "span name/attrs/children must round-trip exactly, not just its \
         flattened text"
    );
}

/// Issue #1996 (`docs/prose-dialect-spec.md` §4.1, RULED 2026-08-01): a
/// hyphenated tag name (`<fade-in>`) round-trips through XLIFF exactly like
/// any other name — clone of
/// `span_with_children_roundtrips_as_pc_through_xliff` with `name:
/// "fade-in"`. This is the ⚠-flagged check that #1734's `<pc>`/`<x/>`
/// inline-code mapping still holds for a hyphenated name: `span.name`
/// rides through `originalData` as an opaque JSON string
/// (`SpanMetaJson`/`brink_intl::xliff_convert`), never as a raw XML
/// element/attribute *name* the mapping itself constructs — so nothing in
/// the pc/x mapping is sensitive to which characters appear in it, and
/// this pins that directly rather than by argument.
#[test]
fn a_hyphenated_span_name_roundtrips_as_pc_through_xliff() {
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
                            name: "fade-in".to_string(),
                            attrs: vec![],
                            children: vec![PartJson::Literal("the lantern".to_string())],
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
    assert!(
        xml.contains("<pc "),
        "expected a <pc> paired inline code in the exported XML, got:\n{xml}"
    );

    let parsed = xliff2::read::read_xliff(&xml).unwrap();
    let translated = fill_targets(parsed);
    let recovered = xliff_to_lines_json(&translated).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content, lines.scopes[0].lines[0].content,
        "a hyphenated span name must round-trip exactly, hyphen included"
    );
    let Some(ContentJson::Template { template }) = &recovered.scopes[0].lines[0].content else {
        panic!("expected a recovered Template");
    };
    let Some(PartJson::Span { span }) = template.get(1) else {
        panic!("expected the recovered span at index 1: {template:?}");
    };
    assert_eq!(span.name, "fade-in");
}

/// A childless (point-marker, §8b.11 — `<pause/>`, `<sfx name="bell"/>`)
/// [`PartJson::Span`] round-trips through XLIFF as a standalone inline
/// code. Under the pre-#1734 flattening path this span vanished entirely
/// (its empty `children` loop pushed nothing) — a silent drop, not merely a
/// lost boundary.
///
/// Uses a non-empty `attrs` (mirroring the motivating `<sfx name="bell"/>`
/// example in `push_part_inline`'s doc comment) — `SpanMetaJson.attrs` is
/// `#[serde(skip_serializing_if = "Vec::is_empty")]`, so an empty-attrs
/// fixture would never exercise the attrs decode path on the `<ph>` branch
/// (the sibling `span_with_children_roundtrips_as_pc_through_xliff` already
/// covers attrs on the `<pc>` branch).
#[test]
fn point_marker_span_roundtrips_through_xliff() {
    let lines = make_lines_json(vec![make_scope(
        "0x0100000000000001",
        Some("root"),
        vec![make_line(
            0,
            "aaaa",
            Some(ContentJson::Template {
                template: vec![
                    PartJson::Literal("Wait...".to_string()),
                    PartJson::Span {
                        span: brink_intl::SpanJson {
                            name: "sfx".to_string(),
                            attrs: vec![brink_intl::AttrJson {
                                name: "name".to_string(),
                                value: "bell".to_string(),
                            }],
                            children: Vec::new(),
                        },
                    },
                    PartJson::Literal(" now.".to_string()),
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

    assert_eq!(
        recovered.scopes[0].lines[0].content, lines.scopes[0].lines[0].content,
        "the point-marker span must survive, not just adjacent text"
    );
}

/// A [`PartJson::Span`] nested inside another span (recursive nesting is
/// the RULED doctrine of `docs/prose-dialect-spec.md` §4.4) round-trips
/// through XLIFF as a `<pc>` nested inside a `<pc>`. The shared
/// `span_counter` in `push_part_inline`/`content_to_inline` drives both the
/// `pc{n}`/`x{n}` element ids and the `dspan{n}` `originalData` ids across
/// the recursion — this is exactly where an id collision or a mis-paired
/// `dataRefStart` would surface, so both the outer and inner span's
/// `name`/`attrs`/`children` (and the plain text on either side of the
/// inner span) must survive intact.
#[test]
fn nested_span_roundtrips_through_xliff() {
    let lines = make_lines_json(vec![make_scope(
        "0x0100000000000001",
        Some("root"),
        vec![make_line(
            0,
            "aaaa",
            Some(ContentJson::Template {
                template: vec![
                    PartJson::Literal("He says, ".to_string()),
                    PartJson::Span {
                        span: brink_intl::SpanJson {
                            name: "quote".to_string(),
                            attrs: vec![brink_intl::AttrJson {
                                name: "speaker".to_string(),
                                value: "narrator".to_string(),
                            }],
                            children: vec![
                                PartJson::Literal("take the ".to_string()),
                                PartJson::Span {
                                    span: brink_intl::SpanJson {
                                        name: "item".to_string(),
                                        attrs: vec![brink_intl::AttrJson {
                                            name: "id".to_string(),
                                            value: "lantern".to_string(),
                                        }],
                                        children: vec![PartJson::Literal("lantern".to_string())],
                                    },
                                },
                                PartJson::Literal(", quickly".to_string()),
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
    assert!(
        xml.matches("<pc ").count() >= 2,
        "expected two nested <pc> elements in the exported XML, got:\n{xml}"
    );

    let parsed = xliff2::read::read_xliff(&xml).unwrap();
    let translated = fill_targets(parsed);
    let recovered = xliff_to_lines_json(&translated).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content, lines.scopes[0].lines[0].content,
        "a span nested inside another span must round-trip exactly, \
         including the outer/inner name/attrs and the id-generation \
         sequence shared across the recursion"
    );
}

/// A translation tool that re-expresses a brink-exported `<pc>` as an
/// `<sc>`/`<ec>` pair (XLIFF 2.0 allows this when a paired code is split
/// across a segment boundary) must not decode as silently-dropped content —
/// this is the same silent-drop failure class #1734 fixed on export, moved
/// to import. `elements_to_parts` must reject it explicitly instead of
/// falling into the inline-elements catch-all.
#[test]
fn split_pc_as_sc_ec_is_rejected_not_silently_dropped() {
    let mut doc = Document {
        version: "2.0".to_string(),
        src_lang: "en".to_string(),
        trg_lang: None,
        files: vec![File {
            id: "root".to_string(),
            original: None,
            notes: Vec::new(),
            skeleton: None,
            groups: Vec::new(),
            units: vec![Unit {
                id: "0x01:0".to_string(),
                name: None,
                translate: None,
                notes: Vec::new(),
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: Some(State::Translated),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("x".to_string())],
                    },
                    target: Some(Content {
                        lang: None,
                        elements: vec![
                            InlineElement::Sc(xliff2::Sc {
                                id: "pc0".to_string(),
                                data_ref: Some("dspan0".to_string()),
                                sub_type: Some("brink:pc".to_string()),
                                can_copy: None,
                                can_delete: None,
                                can_overlap: None,
                                can_reorder: None,
                                extensions: Extensions::default(),
                            }),
                            InlineElement::Text("bold".to_string()),
                            InlineElement::Ec(xliff2::Ec {
                                start_ref: Some("pc0".to_string()),
                                id: None,
                                isolated: None,
                                data_ref: None,
                                sub_type: None,
                                can_copy: None,
                                can_delete: None,
                                can_overlap: None,
                                can_reorder: None,
                                extensions: Extensions::default(),
                            }),
                        ],
                    }),
                })],
                original_data: Some(xliff2::OriginalData {
                    entries: vec![xliff2::DataEntry {
                        id: "dspan0".to_string(),
                        content: "{\"name\":\"b\"}".to_string(),
                    }],
                }),
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![ExtensionAttribute {
                        namespace: "brink".to_string(),
                        local_name: "hash".to_string(),
                        value: "aaaa".to_string(),
                    }],
                },
            }],
            extensions: Extensions {
                elements: Vec::new(),
                attributes: vec![ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "scope-id".to_string(),
                    value: "0x01".to_string(),
                }],
            },
        }],
        extensions: Extensions::default(),
    };
    doc.trg_lang = Some("es".to_string());

    let err = xliff_to_lines_json(&doc).unwrap_err();
    assert!(
        matches!(err, brink_intl::IntlError::UnsupportedSpanSplit(ref id) if id == "pc0"),
        "expected UnsupportedSpanSplit(\"pc0\"), got: {err:?}"
    );
}

/// A TMS is free to return translated text wrapped in a `<![CDATA[...]]>`
/// section — legal XLIFF content, and the same shape #765 already found and
/// fixed on the sibling `xliff2`-crate metadata-extraction path. Unlike the
/// `<sc>`/`<ec>` split above, CDATA carries no structural ambiguity — it's
/// just character data spelled with a different XML quoting mechanism — so
/// `elements_to_parts` decodes it exactly like `<mrk>`-free plain text
/// instead of erroring. This proves it survives the full wire round trip: a
/// translator-authored `<target><![CDATA[...]]></target>` (built directly
/// as the `InlineElement::CData` a TMS would produce, since brink's own
/// exporter never emits CDATA) serializes through the real XML writer,
/// re-parses through the real XML reader, and decodes back to the original
/// text — not silently dropped, per #1799.
#[test]
fn translator_authored_cdata_survives_export_import_roundtrip() {
    let doc = Document {
        version: "2.0".to_string(),
        src_lang: "en".to_string(),
        trg_lang: Some("fr".to_string()),
        files: vec![File {
            id: "root".to_string(),
            original: None,
            notes: Vec::new(),
            skeleton: None,
            groups: Vec::new(),
            units: vec![Unit {
                id: "0x01:0".to_string(),
                name: None,
                translate: None,
                notes: Vec::new(),
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: Some(State::Translated),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Hello world".to_string())],
                    },
                    target: Some(Content {
                        lang: None,
                        elements: vec![InlineElement::CData("Bonjour le monde".to_string())],
                    }),
                })],
                original_data: None,
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![ExtensionAttribute {
                        namespace: "brink".to_string(),
                        local_name: "hash".to_string(),
                        value: "aaaa".to_string(),
                    }],
                },
            }],
            extensions: Extensions {
                elements: Vec::new(),
                attributes: vec![ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "scope-id".to_string(),
                    value: "0x01".to_string(),
                }],
            },
        }],
        extensions: Extensions::default(),
    };

    // Round-trip through the real XML writer/reader, not just the parsed
    // AST — proves the on-wire `<![CDATA[...]]>` shape survives, not merely
    // that `elements_to_parts` handles a hand-built `InlineElement::CData`.
    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(
        xml.contains("<![CDATA[Bonjour le monde]]>"),
        "expected a real CDATA section in the serialized XLIFF, got: {xml}"
    );
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Plain("Bonjour le monde".to_string())),
        "translator-authored CDATA target text must decode like plain \
         text, not be silently dropped"
    );
}

/// The single-CDATA-element test above only proves the `inline_to_content`
/// plain-text fast path handles CDATA: `elements_to_parts`'s own output is
/// always wrapped in `ContentJson::Template` (never `Plain`), so a `Plain`
/// result can only ever come from that fast path, and reverting the
/// `elements_to_parts` decode arm alone would leave that test green. This
/// forces the multi-element path instead: the reader does not coalesce
/// CDATA into adjacent text (`push_text` only merges into a trailing
/// `InlineElement::Text`, `xliff2::read::inline`), so a translator-authored
/// target that mixes plain text with a CDATA section survives as two
/// separate inline elements and must go through `elements_to_parts`'s own
/// `Text | CData` match arm to avoid being silently dropped.
#[test]
fn cdata_mixed_with_text_is_recovered_by_elements_to_parts() {
    let doc = Document {
        version: "2.0".to_string(),
        src_lang: "en".to_string(),
        trg_lang: Some("fr".to_string()),
        files: vec![File {
            id: "root".to_string(),
            original: None,
            notes: Vec::new(),
            skeleton: None,
            groups: Vec::new(),
            units: vec![Unit {
                id: "0x01:0".to_string(),
                name: None,
                translate: None,
                notes: Vec::new(),
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: Some(State::Translated),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Hello world".to_string())],
                    },
                    target: Some(Content {
                        lang: None,
                        elements: vec![
                            InlineElement::Text("Bonjour ".to_string()),
                            InlineElement::CData("le monde".to_string()),
                        ],
                    }),
                })],
                original_data: None,
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![ExtensionAttribute {
                        namespace: "brink".to_string(),
                        local_name: "hash".to_string(),
                        value: "aaaa".to_string(),
                    }],
                },
            }],
            extensions: Extensions {
                elements: Vec::new(),
                attributes: vec![ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "scope-id".to_string(),
                    value: "0x01".to_string(),
                }],
            },
        }],
        extensions: Extensions::default(),
    };

    let recovered = xliff_to_lines_json(&doc).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a CDATA section mixed with plain text must decode via \
         elements_to_parts's Text | CData arm, not be silently dropped"
    );
}

/// Covers the recursive path the `elements_to_parts` doc comment claims: a
/// CDATA node nested inside a brink `<pc>`'s own content must decode via
/// the same recursive call to `elements_to_parts` that reconstructs the
/// `<pc>`'s children, not just the top-level element stream.
#[test]
fn cdata_inside_pc_content_is_recovered_by_elements_to_parts() {
    let doc = Document {
        version: "2.0".to_string(),
        src_lang: "en".to_string(),
        trg_lang: Some("fr".to_string()),
        files: vec![File {
            id: "root".to_string(),
            original: None,
            notes: Vec::new(),
            skeleton: None,
            groups: Vec::new(),
            units: vec![Unit {
                id: "0x01:0".to_string(),
                name: None,
                translate: None,
                notes: Vec::new(),
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: Some(State::Translated),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("bold".to_string())],
                    },
                    target: Some(Content {
                        lang: None,
                        elements: vec![InlineElement::Pc(xliff2::Pc {
                            id: "pc0".to_string(),
                            data_ref_start: Some("dspan0".to_string()),
                            data_ref_end: None,
                            sub_type: Some("brink:pc".to_string()),
                            content: vec![InlineElement::CData("gras".to_string())],
                            extensions: Extensions::default(),
                        })],
                    }),
                })],
                original_data: Some(xliff2::OriginalData {
                    entries: vec![xliff2::DataEntry {
                        id: "dspan0".to_string(),
                        content: "{\"name\":\"b\"}".to_string(),
                    }],
                }),
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![ExtensionAttribute {
                        namespace: "brink".to_string(),
                        local_name: "hash".to_string(),
                        value: "aaaa".to_string(),
                    }],
                },
            }],
            extensions: Extensions {
                elements: Vec::new(),
                attributes: vec![ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "scope-id".to_string(),
                    value: "0x01".to_string(),
                }],
            },
        }],
        extensions: Extensions::default(),
    };

    let recovered = xliff_to_lines_json(&doc).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![PartJson::Span {
                span: SpanJson {
                    name: "b".to_string(),
                    attrs: Vec::new(),
                    children: vec![PartJson::Literal("gras".to_string())],
                },
            }],
        }),
        "a CDATA node inside a <pc>'s content must decode via the \
         recursive elements_to_parts call, not be silently dropped"
    );
}

/// Span hash-transparency (`docs/prose-dialect-spec.md` §4.4) is a
/// compile-time property of `source_hash` — markup is normalized out
/// before hashing, so `Hello <wave>world</wave>` hashes identically to
/// `Hello world`. This module never re-derives that hash: `LineJson.hash`
/// is carried through as an opaque field regardless of what shape the
/// line's content takes. This test proves the new `<pc>`/`<ph>` span
/// mapping doesn't change that — the same externally-assigned hash comes
/// back unchanged whether the line's content is bare text or the same text
/// wrapped in a span, so a translator's TMS key for this line never moves
/// just because markup was added around already-translated words.
#[test]
fn span_mapping_does_not_disturb_line_hash() {
    const SHARED_HASH: &str = "abcd1234ef";

    let bare = make_lines_json(vec![make_scope(
        "0x01",
        None,
        vec![make_line(
            0,
            SHARED_HASH,
            Some(ContentJson::Plain("Hello world".to_string())),
            None,
        )],
    )]);
    let marked_up = make_lines_json(vec![make_scope(
        "0x01",
        None,
        vec![make_line(
            0,
            SHARED_HASH,
            Some(ContentJson::Template {
                template: vec![
                    PartJson::Literal("Hello ".to_string()),
                    PartJson::Span {
                        span: brink_intl::SpanJson {
                            name: "wave".to_string(),
                            attrs: Vec::new(),
                            children: vec![PartJson::Literal("world".to_string())],
                        },
                    },
                ],
            }),
            None,
        )],
    )]);

    for lines in [&bare, &marked_up] {
        let doc = lines_json_to_xliff(lines, "en", None);
        let xml = xliff2::write::to_string(&doc).unwrap();
        let parsed = xliff2::read::read_xliff(&xml).unwrap();
        let translated = fill_targets(parsed);
        let recovered = xliff_to_lines_json(&translated).unwrap();
        assert_eq!(
            recovered.scopes[0].lines[0].hash, SHARED_HASH,
            "hash must survive the XLIFF round trip unchanged regardless \
             of markup content: {lines:?}"
        );
    }
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

// ---------------------------------------------------------------------------
// `elements_to_parts` inline-element dispositions (#1811, #1812)
//
// These cover the shapes a *translation management system* hands back that
// brink's own exporter never emits, so each `Document` is built directly
// rather than by exporting a `LinesJson` first.
// ---------------------------------------------------------------------------

/// Build a one-scope/one-line `Document` whose `<target>` is exactly
/// `target`, with the `brink:hash`/`brink:scope-id` extension attributes
/// `xliff_to_lines_json` requires.
fn tms_returned_doc(
    target: Vec<InlineElement>,
    original_data: Option<xliff2::OriginalData>,
) -> Document {
    Document {
        version: "2.0".to_string(),
        src_lang: "en".to_string(),
        trg_lang: Some("fr".to_string()),
        files: vec![File {
            id: "root".to_string(),
            original: None,
            notes: Vec::new(),
            skeleton: None,
            groups: Vec::new(),
            units: vec![Unit {
                id: "0x01:0".to_string(),
                name: None,
                translate: None,
                notes: Vec::new(),
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: Some(State::Translated),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Hello world".to_string())],
                    },
                    target: Some(Content {
                        lang: None,
                        elements: target,
                    }),
                })],
                original_data,
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![ExtensionAttribute {
                        namespace: "brink".to_string(),
                        local_name: "hash".to_string(),
                        value: "aaaa".to_string(),
                    }],
                },
            }],
            extensions: Extensions {
                elements: Vec::new(),
                attributes: vec![ExtensionAttribute {
                    namespace: "brink".to_string(),
                    local_name: "scope-id".to_string(),
                    value: "0x01".to_string(),
                }],
            },
        }],
        extensions: Extensions::default(),
    }
}

/// `decode_cp`'s export inverse: a literal string containing a scalar XML
/// 1.0 text cannot carry — here U+0001, a C0 control character — must be
/// exported as a real `<cp hex="0001"/>` code point, not as a raw byte
/// inside a `<target>`/`<source>` text node. `quick_xml`'s text escaper
/// only escapes `< > & ' "`, so before this fix the byte reached the XML
/// unescaped: not well-formed XML 1.0, even though `quick_xml`'s own
/// non-validating reader parses it back (#1811 follow-up).
///
/// The single literal `"a\u{0001}b"` must split into `Text("a")`,
/// `Cp("0001")`, `Text("b")` — proving `push_literal_inline` splits a run
/// of characters around the illegal scalar rather than only handling a
/// lone one. Goes through the real XML writer and reader so the on-wire
/// `<cp/>` shape is proved, not just the in-memory `InlineElement`.
#[test]
fn control_character_literal_exports_as_cp_not_raw_bytes() {
    let lines = make_lines_json(vec![make_scope(
        "0x0100000000000001",
        Some("root"),
        vec![make_line(
            0,
            "aaaa",
            Some(ContentJson::Template {
                template: vec![PartJson::Literal("a\u{0001}b".to_string())],
            }),
            None,
        )],
    )]);

    let doc = lines_json_to_xliff(&lines, "en", None);
    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(
        xml.contains("<cp hex=\"0001\"/>"),
        "expected the illegal control character to export as a real <cp/> \
         element, got:\n{xml}"
    );
    assert!(
        !xml.as_bytes().contains(&0x01),
        "the raw control byte must not appear anywhere in the exported \
         XML — it must only appear encoded inside <cp hex=\"0001\"/>, got:\n{xml:?}"
    );

    let parsed = xliff2::read::read_xliff(&xml).unwrap();
    let translated = fill_targets(parsed);
    let recovered = xliff_to_lines_json(&translated).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("a".to_string()),
                PartJson::Literal("\u{0001}".to_string()),
                PartJson::Literal("b".to_string()),
            ],
        }),
        "the control character must survive export -> XML -> import intact"
    );
}

/// `<cp hex="…"/>` is XLIFF 2.0's way of writing a character by its Unicode
/// code point, used when the producing tool cannot or will not emit the
/// character literally. It is the exact sibling of the `<![CDATA[...]]>`
/// silent drop fixed in #1799 — same function, same catch-all — and the
/// same rule applies: `<cp>` carries no structure to reconstruct, only a
/// character, so it decodes as literal text rather than erroring (#1811).
///
/// Goes through the real XML writer and reader so the on-wire `<cp/>` shape
/// is proved, and surrounds it with text so the decode happens in
/// `elements_to_parts` proper — `inline_to_content`'s single-element fast
/// path would otherwise mask it.
#[test]
fn translator_authored_cp_survives_export_import_roundtrip() {
    // U+2028 LINE SEPARATOR — a real case for `<cp>`: legal in XML, but
    // routinely escaped by tooling that will not put it in a text node.
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Bonjour".to_string()),
            InlineElement::Cp("2028".to_string()),
            InlineElement::Text("le monde".to_string()),
        ],
        None,
    );

    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(
        xml.contains("<cp hex=\"2028\"/>"),
        "expected a real <cp/> element in the serialized XLIFF, got: {xml}"
    );
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour".to_string()),
                PartJson::Literal("\u{2028}".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a <cp/> code point must decode to its character, not be silently dropped"
    );
}

/// The recursive half of the `<cp>` fix: a `<cp/>` nested inside a brink
/// `<pc>`'s own content goes through the same recursive `elements_to_parts`
/// call that reconstructs the span's children, mirroring
/// `cdata_inside_pc_content_is_recovered_by_elements_to_parts`.
///
/// Also goes through the real XML writer and reader (unlike the other
/// hand-built-`Document` characterization tests in this file) so a bug
/// that only shows up in the writer/reader round trip — e.g. a `<cp/>`
/// nested inside a `<pc>` losing its `dataRefStart` sibling attribute, or
/// the reader mis-nesting the `<cp/>` — would be caught here too.
#[test]
fn cp_inside_pc_content_is_recovered_by_elements_to_parts() {
    let doc = tms_returned_doc(
        vec![InlineElement::Pc(xliff2::Pc {
            id: "pc0".to_string(),
            data_ref_start: Some("dspan0".to_string()),
            data_ref_end: None,
            sub_type: Some("brink:pc".to_string()),
            content: vec![
                InlineElement::Text("gr".to_string()),
                InlineElement::Cp("00E8".to_string()),
                InlineElement::Text("s".to_string()),
            ],
            extensions: Extensions::default(),
        })],
        Some(xliff2::OriginalData {
            entries: vec![xliff2::DataEntry {
                id: "dspan0".to_string(),
                content: "{\"name\":\"b\"}".to_string(),
            }],
        }),
    );

    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(
        xml.contains("<cp hex=\"00E8\"/>"),
        "expected a real <cp/> element nested in the <pc> in the serialized XLIFF, got: {xml}"
    );
    let doc = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&doc).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![PartJson::Span {
                span: SpanJson {
                    name: "b".to_string(),
                    attrs: Vec::new(),
                    children: vec![
                        PartJson::Literal("gr".to_string()),
                        PartJson::Literal("\u{00E8}".to_string()),
                        PartJson::Literal("s".to_string()),
                    ],
                },
            }],
        }),
        "a <cp/> inside a <pc>'s content must decode via the recursive \
         elements_to_parts call, not be silently dropped"
    );
}

/// A `hex` attribute that is not a Unicode scalar value is malformed XLIFF.
/// Skipping it would reintroduce exactly the silent drop #1811 is about, so
/// it is an explicit `IntlError::InvalidCodePoint`. All three failure modes
/// are covered: an unparseable hex string, a well-formed hex number in the
/// surrogate range, and one past the last code point.
#[test]
fn invalid_cp_hex_is_rejected_not_silently_dropped() {
    for bad_hex in ["zzzz", "D800", "110000"] {
        let doc = tms_returned_doc(
            vec![
                InlineElement::Text("Bonjour".to_string()),
                InlineElement::Cp(bad_hex.to_string()),
            ],
            None,
        );

        let err = xliff_to_lines_json(&doc).unwrap_err();
        assert!(
            matches!(err, brink_intl::IntlError::InvalidCodePoint(ref h) if h == bad_hex),
            "expected InvalidCodePoint({bad_hex:?}), got: {err:?}"
        );
    }
}

/// A **non-brink** `<mrk>` — the annotation a commercial TMS injects for
/// terminology, reviewer comments or QA flags — wraps a *span of text*, so
/// falling through the old catch-all lost the whole marked substring, not
/// just one character (#1812). The mark's own `id`/`type` are not brink
/// content and are still discarded, but the text it spans is translator
/// work and must survive.
///
/// Note the `type="term"` here does not start with `brink:`, so this is not
/// the brink-authored shape that
/// `split_pc_as_sc_ec_is_rejected_not_silently_dropped` covers — that one
/// still errors, because a brink span re-expressed as a mark has lost
/// structure that cannot be reconstructed.
#[test]
fn foreign_mrk_spanned_text_survives_export_import_roundtrip() {
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Bonjour ".to_string()),
            InlineElement::Mrk(xliff2::Mrk {
                id: "m1".to_string(),
                translate: None,
                mrk_type: Some("term".to_string()),
                ref_: None,
                value: None,
                content: vec![InlineElement::Text("le monde".to_string())],
                extensions: Extensions::default(),
            }),
        ],
        None,
    );

    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(
        xml.contains("<mrk id=\"m1\" type=\"term\">le monde</mrk>"),
        "expected a real foreign <mrk> in the serialized XLIFF, got: {xml}"
    );
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "the text spanned by a foreign <mrk> must survive import; \
         dropping it destroys translator work that cannot be regenerated"
    );
}

/// The foreign-`<mrk>` arm recurses through `elements_to_parts` rather than
/// pulling out the mark's `Text` nodes, so brink's own inline codes keep
/// working when a TMS wraps an annotation *around* them: here a `<mrk>`
/// encloses a brink `<pc>` span, which must still reconstruct as a
/// `PartJson::Span` with its children intact.
#[test]
fn foreign_mrk_wrapping_a_brink_span_preserves_the_span() {
    let doc = tms_returned_doc(
        vec![InlineElement::Mrk(xliff2::Mrk {
            id: "m1".to_string(),
            translate: None,
            mrk_type: Some("comment".to_string()),
            ref_: None,
            value: None,
            content: vec![InlineElement::Pc(xliff2::Pc {
                id: "pc0".to_string(),
                data_ref_start: Some("dspan0".to_string()),
                data_ref_end: None,
                sub_type: Some("brink:pc".to_string()),
                content: vec![InlineElement::Text("gras".to_string())],
                extensions: Extensions::default(),
            })],
            extensions: Extensions::default(),
        })],
        Some(xliff2::OriginalData {
            entries: vec![xliff2::DataEntry {
                id: "dspan0".to_string(),
                content: "{\"name\":\"b\"}".to_string(),
            }],
        }),
    );

    let recovered = xliff_to_lines_json(&doc).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![PartJson::Span {
                span: SpanJson {
                    name: "b".to_string(),
                    attrs: Vec::new(),
                    children: vec![PartJson::Literal("gras".to_string())],
                },
            }],
        }),
        "a brink <pc> wrapped in a foreign <mrk> must still reconstruct as \
         a span — the mrk arm recurses, it does not just harvest text"
    );
}

/// Characterization test for the `<sm>`/`<em>` half of #1812, which turned
/// out **not** to be a silent drop: unlike `<mrk>`, an `<sm>`/`<em>` pair
/// is two *empty* elements, and the text between them is a sibling of the
/// markers rather than a child, so it already decodes through the ordinary
/// text arm. This pins that down — the `Sm`/`Em` arms may keep ignoring the
/// markers themselves precisely because doing so cannot lose any text.
///
/// Unlike the tests above, this one passes both with and without this PR's
/// change; it exists to guard the reasoning the `Sm`/`Em` ignore arm
/// documents, not to prove a fix.
#[test]
fn foreign_sm_em_spanned_text_survives_export_import_roundtrip() {
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Bonjour ".to_string()),
            InlineElement::Sm(xliff2::Sm {
                id: "a1".to_string(),
                translate: None,
                sm_type: Some("term".to_string()),
                ref_: None,
                value: None,
                extensions: Extensions::default(),
            }),
            InlineElement::Text("le monde".to_string()),
            InlineElement::Em(xliff2::Em {
                start_ref: "a1".to_string(),
            }),
            InlineElement::Text("!".to_string()),
        ],
        None,
    );

    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(
        xml.contains("<sm id=\"a1\" type=\"term\"/>") && xml.contains("<em startRef=\"a1\"/>"),
        "expected real <sm/>/<em/> markers in the serialized XLIFF, got: {xml}"
    );
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("le monde".to_string()),
                PartJson::Literal("!".to_string()),
            ],
        }),
        "text spanned by an <sm>/<em> pair is a sibling of the markers and \
         must survive import even though the markers are ignored"
    );
}

// ---------------------------------------------------------------------------
// #1823: foreign, TMS-legitimate inline codes that merely resemble brink's
// own markers must not hard-fail the whole import. Discriminate on a
// brink-owned marker (`SPAN_MARKER_SUBTYPE`/`SLOT_SUBTYPE`/
// `SELECT_DATA_REF_PREFIX`, mirroring `looks_like_span_marker`), not element
// identity. Every test below goes through the real writer/reader round trip
// (`xliff2::write::to_string` + `xliff2::read::read_xliff`), the actual
// `compile-locale`/`export-xliff` consumer path, not just the in-memory
// `Document` model — proving the fix is reached by a real re-import, not
// merely by a unit that hands `elements_to_parts` a hand-built struct.
//
// Before this fix:
//   - a `<pc>` (same namespace) with no brink `dataRefStart` hard-failed
//     the whole import with `MissingSpanData`
//     (`foreign_pc_without_span_data_errors_not_silently_dropped`, #1821).
//   - a `<ph id="sep1">` (a TMS separator, id merely resembling brink's
//     `s{n}` slot spelling) hard-failed with `InvalidUnitId` trying to
//     parse `ep1` as a slot number.
//   - a `<ph dataRef="d1">` naming a foreign `<data>` payload hard-failed
//     with `InvalidSelectJson`/`MissingSelectData`
//     (`foreign_ph_with_data_ref_errors_not_silently_dropped`, #1821).
// The #1823 owner comment additionally widens this to a second reach path:
// `xliff2::read::local_name` strips namespace prefixes before dispatch, so
// a differently-namespaced element that collides on local name (`mq:pc`,
// `mq:ph`) is decoded as if it were brink/XLIFF-native and hits the exact
// same ungated arms. Since neither `xliff2::Pc` nor `xliff2::Ph` carries a
// namespace field, a same-namespace foreign element and a namespace-blind
// dispatch of a colliding-local-name element are structurally identical by
// the time they reach `elements_to_parts` — one gate covers both reach
// paths.
// ---------------------------------------------------------------------------

/// Same-namespace foreign `<pc>` (trigger path 1): a `<pc>` with no brink
/// `dataRefStart`/`subType` is not brink content, but it has a real content
/// model — unlike `<ph>`/`<sc>`/`<ec>` — so its children are spliced in
/// place rather than dropped or hard-failing, mirroring the foreign-`<mrk>`
/// arm (#1821's philosophy: a wrapper with a content model recovers its
/// children when it isn't brink's own). Was
/// `foreign_pc_without_span_data_errors_not_silently_dropped` pre-#1823,
/// pinning the hard-fail this test now proves is fixed.
#[test]
fn foreign_pc_without_span_data_recovers_children() {
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Bonjour ".to_string()),
            InlineElement::Pc(xliff2::Pc {
                id: "pc0".to_string(),
                data_ref_start: None,
                data_ref_end: None,
                sub_type: None,
                content: vec![InlineElement::Text("le monde".to_string())],
                extensions: Extensions::default(),
            }),
        ],
        None,
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a foreign <pc> without a brink dataRefStart must recover the text \
         it wraps instead of hard-failing the whole import (#1823)"
    );
}

/// Foreign, `dataRef`-bearing `<ph>` (trigger path 1): the *canonical*
/// foreign `<ph>` shape — a native code plus its own `<data>` payload, e.g.
/// `<ph id="ph1" dataRef="d1"/>` with `<data id="d1">&lt;b&gt;</data>` — is
/// ignored (not decoded, not a hard-fail) because it lacks brink's own
/// `dsel{n}` `dataRef` prefix. `<ph>` is an empty element, so ignoring it
/// cannot lose translator work. Covers both the "entry exists but isn't
/// brink `SelectJson`" and "dataRef names no entry at all" shapes — either
/// way, no error. Was `foreign_ph_with_data_ref_errors_not_silently_dropped`
/// pre-#1823, pinning the hard-fail this test now proves is fixed.
#[test]
fn foreign_ph_with_foreign_data_ref_is_ignored_not_hard_failed() {
    // A `<data>` entry exists, but its content is a host format's native
    // code payload, not brink `SelectJson`, and `d1` isn't `dsel`-prefixed.
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Bonjour ".to_string()),
            InlineElement::Ph(xliff2::Ph {
                id: "ph1".to_string(),
                data_ref: Some("d1".to_string()),
                equiv: None,
                disp: None,
                sub_type: None,
                extensions: Extensions::default(),
            }),
            InlineElement::Text("le monde".to_string()),
        ],
        Some(xliff2::OriginalData {
            entries: vec![xliff2::DataEntry {
                id: "d1".to_string(),
                content: "<b>".to_string(),
            }],
        }),
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();
    let recovered = xliff_to_lines_json(&parsed).unwrap();
    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a foreign <ph dataRef> whose entry is not SelectJson must be \
         ignored, not hard-fail (#1823)"
    );

    // The `dataRef` names no entry at all.
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Bonjour ".to_string()),
            InlineElement::Ph(xliff2::Ph {
                id: "ph1".to_string(),
                data_ref: Some("d1".to_string()),
                equiv: None,
                disp: None,
                sub_type: None,
                extensions: Extensions::default(),
            }),
            InlineElement::Text("le monde".to_string()),
        ],
        None,
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();
    let recovered = xliff_to_lines_json(&parsed).unwrap();
    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a foreign <ph dataRef> with no matching <data> entry must be \
         ignored, not hard-fail (#1823)"
    );
}

/// TMS-authored `<ph id="sep1">` (trigger path 1): before #1823 the slot
/// branch discriminated by `id.starts_with('s')` + parse-remainder-as-`u8`,
/// so this separator marker's id was mistaken for a slot spelling and
/// hard-failed trying to parse `ep1` as a number. Gated on brink's own
/// `SLOT_SUBTYPE` marker now, so a non-brink `<ph>` with an `s`-prefixed id
/// is simply ignored.
#[test]
fn tms_separator_ph_with_slot_like_id_is_ignored_not_hard_failed() {
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Bonjour".to_string()),
            InlineElement::Ph(xliff2::Ph {
                id: "sep1".to_string(),
                data_ref: None,
                equiv: None,
                disp: None,
                sub_type: None,
                extensions: Extensions::default(),
            }),
            InlineElement::Text("le monde".to_string()),
        ],
        None,
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a foreign <ph id=\"sep1\"> must not be mistaken for a brink slot \
         and must not hard-fail the import (#1823)"
    );
}

/// Trigger path 2 (owner comment, 2026-08-21): a differently-namespaced
/// `<mq:pc>` collides on local name with brink's own `<pc>` because
/// `xliff2::read::local_name` strips namespace prefixes before dispatch.
/// Hand-built raw XML (rather than `tms_returned_doc` + the writer) because
/// the `Document`/`InlineElement` model has no namespace-prefix field to
/// construct this shape from — the reach path only exists on the wire.
/// Goes through `xliff2::read::read_xliff` directly, the real re-import
/// entry point, so this proves the fix through the actual reach path rather
/// than a hand-built in-memory struct.
#[test]
fn namespaced_mq_pc_colliding_local_name_recovers_children() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff xmlns="urn:oasis:names:tc:xliff:document:2.0" version="2.0" srcLang="en" trgLang="fr" xmlns:brink="urn:brink:xliff:extensions:1.0" xmlns:mq="urn:x-memoq:xliff:extensions:1.0">
  <file id="root" brink:scope-id="0x01">
    <unit id="0x01:0" brink:hash="aaaa">
      <segment state="translated">
        <source>Hello world</source>
        <target>Bonjour <mq:pc id="pc0">le monde</mq:pc></target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    let parsed = xliff2::read::read_xliff(xml).unwrap();
    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a namespaced <mq:pc> colliding on local name with brink's own \
         <pc> must recover the text it wraps, not hard-fail (#1823)"
    );
}

/// Trigger path 2 (owner comment, 2026-08-21): `<mq:ph id="sep1"/>` — same
/// namespace-blind-dispatch reach path as
/// `namespaced_mq_pc_colliding_local_name_recovers_children`, but for the
/// `<ph>`/slot-id-collision half of the bug.
#[test]
fn namespaced_mq_ph_colliding_local_name_is_ignored_not_hard_failed() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff xmlns="urn:oasis:names:tc:xliff:document:2.0" version="2.0" srcLang="en" trgLang="fr" xmlns:brink="urn:brink:xliff:extensions:1.0" xmlns:mq="urn:x-memoq:xliff:extensions:1.0">
  <file id="root" brink:scope-id="0x01">
    <unit id="0x01:0" brink:hash="aaaa">
      <segment state="translated">
        <source>Hello world</source>
        <target>Bonjour <mq:ph id="sep1"/> le monde</target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    let parsed = xliff2::read::read_xliff(xml).unwrap();
    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal(" le monde".to_string()),
            ],
        }),
        "a namespaced <mq:ph id=\"sep1\"/> colliding on local name with a \
         brink slot ph must be ignored, not hard-fail (#1823)"
    );
}

/// The #1821 review's noted widening: a foreign `<ph>` nested *inside* a
/// foreign `<pc>` (both same-namespace here; trigger path 1). Before this
/// fix, the outer `<pc>` alone hard-failed with `MissingSpanData` before
/// the inner `<ph>` was ever reached. Now the outer `<pc>` splices its
/// children (recursing into `elements_to_parts`), so the inner foreign
/// `<ph>` gets the ordinary ignored disposition and the text around it
/// survives.
#[test]
fn foreign_ph_nested_inside_foreign_pc_survives_import() {
    let doc = tms_returned_doc(
        vec![InlineElement::Pc(xliff2::Pc {
            id: "pc0".to_string(),
            data_ref_start: None,
            data_ref_end: None,
            sub_type: None,
            content: vec![
                InlineElement::Text("Bonjour".to_string()),
                InlineElement::Ph(xliff2::Ph {
                    id: "sep1".to_string(),
                    data_ref: None,
                    equiv: None,
                    disp: None,
                    sub_type: None,
                    extensions: Extensions::default(),
                }),
                InlineElement::Text("le monde".to_string()),
            ],
            extensions: Extensions::default(),
        })],
        None,
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour".to_string()),
                PartJson::Literal("le monde".to_string()),
            ],
        }),
        "a foreign <ph> nested inside a foreign <pc> must survive import — \
         both the outer wrapper's splice and the inner ph's ignore must \
         compose (#1823)"
    );
}

/// Regression floor: a *brink-authored* paired span (`subType="brink:pc"`,
/// `dataRefStart` pointing at real span metadata) must still decode exactly
/// as before — the gate added by #1823 must not treat brink's own markers
/// as foreign.
#[test]
fn brink_pc_still_decodes_exactly_as_before() {
    let doc = tms_returned_doc(
        vec![InlineElement::Pc(xliff2::Pc {
            id: "pc0".to_string(),
            data_ref_start: Some("dspan0".to_string()),
            data_ref_end: None,
            sub_type: Some("brink:pc".to_string()),
            content: vec![InlineElement::Text("gras".to_string())],
            extensions: Extensions::default(),
        })],
        Some(xliff2::OriginalData {
            entries: vec![xliff2::DataEntry {
                id: "dspan0".to_string(),
                content: "{\"name\":\"b\"}".to_string(),
            }],
        }),
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![PartJson::Span {
                span: SpanJson {
                    name: "b".to_string(),
                    attrs: Vec::new(),
                    children: vec![PartJson::Literal("gras".to_string())],
                },
            }],
        }),
        "a brink-authored <pc> must still decode as a span (#1823 regression floor)"
    );
}

// ---------------------------------------------------------------------------
// #1824: the xliff2-layer catch-all that could still defeat #1821
// ---------------------------------------------------------------------------

/// #1821 made `elements_to_parts` exhaustive over `xliff2::InlineElement` —
/// but that model is only ever populated by `xliff2::read::read_inline_content`
/// in the first place. Before #1824, an XML element that reader did not
/// recognize (a TMS extension wrapping translator text, e.g. a memoQ QA
/// comment) never became an `InlineElement` at all: `read_inline_content`
/// routed it to `skip_element`, which discarded the text inside before
/// `elements_to_parts` ever ran. No amount of exhaustive matching one layer
/// up could recover text that never arrived.
///
/// This test goes through the **real reading + import path** exactly as
/// `brink compile-locale` and `brink regenerate-xliff` run it
/// (`crates/brink-cli/src/main.rs`: `xliff2::read::read_xliff` then
/// `brink_intl::compile_locale_xliff` / `regenerate_locale`, both of which
/// call `xliff_to_lines_json` immediately after parsing) — a hand-written
/// `.xlf` *string*, not a hand-built `Document`, because the bug this
/// closes is in the XML-reading boundary itself.
#[test]
fn tms_extension_wrapped_text_survives_the_real_import_path() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff version="2.0" srcLang="en" trgLang="fr"
       xmlns="urn:oasis:names:tc:xliff:document:2.0"
       xmlns:brink="urn:brink:xliff:extensions:1.0"
       xmlns:mq="urn:tms:memoq:extensions:1.0">
  <file id="root" brink:scope-id="0x01">
    <unit id="0x01:0" brink:hash="aaaa">
      <segment state="translated">
        <source>Hello world</source>
        <target>Bonjour <mq:comment id="c1"><mq:reason><![CDATA[QA <flag>]]></mq:reason>le monde</mq:comment><mq:flag id="f1"/>!</target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    let doc = xliff2::read::read_xliff(xml).expect("well-formed XLIFF must parse");
    let recovered = xliff_to_lines_json(&doc).expect("import must not fail");

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Bonjour ".to_string()),
                PartJson::Literal("QA <flag>".to_string()),
                PartJson::Literal("le monde!".to_string()),
            ],
        }),
        "text carried inside a TMS extension element — including a nested \
         CDATA reason, and past an unrecognized empty-element sibling — \
         must reach LinesJson through the real read_xliff -> \
         xliff_to_lines_json path, not vanish at the XML-reading boundary"
    );
}

/// The byte-identical translated string from the PR's own RED/GREEN diff:
/// text immediately preceding an unknown TMS wrapper must coalesce with the
/// wrapper's leading text into a *single* `InlineElement::Text`, so it
/// imports as `ContentJson::Plain` — matching what the same string imports
/// as when no TMS wrapper is present at all. Before the fix that routes
/// recursed `Text` children through `push_text` (rather than splicing them
/// in with a bare `extend`), this string produced two adjacent `Text`
/// elements and `inline_to_content` — which only collapses to
/// `ContentJson::Plain` when `elements.len() == 1` — imported it as
/// `ContentJson::Template` instead, flipping `LineContent::Plain` to
/// `LineContent::Template` in the compiled locale and, per
/// `is_empty_content` (`xliff_convert.rs:189`), flipping whether the line
/// registers as non-empty content.
#[test]
fn tms_wrapper_with_no_intervening_structure_imports_as_plain_content() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff version="2.0" srcLang="en" trgLang="fr"
       xmlns="urn:oasis:names:tc:xliff:document:2.0"
       xmlns:brink="urn:brink:xliff:extensions:1.0"
       xmlns:mq="urn:tms:memoq:extensions:1.0">
  <file id="root" brink:scope-id="0x01">
    <unit id="0x01:0" brink:hash="aaaa">
      <segment state="translated">
        <source>Hello world</source>
        <target>Bonjour <mq:c id="c1">le monde</mq:c></target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    let doc = xliff2::read::read_xliff(xml).expect("well-formed XLIFF must parse");
    let recovered = xliff_to_lines_json(&doc).expect("import must not fail");

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Plain("Bonjour le monde".to_string())),
        "text split only by an unknown wrapper's tag boundary — no CDATA, \
         no other sibling — must still coalesce into one Text element and \
         import as ContentJson::Plain, not ContentJson::Template"
    );
}

/// Back-compat regression (#1823 review, BLOCKING finding): every `.xlf`
/// brink exported BEFORE `SLOT_SUBTYPE` existed spells a slot as
/// `<ph id="s{n}" equiv="{slot n}"/>` with no `subType`. Re-importing such
/// a file (they sit at TMSes mid-translation) must still decode the slot —
/// classifying it as foreign would silently drop the placeholder from the
/// translated line: translator-data loss.
#[test]
fn legacy_subtype_less_slot_ph_still_decodes_as_slot() {
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Compte : ".to_string()),
            InlineElement::Ph(xliff2::Ph {
                id: "s0".to_string(),
                data_ref: None,
                equiv: Some("{slot 0}".to_string()),
                disp: None,
                sub_type: None,
                extensions: Extensions::default(),
            }),
        ],
        None,
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Compte : ".to_string()),
                PartJson::Slot { slot: 0 },
            ],
        }),
        "a pre-SLOT_SUBTYPE brink slot ph (id s0 + equiv {{slot 0}}, no \
         subType) must still decode as a slot on re-import (#1823 review)"
    );
}

/// The legacy fallback's narrowness: a foreign subType-less `<ph>` whose id
/// merely LOOKS like a brink slot (`s2`) but lacks brink's own
/// `equiv="{slot 2}"` stays foreign (ignored) — the fallback requires BOTH
/// legacy markers to agree, so the #1823 gate is not weakened.
#[test]
fn coincidental_slot_shaped_foreign_ph_without_equiv_stays_foreign() {
    let doc = tms_returned_doc(
        vec![
            InlineElement::Text("Hello".to_string()),
            InlineElement::Ph(xliff2::Ph {
                id: "s2".to_string(),
                data_ref: None,
                equiv: None,
                disp: None,
                sub_type: None,
                extensions: Extensions::default(),
            }),
            InlineElement::Text(" world".to_string()),
        ],
        None,
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Hello".to_string()),
                PartJson::Literal(" world".to_string()),
            ],
        }),
        "a foreign subType-less ph with a coincidentally slot-shaped id but \
         no brink equiv must stay foreign, not decode as a slot (#1823)"
    );
}

/// Depth regression (#1823 review, NOTE): a *brink-authored* slot nested
/// inside a FOREIGN `<pc>` wrapper must survive the wrapper's splice — the
/// splice recurses through the same decoder, so brink markers inside a
/// foreign wrapper keep decoding with full fidelity.
#[test]
fn brink_slot_inside_foreign_pc_decodes_through_the_splice() {
    let doc = tms_returned_doc(
        vec![InlineElement::Pc(xliff2::Pc {
            id: "pc9".to_string(),
            data_ref_start: None,
            data_ref_end: None,
            sub_type: None,
            content: vec![
                InlineElement::Text("Score: ".to_string()),
                InlineElement::Ph(xliff2::Ph {
                    id: "s1".to_string(),
                    data_ref: None,
                    equiv: Some("{slot 1}".to_string()),
                    disp: None,
                    sub_type: None,
                    extensions: Extensions::default(),
                }),
            ],
            extensions: Extensions::default(),
        })],
        None,
    );
    let xml = xliff2::write::to_string(&doc).unwrap();
    let parsed = xliff2::read::read_xliff(&xml).unwrap();

    let recovered = xliff_to_lines_json(&parsed).unwrap();

    assert_eq!(
        recovered.scopes[0].lines[0].content,
        Some(ContentJson::Template {
            template: vec![
                PartJson::Literal("Score: ".to_string()),
                PartJson::Slot { slot: 1 },
            ],
        }),
        "a brink slot nested inside a foreign <pc> must decode through the \
         wrapper's splice (#1823)"
    );
}
