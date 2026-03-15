use brink_format::{LinePart, SlotInfo, SourceLocation};

use crate::hir;
use crate::hir::display_expr;

use super::content::lower_content_parts_pub;
use super::context::LowerCtx;
use super::expr::lower_expr;
use super::lir;

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
    for part in &content.parts {
        match part {
            hir::ContentPart::Text(_) => {}
            hir::ContentPart::Interpolation(_) => {
                has_interpolation = true;
            }
            _ => return false,
        }
    }
    has_interpolation
}
