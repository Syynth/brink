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
    /// The trimmed text between a balanced `(…)` argument, if the tag has
    /// one (e.g. `@was(old_name)` → `Some("old_name")`). `None` for a bare
    /// directive or one with an unbalanced/missing paren pair. Not
    /// validated for emptiness here — callers that need a non-empty
    /// argument (`@module`, `@was`) check that themselves.
    pub arg: Option<String>,
    /// Range of the whole tag, for diagnostics.
    pub range: TextRange,
    /// `true` when this directive was spelled as an `@[name(args)]`
    /// annotation line (NS-A2) rather than a `#@name(args)` tag-channel
    /// directive. The `#@effects` spelling is a deprecation alias (it
    /// shipped in released surface — `@brink-lang/web@0.11.1`), so the
    /// effects recognizer warns (`E110`) on the tag spelling and the
    /// annotation channel restricts its name set (`E111`).
    pub from_annotation: bool,
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
    // Balanced `(…)` argument text, trimmed — mirrors `module_directive_name`'s
    // extraction so every directive (not just `@module`) can carry one.
    let after_name = rest.trim_start_matches(|c: char| c != '(');
    let arg = after_name
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .map(|s| s.trim().to_string());
    Some(ParsedDirective {
        name,
        bare,
        dynamic,
        arg,
        range: tag.syntax().text_range(),
        from_annotation: false,
    })
}

/// Parse an `@[name(args)]` annotation line (NS-A2, the `@[effects(…)]`
/// assertion final form) into the same [`ParsedDirective`] shape the tag
/// channel produces, so both spellings share one recognizer downstream.
/// `None` when the parser recovered so hard no name token exists (the parse
/// error is already reported).
pub(super) fn parse_annotation_line(al: &ast::AnnotationLine) -> Option<ParsedDirective> {
    let name = al.name_token()?.text().to_string();
    let arg = al.arg_text();
    Some(ParsedDirective {
        bare: arg.is_none(),
        dynamic: false,
        arg,
        name,
        range: al.syntax().text_range(),
        from_annotation: true,
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
                if !matches!(
                    node.kind(),
                    SyntaxKind::TAG_LINE | SyntaxKind::EMPTY_LINE | SyntaxKind::ANNOTATION_LINE
                ) {
                    return false;
                }
                cursor = node.prev_sibling_or_token();
            }
        }
    }
    true
}

/// Is this `@[name(args)]` annotation line inside the leading run of a
/// knot/stitch body (NS-A2)? Mirrors [`in_leading_body_run`]'s rule for tag
/// lines: everything before it in a `KNOT_BODY`/`STITCH_BODY` is trivia,
/// empty lines, tag lines, or further annotation lines. This is the only
/// consumed placement v1 — anywhere else the annotation chokepoint
/// diagnoses `E112` ([`handle_annotation_line`]).
pub(super) fn in_leading_annotation_run(al: &ast::AnnotationLine) -> bool {
    let Some(parent) = al.syntax().parent() else {
        return false;
    };
    if !matches!(
        parent.kind(),
        SyntaxKind::KNOT_BODY | SyntaxKind::STITCH_BODY
    ) {
        return false;
    }
    let mut cursor = al.syntax().prev_sibling_or_token();
    while let Some(el) = cursor {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !is_trivia(tok.kind()) {
                    return false;
                }
                cursor = tok.prev_sibling_or_token();
            }
            rowan::NodeOrToken::Node(node) => {
                if !matches!(
                    node.kind(),
                    SyntaxKind::TAG_LINE | SyntaxKind::EMPTY_LINE | SyntaxKind::ANNOTATION_LINE
                ) {
                    return false;
                }
                cursor = node.prev_sibling_or_token();
            }
        }
    }
    true
}

/// The annotation-line chokepoint (NS-A2) — the `@[…]` analogue of the
/// tag-line chokepoint's `is_consumed_position` rule: an annotation line in
/// the leading run of a knot/stitch body is consumed by that owner
/// ([`leading_body_directives`]) and erased here; anywhere else is a
/// misplacement (`E112`), never a silent drop and never content.
pub(super) fn handle_annotation_line(al: &ast::AnnotationLine, sink: &mut impl LowerSink) {
    if !in_leading_annotation_run(al) {
        sink.diagnose(al.syntax().text_range(), DiagnosticCode::E112);
    }
}

