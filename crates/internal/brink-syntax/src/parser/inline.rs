use crate::SyntaxKind::{
    self, AMP, BACKSLASH, BANG, BLOCK_COMMENT, BRANCH_CONTENT, BRANCHLESS_COND_BODY, COLON,
    CONDITIONAL_WITH_EXPR, DIVERT, DOLLAR, ELSE_BRANCH, EOF, ESCAPE, GLUE, GLUE_NODE,
    IMPLICIT_SEQUENCE, INLINE_BRANCHES_COND, INLINE_BRANCHES_SEQ, INLINE_LOGIC, INNER_EXPRESSION,
    KW_CYCLE, KW_ELSE, KW_ONCE, KW_SHUFFLE, KW_STOPPING, L_BRACE, L_PAREN, LINE_COMMENT, MINUS,
    MULTILINE_BLOCK, MULTILINE_BRANCH_BODY, MULTILINE_BRANCH_COND, MULTILINE_BRANCH_SEQ,
    MULTILINE_BRANCHES_COND, MULTILINE_BRANCHES_SEQ, MULTILINE_CONDITIONAL, NEWLINE, PIPE, PLUS,
    R_BRACE, R_PAREN, SEQUENCE_SYMBOL_ANNOTATION, SEQUENCE_WITH_ANNOTATION,
    SEQUENCE_WORD_ANNOTATION, STAR, TEXT, THREAD, TILDE, TUNNEL_ONWARDS, WHITESPACE,
};

use super::Parser;

/// Parse inline logic: `{ inner_logic }`.
///
/// This is the primary entry point called from content and choice parsers.
///
/// ```text
/// inline_logic = { "{" ~ inner_logic ~ "}" }
/// ```
pub(crate) fn inline_logic(p: &mut Parser<'_>) {
    p.skip_ws(); // flush trivia to the parent before starting the node
    p.start_node(INLINE_LOGIC);
    p.bump(); // L_BRACE
    p.skip_ws();

    inner_logic(p);

    p.skip_ws();
    if p.current() == R_BRACE {
        p.bump();
    } else if !p.at_eof() {
        p.error("expected `}`".into());
    }
    p.finish_node();
}

/// Parse a multiline block: `{ NEWLINE ... }`.
///
/// This is the statement-level form that spans multiple lines.
///
/// ```text
/// multiline_block = { "{" ~ NEWLINE ~ inner_logic_multiline ~ "}" }
/// ```
pub(crate) fn multiline_block(p: &mut Parser<'_>) {
    p.skip_ws(); // flush trivia to the parent before starting the node
    p.start_node(MULTILINE_BLOCK);
    p.bump(); // L_BRACE
    p.skip_ws();

    // Consume the newline that makes this a multiline block
    if p.at(NEWLINE) {
        p.bump();
    }
    skip_blank_lines(p);

    // inner_logic_multiline: sequence_with_annotation | multiline_conditional_bare | conditional_with_expr
    if at_sequence_annotation(p) {
        sequence_with_annotation(p);
    } else if at_multiline_branch_start(p) {
        // multiline_conditional_bare (branches without a leading expression)
        multiline_branches_cond(p);
    } else {
        // conditional_with_expr (expression followed by `:`)
        conditional_with_expr_standalone(p);
    }

    p.skip_ws();
    skip_blank_lines(p);
    if p.current() == R_BRACE {
        p.bump();
    } else if !p.at_eof() {
        p.error("expected `}` to close multiline block".into());
    }
    p.finish_node();
}

/// Scan ahead (without consuming) to find whether PIPE or COLON comes first
/// within the current brace level. Skips nested `{...}` pairs.
/// Returns: `PIPE`, `COLON`, or `R_BRACE`/`NEWLINE`/`EOF` if neither found.
fn scan_for_pipe_or_colon(p: &Parser<'_>) -> SyntaxKind {
    let mut i = 0;
    let mut depth = 0u32;
    loop {
        let kind = p.nth(i);
        match kind {
            L_BRACE => {
                depth += 1;
                i += 1;
            }
            R_BRACE if depth > 0 => {
                depth -= 1;
                i += 1;
            }
            PIPE if depth == 0 => return PIPE,
            COLON if depth == 0 => return COLON,
            R_BRACE | NEWLINE | EOF => return kind,
            _ => {
                i += 1;
            }
        }
    }
}

