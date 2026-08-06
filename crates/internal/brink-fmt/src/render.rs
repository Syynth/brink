//! Rendering: walk the classified lines and emit the formatted source.
//!
//! This is the third and final stage of the [`crate::format`] pipeline — it
//! consumes the [`crate::classify::ClassifiedLine`] stream produced by
//! [`crate::classify`] and the caller's [`FormatConfig`].

use brink_syntax::SyntaxElement;
use brink_syntax::SyntaxNode;
use brink_syntax::syntax_kind::SyntaxKind;
use rowan::NodeOrToken;

use crate::FormatConfig;
use crate::IndentStyle;
use crate::classify::ClassifiedLine;
use crate::classify::LineKind;

fn indent_str(config: &FormatConfig, depth: u32) -> String {
    if depth == 0 {
        return String::new();
    }
    match &config.indent {
        IndentStyle::Tabs => "\t".repeat(depth as usize),
        IndentStyle::Spaces(n) => " ".repeat((depth * n) as usize),
    }
}

#[expect(clippy::too_many_lines)]
pub(crate) fn render(source: &str, lines: &[ClassifiedLine], config: &FormatConfig) -> String {
    let mut out = String::with_capacity(source.len());
    let mut consecutive_blanks: u32 = 0;

    for (i, line) in lines.iter().enumerate() {
        let raw = &source[line.start..line.end];

        match &line.kind {
            LineKind::Blank => {
                consecutive_blanks += 1;
                if consecutive_blanks <= 1 {
                    out.push('\n');
                }
                continue;
            }
            LineKind::BlockComment => {
                consecutive_blanks = 0;
                out.push_str(raw.trim_end());
                out.push('\n');
                continue;
            }
            // Already emitted as part of a preceding `LogicBlock` or
            // `LogicBlockVerbatim` span (T1b `~ { … }` block,
            // docs/t1b-surface-spec.md §2) — nothing to do, and it must not
            // reset `consecutive_blanks` or count as a line of its own.
            LineKind::Skip => continue,
            _ => {}
        }

        // Ensure blank line before knot/stitch headers.
        if matches!(line.kind, LineKind::KnotHeader | LineKind::StitchHeader)
            && i > 0
            && consecutive_blanks == 0
        {
            let prev_kind = &lines[i - 1].kind;
            if !matches!(
                prev_kind,
                LineKind::Blank | LineKind::Comment | LineKind::BlockComment
            ) {
                out.push('\n');
            }
        }

        consecutive_blanks = 0;

        match &line.kind {
            LineKind::KnotHeader => {
                out.push_str(&format_knot_header(raw));
                out.push('\n');
            }
            LineKind::StitchHeader => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(&format_stitch_header(raw));
                out.push('\n');
            }
            LineKind::Choice { depth } => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(&format_choice(raw, *depth));
                out.push('\n');
            }
            LineKind::Gather { .. } => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(&format_gather(raw));
                out.push('\n');
            }
            LineKind::Logic => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                // Retokenize through the CST when available (issue #858) so
                // a single-line `~ expr` gets the same canonical spacing as
                // multi-line `~ { … }` block statements — falling back to
                // the raw-text formatter only for the defensive case where
                // no CST node was attached (shouldn't happen for a
                // well-formed `LOGIC_LINE`, but keeps this path total).
                if let Some(node) = &line.cst_node {
                    out.push_str(&format_logic_line(node));
                } else {
                    out.push_str(&format_logic(raw));
                }
                out.push('\n');
            }
            LineKind::Content | LineKind::Tag | LineKind::Comment | LineKind::Other => {
                let indent = indent_str(config, line.depth);
                out.push_str(&indent);
                out.push_str(raw.trim());
                out.push('\n');
            }
            LineKind::Declaration => {
                // Module `IMPORT` statements (M-4, modules-spec §2/§9) get
                // canonical inner spacing; VAR/CONST/LIST declarations (#642)
                // canonicalize type annotation colons; other declarations are
                // passed through with only trailing whitespace trimmed.
                if raw.trim_start().starts_with("IMPORT") {
                    out.push_str(&format_import(raw));
                } else if raw.trim_start().starts_with("VAR")
                    || raw.trim_start().starts_with("CONST")
                    || raw.trim_start().starts_with("LIST")
                {
                    // Retokenize through the CST via `join_token_text` (#642
                    // fix) so string-literal initializers (`VAR msg = "time
                    // 12:30"`) are emitted byte-for-byte instead of having
                    // their colon/whitespace mutated by a raw-text pass.
                    //
                    // `cst_node` is guaranteed `Some` here: `classify_node`'s
                    // `VAR_DECL | CONST_DECL | LIST_DECL` arm sets `kind` and
                    // `cst_node` together, unconditionally (no parse-error
                    // check gates it, unlike the `STMT_BLOCK`/`STRUCT_DECL`
                    // arms) — the parser is error-resilient, bracketing the
                    // whole construct in its node via `start_node`/
                    // `finish_node` regardless of what's between, so even a
                    // malformed `VAR`/`CONST`/`LIST` line still produces a
                    // node. No other code path sets `LineKind::Declaration`
                    // with raw text starting `VAR`/`CONST`/`LIST` while
                    // leaving `cst_node` unset. The character-based
                    // `collapse_whitespace` fallback that used to run here
                    // when `cst_node` was `None` was therefore dead code —
                    // and unsafe dead code: a raw-text whitespace/colon pass
                    // over a span that may contain a string literal is the
                    // corruption class named in the wave-25 lesson. Removed
                    // per issue #984.
                    let Some(node) = &line.cst_node else {
                        unreachable!(
                            "classify_node always attaches a CST node to VAR/CONST/LIST \
                             Declaration lines"
                        );
                    };
                    out.push_str(&format_declaration_from_cst(node));
                } else {
                    out.push_str(raw.trim_end());
                }
                out.push('\n');
            }
            // Reindent the whole `~ { … }` block as a unit: `cst_node`
            // is the `LOGIC_LINE` CST node (see `mark_logic_block_span`);
            // `render_logic_block` walks its `STMT_BLOCK` recursively,
            // computing indentation independently of `raw`.
            LineKind::LogicBlock => {
                if let Some(node) = &line.cst_node {
                    let base_indent = indent_str(config, line.depth);
                    out.push_str(&render_logic_block(node, &base_indent));
                }
            }
            // `raw` already spans the whole node through its own trailing
            // newline (see `mark_verbatim_span`) — push it byte-for-byte,
            // with no extra trim/indent/newline. This block's subtree has a
            // parse error (#603), so its structure can't be trusted enough
            // to reindent.
            LineKind::LogicBlockVerbatim => {
                out.push_str(raw);
            }
            // Reindent the whole STRUCT declaration as a unit: `cst_node`
            // is the `STRUCT_DECL` CST node (see `mark_struct_decl_span`);
            // `render_struct_decl` walks its field declarations and reindents
            // the body with proper formatting (TM-4b, docs/typed-mode-spec.md §6).
            LineKind::StructDecl => {
                if let Some(node) = &line.cst_node {
                    let base_indent = indent_str(config, line.depth);
                    out.push_str(&render_struct_decl(node, &base_indent, config));
                }
            }
            LineKind::Blank | LineKind::BlockComment | LineKind::Skip => unreachable!(),
        }
    }

    // Ensure single trailing newline (but keep empty input empty).
    while out.ends_with("\n\n") {
        out.pop();
    }
    if out.chars().all(|c| c == '\n') && source.trim().is_empty() {
        return String::new();
    }
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }

    out
}

