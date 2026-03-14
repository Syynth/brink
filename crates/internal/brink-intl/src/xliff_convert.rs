//! Bidirectional conversion between `LinesJson` and XLIFF 2.0 `Document`.

use std::collections::HashMap;

use xliff2::{
    Content, DataEntry, Document, ExtensionAttribute, Extensions, File, InlineElement,
    OriginalData, Ph, Segment, State, SubUnit, Unit,
};

use crate::error::IntlError;
use crate::json_model::{ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson};

/// Brink XLIFF extension namespace URI.
pub const BRINK_NS: &str = "urn:brink:xliff:extensions:1.0";

/// Prefix used for brink extension attributes in XLIFF.
const BRINK_PREFIX: &str = "brink";

/// Convert a `LinesJson` to an XLIFF 2.0 `Document`.
///
/// `source_lang` is a BCP 47 language tag for the source content (e.g. `"en"`).
/// `trg_lang` is an optional BCP 47 target language tag (e.g. `"es"`).
pub fn lines_json_to_xliff(
    lines: &LinesJson,
    source_lang: &str,
    trg_lang: Option<&str>,
) -> Document {
    let files: Vec<File> = lines
        .scopes
        .iter()
        .map(|scope| {
            let display_name = scope.name.as_deref().unwrap_or(&scope.id);
            let units: Vec<Unit> = scope
                .lines
                .iter()
                .map(|line| line_to_unit(display_name, line))
                .collect();
            File {
                id: display_name.to_string(),
                original: None,
                notes: Vec::new(),
                skeleton: None,
                groups: Vec::new(),
                units,
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![ExtensionAttribute {
                        namespace: BRINK_PREFIX.to_string(),
                        local_name: "scope-id".to_string(),
                        value: scope.id.clone(),
                    }],
                },
            }
        })
        .collect();

    Document {
        version: "2.0".to_string(),
        src_lang: source_lang.to_string(),
        trg_lang: trg_lang.map(str::to_string),
        files,
        extensions: Extensions {
            elements: Vec::new(),
            attributes: vec![
                ExtensionAttribute {
                    namespace: "xmlns".to_string(),
                    local_name: BRINK_PREFIX.to_string(),
                    value: BRINK_NS.to_string(),
                },
                ExtensionAttribute {
                    namespace: BRINK_PREFIX.to_string(),
                    local_name: "checksum".to_string(),
                    value: lines.source_checksum.clone(),
                },
                ExtensionAttribute {
                    namespace: BRINK_PREFIX.to_string(),
                    local_name: "version".to_string(),
                    value: lines.version.to_string(),
                },
            ],
        },
    }
}

/// Convert an XLIFF 2.0 `Document` back to `LinesJson`.
///
/// Extracts **target** content when present; lines with no target yield
/// `content: None` (untranslated).
pub fn xliff_to_lines_json(doc: &Document) -> Result<LinesJson, IntlError> {
    let source_checksum = ext_attr_value(&doc.extensions, "checksum")
        .unwrap_or_default()
        .to_string();
    let version: u32 = ext_attr_value(&doc.extensions, "version")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    let mut scopes = Vec::with_capacity(doc.files.len());
    for file in &doc.files {
        let mut lines = Vec::with_capacity(file.units.len());
        for unit in &file.units {
            lines.push(unit_to_line(unit)?);
        }
        // Prefer brink:scope-id extension for the hex definition ID;
        // fall back to file.id for backwards compatibility.
        let scope_id = ext_attr_value(&file.extensions, "scope-id")
            .map_or_else(|| file.id.clone(), str::to_string);
        scopes.push(ScopeJson {
            name: Some(file.id.clone()),
            id: scope_id,
            lines,
        });
    }

    Ok(LinesJson {
        version,
        source_checksum,
        scopes,
    })
}

// ── LinesJson → XLIFF helpers ──────────────────────────────────────────

fn is_whitespace_only(line: &LineJson) -> bool {
    match &line.content {
        None => true,
        Some(ContentJson::Plain(s)) => s.trim().is_empty(),
        Some(ContentJson::Template { .. }) => false,
    }
}