/// 5-way dispatch for inner logic inside `{...}`.
///
/// ```text
/// inner_logic = {
///     sequence_with_annotation
///   | conditional_with_expr
///   | multiline_conditional
///   | implicit_sequence
///   | inner_expression
/// }
/// ```
fn inner_logic(p: &mut Parser<'_>) {
    // 1. sequence_with_annotation: starts with annotation symbol or keyword
    if at_sequence_annotation(p) {
        sequence_with_annotation(p);
        return;
    }

    // 2. multiline_conditional: starts with NEWLINE
    if p.current() == NEWLINE {
        multiline_conditional(p);
        return;
    }

    // Scan ahead to determine brace type before consuming any tokens.
    let lookahead = scan_for_pipe_or_colon(p);

    if lookahead == PIPE {
        // Implicit sequence: {branch|branch|...}
        // Parse first branch as content (not expression) so sentences work.
        p.start_node(IMPLICIT_SEQUENCE);
        branch_content(p); // first branch
        while p.current() == PIPE {
            p.skip_ws();
            p.bump(); // PIPE
            branch_content(p);
        }
        p.finish_node();
        return;
    }

    // COLON, R_BRACE, or other: try expression, then dispatch
    let checkpoint = p.checkpoint();
    super::expression::expression(p);
    p.skip_ws();

    if p.current() == COLON {
        // conditional_with_expr: we already parsed the expression, now wrap it
        p.start_node_at(checkpoint, CONDITIONAL_WITH_EXPR);
        p.bump(); // COLON
        p.skip_ws();
        conditional_body(p);
        p.finish_node();
    } else {
        // Bare expression -- wrap it
        p.start_node_at(checkpoint, INNER_EXPRESSION);
        p.finish_node();
    }
}

/// Parse the body after `expr :` in a conditional.
fn conditional_body(p: &mut Parser<'_>) {
    match p.current() {
        NEWLINE => {
            // Could be multiline branches or branchless body
            if at_multiline_branch_start(p) {
                multiline_branches_cond(p);
            } else {
                branchless_cond_body(p);
            }
        }
        R_BRACE => {
            // Empty inline conditional -- just empty inline branches
            inline_branches_cond(p);
        }
        _ => {
            // inline_branches_cond (content before optional `|`)
            inline_branches_cond(p);
        }
    }
}

// ── Sequence with annotation ────────────────────────────────────────

fn at_sequence_annotation(p: &Parser<'_>) -> bool {
    matches!(
        p.current(),
        AMP | BANG | TILDE | DOLLAR | KW_STOPPING | KW_CYCLE | KW_SHUFFLE | KW_ONCE
    )
}

/// Parse a sequence with annotation.
///
/// ```text
/// sequence_with_annotation = { sequence_annotation ~ (multiline_branches_seq | inline_branches_seq) }
/// ```
fn sequence_with_annotation(p: &mut Parser<'_>) {
    p.start_node(SEQUENCE_WITH_ANNOTATION);

    // Parse annotation
    match p.current() {
        AMP | BANG | TILDE | DOLLAR => {
            p.start_node(SEQUENCE_SYMBOL_ANNOTATION);
            while matches!(p.current(), AMP | BANG | TILDE | DOLLAR) {
                p.bump();
            }
            p.finish_node();
        }
        KW_STOPPING | KW_CYCLE | KW_SHUFFLE | KW_ONCE => {
            p.start_node(SEQUENCE_WORD_ANNOTATION);
            p.bump(); // first keyword
            loop {
                p.skip_ws();
                if matches!(p.current(), KW_STOPPING | KW_CYCLE | KW_SHUFFLE | KW_ONCE) {
                    p.bump();
                } else {
                    break;
                }
            }
            p.skip_ws();
            p.expect(COLON);
            p.finish_node();
        }
        _ => {
            p.error("expected sequence annotation".into());
        }
    }

    p.skip_ws();

    // Branches: multiline or inline
    if p.current() == NEWLINE {
        multiline_branches_seq(p);
    } else {
        inline_branches_seq(p);
    }

    p.finish_node();
}

