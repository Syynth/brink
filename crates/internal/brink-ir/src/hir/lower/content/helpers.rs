use brink_syntax::ast::{self, AstNode};

use crate::provenance::NodeClass;
use crate::{ContentPart, DiagnosticCode, Tag};

use super::super::conditional::lower_inline_logic_into_parts;
use super::super::context::{LowerScope, LowerSink};
use super::super::directive::parse_directive_tag;

/// Lower the inline content children of a syntax node (`TEXT`, `GLUE`, `ESCAPE`,
/// `INLINE_LOGIC`) into a `Vec` of `ContentPart`s.
pub fn lower_content_node_children(
    node: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Vec<ContentPart> {
    use brink_syntax::SyntaxKind;

    let mut parts = Vec::new();
    let mut ws_before = false;
    for child in node.children_with_tokens() {
        let rowan::NodeOrToken::Node(child_node) = child else {
            ws_before = child.kind() == SyntaxKind::WHITESPACE;
            continue;
        };
        let ws_before_this = std::mem::take(&mut ws_before);
        match child_node.kind() {
            SyntaxKind::TEXT => {
                let text = child_node.text().to_string();
                if !text.is_empty() {
                    parts.push(ContentPart::Text(text));
                }
            }
            SyntaxKind::GLUE_NODE => push_glue(&mut parts, ws_before_this),
            SyntaxKind::ESCAPE => {
                let text = child_node.text().to_string();
                if text.len() > 1 {
                    parts.push(ContentPart::Text(text[1..].to_string()));
                }
            }
            SyntaxKind::INLINE_LOGIC => {
                if let Some(inline) = ast::InlineLogic::cast(child_node) {
                    lower_inline_logic_into_parts(&inline, &mut parts, scope, sink);
                }
            }
            // DIVERT_NODE/TAGS are handled elsewhere; ERROR nodes appear on
            // malformed input (already diagnosed by the parser) — skip rather
            // than panic.
            SyntaxKind::DIVERT_NODE | SyntaxKind::TAGS | SyntaxKind::ERROR => {}
            other => {
                debug_assert!(
                    other.is_token(),
                    "unexpected node SyntaxKind in lower_content_node_children: {other:?}"
                );
            }
        }
    }
    parts
}

/// Push a `Glue` part, preceded by a `Spring` when whitespace separated it
/// from an inline construct (issue #3507).
///
/// `{0} <>` then `world` prints `0 world` in ink and printed `0world` in
/// brink. The lexer folds a space that follows TEXT into the TEXT token
/// (`hello <>` always kept its space), but after an inline construct's `}`
/// the space is a WHITESPACE trivia token the parser skips before
/// `GLUE_NODE`, and content lowering only ever looked at node children — so
/// nothing downstream saw it. ink keeps exactly one space there: its runtime
/// collapses the run and trims it at end of output. `ContentPart::Spring` is
/// that conditional space (`OutputBuffer` emits it once, never doubled,
/// never trailing), the same marker `choice::replace_trailing_ws_with_spring`
/// mints for a choice's start text — and not a `Text(" ")`, which would leak
/// whitespace-only text into line recognition and the line tables.
///
/// Only an inline construct earns the spring: after TEXT the space is
/// already in the text, after another `Glue` there is nothing to separate.
pub(super) fn push_glue(parts: &mut Vec<ContentPart>, ws_before_glue: bool) {
    if ws_before_glue && is_inline_construct(parts.last()) {
        parts.push(ContentPart::Spring);
    }
    parts.push(ContentPart::Glue);
}

/// An interpolation, inline conditional/sequence, or markup span — a part
/// whose rendered text is not known at lowering time, so the whitespace
/// after it cannot be folded into text and needs a `Spring` (issue #3507).
pub(super) fn is_inline_construct(part: Option<&ContentPart>) -> bool {
    matches!(
        part,
        Some(
            ContentPart::Interpolation(_)
                | ContentPart::InlineConditional(_)
                | ContentPart::InlineSequence(_)
                | ContentPart::Span(_)
        )
    )
}

/// Lower optional tags into a `Vec<Tag>`.
///
/// Directive tags (`#@…`) are never valid in inline/attached tag
/// positions — they diagnose `E045` and are dropped, preserving the
/// erasure guarantee (a directive never reaches runtime tag output).
pub fn lower_tags(
    tags: Option<ast::Tags>,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Vec<Tag> {
    tags.map_or_else(Vec::new, |t| {
        t.tags()
            .filter_map(|tag| {
                if let Some(d) = parse_directive_tag(&tag) {
                    sink.diagnose(d.range, DiagnosticCode::E045);
                    None
                } else {
                    Some(lower_tag(&tag, scope, sink))
                }
            })
            .collect()
    })
}

pub(super) fn lower_tag(tag: &ast::Tag, scope: &LowerScope, sink: &mut impl LowerSink) -> Tag {
    use brink_syntax::SyntaxKind::HASH;

    let mut parts = Vec::new();
    let mut text_buf = String::new();
    let mut first = true;

    for child in tag.syntax().children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) => {
                if first && tok.kind() == HASH {
                    first = false;
                    continue;
                }
                first = false;
                text_buf.push_str(tok.text());
            }
            rowan::NodeOrToken::Node(node) => {
                first = false;
                if node.kind() == brink_syntax::SyntaxKind::INLINE_LOGIC {
                    if !text_buf.is_empty() {
                        parts.push(ContentPart::Text(std::mem::take(&mut text_buf)));
                    }
                    if let Some(inline) = ast::InlineLogic::cast(node) {
                        lower_inline_logic_into_parts(&inline, &mut parts, scope, sink);
                    }
                }
            }
        }
    }
    let remaining = text_buf.trim_end().to_string();
    if !remaining.is_empty() {
        parts.push(ContentPart::Text(remaining));
    }
    if let Some(ContentPart::Text(t)) = parts.first_mut() {
        *t = t.trim_start().to_string();
        if t.is_empty() {
            parts.remove(0);
        }
    }

    Tag {
        parts,
        ptr: scope.prov(NodeClass::Tag, tag.syntax()),
    }
}
