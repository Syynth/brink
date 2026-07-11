//! Recognition of compiler directives riding the tag channel (`#@…`).
//!
//! Any tag whose text begins with `@` is a brink compiler directive —
//! static text, consumed at compile time and erased from runtime tag
//! output. See `docs/directive-annotations-spec.md` for the channel
//! rules and the v1 placements:
//!
//! - a directive line immediately above a `VAR` declaration attaches to
//!   that declaration;
//! - a directive line in the leading tag-line run of a knot/stitch body
//!   attaches to that knot/stitch.
//!
//! Recognition is split between two layers that must stay in agreement:
//! the *owners* (`decl::var`, `structure::knot`, `structure::stitch`)
//! call [`directives_before`] / [`leading_body_directives`] +
//! [`apply_scope_directives`] to read and validate their directives, and
//! the tag-line lowering chokepoint (`content::tag_line`) uses
//! [`scan_tag_line`] + [`is_consumed_position`] to guarantee erasure —
//! a directive line never lowers to content, and one outside every
//! recognized placement is a hard error (`E045`), never a silent tag.

use brink_syntax::ast::{self, AstNode};
use brink_syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

use super::context::LowerSink;
use crate::DiagnosticCode;

/// A parsed `@…` directive from a single tag.
#[derive(Debug, Clone)]
pub(super) struct ParsedDirective {
    /// Name after the `@`, up to the first `(` or whitespace.
    pub name: String,
    /// `true` when the tag is exactly `@name` — no arguments, no
    /// trailing text. `@local` requires this.
    pub bare: bool,
    /// The tag contains inline `{…}` logic (directives must be static).
    pub dynamic: bool,
    /// Range of the whole tag, for diagnostics.
    pub range: TextRange,
}

/// Classification of one `TAG_LINE`.
pub(super) enum TagLineClass {
    /// No directive tags — an ordinary runtime tag line.
    Plain,
    /// Every tag on the line is a directive (v1 also requires exactly
    /// one, enforced by [`scan_tag_line`] via `Mixed`).
    Directives(Vec<ParsedDirective>),
    /// Directive and plain tags share the line (or several directives
    /// do) — invalid (`E047`); the plain tags survive, directives drop.
    Mixed,
}

/// Parse a single tag as a directive, if its text starts with `@`.
///
/// The leading literal text is the concatenation of the tag's tokens
/// after the `#`; inline-logic child nodes mark the tag `dynamic`.
pub(super) fn parse_directive_tag(tag: &ast::Tag) -> Option<ParsedDirective> {
    let mut text = String::new();
    let mut dynamic = false;
    let mut first = true;
    for child in tag.syntax().children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) => {
                if first && tok.kind() == SyntaxKind::HASH {
                    first = false;
                    continue;
                }
                first = false;
                text.push_str(tok.text());
            }
            rowan::NodeOrToken::Node(_) => {
                first = false;
                dynamic = true;
            }
        }
    }
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix('@')?;
    let name: String = rest
        .chars()
        .take_while(|c| *c != '(' && !c.is_whitespace())
        .collect();
    let bare = !dynamic && rest.len() == name.len();
    Some(ParsedDirective {
        name,
        bare,
        dynamic,
        range: tag.syntax().text_range(),
    })
}

/// Classify a `TAG_LINE` per the directive-channel rules.
pub(super) fn scan_tag_line(tl: &ast::TagLine) -> TagLineClass {
    let mut directives = Vec::new();
    let mut plain = 0usize;
    if let Some(tags) = tl.tags() {
        for tag in tags.tags() {
            match parse_directive_tag(&tag) {
                Some(d) => directives.push(d),
                None => plain += 1,
            }
        }
    }
    if directives.is_empty() {
        TagLineClass::Plain
    } else if plain == 0 && directives.len() == 1 {
        TagLineClass::Directives(directives)
    } else {
        TagLineClass::Mixed
    }
}

fn is_trivia(kind: SyntaxKind) -> bool {
    // `SyntaxKind::is_trivia` covers whitespace and comments; blank lines
    // (NEWLINE tokens) also don't break directive attachment.
    kind.is_trivia() || kind == SyntaxKind::NEWLINE
}

fn is_attachable_decl(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::VAR_DECL
            | SyntaxKind::CONST_DECL
            | SyntaxKind::LIST_DECL
            | SyntaxKind::EXTERNAL_DECL
    )
}

/// Is this node a `TAG_LINE` classified as a pure directive line?
fn is_directive_line(node: &SyntaxNode) -> bool {
    ast::TagLine::cast(node.clone())
        .is_some_and(|tl| matches!(scan_tag_line(&tl), TagLineClass::Directives(_)))
}