fn line_to_unit(scope_name: &str, line: &LineJson) -> Unit {
    let unit_id = format!("{scope_name}:{}", line.index);
    let translate = if is_whitespace_only(line) {
        Some(false)
    } else {
        None
    };

    let mut ext_attrs = vec![ExtensionAttribute {
        namespace: BRINK_PREFIX.to_string(),
        local_name: "hash".to_string(),
        value: line.hash.clone(),
    }];
    if let Some(ref audio) = line.audio {
        ext_attrs.push(ExtensionAttribute {
            namespace: BRINK_PREFIX.to_string(),
            local_name: "audio".to_string(),
            value: audio.clone(),
        });
    }

    let (source_elements, original_data) = match &line.content {
        Some(content) => content_to_inline(content),
        None => (Vec::new(), None),
    };

    let segment = Segment {
        id: None,
        state: Some(State::Initial),
        sub_state: None,
        source: Content {
            lang: None,
            elements: source_elements,
        },
        target: None,
    };

    Unit {
        id: unit_id,
        name: None,
        translate,
        notes: Vec::new(),
        sub_units: vec![SubUnit::Segment(segment)],
        original_data,
        extensions: Extensions {
            elements: Vec::new(),
            attributes: ext_attrs,
        },
    }
}

pub(crate) fn content_to_inline(
    content: &ContentJson,
) -> (Vec<InlineElement>, Option<OriginalData>) {
    match content {
        ContentJson::Plain(s) => (vec![InlineElement::Text(s.clone())], None),
        ContentJson::Template { template } => {
            let mut elements = Vec::new();
            let mut data_entries = Vec::new();
            let mut select_counter: usize = 0;

            for part in template {
                match part {
                    PartJson::Literal(s) => {
                        elements.push(InlineElement::Text(s.clone()));
                    }
                    PartJson::Slot { slot } => {
                        elements.push(InlineElement::Ph(Ph {
                            id: format!("s{slot}"),
                            data_ref: None,
                            equiv: Some(format!("{{slot {slot}}}")),
                            disp: None,
                            sub_type: None,
                            extensions: Extensions::default(),
                        }));
                    }
                    PartJson::Select { select } => {
                        let data_id = format!("dsel{select_counter}");
                        let ph_id = format!("sel{select_counter}");
                        select_counter += 1;

                        // Serialize the SelectJson to JSON for originalData.
                        // This is safe — SelectJson is always serializable.
                        let json = serde_json::to_string(select).unwrap_or_default();

                        data_entries.push(DataEntry {
                            id: data_id.clone(),
                            content: json,
                        });

                        elements.push(InlineElement::Ph(Ph {
                            id: ph_id,
                            data_ref: Some(data_id),
                            equiv: None,
                            disp: None,
                            sub_type: None,
                            extensions: Extensions::default(),
                        }));
                    }
                }
            }

            let original_data = if data_entries.is_empty() {
                None
            } else {
                Some(OriginalData {
                    entries: data_entries,
                })
            };

            (elements, original_data)
        }
    }
}

// ── XLIFF → LinesJson helpers ──────────────────────────────────────────

fn unit_to_line(unit: &Unit) -> Result<LineJson, IntlError> {
    // Parse unit id: "scope_id:line_index"
    let index = parse_unit_index(&unit.id)?;

    let hash = ext_attr_value(&unit.extensions, "hash")
        .ok_or_else(|| IntlError::MissingHash(unit.id.clone()))?
        .to_string();

    let audio = ext_attr_value(&unit.extensions, "audio").map(str::to_string);

    // Build originalData lookup for select reconstruction.
    let data_map: HashMap<&str, &str> = unit
        .original_data
        .as_ref()
        .map(|od| {
            od.entries
                .iter()
                .map(|e| (e.id.as_str(), e.content.as_str()))
                .collect()
        })
        .unwrap_or_default();

    // Extract target content if present, otherwise None (untranslated).
    let content = extract_target_content(unit, &data_map)?;

    Ok(LineJson {
        index,
        content,
        hash,
        audio,
    })
}