// ── Per-line formatters ─────────────────────────────────────────────

/// Format a module `IMPORT` statement (M-4, docs/modules-spec.md §2/§9) with
/// canonical inner spacing.
///
/// Both grammar forms normalize:
/// - bare: `IMPORT {  a , b  AS c } FROM  mod` → `IMPORT { a, b AS c } FROM mod`
/// - qualified: `IMPORT   mod` → `IMPORT mod`
///
/// A statement that does not match either shape (mid-edit / malformed) is left
/// verbatim (only surrounding whitespace trimmed) so formatting never corrupts
/// it. (A trailing `//` comment reclassifies the line away from
/// `LineKind::Declaration` upstream, so it never reaches here — such lines are
/// passed through untouched.)
fn format_import(raw: &str) -> String {
    let trimmed = raw.trim();
    normalize_import_code(trimmed).unwrap_or_else(|| trimmed.to_owned())
}

/// Normalize the code portion (comment already stripped) of an `IMPORT`
/// statement to its canonical spelling, or `None` when it does not match a
/// well-formed shape (leave it verbatim rather than risk corrupting it).
fn normalize_import_code(code: &str) -> Option<String> {
    let rest = code.strip_prefix("IMPORT")?.trim();

    if let Some(open) = rest.find('{') {
        // Bare form: `IMPORT { a, b AS c } FROM mod`.
        let close = rest.find('}')?;
        if close < open {
            return None;
        }
        let items_str = &rest[open + 1..close];
        let after = rest[close + 1..].trim();
        let module = after.strip_prefix("FROM")?.trim();
        if module.is_empty() || module.contains(char::is_whitespace) {
            return None;
        }

        let items: Vec<String> = items_str
            .split(',')
            .map(|item| collapse_whitespace(item.trim()))
            .filter(|item| !item.is_empty())
            .collect();
        if items.is_empty() {
            return None;
        }
        Some(format!("IMPORT {{ {} }} FROM {module}", items.join(", ")))
    } else {
        // Qualified form: `IMPORT mod` (a single bare module name).
        if rest.is_empty() || rest.contains(char::is_whitespace) {
            return None;
        }
        Some(format!("IMPORT {rest}"))
    }
}

