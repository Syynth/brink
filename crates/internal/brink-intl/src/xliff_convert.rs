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

/// `subType` marking a [`PartJson::Slot`] mapped to a standalone `<ph>`
/// inline code (#1823). Before this, decode told a slot `<ph>` apart from a
/// select/foreign one purely by `id` spelling (`id.starts_with('s')`) — a
/// TMS-authored `<ph id="sep1">` (a separator marker, not a brink slot)
/// took this branch and hard-failed the whole import trying to parse `ep1`
/// as a slot number. `subType` is a brink-owned marker no foreign producer
/// would coincidentally emit, the same discrimination `SPAN_MARKER_SUBTYPE`
/// already uses for the point-marker `<ph>` shape — see
/// [`looks_like_span_marker`]'s doc comment for why identity-based
/// discrimination (element name, `id` spelling) is the wrong tool here.
const SLOT_SUBTYPE: &str = "brink:slot";

/// `dataRef`/`dataRefStart` id prefix brink's own exporter always uses for a
/// [`PartJson::Select`]'s `<ph dataRef="dsel{n}">` (see `push_part_inline`).
/// Decode gates the select branch on this prefix rather than "any `dataRef`
/// at all" (#1823) — a TMS's own native `<ph dataRef="…">` pointing at its
/// own `<data>` payload is a legitimate foreign shape, not a brink select
/// that merely failed to look up its data.
const SELECT_DATA_REF_PREFIX: &str = "dsel";

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
/// Append a [`PartJson::Literal`] string as inline elements, escaping any
/// scalar value XML 1.0 text cannot carry as a `<cp hex="…"/>` code point.
///
/// This is [`decode_cp`]'s export inverse. Without it, a literal recovered
/// *from* a `<cp>` on import (or any other literal that happens to contain
/// one of these scalars, e.g. a translator-authored C0 control character)
/// would round-trip back out through `InlineElement::Text` — and
/// `quick_xml`'s text escaper only escapes `< > & ' "`, so a character like
/// U+0001 would be written as a raw byte, producing a `<target>` that is
/// not well-formed XML 1.0 even though `quick_xml`'s own non-validating
/// reader will parse it back (#1811 follow-up).
///
/// XML 1.0's `Char` production forbids C0 controls other than tab/LF/CR
/// (`< '\u{20}'`, excluding `\t`/`\n`/`\r`) and the two noncharacters
/// U+FFFE/U+FFFF; every other scalar `char` can reach here because
/// `PartJson::Literal` is a `String`, which is already valid UTF-8 and
/// cannot hold an unpaired surrogate.
fn push_literal_inline(s: &str, elements: &mut Vec<InlineElement>) {
    let mut run = String::new();
    for c in s.chars() {
        if is_xml_illegal_char(c) {
            if !run.is_empty() {
                elements.push(InlineElement::Text(std::mem::take(&mut run)));
            }
            elements.push(InlineElement::Cp(format!("{:04X}", c as u32)));
        } else {
            run.push(c);
        }
    }
    if !run.is_empty() {
        elements.push(InlineElement::Text(run));
    }
}

