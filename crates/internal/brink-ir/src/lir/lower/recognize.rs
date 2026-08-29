use brink_format::{LinePart, SlotInfo, SourceLocation};

use crate::hir;
use crate::hir::display_expr;

use super::content::lower_content_parts_pub;
use super::context::LowerCtx;
use super::expr::lower_expr;
use super::lir;

/// Compose two HIR content objects by concatenating their parts and tags.
///
/// Adjacent `Text` parts at the boundary are merged into one. The resulting
/// content uses the first content's `ptr` for source location.
pub fn compose_hir_content(a: &hir::Content, b: &hir::Content) -> hir::Content {
    let mut parts = a.parts.clone();

    // Merge adjacent text parts at the boundary, collapsing double
    // whitespace at the join point (e.g., "Hello " + " world" → "Hello world").
    if let (Some(hir::ContentPart::Text(last)), Some(hir::ContentPart::Text(first))) =
        (parts.last(), b.parts.first())
    {
        let merged =
            if last.ends_with(char::is_whitespace) && first.starts_with(char::is_whitespace) {
                format!("{last}{}", first.trim_start())
            } else {
                format!("{last}{first}")
            };
        let len = parts.len();
        parts[len - 1] = hir::ContentPart::Text(merged);
        parts.extend(b.parts.iter().skip(1).cloned());
    } else {
        parts.extend(b.parts.iter().cloned());
    }

    let mut tags = a.tags.clone();
    tags.extend(b.tags.iter().cloned());

    hir::Content {
        ptr: a.ptr,
        parts,
        tags,
    }
}

/// Compose display or output content from optional HIR content parts.
///
/// Returns `None` if both parts are `None`.
pub fn compose_hir_content_opt(
    a: Option<&hir::Content>,
    b: Option<&hir::Content>,
) -> Option<hir::Content> {
    match (a, b) {
        (None, None) => None,
        (Some(c), None) | (None, Some(c)) => Some(c.clone()),
        (Some(a_content), Some(b_content)) => Some(compose_hir_content(a_content, b_content)),
    }
}

/// Check whether HIR content starts with a whitespace-only text part.
///
/// When content with leading whitespace is emitted inline via
/// `push_text`, the runtime's output buffer suppresses whitespace-only
/// text at the start. `EvalLine`/`EmitLine` bypass this filtering
/// (they resolve the template in one shot), so we must skip recognition
/// for content that relies on the runtime's whitespace suppression.
pub fn starts_with_whitespace_only_text(content: &hir::Content) -> bool {
    matches!(content.parts.first(), Some(hir::ContentPart::Text(s)) if !s.is_empty() && s.trim().is_empty())
}

/// Try to recognize a HIR content line as a known pattern.
///
/// Phase 1: matches `[Text(s)]` (exactly one text part, no dynamic content)
/// and returns `ContentEmission` with `RecognizedLine::Plain(s)`.
///
/// Phase 3: matches lines of `Text` and `Interpolation` parts (with at least
/// one `Interpolation`) and returns `RecognizedLine::Template`.
///
/// Returns `None` for any other pattern — the caller falls back to
/// `EmitContent(lower_content(...))`.
pub fn try_recognize(
    content: &hir::Content,
    ctx: &mut LowerCtx<'_>,
) -> Option<lir::ContentEmission> {
    // Phase 1: plain text — exactly one Text part, nothing else.
    if content.parts.len() == 1
        && let hir::ContentPart::Text(s) = &content.parts[0]
    {
        let source_hash = brink_format::content_hash(s);
        let source_location = build_source_location(content, ctx);
        let tags = content
            .tags
            .iter()
            .map(|t| lower_content_parts_pub(&t.parts, ctx))
            .collect();
        return Some(lir::ContentEmission {
            line: lir::RecognizedLine::Plain(s.clone()),
            metadata: lir::LineMetadata {
                source_hash,
                slot_info: Vec::new(),
                source_location,
            },
            tags,
        });
    }

    // Phase 3: template — all parts are Text/Interpolation/Span
    // (recursively, for Span), with ≥1 Interpolation-or-Span and ≥1
    // non-whitespace Text somewhere in the tree.
    if try_recognize_template(content, ctx) {
        let mut template_parts = Vec::new();
        let mut slot_exprs = Vec::new();
        let mut slot_info = Vec::new();
        let mut hash_source = String::new();
        let mut slot_idx: u8 = 0;

        build_recognized_parts(
            &content.parts,
            ctx,
            &mut template_parts,
            &mut hash_source,
            &mut slot_exprs,
            &mut slot_info,
            &mut slot_idx,
        );

        let source_hash = brink_format::content_hash(&hash_source);
        let source_location = build_source_location(content, ctx);
        let tags = content
            .tags
            .iter()
            .map(|t| lower_content_parts_pub(&t.parts, ctx))
            .collect();

        return Some(lir::ContentEmission {
            line: lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            },
            metadata: lir::LineMetadata {
                source_hash,
                slot_info,
                source_location,
            },
            tags,
        });
    }

    None
}