/// Format a knot header: `=== name ===` or `=== function name(params) ===`
fn format_knot_header(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = trimmed.trim_start_matches('=').trim_end_matches('=').trim();

    if inner.is_empty() {
        return "===".to_owned();
    }

    let normalized: String = collapse_whitespace(inner);
    format!("=== {normalized} ===")
}

/// Format a VAR/CONST/LIST declaration by retokenizing its CST node through
/// [`join_token_text`] (#642 fix): that joiner emits token text — including
/// string-literal tokens — byte-for-byte, and only canonicalizes whitespace
/// *between* tokens, so a colon or internal whitespace inside a string value
/// (`VAR msg = "time 12:30"`, `CONST url = "http://x.com"`) is preserved
/// exactly while the declaration's own type-annotation colon still gets
/// canonical `name: type` spacing.
fn format_declaration_from_cst(node: &SyntaxNode) -> String {
    join_token_text(node.descendants_with_tokens())
}

/// Collapse runs of whitespace into single spaces, and canonicalize type
/// annotation colons (#642) to have no space before and one space after.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else if c == ':' {
            // Type annotation canonicalization (#642): no space before colon,
            // one space after. Remove any trailing space before the colon.
            while out.ends_with(' ') {
                out.pop();
            }
            out.push(':');
            out.push(' ');
            prev_ws = true; // Treat as if we just output a space.
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out
}

/// Format a stitch header: `= name`
fn format_stitch_header(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = trimmed.trim_start_matches('=').trim_end_matches('=').trim();

    if inner.is_empty() {
        return "=".to_owned();
    }

    let normalized = collapse_whitespace(inner);
    format!("= {normalized}")
}

