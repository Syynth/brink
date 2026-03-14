use xliff2::*;

fn minimal_doc() -> Document {
    Document {
        version: "2.0".to_owned(),
        src_lang: "en".to_owned(),
        trg_lang: Some("fr".to_owned()),
        files: vec![File {
            id: "f1".to_owned(),
            original: Some("hello.txt".to_owned()),
            notes: vec![],
            skeleton: None,
            groups: vec![],
            units: vec![Unit {
                id: "u1".to_owned(),
                name: None,
                notes: vec![],
                sub_units: vec![SubUnit::Segment(Segment {
                    id: Some("s1".to_owned()),
                    state: Some(State::Initial),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Hello".to_owned())],
                    },
                    target: Some(Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Bonjour".to_owned())],
                    }),
                })],
                original_data: None,
                extensions: Extensions::default(),
            }],
            extensions: Extensions::default(),
        }],
        extensions: Extensions::default(),
    }
}

#[test]
fn write_minimal_document() {
    let doc = minimal_doc();
    let xml = write::to_string(&doc).unwrap();
    insta::assert_snapshot!(xml);
}

#[test]
fn round_trip_minimal() {
    let doc = minimal_doc();
    let xml = write::to_string(&doc).unwrap();
    let parsed = read::read_xliff(&xml).unwrap();
    assert_eq!(doc, parsed);
}

#[test]
fn round_trip_with_inline_codes() {
    let doc = Document {
        version: "2.0".to_owned(),
        src_lang: "en".to_owned(),
        trg_lang: None,
        files: vec![File {
            id: "f1".to_owned(),
            original: None,
            notes: vec![],
            skeleton: None,
            groups: vec![],
            units: vec![Unit {
                id: "u1".to_owned(),
                name: None,
                notes: vec![],
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: None,
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![
                            InlineElement::Text("Click ".to_owned()),
                            InlineElement::Pc(Pc {
                                id: "1".to_owned(),
                                data_ref_start: None,
                                data_ref_end: None,
                                sub_type: None,
                                content: vec![InlineElement::Text("here".to_owned())],
                                extensions: Extensions::default(),
                            }),
                            InlineElement::Text(" or press ".to_owned()),
                            InlineElement::Ph(Ph {
                                id: "2".to_owned(),
                                data_ref: Some("d1".to_owned()),
                                equiv: Some("[Enter]".to_owned()),
                                disp: None,
                                sub_type: None,
                                extensions: Extensions::default(),
                            }),
                        ],
                    },
                    target: None,
                })],
                original_data: Some(OriginalData {
                    entries: vec![DataEntry {
                        id: "d1".to_owned(),
                        content: "&lt;kbd&gt;".to_owned(),
                    }],
                }),
                extensions: Extensions::default(),
            }],
            extensions: Extensions::default(),
        }],
        extensions: Extensions::default(),
    };

    let xml = write::to_string(&doc).unwrap();
    let parsed = read::read_xliff(&xml).unwrap();
    assert_eq!(doc, parsed);
}

#[test]
fn round_trip_with_notes() {
    let doc = Document {
        version: "2.1".to_owned(),
        src_lang: "en".to_owned(),
        trg_lang: None,
        files: vec![File {
            id: "f1".to_owned(),
            original: None,
            notes: vec![Note {
                id: Some("n1".to_owned()),
                category: Some("instructions".to_owned()),
                priority: Some(1),
                applies_to: Some(AppliesTo::Source),
                content: "Translate carefully".to_owned(),
            }],
            skeleton: None,
            groups: vec![],
            units: vec![Unit {
                id: "u1".to_owned(),
                name: Some("greeting".to_owned()),
                notes: vec![Note {
                    id: None,
                    category: None,
                    priority: None,
                    applies_to: None,
                    content: "Unit-level note".to_owned(),
                }],
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: Some(State::Translated),
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![InlineElement::Text("Hi".to_owned())],
                    },
                    target: Some(Content {
                        lang: Some("de".to_owned()),
                        elements: vec![InlineElement::Text("Hallo".to_owned())],
                    }),
                })],
                original_data: None,
                extensions: Extensions::default(),
            }],
            extensions: Extensions::default(),
        }],
        extensions: Extensions::default(),
    };

    let xml = write::to_string(&doc).unwrap();
    let parsed = read::read_xliff(&xml).unwrap();
    assert_eq!(doc, parsed);
}