fn parse_unit_index(unit_id: &str) -> Result<u16, IntlError> {
    let colon_pos = unit_id
        .rfind(':')
        .ok_or_else(|| IntlError::InvalidUnitId(unit_id.to_string()))?;
    let idx_str = &unit_id[colon_pos + 1..];
    idx_str
        .parse::<u16>()
        .map_err(|_| IntlError::InvalidUnitId(unit_id.to_string()))
}

fn extract_target_content(
    unit: &Unit,
    data_map: &HashMap<&str, &str>,
) -> Result<Option<ContentJson>, IntlError> {
    // Find the first segment.
    let segment = unit.sub_units.iter().find_map(|su| match su {
        SubUnit::Segment(seg) => Some(seg),
        SubUnit::Ignorable(_) => None,
    });

    let Some(segment) = segment else {
        return Ok(None);
    };

    // Prefer target content; fall back to source (for untranslated /
    // translate="no" units that still need content for compilation).
    let content = segment
        .target
        .as_ref()
        .filter(|t| !t.elements.is_empty())
        .unwrap_or(&segment.source);

    if content.elements.is_empty() {
        return Ok(None);
    }

    inline_to_content(&content.elements, data_map).map(Some)
}

fn inline_to_content(
    elements: &[InlineElement],
    data_map: &HashMap<&str, &str>,
) -> Result<ContentJson, IntlError> {
    // Check if this is a simple plain text (single Text element).
    if elements.len() == 1
        && let InlineElement::Text(s) = &elements[0]
    {
        return Ok(ContentJson::Plain(s.clone()));
    }

    // Template reconstruction.
    let mut parts = Vec::new();
    for elem in elements {
        match elem {
            InlineElement::Text(s) => {
                parts.push(PartJson::Literal(s.clone()));
            }
            InlineElement::Ph(ph) => {
                if let Some(ref data_ref) = ph.data_ref {
                    // Select: look up in originalData.
                    let json_str = data_map
                        .get(data_ref.as_str())
                        .ok_or_else(|| IntlError::MissingSelectData(data_ref.clone()))?;
                    let select: SelectJson = serde_json::from_str(json_str)
                        .map_err(|e| IntlError::InvalidSelectJson(e.to_string()))?;
                    parts.push(PartJson::Select { select });
                } else if ph.id.starts_with('s') {
                    // Slot: parse slot number from id "s{n}".
                    let slot_str = &ph.id[1..];
                    let slot: u8 = slot_str.parse().map_err(|_| {
                        IntlError::InvalidUnitId(format!("bad slot ph id: {}", ph.id))
                    })?;
                    parts.push(PartJson::Slot { slot });
                }
            }
            // Other inline elements are not produced by brink, ignore.
            _ => {}
        }
    }

    Ok(ContentJson::Template { template: parts })
}