/// Format a choice line: `{bullets} {rest}`
fn format_choice(raw: &str, depth: u32) -> String {
    let trimmed = raw.trim();

    let mut chars = trimmed.chars().peekable();
    let mut bullet_count = 0u32;
    let mut bullet_char = '*';
    while let Some(&c) = chars.peek() {
        if c == '*' || c == '+' {
            bullet_char = c;
            bullet_count += 1;
            chars.next();
        } else if c.is_whitespace() && bullet_count > 0 {
            // Peek ahead: if the next non-whitespace is another bullet, skip.
            let rest: String = chars.clone().collect();
            let next_non_ws = rest.trim_start().chars().next();
            if next_non_ws == Some('*') || next_non_ws == Some('+') {
                chars.next();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if bullet_count == 0 {
        bullet_count = depth;
    }

    let rest: String = chars.collect();
    let rest = rest.trim_start();

    let bullets: String = std::iter::repeat_n(bullet_char, bullet_count as usize).collect();

    if rest.is_empty() {
        bullets
    } else {
        format!("{bullets} {rest}")
    }
}

/// Format a gather line: `{dashes} {rest}`
fn format_gather(raw: &str) -> String {
    let trimmed = raw.trim();

    let mut chars = trimmed.chars().peekable();
    let mut dash_count = 0u32;
    while let Some(&c) = chars.peek() {
        if c == '-' {
            dash_count += 1;
            chars.next();
        } else {
            break;
        }
    }

    if dash_count == 0 {
        dash_count = 1;
    }

    let rest: String = chars.collect();
    let rest = rest.trim_start();

    let dashes: String = std::iter::repeat_n('-', dash_count as usize).collect();

    if rest.is_empty() {
        dashes
    } else {
        format!("{dashes} {rest}")
    }
}

/// Format a logic line: `~ {rest}`. Raw-text fallback for when no CST node
/// is available — the normal path is [`format_logic_line`], which
/// retokenizes through the CST instead of passing the source text through
/// unchanged.
fn format_logic(raw: &str) -> String {
    let trimmed = raw.trim();
    let rest = trimmed.strip_prefix('~').unwrap_or(trimmed).trim();

    if rest.is_empty() {
        "~".to_owned()
    } else {
        format!("~ {rest}")
    }
}

/// Format a single-line `~ expr` logic line (a `LOGIC_LINE` CST node whose
/// child is a single statement, not a `~ { … }` block — see
/// [`mark_logic_block_span`] for the block case) by retokenizing its
/// statement through [`join_token_text`], the same joiner multi-line
/// `~ { … }` block bodies use via [`render_flat_stmt_text`].
///
/// Before issue #858, single-line logic lines only had their outer `~ `
/// prefix normalized (via [`format_logic`]) — the statement body passed
/// through as raw source text, so e.g. `~   temp   name:string=who` kept its
/// internal ragged spacing (`temp   name:string=who`) instead of getting the
/// canonical single-space-around-tokens rendering a `~ { temp name: string
/// = who }` block form already received. This closes that gap: both forms
/// of a logic line now render through the same token joiner.
fn format_logic_line(node: &SyntaxNode) -> String {
    // The `LOGIC_LINE` node's first token is always the leading `~`
    // (`logic_line`'s first act after `start_node` is `p.bump()` for it —
    // see `brink-syntax`'s `parser::logic::logic_line`). Everything from
    // just after it — the optional whitespace and then the statement's own
    // tokens, recursively — gets joined; `join_token_text` already collapses
    // the leading whitespace to nothing since its "pending space" only
    // fires once output has started.
    let Some(tilde) = node.children_with_tokens().next() else {
        return "~".to_owned();
    };
    let after_tilde = tilde.text_range().end();
    let rest = join_token_text(
        node.descendants_with_tokens()
            .filter(|e| e.text_range().start() >= after_tilde),
    );

    if rest.is_empty() {
        "~".to_owned()
    } else {
        format!("~ {rest}")
    }
}

// ── T1b `~ { … }` block rendering (docs/t1b-surface-spec.md §2, #573) ──
//
// Indentation-aware reformatting of multi-line logic block internals: block
// body one level in from the `~` line, nested `if`/`while`/`for` bodies one
// further level each, opening brace on its statement's line, closing brace
// on its own line at the parent's depth, one statement per line, comments
// and blank lines preserved in place. This operates purely on the CST — it
// never touches HIR — since indentation here is a syntactic property and
// T1b blocks are dialect-gated before lowering ever sees them.
//
// The nesting step is a fixed 4 spaces per level regardless of the file's
// configured `FormatConfig::indent` (which continues to govern the *outer*
// placement of the `~` line itself, e.g. inside a knot or choice body) — the
// ruled acceptance criteria for #573 specifies "4-space indent per nesting
// level" for block internals as its own convention, distinct from the
// surrounding weave's indent style.

/// One nesting step inside a `~ { … }` block, appended to the block's own
/// outer `base_indent` (which already accounts for knot/choice/gather depth
/// via `FormatConfig::indent`).
fn block_indent(base_indent: &str, level: u32) -> String {
    format!("{base_indent}{}", " ".repeat((level * 4) as usize))
}

/// Join a stream of CST elements into single-line text: real token text is
/// emitted byte-for-byte (so string-literal content is never touched), and
/// any run of `WHITESPACE`/`NEWLINE` trivia between two real tokens collapses
/// to exactly one space. Mid-statement comments (rare — e.g. inside a
/// parenthesized expression) are kept verbatim with a single space on each
/// side. Node boundaries are transparent; only tokens contribute text.
///
/// Two exceptions to the "collapse to one space" rule:
/// - Whitespace directly touching a `.` field-access dot, or a `[`/`]` index
///   bracket, collapses to *zero* space instead — `party [ leader ] . hp`
///   renders `party[leader].hp`. This is the path-projection display
///   convention itself (`docs/t1e-spec.md` §4: `ref npc.inventory[3]`, no
///   spaces) applied uniformly to every dotted-path/indexed construct this
///   joiner ever renders, T1e `ref lvalue-path` arguments (`brink-fmt
///   path-ref argument formatting`, issue #850) included — not a special case
///   keyed off `ref`, since the CST gives no cheaper way to tell a
///   path-projection segment from any other field access or index expression
///   at this token-stream level, and there's no reason the two should format
///   differently anyway.
/// - Type annotation colons (#642): `:` always renders with no space before
///   and one space after, regardless of source spacing. This canonicalizes
///   `name:type` to `name: type` uniformly across parameters, declarations,
///   and logic lines.
fn join_token_text(elems: impl Iterator<Item = SyntaxElement>) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    let mut last_kind: Option<SyntaxKind> = None;
    for elem in elems {
        let NodeOrToken::Token(tok) = elem else {
            continue;
        };
        match tok.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {
                if !out.is_empty() {
                    pending_space = true;
                }
            }
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                if pending_space || !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(tok.text());
                pending_space = true;
                last_kind = Some(tok.kind());
            }
            SyntaxKind::COLON => {
                // Type annotation canonicalization: no space before, one after.
                // Don't emit pending space, then emit colon, then force space.
                out.push_str(tok.text());
                pending_space = true;
                last_kind = Some(SyntaxKind::COLON);
            }
            kind => {
                let zero_space =
                    matches!(
                        kind,
                        SyntaxKind::DOT | SyntaxKind::L_BRACKET | SyntaxKind::R_BRACKET
                    ) || matches!(last_kind, Some(SyntaxKind::DOT | SyntaxKind::L_BRACKET));
                if pending_space && !zero_space {
                    out.push(' ');
                }
                pending_space = false;
                out.push_str(tok.text());
                last_kind = Some(kind);
            }
        }
    }
    out
}