#[test]
fn round_trip_with_groups() {
    let doc = Document {
        version: "2.0".to_owned(),
        src_lang: "en".to_owned(),
        trg_lang: None,
        files: vec![File {
            id: "f1".to_owned(),
            original: None,
            notes: vec![],
            skeleton: None,
            groups: vec![Group {
                id: "g1".to_owned(),
                name: Some("menu".to_owned()),
                notes: vec![],
                groups: vec![Group {
                    id: "g2".to_owned(),
                    name: None,
                    notes: vec![],
                    groups: vec![],
                    units: vec![Unit {
                        id: "u1".to_owned(),
                        name: None,
                        notes: vec![],
                        sub_units: vec![SubUnit::Segment(Segment {
                            id: None,
                            state: None,
                            sub_state: None,
                            source: Content {
                                lang: None,
                                elements: vec![InlineElement::Text("File".to_owned())],
                            },
                            target: None,
                        })],
                        original_data: None,
                        extensions: Extensions::default(),
                    }],
                    extensions: Extensions::default(),
                }],
                units: vec![],
                extensions: Extensions::default(),
            }],
            units: vec![],
            extensions: Extensions::default(),
        }],
        extensions: Extensions::default(),
    };

    let xml = write::to_string(&doc).unwrap();
    let parsed = read::read_xliff(&xml).unwrap();
    assert_eq!(doc, parsed);
}

#[test]
fn round_trip_spanning_codes() {
    let doc = Document {
        version: "2.0".to_owned(),
        src_lang: "en".to_owned(),
        trg_lang: None,
        files: vec![File {
            id: "f1".to_owned(),
            original: None,
            notes: vec![],
            skeleton: None,
            groups: vec![],
            units: vec![Unit {
                id: "u1".to_owned(),
                name: None,
                notes: vec![],
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: None,
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![
                            InlineElement::Sc(Sc {
                                id: "sc1".to_owned(),
                                data_ref: None,
                                sub_type: None,
                                can_copy: Some(true),
                                can_delete: Some(false),
                                can_overlap: None,
                                can_reorder: Some(CanReorder::No),
                                extensions: Extensions::default(),
                            }),
                            InlineElement::Text("bold".to_owned()),
                            InlineElement::Ec(Ec {
                                start_ref: Some("sc1".to_owned()),
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
                    },
                    target: None,
                })],
                original_data: None,
                extensions: Extensions::default(),
            }],
            extensions: Extensions::default(),
        }],
        extensions: Extensions::default(),
    };

    let xml = write::to_string(&doc).unwrap();
    let parsed = read::read_xliff(&xml).unwrap();
    assert_eq!(doc, parsed);
}

#[test]
fn round_trip_annotations() {
    let doc = Document {
        version: "2.0".to_owned(),
        src_lang: "en".to_owned(),
        trg_lang: None,
        files: vec![File {
            id: "f1".to_owned(),
            original: None,
            notes: vec![],
            skeleton: None,
            groups: vec![],
            units: vec![Unit {
                id: "u1".to_owned(),
                name: None,
                notes: vec![],
                sub_units: vec![SubUnit::Segment(Segment {
                    id: None,
                    state: None,
                    sub_state: None,
                    source: Content {
                        lang: None,
                        elements: vec![
                            InlineElement::Mrk(Mrk {
                                id: "m1".to_owned(),
                                translate: Some(false),
                                mrk_type: Some("term".to_owned()),
                                ref_: None,
                                value: None,
                                content: vec![InlineElement::Text("XLIFF".to_owned())],
                                extensions: Extensions::default(),
                            }),
                            InlineElement::Text(" is great. ".to_owned()),
                            InlineElement::Sm(Sm {
                                id: "sm1".to_owned(),
                                translate: None,
                                sm_type: Some("comment".to_owned()),
                                ref_: None,
                                value: Some("reviewer note".to_owned()),
                                extensions: Extensions::default(),
                            }),
                            InlineElement::Text("check this".to_owned()),
                            InlineElement::Em(Em {
                                start_ref: "sm1".to_owned(),
                            }),
                        ],
                    },
                    target: None,
                })],
                original_data: None,
                extensions: Extensions::default(),
            }],
            extensions: Extensions::default(),
        }],
        extensions: Extensions::default(),
    };

    let xml = write::to_string(&doc).unwrap();
    let parsed = read::read_xliff(&xml).unwrap();
    assert_eq!(doc, parsed);
}

