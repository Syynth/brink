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

    // Phase 3: template — all parts are Text or Interpolation, ≥1 Interpolation.
    if try_recognize_template(content, ctx) {
        // All parts are Text or Interpolation — build template.
        let mut template_parts = Vec::new();
        let mut slot_exprs = Vec::new();
        let mut slot_info = Vec::new();
        let mut hash_source = String::new();
        let mut slot_idx: u8 = 0;

        for part in &content.parts {
            match part {
                hir::ContentPart::Text(s) => {
                    template_parts.push(LinePart::Literal(s.clone()));
                    hash_source.push_str(s);
                }
                hir::ContentPart::Interpolation(expr) => {
                    template_parts.push(LinePart::Slot(slot_idx));
                    slot_exprs.push(lower_expr(expr, ctx));
                    slot_info.push(SlotInfo {
                        index: slot_idx,
                        name: display_expr(expr),
                    });
                    hash_source.push_str("{…}");
                    slot_idx = slot_idx.saturating_add(1);
                }
                _ => unreachable!("try_recognize_template already validated"),
            }
        }

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

/// Build a `SourceLocation` from the content's syntax pointer and the file path map.
fn build_source_location(content: &hir::Content, ctx: &LowerCtx<'_>) -> Option<SourceLocation> {
    let ptr = content.ptr.as_ref()?;
    let range = ptr.text_range();
    let file = ctx.file_paths.get(&ctx.file)?;
    Some(SourceLocation {
        file: file.clone(),
        range_start: range.start().into(),
        range_end: range.end().into(),
    })
}

/// Check if all content parts are Text or Interpolation, with ≥1 Interpolation.
fn try_recognize_template(content: &hir::Content, _ctx: &LowerCtx<'_>) -> bool {
    let mut has_interpolation = false;
    let mut has_text = false;
    for part in &content.parts {
        match part {
            hir::ContentPart::Text(_) => {
                has_text = true;
            }
            hir::ContentPart::Interpolation(_) => {
                has_interpolation = true;
            }
            _ => return false,
        }
    }
    // Require at least one Text part — a template with only slots has no
    // translatable source text and should fall through to EmitContent,
    // which uses EmitValue (correctly suppresses null/void results).
    has_interpolation && has_text
}