/// Render a single-line block statement (`TEMP_DECL`, `ASSIGNMENT`,
/// `RETURN_STMT`, `BREAK_STMT`, `CONTINUE_STMT`, `EXPR_STMT`, or any other
/// node kind reached defensively) as flat, single-spaced text — every token
/// under `node`, recursively, joined per `join_token_text`.
fn render_flat_stmt_text(node: &SyntaxNode) -> String {
    join_token_text(node.descendants_with_tokens())
}

/// Reconstruct the header expression text of an `IF_STMT`/`WHILE_STMT`/
/// `FOR_STMT` node — everything between its leading keyword token and its
/// body `STMT_BLOCK` (a condition for `if`/`while`, or `name in expr` for
/// `for`) — collapsed to single-spaced flat text via `join_token_text`.
fn header_expr_text(node: &SyntaxNode) -> String {
    let Some(first) = node.children_with_tokens().next() else {
        return String::new();
    };
    let first_end = first.text_range().end();
    let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK) else {
        return String::new();
    };
    let body_start = body.text_range().start();
    join_token_text(
        node.descendants_with_tokens()
            .filter(|e| e.text_range().start() >= first_end && e.text_range().start() < body_start),
    )
}

/// Render a `~ { … }` block's `LOGIC_LINE` node — the header `~ {`, the
/// reindented body, and the closing `}` — as a complete, newline-terminated
/// string. `base_indent` is the outer depth of the `~` line itself.
///
/// Comments and blank lines *inside* the block are emitted by
/// `render_stmt_block`. But a comment that is a direct child of the
/// `LOGIC_LINE` itself sits *outside* the `STMT_BLOCK` — either between `~`
/// and `{` (a leading comment) or after the closing `}` (a trailing comment)
/// — and would be silently dropped since the body is rebuilt from the
/// `STMT_BLOCK` alone. Emit leading comments on the header line
/// (`~ /* c */ {`) and trailing comments on the closing line (`} /* c */`)
/// so `} // note` survives.
fn render_logic_block(node: &SyntaxNode, base_indent: &str) -> String {
    let body = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK);
    let block_start = body.as_ref().map(|b| b.text_range().start());

    let mut leading: Vec<String> = Vec::new();
    let mut trailing: Vec<String> = Vec::new();
    for elem in node.children_with_tokens() {
        let NodeOrToken::Token(tok) = elem else {
            continue;
        };
        if !matches!(
            tok.kind(),
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        ) {
            continue;
        }
        // A comment before the `STMT_BLOCK` is leading; anything else
        // (including the no-block degenerate case) is trailing.
        if block_start.is_some_and(|start| tok.text_range().start() < start) {
            leading.push(tok.text().trim().to_owned());
        } else {
            trailing.push(tok.text().trim().to_owned());
        }
    }

    let mut out = String::new();
    out.push_str(base_indent);
    out.push('~');
    for comment in &leading {
        out.push(' ');
        out.push_str(comment);
    }
    out.push_str(" {\n");
    if let Some(body) = &body {
        render_stmt_block(body, base_indent, 1, &mut out);
    }
    out.push_str(base_indent);
    out.push('}');
    for comment in &trailing {
        out.push(' ');
        out.push_str(comment);
    }
    out.push('\n');
    out
}