/// Strip leading and trailing `Glue` parts from content and merge interior
/// `[Text, Glue, Text]` runs into a single `Text`.
///
/// Returns `(has_leading_glue, stripped_content, has_trailing_glue)`.
/// Interior glue adjacent to non-text parts (Interpolation, `InlineConditional`,
/// etc.) is NOT stripped — those prevent recognition.
pub fn strip_boundary_glue(content: &hir::Content) -> (bool, hir::Content, bool) {
    let parts = &content.parts;

    // Strip leading glue
    let mut start = 0;
    let mut has_leading = false;
    while start < parts.len() && parts[start] == hir::ContentPart::Glue {
        has_leading = true;
        start += 1;
    }

    // Strip trailing glue
    let mut end = parts.len();
    let mut has_trailing = false;
    while end > start && parts[end - 1] == hir::ContentPart::Glue {
        has_trailing = true;
        end -= 1;
    }

    // Merge interior [Text, Glue, Text] runs into single Text.
    // Interior glue adjacent to non-Text parts is left alone (will prevent recognition).
    let interior = &parts[start..end];
    let mut merged_parts: Vec<hir::ContentPart> = Vec::with_capacity(interior.len());
    for part in interior {
        match part {
            hir::ContentPart::Glue => {
                // Check if both the previous and next parts are Text.
                // At this point we only have the previous part available, so we
                // check the previous. We'll merge when we see the next Text.
                if matches!(merged_parts.last(), Some(hir::ContentPart::Text(_))) {
                    // Tentatively mark as "pending merge" by pushing Glue.
                    // We'll resolve this when the next part arrives.
                    merged_parts.push(hir::ContentPart::Glue);
                } else {
                    // Glue adjacent to non-Text — keep it (will block recognition).
                    merged_parts.push(hir::ContentPart::Glue);
                }
            }
            hir::ContentPart::Text(s) => {
                // If the previous part is Glue and the part before that is Text,
                // merge all three into one Text.
                if matches!(merged_parts.last(), Some(hir::ContentPart::Glue)) {
                    merged_parts.pop(); // remove the Glue
                    if let Some(hir::ContentPart::Text(prev)) = merged_parts.last_mut() {
                        prev.push_str(s);
                    } else {
                        // Glue was at the start of interior (shouldn't happen after
                        // boundary stripping, but be safe) — keep as separate text.
                        merged_parts.push(hir::ContentPart::Text(s.clone()));
                    }
                } else {
                    merged_parts.push(part.clone());
                }
            }
            _ => {
                merged_parts.push(part.clone());
            }
        }
    }

    let stripped = hir::Content {
        ptr: content.ptr,
        parts: merged_parts,
        tags: content.tags.clone(),
    };

    (has_leading, stripped, has_trailing)
}

/// Try to recognize content after stripping boundary glue.
///
/// Returns `None` if no glue was stripped (caller already tried plain
/// `try_recognize`) or if the stripped interior is still unrecognizable.
pub fn try_recognize_with_glue(
    content: &hir::Content,
    ctx: &mut LowerCtx<'_>,
) -> Option<(bool, lir::ContentEmission, bool)> {
    let (has_leading, stripped, has_trailing) = strip_boundary_glue(content);

    // If nothing changed, don't retry — caller already tried try_recognize.
    if !has_leading && !has_trailing && stripped.parts.len() == content.parts.len() {
        return None;
    }

    // Empty interior after stripping? Not recognizable.
    if stripped.parts.is_empty() {
        return None;
    }

    let emission = try_recognize(&stripped, ctx)?;
    Some((has_leading, emission, has_trailing))
}

