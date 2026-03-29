use brink_syntax::ast::{self, AstNode};

use crate::{ContentPart, Tag};

use super::super::conditional::lower_inline_logic_into_parts;
use super::super::context::{LowerScope, LowerSink};

/// Lower the inline content children of a syntax node (`TEXT`, `GLUE`, `ESCAPE`,
/// `INLINE_LOGIC`) into a `Vec` of `ContentPart`s.
pub fn lower_content_node_children(
    node: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Vec<ContentPart> {
    use brink_syntax::SyntaxKind;

    let mut parts = Vec::new();
    for child in node.children_with_tokens() {
        if let rowan::NodeOrToken::Node(child_node) = child {
            match child_node.kind() {
                SyntaxKind::TEXT => {
                    let text = child_node.text().to_string();
                    if !text.is_empty() {
                        parts.push(ContentPart::Text(text));
                    }
                }
                SyntaxKind::GLUE_NODE => parts.push(ContentPart::Glue),
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
                SyntaxKind::DIVERT_NODE | SyntaxKind::TAGS => {}
                other => {
                    debug_assert!(
                        other.is_token(),
                        "unexpected node SyntaxKind in lower_content_node_children: {other:?}"
                    );
                }
            }
        }
    }
    parts
}

/// Lower optional tags into a `Vec<Tag>`.
pub fn lower_tags(
    tags: Option<ast::Tags>,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Vec<Tag> {
    tags.map_or_else(Vec::new, |t| {
        t.tags().map(|tag| lower_tag(&tag, scope, sink)).collect()
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
        ptr: ast::AstPtr::new(tag),
    }
}