/// Render every statement, comment, and blank line directly inside a
/// `STMT_BLOCK` at `level` (nesting steps in from `base_indent`). Consecutive
/// blank lines collapse to one, matching the rest of the formatter; a
/// same-line trailing comment (no intervening `NEWLINE` token) stays attached
/// to the line it follows instead of becoming its own line.
fn render_stmt_block(block: &SyntaxNode, base_indent: &str, level: u32, out: &mut String) {
    let indent = block_indent(base_indent, level);
    let mut newline_run: u32 = 0;
    let mut has_emitted = false;

    for child in block.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::NEWLINE => newline_run += 1,
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                    let text = tok.text().trim_end();
                    if has_emitted && newline_run == 0 {
                        // Trailing comment on the same physical line as the
                        // previous statement/comment — stays attached to it.
                        if out.ends_with('\n') {
                            out.pop();
                        }
                        out.push(' ');
                        out.push_str(text);
                        out.push('\n');
                    } else {
                        if newline_run >= 2 {
                            out.push('\n');
                        }
                        out.push_str(&indent);
                        out.push_str(text);
                        out.push('\n');
                    }
                    has_emitted = true;
                    newline_run = 0;
                }
                // Braces, whitespace, and anything unrecognized: dropped —
                // indentation is re-rendered from structure.
                _ => {}
            },
            NodeOrToken::Node(n) => {
                if newline_run >= 2 {
                    out.push('\n');
                }
                newline_run = 0;
                render_block_stmt(&n, base_indent, level, out);
                has_emitted = true;
            }
        }
    }

    // A blank line immediately before the closing `}` is preserved too.
    if newline_run >= 2 {
        out.push('\n');
    }
}

/// Dispatch one statement node inside a `STMT_BLOCK`. `if`/`while`/`for`
/// recurse (their bodies are themselves `STMT_BLOCK`s at `level + 1`);
/// everything else is a single-line statement.
fn render_block_stmt(node: &SyntaxNode, base_indent: &str, level: u32, out: &mut String) {
    match node.kind() {
        SyntaxKind::IF_STMT => render_if_stmt(node, base_indent, level, false, out),
        SyntaxKind::WHILE_STMT => render_header_stmt(node, base_indent, level, "while", out),
        SyntaxKind::FOR_STMT => render_header_stmt(node, base_indent, level, "for", out),
        _ => {
            out.push_str(&block_indent(base_indent, level));
            out.push_str(&render_flat_stmt_text(node));
            out.push('\n');
        }
    }
}

/// Render `if cond { … } (else if cond { … })* (else { … })?`. `chained` is
/// `true` for an else-if's nested `IF_STMT` — it continues on the previous
/// line (`} else `) instead of starting a fresh indented line.
fn render_if_stmt(
    node: &SyntaxNode,
    base_indent: &str,
    level: u32,
    chained: bool,
    out: &mut String,
) {
    if !chained {
        out.push_str(&block_indent(base_indent, level));
    }
    out.push_str("if ");
    out.push_str(&header_expr_text(node));
    out.push_str(" {\n");
    if let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK) {
        render_stmt_block(&body, base_indent, level + 1, out);
    }
    out.push_str(&block_indent(base_indent, level));
    out.push('}');

    if let Some(else_clause) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::ELSE_CLAUSE)
    {
        if let Some(nested_if) = else_clause
            .children()
            .find(|c| c.kind() == SyntaxKind::IF_STMT)
        {
            out.push_str(" else ");
            render_if_stmt(&nested_if, base_indent, level, true, out);
            return;
        }
        if let Some(else_body) = else_clause
            .children()
            .find(|c| c.kind() == SyntaxKind::STMT_BLOCK)
        {
            out.push_str(" else {\n");
            render_stmt_block(&else_body, base_indent, level + 1, out);
            out.push_str(&block_indent(base_indent, level));
            out.push('}');
        }
    }
    out.push('\n');
}