/// Parse inline sequence branches: `content | content | ...`
fn inline_branches_seq(p: &mut Parser<'_>) {
    p.start_node(INLINE_BRANCHES_SEQ);
    branch_content(p);
    while p.current() == PIPE {
        p.skip_ws();
        p.bump(); // PIPE
        branch_content(p);
    }
    p.finish_node();
}

/// Parse multiline sequence branches.
fn multiline_branches_seq(p: &mut Parser<'_>) {
    p.start_node(MULTILINE_BRANCHES_SEQ);
    // Consume leading newlines and whitespace
    while matches!(p.current(), NEWLINE) {
        p.bump();
        p.skip_ws();
    }
    skip_blank_lines(p);

    while at_multiline_branch_start(p) {
        multiline_branch_seq(p);
    }
    p.finish_node();
}

/// Parse a single multiline sequence branch: `- content`.
fn multiline_branch_seq(p: &mut Parser<'_>) {
    p.start_node(MULTILINE_BRANCH_SEQ);
    // Skip newlines and whitespace before the branch marker `-`.
    while matches!(p.nth_raw(0), NEWLINE | WHITESPACE) {
        p.bump();
    }
    p.bump(); // MINUS (the dash)
    p.skip_ws();
    multiline_branch_body(p);
    p.finish_node();
}

// ── Conditional ─────────────────────────────────────────────────────

/// Parse `expr : ...` as a standalone `conditional_with_expr`
/// (used in multiline block context).
fn conditional_with_expr_standalone(p: &mut Parser<'_>) {
    p.start_node(CONDITIONAL_WITH_EXPR);
    super::expression::expression(p);
    p.skip_ws();
    if p.current() == COLON {
        p.bump();
        p.skip_ws();
        conditional_body(p);
    } else {
        p.error("expected `:` in conditional".into());
    }
    p.finish_node();
}

/// Parse inline conditional branches: `true_content | false_content?`
fn inline_branches_cond(p: &mut Parser<'_>) {
    p.start_node(INLINE_BRANCHES_COND);
    branch_content(p);
    if p.current() == PIPE {
        p.skip_ws();
        p.bump(); // PIPE
        branch_content(p);
    }
    p.finish_node();
}

/// Parse multiline conditional branches.
fn multiline_branches_cond(p: &mut Parser<'_>) {
    p.start_node(MULTILINE_BRANCHES_COND);
    while matches!(p.current(), NEWLINE) {
        p.bump();
        p.skip_ws();
    }
    skip_blank_lines(p);

    while at_multiline_branch_start(p) {
        multiline_branch_cond(p);
    }
    p.finish_node();
}

/// Parse a multiline conditional: just branches after a NEWLINE.
fn multiline_conditional(p: &mut Parser<'_>) {
    p.start_node(MULTILINE_CONDITIONAL);
    while matches!(p.current(), NEWLINE) {
        p.bump();
        p.skip_ws();
    }
    skip_blank_lines(p);

    while at_multiline_branch_start(p) {
        multiline_branch_cond(p);
    }
    p.finish_node();
}

