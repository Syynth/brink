#![allow(clippy::unwrap_used)]

use brink_format::{
    DecodeError, DefinitionId, DefinitionTag, LineContent, LinePart, LocaleData, LocaleLineEntry,
    LocaleScopeTable, PluralCategory, SelectKey, read_inkl, write_inkl,
};

fn scope_id(hash: u64) -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, hash)
}

#[test]
fn roundtrip_empty() {
    let data = LocaleData {
        locale_tag: "en".to_string(),
        base_checksum: 0x1234_5678,
        line_tables: vec![],
    };
    let mut buf = Vec::new();
    write_inkl(&data, &mut buf);
    let recovered = read_inkl(&buf).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn roundtrip_single_scope_plain() {
    let data = LocaleData {
        locale_tag: "es".to_string(),
        base_checksum: 0xAABB_CCDD,
        line_tables: vec![LocaleScopeTable {
            scope_id: scope_id(1),
            lines: vec![LocaleLineEntry {
                content: LineContent::Plain("Hola mundo".to_string()),
                audio_ref: None,
            }],
        }],
    };
    let mut buf = Vec::new();
    write_inkl(&data, &mut buf);
    let recovered = read_inkl(&buf).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn roundtrip_single_scope_with_audio() {
    let data = LocaleData {
        locale_tag: "ja".to_string(),
        base_checksum: 0x0000_0001,
        line_tables: vec![LocaleScopeTable {
            scope_id: scope_id(42),
            lines: vec![LocaleLineEntry {
                content: LineContent::Plain("こんにちは".to_string()),
                audio_ref: Some("audio/hello.wav".to_string()),
            }],
        }],
    };
    let mut buf = Vec::new();
    write_inkl(&data, &mut buf);
    let recovered = read_inkl(&buf).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn roundtrip_multiple_scopes() {
    let data = LocaleData {
        locale_tag: "fr".to_string(),
        base_checksum: 0xDEAD_BEEF,
        line_tables: vec![
            // Empty scope (0 lines)
            LocaleScopeTable {
                scope_id: scope_id(100),
                lines: vec![],
            },
            // 1 line
            LocaleScopeTable {
                scope_id: scope_id(200),
                lines: vec![LocaleLineEntry {
                    content: LineContent::Plain("Bonjour".to_string()),
                    audio_ref: None,
                }],
            },
            // 3 lines
            LocaleScopeTable {
                scope_id: scope_id(300),
                lines: vec![
                    LocaleLineEntry {
                        content: LineContent::Plain("Un".to_string()),
                        audio_ref: None,
                    },
                    LocaleLineEntry {
                        content: LineContent::Plain("Deux".to_string()),
                        audio_ref: Some("audio/deux.ogg".to_string()),
                    },
                    LocaleLineEntry {
                        content: LineContent::Plain("Trois".to_string()),
                        audio_ref: None,
                    },
                ],
            },
        ],
    };
    let mut buf = Vec::new();
    write_inkl(&data, &mut buf);
    let recovered = read_inkl(&buf).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn roundtrip_template_content() {
    let data = LocaleData {
        locale_tag: "de".to_string(),
        base_checksum: 0,
        line_tables: vec![LocaleScopeTable {
            scope_id: scope_id(1),
            lines: vec![LocaleLineEntry {
                content: LineContent::Template(vec![
                    LinePart::Literal("Hello, ".to_string()),
                    LinePart::Slot(0),
                    LinePart::Literal("!".to_string()),
                    LinePart::Select {
                        slot: 1,
                        variants: vec![
                            (SelectKey::Cardinal(PluralCategory::One), "item".to_string()),
                            (
                                SelectKey::Cardinal(PluralCategory::Other),
                                "items".to_string(),
                            ),
                        ],
                        default: "items".to_string(),
                    },
                ]),
                audio_ref: None,
            }],
        }],
    };
    let mut buf = Vec::new();
    write_inkl(&data, &mut buf);
    let recovered = read_inkl(&buf).unwrap();
    assert_eq!(data, recovered);
}

#[test]
fn bad_magic() {
    let mut buf = vec![0u8; 32];
    buf[0..4].copy_from_slice(b"XYZW");
    let err = read_inkl(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::BadInklMagic([b'X', b'Y', b'Z', b'W'])),
        "expected BadInklMagic, got {err:?}"
    );
}

#[test]
fn unsupported_version() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"INKL");
    buf.push(99); // wrong version
    // Pad enough bytes for the rest of the header
    buf.extend_from_slice(&[0u8; 20]);
    let err = read_inkl(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::UnsupportedInklVersion(99)),
        "expected UnsupportedInklVersion(99), got {err:?}"
    );
}

#[test]
fn truncated_header() {
    // Only 3 bytes — not enough for magic
    let buf = vec![b'I', b'N', b'K'];
    let err = read_inkl(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::UnexpectedEof),
        "expected UnexpectedEof, got {err:?}"
    );
}

#[test]
fn truncated_payload() {
    // Valid header, but truncate the line tables payload
    let data = LocaleData {
        locale_tag: "en".to_string(),
        base_checksum: 0,
        line_tables: vec![LocaleScopeTable {
            scope_id: scope_id(1),
            lines: vec![LocaleLineEntry {
                content: LineContent::Plain("hello".to_string()),
                audio_ref: None,
            }],
        }],
    };
    let mut buf = Vec::new();
    write_inkl(&data, &mut buf);
    // Truncate — remove last 4 bytes from the payload
    buf.truncate(buf.len() - 4);
    let err = read_inkl(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::UnexpectedEof),
        "expected UnexpectedEof, got {err:?}"
    );
}