/// True for a Unicode scalar value XML 1.0 text content cannot carry
/// literally — see [`push_literal_inline`].
fn is_xml_illegal_char(c: char) -> bool {
    (c < '\u{20}' && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{FFFE}' | '\u{FFFF}')
}

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
            push_literal_inline(s, elements);
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
                sub_type: Some(SLOT_SUBTYPE.to_string()),
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

/// The legacy (pre-`SLOT_SUBTYPE`) brink slot spelling: `<ph id="s{n}"
/// equiv="{slot n}"/>` with no `subType`. Returns the slot number only when
/// BOTH markers agree — the id is `s` + a fully numeric `u8` AND `equiv` is
/// brink's own `{slot n}` for the same `n` — so old brink exports keep
/// re-importing losslessly while a TMS `<ph id="sep1">` (non-numeric) or a
/// coincidental `<ph id="s2">` without brink's exact `equiv` stays foreign.
/// See the call site's comment for the compat story (#1823 review).
fn legacy_slot(ph: &xliff2::model::Ph) -> Option<u8> {
    if ph.sub_type.is_some() {
        return None;
    }
    let slot: u8 = ph.id.strip_prefix('s')?.parse().ok()?;
    (ph.equiv.as_deref() == Some(format!("{{slot {slot}}}").as_str())).then_some(slot)
}

/// [`inline_to_content`]'s per-element reconstruction, factored out so it
/// can recurse into the child content of a `<pc>` (a paired span) or a
/// foreign `<mrk>` (a TMS annotation) — those children are themselves
/// [`InlineElement`]s that need the exact same handling as the top-level
/// stream.
///
/// # Disposition of every [`InlineElement`] variant
///
/// Translator work that reaches this function and is not decoded is
/// *unrecoverable* — there is no second copy of a TMS-returned target — so
/// the match below is **exhaustive on purpose**: there is no `_` catch-all,
/// and a new variant in the `xliff2` model is a compile error here rather
/// than another silent drop. Every arm is one of three dispositions:
///
/// | element | disposition | rationale |
/// |---|---|---|
/// | `Text`, `CData` | **decoded** as [`PartJson::Literal`] | CDATA is plain character data spelled with a different XML quoting mechanism — no structural ambiguity, nothing to lose (#1799, and #765 on the sibling `xliff2` metadata path) |
/// | `Cp` | **decoded** as [`PartJson::Literal`] | `<cp hex="…"/>` is XLIFF's escape hatch for a character its producer could not or would not write literally; decoding the code point restores exactly the character the translator meant, with no structure to reconstruct (#1811) |
/// | `Ph` | **decoded** as a span point marker when `subType` is [`SPAN_MARKER_SUBTYPE`]; **decoded** as [`PartJson::Select`] when `dataRef` starts with [`SELECT_DATA_REF_PREFIX`]; **decoded** as [`PartJson::Slot`] when `subType` is [`SLOT_SUBTYPE`]; **ignored** in every other case | `<ph>` is an empty element — it holds attributes only, never character data — so every disposition here that isn't a brink-owned marker is safe to ignore rather than fail loudly. Before #1823, the select/slot branches discriminated by element shape instead of a brink-owned marker (*any* `dataRef` at all; `id.starts_with('s')` + parse-as-number), so a foreign, TMS-legitimate `<ph>` that merely resembled one of brink's own shapes (a native code with its own `dataRef`/`<data>` payload; a separator id like `sep1`) hard-failed the whole import. `SPAN_MARKER_SUBTYPE`/`SLOT_SUBTYPE`/`SELECT_DATA_REF_PREFIX` are markers no foreign producer would coincidentally emit — the same discrimination [`looks_like_span_marker`] already used for `<sc>`/`<ec>`/`<mrk>` |
/// | `Pc` *with* a brink marker ([`looks_like_span_marker`] on `subType`/`dataRefStart`) | **decoded** as [`PartJson::Span`], recursing into its children | brink's own paired-span shape |
/// | `Pc` *without* a brink marker | **decoded** by splicing its children in place (#1823) | a foreign `<pc>` — same-namespace TMS markup, or a differently-namespaced element (e.g. `mq:pc`) that collides on local name because `xliff2::read::read_inline_content` dispatches on local name only — is not brink content, but unlike `<ph>`/`<sc>`/`<ec>` it has a real content model that can carry translator text. Before #1823 the `Pc` arm was unconditional and called [`decode_span_meta`] on every `<pc>`, so a foreign one without a brink `dataRefStart` hard-failed with [`IntlError::MissingSpanData`] instead of recovering the text it wraps. Splicing mirrors the foreign-`<mrk>` arm below — same taxonomy: a wrapper with a content model recovers its children when it isn't brink's own, the same way #1821 already treats `<mrk>` |
/// | `Sc`, `Ec`, `Mrk` *with* a brink marker | **explicit [`IntlError::UnsupportedSpanSplit`]** | a brink `<pc>` that a tool re-expressed as a split pair or a wrapping mark: the structure cannot be reconstructed, so failing loudly beats decoding a span that quietly lost its content — the same failure class fixed on export by #1734 |
/// | `Mrk` *without* a brink marker | **decoded** by splicing its children in place | a TMS `<mrk>` (terminology, comment, QA flag) wraps a *span of translated text*; the annotation's own `id`/`type`/`ref`/`value` are not brink content and are dropped, but the text it marks is translator work and must survive (#1812) |
/// | `Sc`, `Ec` *without* a brink marker, `Sm`, `Em` | **ignored** | these are empty elements carrying attributes only — they never hold character data. The text a foreign `<sc>`/`<ec>` or `<sm>`/`<em>` pair *spans* is not nested inside them; the reader emits it as sibling `Text` elements that the `Text`/`CData` arm already recovers (proved by `foreign_sm_em_spanned_text_survives_export_import_roundtrip`) |
///
/// `<sc>`/`<ec>`/`<mrk>` reconstruction of a brink span remains a known
/// limitation (`docs/prose-dialect-spec.md` §4.4).
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
            InlineElement::Cp(hex) => {
                parts.push(PartJson::Literal(decode_cp(hex)?.to_string()));
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
                } else if let Some(data_ref) = ph
                    .data_ref
                    .as_deref()
                    .filter(|r| r.starts_with(SELECT_DATA_REF_PREFIX))
                {
                    // Select: look up in originalData. Gated on brink's own
                    // `dsel{n}` dataRef prefix (#1823) — a foreign `<ph>`
                    // with some other `dataRef` (a real XLIFF 2.0 shape: a
                    // native code plus its own `<data>` payload) is not a
                    // brink select that lost its data, it's not brink's at
                    // all, so it falls through to the ignore case below.
                    let json_str = data_map
                        .get(data_ref)
                        .ok_or_else(|| IntlError::MissingSelectData(data_ref.to_string()))?;
                    let select: SelectJson = serde_json::from_str(json_str)
                        .map_err(|e| IntlError::InvalidSelectJson(e.to_string()))?;
                    parts.push(PartJson::Select { select });
                } else if ph.sub_type.as_deref() == Some(SLOT_SUBTYPE) {
                    // Slot: parse slot number from id "s{n}". Gated on
                    // brink's own `SLOT_SUBTYPE` marker (#1823), not on
                    // `id` spelling — a TMS-authored `<ph id="sep1">` (a
                    // separator, not a brink slot) no longer even attempts
                    // the numeric parse.
                    let slot_str = ph.id.strip_prefix('s').ok_or_else(|| {
                        IntlError::InvalidUnitId(format!("bad slot ph id: {}", ph.id))
                    })?;
                    let slot: u8 = slot_str.parse().map_err(|_| {
                        IntlError::InvalidUnitId(format!("bad slot ph id: {}", ph.id))
                    })?;
                    parts.push(PartJson::Slot { slot });
                } else if let Some(slot) = legacy_slot(ph) {
                    // Back-compat (#1823 review): every `.xlf` brink
                    // exported BEFORE `SLOT_SUBTYPE` existed spells a slot
                    // as `<ph id="s{n}" equiv="{slot n}"/>` with no
                    // `subType` at all. Those files sit at TMSes
                    // mid-translation; classifying their slots as foreign
                    // would silently drop them on re-import — translator
                    // data loss, the exact failure mode this issue exists
                    // to prevent. So a subType-less `<ph>` is still a slot
                    // when BOTH legacy markers agree: id is `s` + a fully
                    // numeric `u8`, AND `equiv` spells brink's own
                    // `{slot n}` for the same n. A TMS `<ph id="sep1">`
                    // fails the numeric parse and a foreign `<ph id="s2">`
                    // without brink's exact `equiv` fails the second check
                    // — both stay foreign (ignored below), so the #1823
                    // fix is not weakened.
                    parts.push(PartJson::Slot { slot });
                }
                // No trailing `else`: a `<ph>` that carries none of brink's
                // own markers — no `SPAN_MARKER_SUBTYPE`, no `dsel`-prefixed
                // `dataRef`, no `SLOT_SUBTYPE`, and not the legacy
                // id+equiv slot pair — is a foreign standalone
                // code placeholder (whatever its `id` spelling or whatever
                // *other* `dataRef` it carries, #1823). `<ph>` is an empty
                // element — it holds attributes only, never character data —
                // so ignoring it cannot lose translator work.
            }
            InlineElement::Pc(pc) => {
                if looks_like_span_marker(pc.sub_type.as_deref(), pc.data_ref_start.as_deref()) {
                    // Brink's own paired span: reconstruct name/attrs from
                    // originalData, children by recursing into the `<pc>`'s
                    // own content.
                    let meta = decode_span_meta(pc.data_ref_start.as_deref(), data_map)?;
                    let children = elements_to_parts(&pc.content, data_map)?;
                    parts.push(PartJson::Span {
                        span: SpanJson {
                            name: meta.name,
                            attrs: meta.attrs,
                            children,
                        },
                    });
                } else {
                    // Foreign `<pc>` (#1823): same-namespace TMS markup, or
                    // a differently-namespaced element that collides on
                    // local name (`mq:pc`) because the reader dispatches on
                    // local name only. Not brink content, but a genuine
                    // wrapper with a content model that can hold translator
                    // text — splice its children in place, mirroring the
                    // foreign-`<mrk>` arm below (#1821): the wrapper's own
                    // `id`/`subType` are discarded, the text it wraps is
                    // not.
                    parts.extend(elements_to_parts(&pc.content, data_map)?);
                }
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
            InlineElement::Mrk(mrk) => {
                // Foreign `<mrk>` (#1812): a TMS annotation — terminology,
                // a reviewer comment, a QA flag — wrapping a span of
                // *translated text*. The annotation itself is not brink
                // content and is discarded, but the text it marks is
                // translator work: splice the children in place so the
                // marked substring survives instead of vanishing with the
                // mark. Recursing (rather than pulling out `Text` nodes)
                // keeps any brink inline codes inside the mark working.
                parts.extend(elements_to_parts(&mrk.content, data_map)?);
            }
            // Foreign `<sc>`/`<ec>` and every `<sm>`/`<em>`: empty elements
            // that carry attributes only and never hold character data.
            // The text such a pair *spans* is a sibling of the marker, not
            // a child of it, so it is already recovered by the
            // `Text`/`CData` arm above — ignoring the markers themselves
            // drops annotation metadata, never translator work.
            InlineElement::Sc(_)
            | InlineElement::Ec(_)
            | InlineElement::Sm(_)
            | InlineElement::Em(_) => {}
        }
    }

    Ok(parts)
}

/// Decode an XLIFF `<cp hex="…"/>` element's `hex` attribute into the
/// character it stands for.
///
/// `<cp>` is XLIFF 2.0's representation of a character by its Unicode code
/// point, used when a producer cannot (or will not) write the character
/// literally — typically C0 control characters, which are illegal in XML
/// text, but also anything a tool's encoding could not carry. Decoding
/// yields exactly the character the translator meant, so the caller treats
/// the result as ordinary literal text (#1811).
///
/// A `hex` that is not a valid Unicode scalar value (unparseable, out of
/// range, or a surrogate) is malformed XLIFF: it is reported as
/// [`IntlError::InvalidCodePoint`] rather than skipped, because silently
/// dropping it is the very failure this arm exists to prevent.
///
/// [`push_literal_inline`] is this function's export inverse — a decoded
/// literal that is re-serialized (e.g. by `regenerate-xliff`) writes the
/// same illegal scalar back out as `<cp>` rather than as raw text, so the
/// round trip stays well-formed XML 1.0.
fn decode_cp(hex: &str) -> Result<char, IntlError> {
    u32::from_str_radix(hex, 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| IntlError::InvalidCodePoint(hex.to_owned()))
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