fn ext_attr_value<'a>(ext: &'a Extensions, local_name: &str) -> Option<&'a str> {
    ext.attributes
        .iter()
        .find(|a| a.namespace == BRINK_PREFIX && a.local_name == local_name)
        .map(|a| a.value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lines_json(scopes: Vec<ScopeJson>) -> LinesJson {
        LinesJson {
            version: 1,
            source_checksum: "0xdeadbeef".to_string(),
            scopes,
        }
    }

    fn make_scope(id: &str, name: Option<&str>, lines: Vec<LineJson>) -> ScopeJson {
        ScopeJson {
            name: name.map(str::to_string),
            id: id.to_string(),
            lines,
        }
    }

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
        }
    }

    fn roundtrip(lines: &LinesJson) -> LinesJson {
        let doc = lines_json_to_xliff(lines, "en", None);
        // Set targets to source content (simulating translation).
        let doc = set_targets_from_source(doc);
        xliff_to_lines_json(&doc).unwrap()
    }

    /// Copy source content to target in all segments (simulates a "copy source" workflow).
    fn set_targets_from_source(mut doc: Document) -> Document {
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

    #[test]
    fn plain_content_roundtrip() {
        let lines = make_lines_json(vec![make_scope(
            "0x0100000000000001",
            Some("root"),
            vec![make_line(
                0,
                "abcd1234",
                Some(ContentJson::Plain("Hello world".to_string())),
                None,
            )],
        )]);
        let result = roundtrip(&lines);
        assert_eq!(
            result.scopes[0].lines[0].content,
            lines.scopes[0].lines[0].content
        );
        assert_eq!(result.source_checksum, lines.source_checksum);
        assert_eq!(result.version, lines.version);
    }

    #[test]
    fn template_with_slots_roundtrip() {
        let content = ContentJson::Template {
            template: vec![
                PartJson::Literal("Hello, ".to_string()),
                PartJson::Slot { slot: 0 },
                PartJson::Literal("!".to_string()),
            ],
        };
        let lines = make_lines_json(vec![make_scope(
            "0x01",
            None,
            vec![make_line(0, "hash1", Some(content.clone()), None)],
        )]);
        let result = roundtrip(&lines);
        assert_eq!(result.scopes[0].lines[0].content, Some(content));
    }

    #[test]
    fn template_with_selects_roundtrip() {
        let mut v1 = serde_json::Map::new();
        v1.insert(
            "cardinal:One".to_string(),
            serde_json::Value::String("item".to_string()),
        );
        let mut v2 = serde_json::Map::new();
        v2.insert(
            "cardinal:Other".to_string(),
            serde_json::Value::String("items".to_string()),
        );

        let content = ContentJson::Template {
            template: vec![PartJson::Select {
                select: SelectJson {
                    slot: 0,
                    variants: vec![v1, v2],
                    default: "items".to_string(),
                },
            }],
        };
        let lines = make_lines_json(vec![make_scope(
            "0x01",
            None,
            vec![make_line(0, "hash1", Some(content.clone()), None)],
        )]);
        let result = roundtrip(&lines);
        assert_eq!(result.scopes[0].lines[0].content, Some(content));
    }

    #[test]
    fn audio_and_hash_preserved() {
        let lines = make_lines_json(vec![make_scope(
            "0x01",
            None,
            vec![make_line(
                0,
                "626e7681b4e2e7bc",
                Some(ContentJson::Plain("hi".to_string())),
                Some("audio/hi.wav"),
            )],
        )]);
        let result = roundtrip(&lines);
        assert_eq!(result.scopes[0].lines[0].hash, "626e7681b4e2e7bc");
        assert_eq!(
            result.scopes[0].lines[0].audio,
            Some("audio/hi.wav".to_string())
        );
    }

    #[test]
    fn untranslated_lines_no_target() {
        let lines = make_lines_json(vec![make_scope(
            "0x01",
            None,
            vec![make_line(
                0,
                "hash1",
                Some(ContentJson::Plain("hello".to_string())),
                None,
            )],
        )]);
        let doc = lines_json_to_xliff(&lines, "en", None);
        // Don't set targets — untranslated lines fall back to source content.
        let result = xliff_to_lines_json(&doc).unwrap();
        assert_eq!(
            result.scopes[0].lines[0].content,
            Some(ContentJson::Plain("hello".to_string()))
        );
    }

    #[test]
    fn multiple_scopes() {
        let lines = make_lines_json(vec![
            make_scope(
                "0x01",
                Some("root"),
                vec![make_line(
                    0,
                    "aaa",
                    Some(ContentJson::Plain("Hello".to_string())),
                    None,
                )],
            ),
            make_scope(
                "0x02",
                Some("knot"),
                vec![
                    make_line(
                        0,
                        "bbb",
                        Some(ContentJson::Plain("World".to_string())),
                        None,
                    ),
                    make_line(1, "ccc", Some(ContentJson::Plain("!".to_string())), None),
                ],
            ),
        ]);
        let result = roundtrip(&lines);
        assert_eq!(result.scopes.len(), 2);
        assert_eq!(result.scopes[0].id, "0x01");
        assert_eq!(result.scopes[0].name, Some("root".to_string()));
        assert_eq!(result.scopes[1].lines.len(), 2);
    }

    #[test]
    fn content_none_line_roundtrips() {
        let lines = make_lines_json(vec![make_scope(
            "0x01",
            None,
            vec![make_line(0, "hash1", None, None)],
        )]);
        let doc = lines_json_to_xliff(&lines, "en", None);
        // Source should be empty for None content.
        let SubUnit::Segment(seg) = &doc.files[0].units[0].sub_units[0] else {
            unreachable!()
        };
        assert!(seg.source.elements.is_empty());
    }
}