/// Build a `SourceLocation` from the content's syntax pointer and the file
/// path map. `pub(super)` — also used by `lir::lower::content::lower_content`
/// (issue #3181) so the `EmitContent`/`ChoiceOutput` fallback path resolves
/// a real location the same way the recognized-line path does, from the
/// same `hir::Content::ptr`.
pub(super) fn build_source_location(
    content: &hir::Content,
    ctx: &LowerCtx<'_>,
) -> Option<SourceLocation> {
    let ptr = content.ptr.as_ref()?;
    let range = ptr.text_range();
    let file = ctx.file_paths.get(&ctx.file)?;
    Some(SourceLocation {
        file: file.clone(),
        range_start: range.start().into(),
        range_end: range.end().into(),
    })
}

/// Check if all content parts are admissible for `Template`/`Span` wire
/// recognition — Text, Interpolation, or (§4.4) a Span whose own
/// `children` are, recursively, the same three shapes — with ≥1
/// Interpolation-or-Span (the reason a markup-only line like `Hello
/// <wave>world</wave>` still needs admission even with zero
/// interpolations: once a Span splits the text into more than one
/// top-level part, it is no longer Phase 1's single-Text-part `Plain`
/// shape either) and (≥1 non-whitespace Text part somewhere in the tree,
/// **or** ≥1 `Span` present at all).
///
/// The `Span`-present escape hatch matters for a point-marker-only line
/// (§8b.11 — `<pause/>` alone, with no surrounding text): the
/// non-whitespace-text requirement (`d7058cd2d`, "skip template
/// recognition for whitespace-only text between slots" — deliberately
/// keeps a bare `{f()} {g()}` off the `Template` path, since the
/// whitespace there is structural glue, not translatable content) would
/// otherwise decline a line whose *entire* content is a childless span,
/// sending it to `EmitContent`'s flattening — which for a span with no
/// children drops `name`/`attrs` and emits **nothing**, silently, with no
/// diagnostic (unlike the interior-`InlineConditional`/`InlineSequence`
/// case below, that flattening loses real data, not just the
/// presentational boundary). A `Span` occupying a content-part slot is
/// never itself whitespace glue — even self-closing, its `name`/`attrs`
/// are the translatable content — so its mere presence satisfies the
/// "real content" requirement the whitespace-glue guard exists for.
///
/// A Span containing something this doesn't admit (a `DIVERT`-adjacent
/// shape has none — `Span`'s HIR children can only ever be
/// Text/Interpolation/Span/InlineConditional/InlineSequence by
/// construction — but an `InlineConditional`/`InlineSequence` nested in a
/// Span is exactly that "something else": §4.4's still-open "span
/// admission" note) declines the *whole* line, which falls back to
/// `EmitContent`'s flattening (`lir::lower::content`'s own doc) — not a
/// silent drop, just not a translation-table entry yet.
fn try_recognize_template(content: &hir::Content, _ctx: &LowerCtx<'_>) -> bool {
    is_template_admissible(&content.parts)
        && content_has_span_or_interpolation(&content.parts)
        && (content_has_nonempty_text(&content.parts) || content_has_span(&content.parts))
}

fn is_template_admissible(parts: &[hir::ContentPart]) -> bool {
    parts.iter().all(|p| match p {
        hir::ContentPart::Text(_) | hir::ContentPart::Interpolation(_) => true,
        hir::ContentPart::Span(span) => is_template_admissible(&span.children),
        hir::ContentPart::Glue
        | hir::ContentPart::Spring
        | hir::ContentPart::InlineConditional(_)
        | hir::ContentPart::InlineSequence(_) => false,
    })
}

fn content_has_span_or_interpolation(parts: &[hir::ContentPart]) -> bool {
    parts.iter().any(|p| {
        matches!(
            p,
            hir::ContentPart::Interpolation(_) | hir::ContentPart::Span(_)
        )
    })
}

