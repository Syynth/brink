//! Bidirectional conversion between `LinesJson` and XLIFF 2.0 `Document`.

use std::collections::HashMap;

use xliff2::{
    Content, DataEntry, Document, ExtensionAttribute, Extensions, File, InlineElement,
    OriginalData, Pc, Ph, Segment, State, SubUnit, Unit,
};

use crate::error::IntlError;
use crate::json_model::{
    AttrJson, ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson, SpanJson,
};

/// `subType` marking a childless [`PartJson::Span`] (the point-marker shape,
/// §8b.11 — `<pause/>`, `<sfx name="bell"/>`) mapped to a standalone `<ph>`
/// inline code. XLIFF 2.0 core has no literal `<x/>` element — that's an
/// XLIFF 1.2-ism; `<ph>` is the 2.0 standalone-code element, the same one
/// [`PartJson::Slot`] already maps to. The `subType` token is what lets
/// decode tell a span-marker `<ph>` apart from a slot/select `<ph>`.
const SPAN_MARKER_SUBTYPE: &str = "brink:x";

/// `subType` marking a non-empty [`PartJson::Span`] mapped to a paired
/// `<pc>` inline code.
const SPAN_PAIRED_SUBTYPE: &str = "brink:pc";

/// Wire-only companion to [`SpanJson`] carried in XLIFF `originalData`:
/// `name`/`attrs` only. `children` is never part of this payload — for a
/// paired `<pc>` the children live structurally as the `<pc>`'s own
/// `content`; for a childless point-marker `<ph>` there are none.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SpanMetaJson {
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attrs: Vec<AttrJson>,
}

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
                .map(|line| line_to_unit(&scope.id, scope.name.as_deref(), line))
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