/// Is this directive line in a position that an owner consumes (a
/// declaration lookback, a knot/stitch leading run, or a file-level
/// `#@module` at the top of the file)? Used by the tag-line chokepoint
/// to decide between silent erasure (an owner reports any problems) and
/// `E045`.
pub(super) fn is_consumed_position(tl: &ast::TagLine) -> bool {
    attached_declaration(tl).is_some()
        || in_leading_body_run(tl)
        || is_file_module_line(tl)
        || is_file_was_line(tl)
}

/// The single directive on a pure-directive line, if the line carries
/// exactly one.
fn sole_directive(tl: &ast::TagLine) -> Option<ParsedDirective> {
    match scan_tag_line(tl) {
        TagLineClass::Directives(mut dirs) if dirs.len() == 1 => dirs.pop(),
        _ => None,
    }
}

/// Is this tag line a file-level `#@module(…)` directive — a pure
/// directive line whose single directive is `@module`, sitting in the
/// leading run at the top of the file (only trivia / tag lines / empty
/// lines precede it under the `SOURCE_FILE` root)?
///
/// This is the one file-level directive placement M-1 recognizes
/// (docs/modules-spec.md §1); every other file-level directive stays an
/// `E045` misplacement error, per the reserved-`@`-namespace rule.
pub(super) fn is_file_module_line(tl: &ast::TagLine) -> bool {
    is_file_leading_line(tl, "module")
}

/// Is this tag line a file-level `#@was(…)` directive — the same
/// leading-run placement as [`is_file_module_line`], naming the module's
/// old name (M-3, `docs/modules-spec.md` §5). Works whether or not the
/// file also carries `#@module`: an undeclared stem-module's `#@was` would
/// have no declared-module identity to alias against, so
/// [`super::structure::lower_file`]'s consumer only acts on it when
/// `#@module` is also present — but the placement itself is recognized
/// either way, so a stray `#@was` with no `#@module` gets the *targeted*
/// M-3 diagnostic rather than a generic `E045` misplacement.
pub(super) fn is_file_was_line(tl: &ast::TagLine) -> bool {
    is_file_leading_line(tl, "was")
}