/// Render `while cond { … }` or `for name in expr { … }` — the shared shape
/// of a keyword, a header expression, and a `STMT_BLOCK` body.
fn render_header_stmt(
    node: &SyntaxNode,
    base_indent: &str,
    level: u32,
    keyword: &str,
    out: &mut String,
) {
    out.push_str(&block_indent(base_indent, level));
    out.push_str(keyword);
    out.push(' ');
    let header = header_expr_text(node);
    if !header.is_empty() {
        out.push_str(&header);
        out.push(' ');
    }
    out.push_str("{\n");
    if let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::STMT_BLOCK) {
        render_stmt_block(&body, base_indent, level + 1, out);
    }
    out.push_str(&block_indent(base_indent, level));
    out.push_str("}\n");
}

/// Render a STRUCT declaration (TM-4b, docs/typed-mode-spec.md §6). For
/// single-line structs, output as-is; for multiline structs, format like
/// blocks with proper field indentation and trailing commas on each field.
pub(crate) fn render_struct_decl(
    node: &SyntaxNode,
    base_indent: &str,
    config: &FormatConfig,
) -> String {
    // Collect field declarations to determine multiline status and content.
    let fields: Vec<SyntaxNode> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::STRUCT_FIELD_DECL)
        .collect();

    // Determine if the struct body spans multiple lines by checking if the
    // first and last field (or the opening and closing braces) are on different lines.
    // Check the source text between `#{` and `}` for newlines.
    let opening_brace = node
        .children_with_tokens()
        .find(|e| e.kind() == SyntaxKind::L_BRACE)
        .map(|e| e.text_range().start());
    let closing_brace = node.children_with_tokens().find_map(|e| {
        if e.kind() == SyntaxKind::R_BRACE {
            Some(e.text_range().start())
        } else {
            None
        }
    });

    let is_multiline =
        if let (Some(open_offset), Some(close_offset)) = (opening_brace, closing_brace) {
            // Check if the range between `{` and `}` contains any field on a different line.
            // We do this by checking if any field's text range starts on a line after the
            // opening brace line. Since we don't have direct access to line numbers in the
            // CST, we use a simpler heuristic: if any field has NEWLINE tokens in the parent's
            // descendants before it, or between it and the next field, it's multiline.
            !fields.is_empty()
                && node
                    .children_with_tokens()
                    .skip_while(|e| e.text_range().end() <= open_offset)
                    .take_while(|e| e.text_range().start() < close_offset)
                    .any(|e| e.kind() == SyntaxKind::NEWLINE)
        } else {
            false
        };

    if !is_multiline {
        // Single-line struct: output as a single line with single space after colon.
        // Collapse all whitespace and format as `STRUCT Name = #{field: type, …}`.
        return format_struct_decl_single_line(node, base_indent);
    }

    // Multiline struct: format like a block.
    let mut out = String::new();
    out.push_str(base_indent);
    out.push_str("STRUCT ");
    out.push_str(&struct_decl_name(node));
    out.push_str(" = #{\n");

    // Field indent is base_indent + one level of the file's configured
    // indent (unlike logic blocks which use hardcoded 4 spaces).
    let field_indent = format!("{base_indent}{}", indent_str(config, 1));
    let (Some(open_offset), Some(close_offset)) = (opening_brace, closing_brace) else {
        out.push_str(base_indent);
        out.push_str("}\n");
        return out;
    };
    render_struct_body_multiline(node, open_offset, close_offset, &field_indent, &mut out);

    // Close the struct.
    out.push_str(base_indent);
    out.push_str("}\n");
    out
}