/// The declaration this directive line attaches to, if any: the next
/// significant sibling (skipping trivia and further directive lines) is
/// a `VAR`/`CONST`/`LIST`/`EXTERNAL` declaration.
pub(super) fn attached_declaration(tl: &ast::TagLine) -> Option<SyntaxNode> {
    let mut cursor = tl.syntax().next_sibling_or_token();
    while let Some(el) = cursor {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !is_trivia(tok.kind()) {
                    return None;
                }
                cursor = tok.next_sibling_or_token();
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::EMPTY_LINE || is_directive_line(&node) {
                    cursor = node.next_sibling_or_token();
                    continue;
                }
                return is_attachable_decl(node.kind()).then_some(node);
            }
        }
    }
    None
}

/// Is this tag line inside the leading tag-line run of a knot/stitch
/// body? (Everything before it in the body is trivia or tag lines.)
pub(super) fn in_leading_body_run(tl: &ast::TagLine) -> bool {
    let Some(parent) = tl.syntax().parent() else {
        return false;
    };
    if !matches!(
        parent.kind(),
        SyntaxKind::KNOT_BODY | SyntaxKind::STITCH_BODY
    ) {
        return false;
    }
    let mut cursor = tl.syntax().prev_sibling_or_token();
    while let Some(el) = cursor {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !is_trivia(tok.kind()) {
                    return false;
                }
                cursor = tok.prev_sibling_or_token();
            }
            rowan::NodeOrToken::Node(node) => {
                if !matches!(node.kind(), SyntaxKind::TAG_LINE | SyntaxKind::EMPTY_LINE) {
                    return false;
                }
                cursor = node.prev_sibling_or_token();
            }
        }
    }
    true
}

/// Is this directive line in a position that an owner consumes (a
/// declaration lookback or a knot/stitch leading run)? Used by the
/// tag-line chokepoint to decide between silent erasure (an owner
/// reports any problems) and `E045`.
pub(super) fn is_consumed_position(tl: &ast::TagLine) -> bool {
    attached_declaration(tl).is_some() || in_leading_body_run(tl)
}

/// Collect the directives from the directive lines immediately
/// preceding a declaration node (consecutive, skipping trivia), in
/// source order.
pub(super) fn directives_before(node: &SyntaxNode) -> Vec<ParsedDirective> {
    let mut collected: Vec<ParsedDirective> = Vec::new();
    let mut cursor = node.prev_sibling_or_token();
    while let Some(el) = cursor {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !is_trivia(tok.kind()) {
                    break;
                }
                cursor = tok.prev_sibling_or_token();
            }
            rowan::NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::EMPTY_LINE {
                    cursor = n.prev_sibling_or_token();
                    continue;
                }
                let Some(tl) = ast::TagLine::cast(n.clone()) else {
                    break;
                };
                let TagLineClass::Directives(dirs) = scan_tag_line(&tl) else {
                    break;
                };
                // Walking backwards — prepend to keep source order.
                for d in dirs.into_iter().rev() {
                    collected.insert(0, d);
                }
                cursor = n.prev_sibling_or_token();
            }
        }
    }
    collected
}

/// Collect the directives in the leading tag-line run of a knot/stitch
/// body that do *not* attach to a following declaration (those belong
/// to the declaration's own lookback).
pub(super) fn leading_body_directives(body: &SyntaxNode) -> Vec<ParsedDirective> {
    let mut collected = Vec::new();
    for el in body.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !is_trivia(tok.kind()) {
                    break;
                }
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::EMPTY_LINE {
                    continue;
                }
                let Some(tl) = ast::TagLine::cast(node) else {
                    break;
                };
                if let TagLineClass::Directives(dirs) = scan_tag_line(&tl)
                    && attached_declaration(&tl).is_none()
                {
                    collected.extend(dirs);
                }
                // Plain and mixed tag lines keep the run going; mixed
                // lines error at the tag-line chokepoint.
            }
        }
    }
    collected
}

/// What a directive is attached to, for validity checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectiveTarget {
    Var,
    Const,
    List,
    External,
    Knot,
    Stitch,
}

impl DirectiveTarget {
    fn supports_local(self) -> bool {
        matches!(self, Self::Var | Self::Knot | Self::Stitch)
    }
}

/// Interpret collected directives for one target. Returns whether the
/// target is marked `@local`; every invalid combination diagnoses.
pub(super) fn apply_scope_directives(
    dirs: &[ParsedDirective],
    target: DirectiveTarget,
    sink: &mut impl LowerSink,
) -> bool {
    let mut is_local = false;
    for d in dirs {
        if d.dynamic {
            sink.diagnose(d.range, DiagnosticCode::E046);
        } else if d.name != "local" {
            sink.diagnose(d.range, DiagnosticCode::E044);
        } else if !d.bare {
            sink.diagnose(d.range, DiagnosticCode::E050);
        } else if !target.supports_local() {
            sink.diagnose(d.range, DiagnosticCode::E049);
        } else if is_local {
            sink.diagnose(d.range, DiagnosticCode::E048);
        } else {
            is_local = true;
        }
    }
    is_local
}