/// Whether any top-level part is a `Span` (self-closing or not). Doesn't need
/// to recurse — a nested `Span` always has a top-level `Span` ancestor here,
/// since `ContentPart` children only ever live inside another `Span`.
fn content_has_span(parts: &[hir::ContentPart]) -> bool {
    parts.iter().any(|p| matches!(p, hir::ContentPart::Span(_)))
}

fn content_has_nonempty_text(parts: &[hir::ContentPart]) -> bool {
    parts.iter().any(|p| match p {
        hir::ContentPart::Text(s) => !s.trim().is_empty(),
        hir::ContentPart::Span(span) => content_has_nonempty_text(&span.children),
        _ => false,
    })
}

/// Recursively build wire `LinePart`s from admitted `hir::ContentPart`s
/// (`try_recognize_template` already validated the shape — the `_ =>
/// unreachable!` arm mirrors that same validated-shape invariant this
/// function's caller has always relied on).
///
/// Accumulates the flat `hash_source` string `source_hash` is computed
/// from — **hash-transparency** (§4.4, RULED before any markup ships)
/// means a `Span`'s `name`/`attrs` never touch it, only its `children`'s
/// own text/interpolation-placeholders do, exactly the way a bare
/// `Interpolation` already contributes the placeholder `"{…}"` rather than
/// its resolved value: `Hello <wave>world</wave>` and `Hello world` hash
/// identically. Also accumulates `slot_exprs`/`slot_info` flatly and
/// `slot_idx` globally across the *whole* line, spans included — a
/// `<b>{x}</b>` inside `…{y}…` numbers `x`/`y` in the one left-to-right
/// order `emit_slot_expr` will later push them onto the evaluation stack
/// in, span boundaries notwithstanding.
fn build_recognized_parts(
    parts: &[hir::ContentPart],
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<LinePart>,
    hash_source: &mut String,
    slot_exprs: &mut Vec<lir::Expr>,
    slot_info: &mut Vec<SlotInfo>,
    slot_idx: &mut u8,
) {
    for part in parts {
        match part {
            hir::ContentPart::Text(s) => {
                out.push(LinePart::Literal(s.clone()));
                hash_source.push_str(s);
            }
            hir::ContentPart::Interpolation(expr) => {
                out.push(LinePart::Slot(*slot_idx));
                slot_exprs.push(lower_expr(expr, ctx));
                slot_info.push(SlotInfo {
                    index: *slot_idx,
                    name: display_expr(expr),
                });
                hash_source.push_str("{…}");
                *slot_idx = slot_idx.saturating_add(1);
            }
            hir::ContentPart::Span(span) => {
                let mut children = Vec::with_capacity(span.children.len());
                build_recognized_parts(
                    &span.children,
                    ctx,
                    &mut children,
                    hash_source,
                    slot_exprs,
                    slot_info,
                    slot_idx,
                );
                out.push(LinePart::Span {
                    name: span.name.clone(),
                    // `LinePart::Span::attrs` is the wire shape's flat
                    // `Vec<(String, String)>` (untouched by #1782 and by
                    // #1829: E164/E165 fire during HIR analysis, before LIR
                    // lowering ever runs, so per-attribute provenance has
                    // nothing to carry across this boundary).
                    attrs: span
                        .attrs
                        .iter()
                        .map(|attr| (attr.name.clone(), attr.value.clone()))
                        .collect(),
                    children,
                });
            }
            hir::ContentPart::Glue
            | hir::ContentPart::Spring
            | hir::ContentPart::InlineConditional(_)
            | hir::ContentPart::InlineSequence(_) => {
                unreachable!("try_recognize_template already validated")
            }
        }
    }
}

// ─── Line-variant enumeration (#3273, stage 1) ──────────────────────────

/// The per-line cap on enumerated variants (`dims.iter().product()`).
///
/// `4×4×2` fits comfortably; `8×8×8` does not — each variant is a real
/// line-table entry, a translation unit, and a VO slot, so an unbounded
/// product is unbounded artifact growth ("guard against unbounded
/// growth"). Exceeding it is a **worded diagnostic** at the caller, never
/// a silent fallback: an author whose line quietly stopped being
/// VO-addressable would have no way to notice.
pub const VARIANT_CAP: usize = 32;