/// Extract the struct name from a `STRUCT_DECL`'s `IDENTIFIER` child.
fn struct_decl_name(node: &SyntaxNode) -> String {
    node.children()
        .find(|c| c.kind() == SyntaxKind::IDENTIFIER)
        .and_then(|id| {
            id.children_with_tokens().find_map(|e| {
                if let NodeOrToken::Token(tok) = e
                    && tok.kind() == SyntaxKind::IDENT
                {
                    Some(tok.text().to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Render every field declaration and comment directly inside a multiline
/// `STRUCT_DECL` body, in source order, with trailing commas on each field.
/// Comments are direct children of `STRUCT_DECL` (see
/// `skip_struct_body_trivia` in the parser) rather than living inside any
/// field node, so — mirroring `render_stmt_block` — this walks the node's
/// direct children/tokens between the braces instead of iterating field
/// nodes alone; that preserves leading, interleaved, and same-line trailing
/// comments.
fn render_struct_body_multiline(
    node: &SyntaxNode,
    open_offset: rowan::TextSize,
    close_offset: rowan::TextSize,
    field_indent: &str,
    out: &mut String,
) {
    let mut newline_run: u32 = 0;
    let mut has_emitted = false;
    for elem in node
        .children_with_tokens()
        .skip_while(|e| e.text_range().end() <= open_offset)
        .take_while(|e| e.text_range().start() < close_offset)
    {
        match elem {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::NEWLINE => newline_run += 1,
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                    let text = tok.text().trim_end();
                    if has_emitted && newline_run == 0 {
                        // Trailing comment on the same physical line as the
                        // previous field/comment — stays attached to it.
                        if out.ends_with('\n') {
                            out.pop();
                        }
                        out.push(' ');
                        out.push_str(text);
                        out.push('\n');
                    } else {
                        if newline_run >= 2 {
                            out.push('\n');
                        }
                        out.push_str(field_indent);
                        out.push_str(text);
                        out.push('\n');
                    }
                    has_emitted = true;
                    newline_run = 0;
                }
                _ => {}
            },
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::STRUCT_FIELD_DECL => {
                if newline_run >= 2 {
                    out.push('\n');
                }
                newline_run = 0;
                out.push_str(field_indent);
                let field_text = join_token_text(n.descendants_with_tokens());
                out.push_str(&field_text);
                out.push(',');
                out.push('\n');
                has_emitted = true;
            }
            NodeOrToken::Node(_) => {}
        }
    }
}

/// Format a single-line STRUCT declaration: `STRUCT Name = #{field: type, …}`.
/// Collapses all whitespace and ensures canonical spacing: single space after colon.
fn format_struct_decl_single_line(node: &SyntaxNode, base_indent: &str) -> String {
    let mut out = String::new();
    out.push_str(base_indent);
    out.push_str("STRUCT ");
    out.push_str(&struct_decl_name(node));
    out.push_str(" = #{");

    // Comments are direct children of `STRUCT_DECL` (see
    // `skip_struct_body_trivia` in the parser), not children of any field
    // node — a single-line struct can only contain `BLOCK_COMMENT`s (a
    // `LINE_COMMENT` forces a `NEWLINE`, which would make the struct
    // multiline), but they still need to be preserved rather than dropped.
    let opening_brace = node
        .children_with_tokens()
        .find(|e| e.kind() == SyntaxKind::L_BRACE)
        .map(|e| e.text_range().start());
    let closing_brace = node.children_with_tokens().find_map(|e| {
        if e.kind() == SyntaxKind::R_BRACE {
            Some(e.text_range().start())
        } else {
            None
        }
    });

    if let (Some(open_offset), Some(close_offset)) = (opening_brace, closing_brace) {
        let mut has_field = false;
        let mut has_content = false;
        for elem in node
            .children_with_tokens()
            .skip_while(|e| e.text_range().end() <= open_offset)
            .take_while(|e| e.text_range().start() < close_offset)
        {
            match elem {
                NodeOrToken::Node(n) if n.kind() == SyntaxKind::STRUCT_FIELD_DECL => {
                    if has_field {
                        out.push_str(", ");
                    } else if has_content {
                        out.push(' ');
                    }
                    format_struct_field_for_single_line(&n, &mut out);
                    has_field = true;
                    has_content = true;
                }
                NodeOrToken::Token(tok)
                    if matches!(
                        tok.kind(),
                        SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
                    ) =>
                {
                    if has_content {
                        out.push(' ');
                    }
                    out.push_str(tok.text().trim());
                    has_content = true;
                }
                _ => {}
            }
        }
    }

    out.push_str("}\n");
    out
}

/// Extract and format a `STRUCT_FIELD_DECL` for single-line output: `name: type`.
fn format_struct_field_for_single_line(field: &SyntaxNode, out: &mut String) {
    // Use join_token_text to get the entire field as a single-spaced string,
    // then split on the colon to extract name and type, and reconstruct with
    // canonical spacing: `name: type` with single space after colon.
    let field_text = join_token_text(field.descendants_with_tokens());
    if let Some(colon_pos) = field_text.find(':') {
        let name = field_text[..colon_pos].trim();
        let type_str = field_text[colon_pos + 1..].trim();
        out.push_str(name);
        out.push_str(": ");
        out.push_str(type_str);
    } else {
        // Fallback if no colon found (shouldn't happen for valid input).
        out.push_str(&field_text);
    }
}