/// Parse a multiline conditional branch.
///
/// ```text
/// multiline_branch_cond = {
///     NEWLINE? ~ WS* ~ "-" ~ !">" ~ WS*
///   ~ ("else" ~ ":"? | expression ~ ":")? ~ WS*
///   ~ multiline_branch_body
/// }
/// ```
fn multiline_branch_cond(p: &mut Parser<'_>) {
    p.start_node(MULTILINE_BRANCH_COND);
    // Skip newlines and whitespace before the branch marker `-`.
    // The body parser breaks on the NEWLINE before the next branch,
    // so there may be multiple NEWLINEs (blank lines) to consume here.
    while matches!(p.nth_raw(0), NEWLINE | WHITESPACE) {
        p.bump();
    }
    p.bump(); // MINUS (the dash)
    p.skip_ws();

    // Optional condition: "else" or expression ":"
    if p.current() == KW_ELSE {
        p.bump(); // else
        p.skip_ws();
        if p.current() == COLON {
            p.bump();
        }
        p.skip_ws();
    } else if !matches!(p.current(), NEWLINE | EOF | R_BRACE) && p.current() != MINUS {
        // Try to parse as expression : condition
        let has_condition = looks_like_condition(p);
        if has_condition {
            super::expression::expression(p);
            p.skip_ws();
            if p.current() == COLON {
                p.bump();
                p.skip_ws();
            }
        }
    }

    multiline_branch_body(p);
    p.finish_node();
}