/// One authored alternative admitted to the variant model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantAlt {
    /// Index of the `InlineSequence` part in the original content's parts.
    pub part_idx: usize,
    /// The alternative's sequence kind (plain `CYCLE`/`STOPPING`/`ONCE`/
    /// `SHUFFLE` — combinations are not admitted, see
    /// [`enumerate_variant_contents`]).
    pub kind: hir::SequenceType,
    /// Authored branch count (NOT the dim — a `once` alternative's dim is
    /// `branch_count + 1`, the extra being the exhausted empty variant).
    pub branch_count: u16,
}

/// A content line enumerated into whole-line variants (#3273).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantEnumeration {
    /// The admitted alternatives, in part order (= source order).
    pub alts: Vec<VariantAlt>,
    /// Branch count per alternative **as laid out in the line table** —
    /// `branch_count`, plus one for a `once` alternative's exhausted
    /// (empty) variant. `dims.iter().product()` is the variant count.
    pub dims: Vec<u16>,
    /// One substituted whole-line content per variant, row-major with the
    /// FIRST alternative varying slowest — variant `(i, j)` lives at
    /// `i * dims[1] + j`, matching `brink_format::LineVariantGroup`'s
    /// layout contract. Each is an ordinary content line (the alternative
    /// parts replaced by the chosen branch's parts, adjacent text merged),
    /// ready for [`try_recognize`].
    pub variants: Vec<hir::Content>,
}

/// The cap breach — the ONE way an otherwise-admissible variant line is
/// refused. Carried as data so the caller can word the diagnostic with
/// the line's own numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantCapExceeded {
    /// What `dims.iter().product()` would have been.
    pub product: usize,
    /// [`VARIANT_CAP`].
    pub cap: usize,
}

/// Try to enumerate a content line's inline stateful alternatives into
/// whole-line variants (#3273, stage 1 of the shared-alternatives track).
///
/// `Ok(None)` — not a variant line; the caller keeps its current path.
/// Admission requires:
///
/// * at least one `InlineSequence` part, every one of a **plain** kind
///   (`cycle` / `stopping` / `once` / `shuffle` — a `shuffle|once` or
///   `shuffle|stopping` combination routes to the shared-inline-container
///   fallback instead, where the exhaustion logic already lives);
/// * every alternative branch **textual**: an empty body, or exactly one
///   `Content` statement of `Text`/`Interpolation`/`Span` parts with no
///   tags — a branch carrying structure (a divert, a nested alternative,
///   glue) cannot be a whole-line variant under ANY model;
/// * no `InlineConditional` and no `Glue`/`Spring` anywhere on the line —
///   conditionals are condition-driven, not visit-driven, and mixed lines
///   keep their current path until ruled otherwise.
///
/// The substitution itself cannot fail; the one error is the
/// [`VARIANT_CAP`] breach, returned as data for the caller to word.
pub fn enumerate_variant_contents(
    content: &hir::Content,
) -> Result<Option<VariantEnumeration>, VariantCapExceeded> {
    let mut alts = Vec::new();
    for (idx, part) in content.parts.iter().enumerate() {
        match part {
            hir::ContentPart::InlineSequence(seq) => {
                let kind = seq.kind;
                let plain = [
                    hir::SequenceType::CYCLE,
                    hir::SequenceType::STOPPING,
                    hir::SequenceType::ONCE,
                    hir::SequenceType::SHUFFLE,
                ];
                if !plain.contains(&kind) {
                    return Ok(None);
                }
                if seq.branches.is_empty() || seq.branches.len() > usize::from(u16::MAX) {
                    return Ok(None);
                }
                for branch in &seq.branches {
                    if branch_textual_parts(&branch.body).is_none() {
                        return Ok(None);
                    }
                }
                let branch_count = u16::try_from(seq.branches.len()).unwrap_or(u16::MAX);
                alts.push(VariantAlt {
                    part_idx: idx,
                    kind,
                    branch_count,
                });
            }
            hir::ContentPart::InlineConditional(_)
            | hir::ContentPart::Glue
            | hir::ContentPart::Spring => return Ok(None),
            hir::ContentPart::Text(_)
            | hir::ContentPart::Interpolation(_)
            | hir::ContentPart::Span(_) => {}
        }
    }
    if alts.is_empty() {
        return Ok(None);
    }

    let dims: Vec<u16> = alts
        .iter()
        .map(|alt| {
            if alt.kind == hir::SequenceType::ONCE {
                alt.branch_count.saturating_add(1)
            } else {
                alt.branch_count
            }
        })
        .collect();
    let product = dims.iter().try_fold(1usize, |acc, &d| {
        acc.checked_mul(usize::from(d))
            .filter(|p| *p <= VARIANT_CAP)
    });
    let Some(product) = product else {
        return Err(VariantCapExceeded {
            product: dims.iter().map(|&d| usize::from(d)).product(),
            cap: VARIANT_CAP,
        });
    };

    // Row-major enumeration, first alternative slowest.
    let mut variants = Vec::with_capacity(product);
    let mut combo = vec![0u16; alts.len()];
    loop {
        variants.push(substitute_combo(content, &alts, &combo));
        // Mixed-radix increment, last dim fastest.
        let mut k = alts.len();
        loop {
            if k == 0 {
                break;
            }
            k -= 1;
            combo[k] += 1;
            if combo[k] < dims[k] {
                break;
            }
            combo[k] = 0;
            if k == 0 {
                debug_assert_eq!(
                    variants.len(),
                    product,
                    "mixed-radix walk covers the product"
                );
                return Ok(Some(VariantEnumeration {
                    alts,
                    dims,
                    variants,
                }));
            }
        }
    }
}

