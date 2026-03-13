use crate::hir;

use super::content::lower_content_parts_pub;
use super::context::LowerCtx;
use super::lir;

/// Try to recognize a HIR content line as a known pattern.
///
/// Phase 1: matches `[Text(s)]` (exactly one text part, no dynamic content)
/// and returns `ContentEmission` with `RecognizedLine::Plain(s)`.
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
        let tags = content
            .tags
            .iter()
            .map(|t| lower_content_parts_pub(&t.parts, ctx))
            .collect();
        return Some(lir::ContentEmission {
            line: lir::RecognizedLine::Plain(s.clone()),
            metadata: lir::LineMetadata { source_hash },
            tags,
        });
    }
    None
}