/// Heuristic: does the current position start a condition (expression followed by `:`)?
/// We scan ahead looking for `:` before NEWLINE or `}`.
fn looks_like_condition(p: &Parser<'_>) -> bool {
    let mut i = 0;
    let mut depth: u32 = 0;
    loop {
        let kind = p.nth(i);
        match kind {
            COLON if depth == 0 => return true,
            EOF => return false,
            NEWLINE | R_BRACE if depth == 0 => return false,
            L_BRACE | L_PAREN => depth += 1,
            R_BRACE | R_PAREN => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
}

/// Parse branchless conditional body (content after `:` with no `-` branch markers).
fn branchless_cond_body(p: &mut Parser<'_>) {
    p.start_node(BRANCHLESS_COND_BODY);

    // Consume the leading NEWLINE
    if p.current() == NEWLINE {
        p.bump();
    }

    // Content until we hit a branch start, closing brace
    loop {
        match p.current() {
            R_BRACE | EOF => break,
            NEWLINE => {
                // Check if next line starts a branch
                if at_multiline_branch_start(p) {
                    // This is an else branch
                    else_branch(p);
                    break;
                }
                p.bump();
            }
            TILDE => {
                p.skip_ws();
                super::logic::logic_line(p);
            }
            L_BRACE => {
                inline_logic(p);
            }
            DIVERT | TUNNEL_ONWARDS | THREAD => {
                super::divert::divert(p);
            }
            GLUE => {
                p.start_node(GLUE_NODE);
                p.bump();
                p.finish_node();
            }
            BACKSLASH => {
                if matches!(p.nth(1), NEWLINE | EOF) {
                    p.start_node(TEXT);
                    p.bump();
                    p.finish_node();
                } else {
                    p.start_node(ESCAPE);
                    p.bump(); // backslash
                    p.bump(); // escaped char
                    p.finish_node();
                }
            }
            _ => {
                let before = p.pos();
                multiline_branch_text(p);
                if p.pos() == before {
                    break;
                }
            }
        }
    }

    p.finish_node();
}

/// Parse an else branch at the end of a branchless conditional body.
fn else_branch(p: &mut Parser<'_>) {
    p.start_node(ELSE_BRANCH);
    multiline_branch_cond(p);
    p.finish_node();
}

// ── Shared branch helpers ───────────────────────────────────────────

/// Returns `true` if we're at a multiline branch start.
/// Peeks past optional NEWLINE and whitespace to check for `-` not followed by `>`.
fn at_multiline_branch_start(p: &Parser<'_>) -> bool {
    let mut i = 0;
    loop {
        match p.nth(i) {
            NEWLINE => {
                i += 1;
            }
            MINUS => return true,
            _ => return false,
        }
    }
}

/// Parse a multiline branch body.
///
/// ```text
/// multiline_branch_body = { (multiline_branch_body_item | body_newline)* }
/// ```
///
/// NOTE: Gathers (`-`) inside inner blocks are forbidden by the ink spec
/// but we don't yet emit a diagnostic for them — the MINUS arm just breaks
/// out of the body loop (same as a branch separator).
fn multiline_branch_body(p: &mut Parser<'_>) {
    p.start_node(MULTILINE_BRANCH_BODY);
    loop {
        match p.current() {
            EOF | R_BRACE | MINUS => break, // MINUS = branch separator; gathers forbidden in inner blocks
            NEWLINE => {
                // body_newline: NEWLINE not followed by branch start
                if next_line_is_branch(p) {
                    break;
                }
                p.bump();
            }
            STAR | PLUS => {
                // Choices participate in the outer weave structure.
                super::choice::choice(p);
            }
            TILDE => {
                p.skip_ws();
                super::logic::logic_line(p);
            }
            L_BRACE => {
                inline_logic(p);
            }
            GLUE => {
                p.start_node(GLUE_NODE);
                p.bump();
                p.finish_node();
            }
            BACKSLASH => {
                if matches!(p.nth(1), NEWLINE | EOF) {
                    // Backslash before newline/EOF — consume as text to avoid stall
                    p.start_node(TEXT);
                    p.bump();
                    p.finish_node();
                } else {
                    p.start_node(ESCAPE);
                    p.bump(); // backslash
                    p.bump(); // escaped char
                    p.finish_node();
                }
            }
            DIVERT | TUNNEL_ONWARDS | THREAD => {
                super::divert::divert(p);
            }
            _ => {
                let before = p.pos();
                multiline_branch_text(p);
                if p.pos() == before {
                    break;
                }
            }
        }
    }
    p.finish_node();
}

/// Check if the line after the current NEWLINE starts a branch (WS* - !>).
fn next_line_is_branch(p: &Parser<'_>) -> bool {
    let mut offset = 1; // skip past the NEWLINE
    loop {
        match p.nth(offset) {
            NEWLINE => offset += 1,
            MINUS => return true,
            _ => return false,
        }
    }
}

/// Parse multiline branch text.
fn multiline_branch_text(p: &mut Parser<'_>) {
    p.start_node(TEXT);
    loop {
        if p.at_eof() {
            break;
        }
        match p.nth_raw(0) {
            NEWLINE | L_BRACE | R_BRACE | GLUE | DIVERT | TUNNEL_ONWARDS | LINE_COMMENT
            | BLOCK_COMMENT | BACKSLASH | EOF | TILDE | THREAD => break,
            _ => p.bump(),
        }
    }
    p.finish_node();
}

// ── Shared inline branch content ────────────────────────────────────

/// Parse branch content (inline): text, `inline_logic`, glue, escapes until `|` or `}`.
fn branch_content(p: &mut Parser<'_>) {
    p.start_node(BRANCH_CONTENT);
    loop {
        match p.current() {
            PIPE | R_BRACE | NEWLINE | EOF => break,
            L_BRACE => {
                inline_logic(p);
            }
            GLUE => {
                p.start_node(GLUE_NODE);
                p.bump();
                p.finish_node();
            }
            DIVERT | TUNNEL_ONWARDS | THREAD => {
                super::divert::divert(p);
            }
            BACKSLASH => {
                if matches!(p.nth(1), NEWLINE | EOF) {
                    // Backslash before newline/EOF — consume as text to avoid stall
                    p.start_node(TEXT);
                    p.bump();
                    p.finish_node();
                } else {
                    p.start_node(ESCAPE);
                    p.bump(); // backslash
                    p.bump(); // escaped char
                    p.finish_node();
                }
            }
            _ => {
                let before = p.pos();
                branch_text(p);
                if p.pos() == before {
                    break;
                }
            }
        }
    }
    p.finish_node();
}

/// Parse a run of branch text characters.
fn branch_text(p: &mut Parser<'_>) {
    p.start_node(TEXT);
    loop {
        if p.at_eof() {
            break;
        }
        match p.nth_raw(0) {
            PIPE | L_BRACE | R_BRACE | GLUE | DIVERT | TUNNEL_ONWARDS | LINE_COMMENT
            | BLOCK_COMMENT | NEWLINE | BACKSLASH | EOF | THREAD => break,
            _ => p.bump(),
        }
    }
    p.finish_node();
}

// ── Helpers ─────────────────────────────────────────────────────────

fn skip_blank_lines(p: &mut Parser<'_>) {
    while p.current() == NEWLINE {
        p.bump();
        p.skip_ws();
    }
}