/// A branch body's content parts, if the branch is textual (see
/// [`enumerate_variant_contents`]'s admission rules): empty body → empty
/// parts; exactly one tag-free `Content` stmt of `Text`/`Interpolation`/
/// `Span` parts → those parts. `None` otherwise.
fn branch_textual_parts(body: &hir::Block) -> Option<Vec<hir::ContentPart>> {
    match body.stmts.as_slice() {
        [] => Some(Vec::new()),
        [hir::Stmt::Content(c)] if c.tags.is_empty() => {
            let ok = c.parts.iter().all(|p| {
                matches!(
                    p,
                    hir::ContentPart::Text(_)
                        | hir::ContentPart::Interpolation(_)
                        | hir::ContentPart::Span(_)
                )
            });
            ok.then(|| c.parts.clone())
        }
        _ => None,
    }
}

/// Build one variant's whole-line content: each admitted alternative
/// replaced by its chosen branch's parts (or by nothing, for a `once`
/// alternative's exhausted index), adjacent text merged with the same
/// whitespace-collapse rule as [`compose_hir_content`].
fn substitute_combo(content: &hir::Content, alts: &[VariantAlt], combo: &[u16]) -> hir::Content {
    let mut parts: Vec<hir::ContentPart> = Vec::with_capacity(content.parts.len());
    let mut push_merged = |parts: &mut Vec<hir::ContentPart>, part: &hir::ContentPart| {
        if let (Some(hir::ContentPart::Text(last)), hir::ContentPart::Text(next)) =
            (parts.last_mut(), part)
        {
            if last.ends_with(char::is_whitespace) && next.starts_with(char::is_whitespace) {
                last.push_str(next.trim_start());
            } else {
                last.push_str(next);
            }
        } else {
            parts.push(part.clone());
        }
    };

    for (idx, part) in content.parts.iter().enumerate() {
        if let Some(alt_pos) = alts.iter().position(|a| a.part_idx == idx) {
            let hir::ContentPart::InlineSequence(seq) = part else {
                unreachable!("alts only index InlineSequence parts");
            };
            let chosen = usize::from(combo[alt_pos]);
            if chosen < seq.branches.len() {
                let branch_parts =
                    branch_textual_parts(&seq.branches[chosen].body).unwrap_or_default();
                for bp in &branch_parts {
                    push_merged(&mut parts, bp);
                }
            }
            // else: a `once` alternative's exhausted variant — nothing.
        } else {
            push_merged(&mut parts, part);
        }
    }

    hir::Content {
        ptr: content.ptr,
        parts,
        tags: content.tags.clone(),
    }
}