/// Migrate an XLIFF document's unit ids from the legacy display-name-based
/// scheme (`{scope_name}:{line_index}`) to the stable scope-id-based scheme
/// (`{scope_id}:{line_index}`) introduced by #1442.
///
/// This is a pure `id`-attribute rewrite: `<source>`/`<target>` content,
/// segment `state`, and every `brink:*` extension attribute (`hash`,
/// `audio`, `scope-id`) are left untouched, so existing translations bind to
/// the migrated units exactly as they did before. Units already on the new
/// scheme are left as-is, so this is idempotent and safe to run
/// unconditionally on any `.xlf` file — including ones exported by a version
/// of brink that never had the bug.
///
/// Returns the number of unit ids that were actually rewritten.
///
/// # Errors
///
/// Returns [`IntlError::InvalidUnitId`] if a unit id cannot be parsed for
/// its trailing line index (see [`parse_unit_index`]).
pub fn migrate_unit_ids(doc: &mut Document) -> Result<usize, IntlError> {
    let mut changed = 0;
    for file in &mut doc.files {
        // Same fallback as `xliff_to_lines_json`: prefer the durable
        // `brink:scope-id` extension, fall back to `file.id` for documents
        // that predate even that.
        let scope_id = ext_attr_value(&file.extensions, "scope-id")
            .map_or_else(|| file.id.clone(), str::to_string);
        for unit in &mut file.units {
            let index = parse_unit_index(&unit.id)?;
            let new_id = format!("{scope_id}:{index}");
            if unit.id != new_id {
                unit.id = new_id;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

// ── LinesJson → XLIFF helpers ──────────────────────────────────────────

fn is_whitespace_only(line: &LineJson) -> bool {
    match &line.content {
        None => true,
        Some(ContentJson::Plain(s)) => s.trim().is_empty(),
        Some(ContentJson::Template { .. }) => false,
    }
}

fn line_to_unit(scope_id: &str, scope_name: Option<&str>, line: &LineJson) -> Unit {
    // The unit id is keyed on the scope's `DefinitionId` (`scope_id`, e.g.
    // "0x0100000000000001"), never on its display name. This does NOT make
    // unit ids rename-stable: a `DefinitionId` is itself a hash of the
    // scope's (qualified) name/path (see `manifest.rs::hash_name`,
    // `hir/stamp.rs::alloc_address`), so renaming a knot or stitch still
    // produces a new `DefinitionId` and every unit id beneath it changes.
    // Name-derived identity is the ruled model (R1, 2026-07-27,
    // `docs/modules-spec.md` §5), so that churn is by design. What #1442
    // fixed is the *consequence*: `compile_locale` and `regenerate_lines`
    // now follow the compiled `#@was` alias table (`crate::scope_alias`), so
    // a declared rename rebinds its translations instead of orphaning them.
    //
    // What this DOES fix relative to the display-name scheme it replaces:
    // the id is now a canonical, NMTOKEN-safe hex string (display names can
    // contain characters that aren't valid in an XML `id`), it matches the
    // format `brink:scope-id` and `IntlError::InvalidUnitId` already
    // documented (`scope_id:line_index`), and it's decoupled from
    // `scope.name`, a mutable, non-unique-across-scopes display field —
    // collisions between two same-named scopes are no longer possible. The
    // human-readable name, when present, rides the `name` attribute instead,
    // for translator context.
    let unit_id = format!("{scope_id}:{}", line.index);
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
        Some(content) => content_to_inline(content, &line.slots),
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

    // `name` carries the legacy readable id verbatim (`{scope_name}:{index}`,
    // what `id` used to be pre-#1442), not the bare scope name — a bare name
    // is identical across every unit in a file and adds nothing over the
    // containing `<file id>`. This is what genuinely preserves the
    // 2026-03-14 decision's readability rationale.
    let name = scope_name.map(|n| format!("{n}:{}", line.index));

    Unit {
        id: unit_id,
        name,
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
    slots: &[crate::json_model::SlotJson],
) -> (Vec<InlineElement>, Option<OriginalData>) {
    match content {
        ContentJson::Plain(s) => (vec![InlineElement::Text(s.clone())], None),
        ContentJson::Template { template } => {
            let mut elements = Vec::new();
            let mut data_entries = Vec::new();
            let mut select_counter: usize = 0;
            let mut span_counter: usize = 0;

            for part in template {
                push_part_inline(
                    part,
                    slots,
                    &mut elements,
                    &mut data_entries,
                    &mut select_counter,
                    &mut span_counter,
                );
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

/// One [`PartJson`] → zero-or-more [`InlineElement`]s, appended to
/// `elements`.
///
/// [`PartJson::Span`] (#1716, real inline-code mapping #1734) maps to a
/// genuine XLIFF inline code, keyed on whether it has children:
///
/// - Non-empty `children` (the ordinary paired shape, e.g. `<b>bold</b>`)
///   → a paired `<pc>` inline code, recursively containing the mapped
///   children.
/// - Empty `children` (the point-marker shape, §8b.11 — `<pause/>`,
///   `<sfx name="bell"/>`) → a standalone `<ph>` inline code (XLIFF 2.0
///   core has no literal `<x/>`; `<ph>` is its standalone-code element).
///
/// Either way `name`/`attrs` ride along in `originalData` (mirroring how
/// [`PartJson::Select`] already stashes its structured payload there),
/// referenced by `dataRefStart`/`dataRef` — so a translated XLIFF file
/// round-trips the exact span structure back, not just its flattened text.
///
/// This does **not** touch `line.hash` — the wire-level "span
/// hash-transparency" ruling (`docs/prose-dialect-spec.md` §4.4) that keeps
/// `Hello <wave>world</wave>` keyed identically to `Hello world` is
/// computed upstream, at compile time, before `LineJson.hash` ever reaches
/// this module; this function only ever reads `hash` as an opaque
/// passthrough field on [`LineJson`], never derives from span content.
fn push_part_inline(
    part: &PartJson,
    slots: &[crate::json_model::SlotJson],
    elements: &mut Vec<InlineElement>,
    data_entries: &mut Vec<DataEntry>,
    select_counter: &mut usize,
    span_counter: &mut usize,
) {
    match part {
        PartJson::Literal(s) => {
            elements.push(InlineElement::Text(s.clone()));
        }
        PartJson::Slot { slot } => {
            let disp = slots
                .iter()
                .find(|s| s.index == *slot)
                .map(|s| s.name.clone());
            elements.push(InlineElement::Ph(Ph {
                id: format!("s{slot}"),
                data_ref: None,
                equiv: Some(format!("{{slot {slot}}}")),
                disp,
                sub_type: None,
                extensions: Extensions::default(),
            }));
        }
        PartJson::Select { select } => {
            let data_id = format!("dsel{select_counter}");
            let ph_id = format!("sel{select_counter}");
            *select_counter += 1;

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
        PartJson::Span { span } => {
            let n = *span_counter;
            *span_counter += 1;

            let meta = SpanMetaJson {
                name: span.name.clone(),
                attrs: span.attrs.clone(),
            };
            // Safe — `SpanMetaJson` is always serializable.
            let json = serde_json::to_string(&meta).unwrap_or_default();
            let data_id = format!("dspan{n}");
            data_entries.push(DataEntry {
                id: data_id.clone(),
                content: json,
            });

            if span.children.is_empty() {
                // Point marker (§8b.11): no text to carry, so a standalone
                // code — mapped to `<ph>` (see this function's doc comment).
                elements.push(InlineElement::Ph(Ph {
                    id: format!("x{n}"),
                    data_ref: Some(data_id),
                    equiv: None,
                    disp: None,
                    sub_type: Some(SPAN_MARKER_SUBTYPE.to_string()),
                    extensions: Extensions::default(),
                }));
            } else {
                let mut content = Vec::new();
                for child in &span.children {
                    push_part_inline(
                        child,
                        slots,
                        &mut content,
                        data_entries,
                        select_counter,
                        span_counter,
                    );
                }
                elements.push(InlineElement::Pc(Pc {
                    id: format!("pc{n}"),
                    data_ref_start: Some(data_id),
                    data_ref_end: None,
                    sub_type: Some(SPAN_PAIRED_SUBTYPE.to_string()),
                    content,
                    extensions: Extensions::default(),
                }));
            }
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
        slots: Vec::new(),
        source: None,
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
    // Check if this is a simple plain text (single Text or CData element).
    if elements.len() == 1
        && let InlineElement::Text(s) | InlineElement::CData(s) = &elements[0]
    {
        return Ok(ContentJson::Plain(s.clone()));
    }

    let parts = elements_to_parts(elements, data_map)?;
    Ok(ContentJson::Template { template: parts })
}

/// True if a `subType` or `dataRef` attribute looks like it originated from
/// a brink [`PartJson::Span`] export (`SPAN_MARKER_SUBTYPE`/
/// `SPAN_PAIRED_SUBTYPE`, or a `dspan{n}` `originalData` id). Shared by the
/// `<sc>`/`<ec>`/`<mrk>` decode guards below.
fn looks_like_span_marker(sub_type: Option<&str>, data_ref: Option<&str>) -> bool {
    sub_type.is_some_and(|s| s.starts_with("brink:"))
        || data_ref.is_some_and(|r| r.starts_with("dspan"))
}

/// [`inline_to_content`]'s per-element reconstruction, factored out so it
/// can recurse into a `<pc>`'s own `content` — a paired span's children are
/// themselves [`InlineElement`]s that need the exact same Text/CData/Ph/Pc
/// handling as the top-level stream.
///
/// `<![CDATA[...]]>` decodes exactly like plain character data (#1799): a
/// TMS is free to return translated text wrapped in a CDATA section — it's
/// legal XLIFF content with no structural ambiguity, unlike a re-expressed
/// span (below), so there is no reason to reject it. The alternative of
/// treating it as an error was considered and rejected: CDATA carries no
/// span markers to lose, so an error would only punish translators for a
/// no-op XML quoting choice. This is the same class of bug already fixed on
/// the sibling `xliff2`-crate metadata-extraction path (#765).
///
/// A brink-exported paired span always round-trips as `<pc>` (see
/// [`push_part_inline`]) — but XLIFF 2.0 lets a translation tool
/// re-express a `<pc>` that spans a segment split as an `<sc>`/`<ec>` pair,
/// or wrap it in `<mrk>`, while preserving `subType`/`dataRef`. Silently
/// falling through the catch-all below for those shapes would decode as a
/// span that quietly lost its content — the same "silent drop" failure
/// class this module fixes on export (#1734), just moved to import. When
/// the `subType`/`dataRef` marks the element as brink-authored, this is an
/// explicit decode error instead. `<sc>`/`<ec>`/`<mrk>` reconstruction is a
/// known limitation (`docs/prose-dialect-spec.md` §4.4); genuinely foreign
/// codes (no brink marker) are still ignored, same as before.
fn elements_to_parts(
    elements: &[InlineElement],
    data_map: &HashMap<&str, &str>,
) -> Result<Vec<PartJson>, IntlError> {
    let mut parts = Vec::new();
    for elem in elements {
        match elem {
            InlineElement::Text(s) | InlineElement::CData(s) => {
                parts.push(PartJson::Literal(s.clone()));
            }
            InlineElement::Ph(ph) => {
                if ph.sub_type.as_deref() == Some(SPAN_MARKER_SUBTYPE) {
                    // Point-marker span (§8b.11): reconstruct name/attrs
                    // from originalData, no children.
                    let meta = decode_span_meta(ph.data_ref.as_deref(), data_map)?;
                    parts.push(PartJson::Span {
                        span: SpanJson {
                            name: meta.name,
                            attrs: meta.attrs,
                            children: Vec::new(),
                        },
                    });
                } else if let Some(ref data_ref) = ph.data_ref {
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
            InlineElement::Pc(pc) => {
                // Paired span: reconstruct name/attrs from originalData,
                // children by recursing into the `<pc>`'s own content.
                let meta = decode_span_meta(pc.data_ref_start.as_deref(), data_map)?;
                let children = elements_to_parts(&pc.content, data_map)?;
                parts.push(PartJson::Span {
                    span: SpanJson {
                        name: meta.name,
                        attrs: meta.attrs,
                        children,
                    },
                });
            }
            InlineElement::Sc(sc)
                if looks_like_span_marker(sc.sub_type.as_deref(), sc.data_ref.as_deref()) =>
            {
                return Err(IntlError::UnsupportedSpanSplit(sc.id.clone()));
            }
            InlineElement::Ec(ec)
                if looks_like_span_marker(ec.sub_type.as_deref(), ec.data_ref.as_deref()) =>
            {
                let id = ec
                    .id
                    .clone()
                    .or_else(|| ec.start_ref.clone())
                    .unwrap_or_default();
                return Err(IntlError::UnsupportedSpanSplit(id));
            }
            InlineElement::Mrk(mrk) if looks_like_span_marker(mrk.mrk_type.as_deref(), None) => {
                return Err(IntlError::UnsupportedSpanSplit(mrk.id.clone()));
            }
            // Other inline elements — and Sc/Ec/Mrk without a brink marker —
            // are not produced by brink, ignore.
            _ => {}
        }
    }

    Ok(parts)
}

/// Look up and deserialize a [`SpanMetaJson`] from `originalData` by
/// `dataRef`/`dataRefStart`. Shared by the point-marker `<ph>` and paired
/// `<pc>` decode paths.
fn decode_span_meta(
    data_ref: Option<&str>,
    data_map: &HashMap<&str, &str>,
) -> Result<SpanMetaJson, IntlError> {
    let data_ref =
        data_ref.ok_or_else(|| IntlError::MissingSpanData("<no dataRef>".to_string()))?;
    let json_str = data_map
        .get(data_ref)
        .ok_or_else(|| IntlError::MissingSpanData(data_ref.to_string()))?;
    serde_json::from_str(json_str).map_err(|e| IntlError::InvalidSpanJson(e.to_string()))
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
            slots: Vec::new(),
            source: None,
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

    // ── #1442: unit ids are keyed on scope-id, not display name ─────────
    //
    // NOTE: this does not make unit ids rename-stable — a `DefinitionId` is
    // itself a hash of the scope's (qualified) name/path, so renaming a
    // knot/stitch still produces a new `DefinitionId` and still orphans its
    // translations (`docs/intl-spec.md:415`). Real rename stability needs a
    // `DefinitionId`-level change, out of scope for this PR (see #1442).

    #[test]
    fn unit_id_is_scope_id_based_not_display_name() {
        let lines = make_lines_json(vec![make_scope(
            "0x0100000000000001",
            Some("intro"),
            vec![make_line(
                0,
                "aaa",
                Some(ContentJson::Plain("Hello".to_string())),
                None,
            )],
        )]);
        let doc = lines_json_to_xliff(&lines, "en", None);
        assert_eq!(doc.files[0].units[0].id, "0x0100000000000001:0");
        // The legacy readable id (`{scope_name}:{index}`) still rides along
        // as the `name` attribute, not as part of `id`, so translators
        // still get readable labels in tooling that shows `name`.
        assert_eq!(doc.files[0].units[0].name, Some("intro:0".to_string()));
    }

    #[test]
    fn unit_id_ignores_scope_display_name() {
        // Same `scope.id`, different `scope.name`. This is NOT a rename
        // simulation — a real rename changes `scope.id` too, since the id
        // is a hash of the scope's name/path (see the module note above).
        // This only proves `line_to_unit` keys `id` off `scope_id`, not off
        // `scope.name`.
        let a = make_lines_json(vec![make_scope(
            "0x0100000000000001",
            Some("intro"),
            vec![make_line(
                0,
                "aaa",
                Some(ContentJson::Plain("Hello".to_string())),
                None,
            )],
        )]);
        let b = make_lines_json(vec![make_scope(
            "0x0100000000000001",
            Some("prologue"),
            vec![make_line(
                0,
                "aaa",
                Some(ContentJson::Plain("Hello".to_string())),
                None,
            )],
        )]);

        let doc_a = lines_json_to_xliff(&a, "en", None);
        let doc_b = lines_json_to_xliff(&b, "en", None);

        assert_eq!(doc_a.files[0].units[0].id, doc_b.files[0].units[0].id);
        // The display name (carried on `File.id` and `Unit.name`) did
        // change, proving this isn't a no-op.
        assert_ne!(doc_a.files[0].id, doc_b.files[0].id);
    }

    #[test]
    fn migrate_unit_ids_rewrites_legacy_ids_preserving_translations() {
        // Build a document the way pre-#1442 brink would have: unit id
        // built from the display name.
        let mut doc = Document {
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
                    name: None,
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
                        attributes: vec![ExtensionAttribute {
                            namespace: BRINK_PREFIX.to_string(),
                            local_name: "hash".to_string(),
                            value: "aaa".to_string(),
                        }],
                    },
                }],
                extensions: Extensions {
                    elements: Vec::new(),
                    attributes: vec![ExtensionAttribute {
                        namespace: BRINK_PREFIX.to_string(),
                        local_name: "scope-id".to_string(),
                        value: "0x0100000000000001".to_string(),
                    }],
                },
            }],
            extensions: Extensions::default(),
        };

        let changed = migrate_unit_ids(&mut doc).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(doc.files[0].units[0].id, "0x0100000000000001:0");

        // Translation content, state, and hash extension are untouched.
        let SubUnit::Segment(seg) = &doc.files[0].units[0].sub_units[0] else {
            unreachable!()
        };
        assert_eq!(seg.state, Some(State::Translated));
        assert_eq!(
            seg.target.as_ref().unwrap().elements,
            vec![InlineElement::Text("Hola".to_string())]
        );
        assert_eq!(
            ext_attr_value(&doc.files[0].units[0].extensions, "hash"),
            Some("aaa")
        );
    }

    #[test]
    fn migrate_unit_ids_is_idempotent() {
        let lines = make_lines_json(vec![make_scope(
            "0x0100000000000001",
            Some("intro"),
            vec![make_line(
                0,
                "aaa",
                Some(ContentJson::Plain("Hello".to_string())),
                None,
            )],
        )]);
        let mut doc = lines_json_to_xliff(&lines, "en", None);

        // Already on the new scheme — migrating should be a no-op.
        let first_pass = migrate_unit_ids(&mut doc).unwrap();
        assert_eq!(first_pass, 0);
        assert_eq!(doc.files[0].units[0].id, "0x0100000000000001:0");

        let second_pass = migrate_unit_ids(&mut doc).unwrap();
        assert_eq!(second_pass, 0);
    }
}