/// Shared placement check for a file-level directive: a pure directive
/// line named `name`, sitting in the leading run at the top of the file
/// (only trivia / tag lines / empty lines precede it under the
/// `SOURCE_FILE` root).
fn is_file_leading_line(tl: &ast::TagLine, name: &str) -> bool {
    let Some(dir) = sole_directive(tl) else {
        return false;
    };
    if dir.name != name {
        return false;
    }
    let Some(parent) = tl.syntax().parent() else {
        return false;
    };
    if parent.kind() != SyntaxKind::SOURCE_FILE {
        return false;
    }
    // Nothing but trivia / tag lines / empty lines before it.
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

/// Collect every `#@private` / `#@public` directive occurrence in the file,
/// for the dialect gate (M-2, docs/modules-spec.md §4). A flat pass over all
/// directive tag lines — attachment to a specific declaration is handled
/// separately via the declaration owners (`DeclaredSymbol::visibility`); the
/// gate only needs each occurrence's range and requested visibility.
pub(super) fn collect_visibility_directives(
    source_file: &SyntaxNode,
) -> Vec<crate::VisibilityDirective> {
    use crate::VisibilityMark;
    let mut out = Vec::new();
    for node in source_file.descendants() {
        let Some(tl) = ast::TagLine::cast(node) else {
            continue;
        };
        let Some(tags) = tl.tags() else {
            continue;
        };
        for tag in tags.tags() {
            if let Some(dir) = parse_directive_tag(&tag) {
                let mark = match dir.name.as_str() {
                    "private" => VisibilityMark::Private,
                    "public" => VisibilityMark::Public,
                    _ => continue,
                };
                out.push(crate::VisibilityDirective {
                    mark,
                    range: dir.range,
                });
            }
        }
    }
    out
}

/// Collect every `#@was(…)` directive tag's range in the file, module-level
/// and definition-level alike (M-3, `docs/modules-spec.md` §5), for the
/// dialect gate. Mirrors [`collect_visibility_directives`]: a flat pass,
/// independent of the attachment/validation walk owners run separately.
pub(super) fn collect_was_directives(source_file: &SyntaxNode) -> Vec<TextRange> {
    let mut out = Vec::new();
    for node in source_file.descendants() {
        let Some(tl) = ast::TagLine::cast(node) else {
            continue;
        };
        let Some(tags) = tl.tags() else {
            continue;
        };
        for tag in tags.tags() {
            if let Some(dir) = parse_directive_tag(&tag)
                && dir.name == "was"
            {
                out.push(dir.range);
            }
        }
    }
    out
}

/// Extract and validate the file's `#@module(name)` declaration, if any.
///
/// Scans direct children of the `SOURCE_FILE` root for file-level
/// `#@module` directive lines (via [`is_file_module_line`]) and returns
/// the first valid `(name, range)`. Malformed forms diagnose:
///
/// - a dynamic (`#@module({expr})`) argument → `E046`;
/// - a missing / empty name argument (`#@module`, `#@module()`) → `E086`;
/// - a second `#@module` anywhere in the file → `E086`.
///
/// The name is the balanced text between the parentheses, trimmed. The
/// brink-dialect gate (`#@module` is a brink extension) is applied later
/// by the analyzer over the returned range.
pub(super) fn file_module_declaration(
    source_file: &SyntaxNode,
    sink: &mut impl LowerSink,
) -> Option<(String, TextRange)> {
    let mut found: Option<(String, TextRange)> = None;
    for child in source_file.children() {
        let Some(tl) = ast::TagLine::cast(child) else {
            continue;
        };
        if !is_file_module_line(&tl) {
            continue;
        }
        let Some(dir) = sole_directive(&tl) else {
            continue;
        };
        if dir.dynamic {
            sink.diagnose(dir.range, DiagnosticCode::E046);
            continue;
        }
        let Some(name) = module_directive_name(&tl) else {
            sink.diagnose(dir.range, DiagnosticCode::E086);
            continue;
        };
        if found.is_some() {
            // A second `#@module` in the file — duplicate declaration.
            sink.diagnose(dir.range, DiagnosticCode::E086);
            continue;
        }
        found = Some((name, dir.range));
    }
    found
}

/// Extract and validate the file's `#@was(old_name)` declaration, if any
/// (M-3, `docs/modules-spec.md` §5) — the module-level counterpart of
/// [`file_module_declaration`], same placement, same malformed-form
/// diagnostics (`E046` dynamic, `E094` missing/empty argument, `E048`
/// second occurrence — `E048` rather than a dedicated code because it's
/// the same "repeated directive" shape `apply_scope_directives` already
/// uses for `#@local`).
/// The third tuple element reports whether the claimed `#@was` line ALSO
/// attaches to a following `VAR`/`CONST`/`LIST`/`EXTERNAL` declaration
/// (`attached_declaration`) — the placement is ambiguous at the top of a
/// file whose first declaration directly follows the leading run, and the
/// declaration's own `directives_before` lookback claims the same line.
/// The module arbitration uses it to withhold the orphan diagnostic
/// (`E049`) for a decl-owned `#@was`: the CLI/editor rename flow (#1672)
/// stamps `#@was(old)` directly above a renamed declaration, which on a
/// line-1 declaration is byte-identical to the file-level placement.
pub(super) fn file_module_was(
    source_file: &SyntaxNode,
    sink: &mut impl LowerSink,
) -> Option<(String, TextRange, bool)> {
    let mut found: Option<(String, TextRange, bool)> = None;
    for child in source_file.children() {
        let Some(tl) = ast::TagLine::cast(child) else {
            continue;
        };
        if !is_file_was_line(&tl) {
            continue;
        }
        let Some(dir) = sole_directive(&tl) else {
            continue;
        };
        if dir.dynamic {
            sink.diagnose(dir.range, DiagnosticCode::E046);
            continue;
        }
        let Some(name) = dir.arg.as_ref().filter(|n| !n.is_empty()) else {
            sink.diagnose(dir.range, DiagnosticCode::E094);
            continue;
        };
        if found.is_some() {
            sink.diagnose(dir.range, DiagnosticCode::E048);
            continue;
        }
        found = Some((name.clone(), dir.range, attached_declaration(&tl).is_some()));
    }
    found
}

/// Parse the balanced text argument of a `@module(name)` directive tag,
/// returning the trimmed name if it is non-empty.
fn module_directive_name(tl: &ast::TagLine) -> Option<String> {
    let tags = tl.tags()?;
    let tag = tags.tags().next()?;
    // Reconstruct the tag's literal text (tokens after the leading `#`).
    let mut text = String::new();
    let mut first = true;
    for child in tag.syntax().children_with_tokens() {
        if let rowan::NodeOrToken::Token(tok) = child {
            if first && tok.kind() == SyntaxKind::HASH {
                first = false;
                continue;
            }
            first = false;
            text.push_str(tok.text());
        } else {
            first = false;
        }
    }
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix('@')?;
    let after_name = rest.trim_start_matches(|c: char| c != '(');
    let inner = after_name.strip_prefix('(')?.strip_suffix(')')?;
    let name = inner.trim();
    (!name.is_empty()).then(|| name.to_string())
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
                // NS-A2: `@[name(args)]` annotation lines join the leading
                // run and collect alongside tag-channel directives.
                if let Some(al) = ast::AnnotationLine::cast(node.clone()) {
                    collected.extend(parse_annotation_line(&al));
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
        if d.from_annotation && d.name != "effects" {
            // NS-A2: the `@[…]` annotation channel recognizes only
            // `effects` v1 — the tag-channel directive names (`local`,
            // `private`, `was`, `module`, …) do not alias into it.
            sink.diagnose(d.range, DiagnosticCode::E111);
        } else if d.name == "private" || d.name == "public" {
            // Visibility directives (M-2, modules-spec §4) — handled by
            // [`visibility_from_directives`]; not an unknown-directive error
            // here.
        } else if d.name == "was" {
            // Rename directive (M-3, modules-spec §5) — handled by
            // [`was_from_directives`]; not an unknown-directive error here.
        } else if d.name == "effects" {
            // T2-2 (docs/effects-spec.md §10, issue #861): the effects
            // assertion only attaches at the knot/stitch top-of-body
            // placement — parsed by `effects_assertion_from_directives` at
            // that call site. A `#@effects` collected via a declaration's
            // `directives_before` lookback (VAR/CONST/LIST/EXTERNAL) is an
            // invalid target, since only `directives_before` owners reach
            // this arm.
            if !matches!(target, DirectiveTarget::Knot | DirectiveTarget::Stitch) {
                sink.diagnose(d.range, DiagnosticCode::E049);
            }
        } else if d.name == "module" {
            // File-level module declaration (M-1, modules-spec §1) — handled
            // by [`file_module_declaration`]'s own top-of-file scan. Named
            // here only because `directives_before` collects by *position*
            // (consecutive directive lines immediately above a
            // declaration), not by meaning: `#@module(name)` sitting
            // directly above the file's first VAR/CONST/LIST/EXTERNAL (no
            // blank line between them — an entirely ordinary authoring
            // style) is *also* in that declaration's lookback run, so this
            // owner would otherwise misfire `E044` on a directive that
            // already validated cleanly at the file level.
        } else if d.name != "local" {
            // Unknown directive — emit E046 for dynamic content, E044 otherwise.
            if d.dynamic {
                sink.diagnose(d.range, DiagnosticCode::E046);
            } else {
                sink.diagnose(d.range, DiagnosticCode::E044);
            }
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

/// Interpret the `#@was(old_name)` rename directive(s) among a
/// declaration's collected directives (M-3, `docs/modules-spec.md` §5).
///
/// Returns the old name and the directive's range, or `None` when no
/// `#@was` is present. `dynamic` is `E046`; a missing/empty argument is
/// `E094`; a second `#@was` on the same target is `E048` (the same
/// duplicate-directive code `apply_scope_directives` uses for a repeated
/// `#@local`) — the first valid occurrence wins. The self-alias check
/// (`old_name` equals the target's own current name, `E095`) is the
/// caller's job: this function doesn't know what it's attached to.
pub(super) fn was_from_directives(
    dirs: &[ParsedDirective],
    sink: &mut impl LowerSink,
) -> Option<(String, TextRange)> {
    let mut chosen: Option<(String, TextRange)> = None;
    for d in dirs {
        if d.name != "was" {
            continue;
        }
        if d.dynamic {
            sink.diagnose(d.range, DiagnosticCode::E046);
            continue;
        }
        let Some(name) = d.arg.as_ref().filter(|n| !n.is_empty()) else {
            sink.diagnose(d.range, DiagnosticCode::E094);
            continue;
        };
        if chosen.is_some() {
            sink.diagnose(d.range, DiagnosticCode::E048);
            continue;
        }
        chosen = Some((name.clone(), d.range));
    }
    chosen
}

/// Interpret the `#@effects(…)` assertion directive among a knot/stitch's
/// collected directives (T2-2, docs/effects-spec.md §10, sitting 2 —
/// 2026-07-14 — issue #861). `#@effects(pure)` is sugar for the empty row;
/// otherwise the argument is a comma-separated list of `reads:`/`writes:`/
/// `calls:` clauses, each naming zero or more identifiers — a bare value
/// with no `key:` prefix continues the most recently opened clause, so
/// `reads: a, b` is one clause naming two cells, not two clauses.
///
/// Returns `None` when no `#@effects` directive is present (or every
/// occurrence was malformed and already diagnosed). Malformed forms
/// diagnose: dynamic content is `E046` (the generic directive-content
/// code); a missing/empty argument (bare `#@effects`, `#@effects()`,
/// `#@effects(  )`) is `E100`; an unrecognized clause keyword, a
/// non-identifier value, or a bare value with no clause open yet is `E101`;
/// a second `#@effects` on the same target is `E048` (the same
/// duplicate-directive code `apply_scope_directives` uses for a repeated
/// `#@local`) — the first valid occurrence wins. Resolving the declared
/// names against the project symbol index, and the exceedance check
/// itself, are `brink-analyzer`'s job — this function only parses the
/// directive's own grammar.
pub(super) fn effects_assertion_from_directives(
    dirs: &[ParsedDirective],
    sink: &mut impl LowerSink,
) -> Option<crate::EffectsAssertion> {
    let mut chosen: Option<crate::EffectsAssertion> = None;
    for d in dirs {
        if d.name != "effects" {
            continue;
        }
        if !d.from_annotation {
            // NS-A2 (docs/stdlib-spec.md §9.2): `@[effects(…)]` is the
            // assertion final form; the `#@effects(…)` tag spelling stays a
            // deprecation alias (it shipped in `@brink-lang/web@0.11.1`)
            // and warns. Parsing continues unchanged either way.
            sink.diagnose(d.range, DiagnosticCode::E110);
        }
        if d.dynamic {
            sink.diagnose(d.range, DiagnosticCode::E046);
            continue;
        }
        let raw = if d.bare { None } else { d.arg.as_deref() };
        let Some(raw) = raw else {
            sink.diagnose(d.range, DiagnosticCode::E100);
            continue;
        };
        let trimmed = raw.trim();
        let parsed = if trimmed.is_empty() {
            sink.diagnose(d.range, DiagnosticCode::E100);
            None
        } else if d.from_annotation {
            // NS-A2 clause-grammar amendment (2026-07-19, stdlib-spec §9.2,
            // issue #1120): the annotation spelling uses the Rust-meta-item
            // paren clause grammar; the deprecated tag spelling below keeps
            // the legacy colon grammar FROZEN.
            parse_effects_paren_clauses(trimmed, d.range, sink)
        } else {
            parse_effects_clauses(trimmed, d.range, sink)
        };
        let Some(parsed) = parsed else {
            continue; // already diagnosed
        };
        if chosen.is_some() {
            sink.diagnose(d.range, DiagnosticCode::E048);
            continue;
        }
        chosen = Some(parsed);
    }
    chosen
}

/// Which clause a bare (no `key:` prefix) value continues.
#[derive(Clone, Copy)]
enum EffectsClauseKind {
    Reads,
    Writes,
    Calls,
}

/// Parse the `@[effects(…)]` annotation argument mini-grammar — the
/// **Rust-meta-item paren shape** (clause grammar AMENDED 2026-07-19,
/// stdlib-spec §9.2 / decision-log "A2 judgment calls confirmed", issue
/// #1120's second item): a top-level comma-joined sequence where
///
/// - every **bare identifier** at top level is a FLAG — exactly
///   `pure`/`silent`/`total`; any other bare ident is malformed (`E101`);
/// - every **clause** is a parenthesized meta item — `reads(gold, hp)`,
///   `writes(mood)`, `calls(sfx)` — naming zero or more identifiers.
///
/// Because clauses are delimited, a flag can never be swallowed into an
/// open clause — the footgun the respell kills structurally: under the old
/// colon grammar `reads: gold, silent` read `silent` as a *cell named
/// silent* continuing the `reads` clause; under this grammar
/// `reads(gold), silent` parses `silent` as the flag, unambiguously. The
/// colon spelling inside an annotation is malformed (`E101`); the
/// deprecated `#@effects(…)` tag spelling keeps the legacy colon grammar
/// FROZEN ([`parse_effects_clauses`] — the E110-warned surface does not
/// evolve). Same `pure`-contradiction and empty-assertion (`E100`) rules
/// as the legacy parser.
fn parse_effects_paren_clauses(
    text: &str,
    range: TextRange,
    sink: &mut impl LowerSink,
) -> Option<crate::EffectsAssertion> {
    let mut pure = false;
    let mut silent = false;
    let mut total = false;
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut calls = Vec::new();
    let mut ok = true;

    for piece in split_top_level_commas(text) {
        let piece = piece.trim();
        if piece.is_empty() {
            // Tolerate a stray/trailing comma — matches the legacy parser.
            continue;
        }
        if let Some(open) = piece.find('(') {
            let Some(inner) = piece[open + 1..].strip_suffix(')') else {
                ok = false; // unbalanced/trailing junk after the clause
                continue;
            };
            let target = match piece[..open].trim() {
                "reads" => &mut reads,
                "writes" => &mut writes,
                "calls" => &mut calls,
                _ => {
                    ok = false; // unknown clause name (flags take no parens)
                    continue;
                }
            };
            for value in inner.split(',') {
                let value = value.trim();
                if value.is_empty() {
                    continue; // `reads()` / a trailing comma inside a clause
                }
                if is_effects_ident(value) {
                    target.push(value.to_string());
                } else {
                    ok = false;
                }
            }
        } else {
            // Bare top-level ident: ALWAYS a flag — the structural rule.
            match piece {
                "pure" => pure = true,
                "silent" => silent = true,
                "total" => total = true,
                _ => ok = false,
            }
        }
    }

    // `pure` asserts the EMPTY state row — combining it with a clause that
    // grants state atoms is contradictory, not a union (`E101`).
    if pure && !(reads.is_empty() && writes.is_empty() && calls.is_empty()) {
        ok = false;
    }
    if !ok {
        sink.diagnose(range, DiagnosticCode::E101);
        return None;
    }
    if !pure && !silent && !total && reads.is_empty() && writes.is_empty() && calls.is_empty() {
        sink.diagnose(range, DiagnosticCode::E100);
        return None;
    }
    Some(crate::EffectsAssertion {
        pure,
        silent,
        total,
        reads,
        writes,
        calls,
        range,
    })
}

/// Split `text` at commas that sit outside every `(…)` pair — the
/// top-level piece boundaries of the paren clause grammar. Parens are the
/// only nesting delimiter the grammar has; an unbalanced `)` simply stops
/// suppressing splits (the malformed piece then fails
/// [`parse_effects_paren_clauses`]'s own shape checks).
fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

/// Parse the non-`pure` `#@effects(…)` argument mini-grammar — the **legacy
/// colon shape**, FROZEN as-is for the deprecated tag-channel spelling only
/// (the E110-warned surface does not evolve; the `@[effects(…)]` annotation
/// spelling uses [`parse_effects_paren_clauses`] instead): a
/// comma-separated sequence of `reads:`/`writes:`/`calls:` clause openers,
/// each optionally followed by identifier values (more values continue as
/// bare, comma-separated tokens until the next `key:` opener). Diagnoses
/// `E101` (at the whole directive's range — the raw argument text carries no
/// per-token sub-spans) and returns `None` on any malformed piece.
fn parse_effects_clauses(
    text: &str,
    range: TextRange,
    sink: &mut impl LowerSink,
) -> Option<crate::EffectsAssertion> {
    let mut pure = false;
    let mut silent = false;
    let mut total = false;
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut calls = Vec::new();
    let mut current: Option<EffectsClauseKind> = None;
    let mut ok = true;

    let mut push = |kind: EffectsClauseKind, value: &str| -> bool {
        if !is_effects_ident(value) {
            return false;
        }
        match kind {
            EffectsClauseKind::Reads => reads.push(value.to_string()),
            EffectsClauseKind::Writes => writes.push(value.to_string()),
            EffectsClauseKind::Calls => calls.push(value.to_string()),
        }
        true
    };

    for piece in text.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            // Tolerate a stray/trailing comma — not itself a clause.
            continue;
        }
        if let Some((key, rest)) = piece.split_once(':') {
            let key = key.trim();
            let kind = match key {
                "reads" => EffectsClauseKind::Reads,
                "writes" => EffectsClauseKind::Writes,
                "calls" => EffectsClauseKind::Calls,
                _ => {
                    ok = false;
                    continue;
                }
            };
            current = Some(kind);
            let rest = rest.trim();
            if !rest.is_empty() && !push(kind, rest) {
                ok = false;
            }
        } else {
            // NS-A2 (docs/stdlib-spec.md §9.2): the assertion-flag args —
            // any subset of {pure, silent, total}, comma-joined. A bare
            // piece is a FLAG only while no clause is open; after a
            // `reads:`/`writes:`/`calls:` opener, bare pieces continue that
            // clause (so a cell genuinely named `silent` stays assertable
            // as `reads: silent`).
            if current.is_none() {
                match piece {
                    "pure" => {
                        pure = true;
                        continue;
                    }
                    "silent" => {
                        silent = true;
                        continue;
                    }
                    "total" => {
                        total = true;
                        continue;
                    }
                    _ => {}
                }
            }
            let Some(kind) = current else {
                ok = false;
                continue;
            };
            if !push(kind, piece) {
                ok = false;
            }
        }
    }

    // `pure` asserts the EMPTY state row — combining it with a clause that
    // grants state atoms is contradictory, not a union (`E101`).
    if pure && !(reads.is_empty() && writes.is_empty() && calls.is_empty()) {
        ok = false;
    }
    if !ok {
        sink.diagnose(range, DiagnosticCode::E101);
        return None;
    }
    if !pure && !silent && !total && reads.is_empty() && writes.is_empty() && calls.is_empty() {
        sink.diagnose(range, DiagnosticCode::E100);
        return None;
    }
    Some(crate::EffectsAssertion {
        pure,
        silent,
        total,
        reads,
        writes,
        calls,
        range,
    })
}

/// A plain identifier (letter/underscore start, alphanumeric/underscore
/// rest) — the only legal shape for a `#@effects` clause value. No dotted
/// paths, no qualified names: the spec's vocabulary is bare global cell
/// names and external names.
fn is_effects_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Interpret the `#@private` / `#@public` visibility directives among a
/// declaration's collected directives (M-2, docs/modules-spec.md §4).
///
/// Returns the explicit override, or `None` when neither is present (take
/// the module default). Both directives must be bare (`#@private`, no args);
/// a non-bare form is `E050`, a dynamic argument `E046`. Supplying *both*
/// `#@private` and `#@public`, or repeating one, is a conflict (`E093`) —
/// the first wins. Validity of the override against the module default
/// (redundant-override warning `E092`) is checked later by the analyzer,
/// which knows the file's declared-ness.
pub(super) fn visibility_from_directives(
    dirs: &[ParsedDirective],
    sink: &mut impl LowerSink,
) -> Option<crate::VisibilityMark> {
    use crate::VisibilityMark;
    let mut chosen: Option<VisibilityMark> = None;
    for d in dirs {
        let mark = match d.name.as_str() {
            "private" => VisibilityMark::Private,
            "public" => VisibilityMark::Public,
            _ => continue,
        };
        if d.dynamic {
            sink.diagnose(d.range, DiagnosticCode::E046);
        } else if !d.bare {
            sink.diagnose(d.range, DiagnosticCode::E050);
        } else if chosen.is_some() {
            // A second visibility directive — conflict or repeat.
            sink.diagnose(d.range, DiagnosticCode::E093);
        } else {
            chosen = Some(mark);
        }
    }
    chosen
}