#[test]
fn round_trip_ignorable() {
    let doc = Document {
        version: "2.0".to_owned(),
        src_lang: "en".to_owned(),
        trg_lang: None,
        files: vec![File {
            id: "f1".to_owned(),
            original: None,
            notes: vec![],
            skeleton: None,
            groups: vec![],
            units: vec![Unit {
                id: "u1".to_owned(),
                name: None,
                notes: vec![],
                sub_units: vec![
                    SubUnit::Segment(Segment {
                        id: None,
                        state: None,
                        sub_state: None,
                        source: Content {
                            lang: None,
                            elements: vec![InlineElement::Text("Hello".to_owned())],
                        },
                        target: None,
                    }),
                    SubUnit::Ignorable(Ignorable {
                        id: Some("i1".to_owned()),
                        source: Content {
                            lang: None,
                            elements: vec![InlineElement::Text(" ".to_owned())],
                        },
                        target: None,
                    }),
                    SubUnit::Segment(Segment {
                        id: None,
                        state: None,
                        sub_state: None,
                        source: Content {
                            lang: None,
                            elements: vec![InlineElement::Text("World".to_owned())],
                        },
                        target: None,
                    }),
                ],
                original_data: None,
                extensions: Extensions::default(),
            }],
            extensions: Extensions::default(),
        }],
        extensions: Extensions::default(),
    };

    let xml = write::to_string(&doc).unwrap();
    let parsed = read::read_xliff(&xml).unwrap();
    assert_eq!(doc, parsed);
}

#[test]
fn validate_minimal_valid() {
    let doc = minimal_doc();
    let errors = validate::validate(&doc);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn validate_bad_version() {
    let mut doc = minimal_doc();
    doc.version = "1.2".to_owned();
    let errors = validate::validate(&doc);
    assert!(errors.iter().any(|e| e.message.contains("version")));
}

#[test]
fn validate_empty_src_lang() {
    let mut doc = minimal_doc();
    doc.src_lang = String::new();
    let errors = validate::validate(&doc);
    assert!(errors.iter().any(|e| e.message.contains("srcLang")));
}

#[test]
fn validate_no_files() {
    let mut doc = minimal_doc();
    doc.files.clear();
    let errors = validate::validate(&doc);
    assert!(errors.iter().any(|e| e.message.contains("<file>")));
}

#[test]
fn validate_duplicate_unit_ids() {
    let mut doc = minimal_doc();
    let unit2 = doc.files[0].units[0].clone();
    doc.files[0].units.push(unit2);
    let errors = validate::validate(&doc);
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("duplicate unit id"))
    );
}

#[test]
fn validate_ec_without_sc() {
    let mut doc = minimal_doc();
    doc.files[0].units[0].sub_units = vec![SubUnit::Segment(Segment {
        id: None,
        state: None,
        sub_state: None,
        source: Content {
            lang: None,
            elements: vec![
                InlineElement::Text("text".to_owned()),
                InlineElement::Ec(Ec {
                    start_ref: Some("missing".to_owned()),
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
        },
        target: None,
    })];
    let errors = validate::validate(&doc);
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("no matching <sc>"))
    );
}

#[test]
fn validate_data_ref_missing() {
    let mut doc = minimal_doc();
    doc.files[0].units[0].original_data = Some(OriginalData {
        entries: vec![DataEntry {
            id: "d1".to_owned(),
            content: "x".to_owned(),
        }],
    });
    doc.files[0].units[0].sub_units = vec![SubUnit::Segment(Segment {
        id: None,
        state: None,
        sub_state: None,
        source: Content {
            lang: None,
            elements: vec![InlineElement::Ph(Ph {
                id: "1".to_owned(),
                data_ref: Some("d999".to_owned()),
                equiv: None,
                disp: None,
                sub_type: None,
                extensions: Extensions::default(),
            })],
        },
        target: None,
    })];
    let errors = validate::validate(&doc);
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("not found in <originalData>"))
    );
}

#[test]
fn validate_note_priority_out_of_range() {
    let mut doc = minimal_doc();
    doc.files[0].notes.push(Note {
        id: None,
        category: None,
        priority: Some(11),
        applies_to: None,
        content: "bad priority".to_owned(),
    });
    let errors = validate::validate(&doc);
    assert!(errors.iter().any(|e| e.message.contains("priority")));
}
