use crate::SyntaxKind::{
    ASSIGNMENT, AWAIT_STMT, BREAK_STMT, CONTINUE_STMT, DOT, ELSE_CLAUSE, EOF, EQ, EXPR_STMT,
    FIELD_ACCESS_EXPR, FOR_STMT, IDENT, IDENTIFIER, IF_STMT, INDEX_EXPR, KW_ELSE, KW_RETURN,
    KW_TEMP, L_BRACE, L_BRACKET, LOGIC_LINE, MINUS_EQ, NEWLINE, PLUS_EQ, R_BRACE, R_BRACKET,
    RETURN_STMT, STMT_BLOCK, TEMP_DECL, WHILE_STMT,
};

use super::Parser;
use super::types::{at_type_annotation, type_annotation};

/// Parse a logic line: `~ statement NEWLINE?`, or a T1b multi-line block
/// `~ { … }` (docs/t1b-surface-spec.md §2) when the expression position
/// opens with `{`.
///
/// ```text
/// logic_line = { "~" ~ (stmt_block | return_statement | temp_declaration | assignment | expression) ~ NEWLINE? }
/// ```
pub(crate) fn logic_line(p: &mut Parser<'_, '_>) {
    p.start_node(LOGIC_LINE);
    p.bump(); // TILDE
    p.skip_ws();

    if p.current() == L_BRACE {
        stmt_block(p);
        p.skip_ws();
        if p.at(NEWLINE) {
            p.bump();
        }
        p.finish_node();
        return;
    }

    match p.current() {
        KW_RETURN => return_statement(p),
        KW_TEMP => temp_declaration(p),
        IDENT if p.at_kw_text("await") && !is_assignment_ahead(p) => await_statement(p),
        IDENT if is_assignment_ahead(p) => assignment(p),
        _ => {
            // Bare expression
            super::expression::expression(p);
        }
    }

    p.skip_ws();
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Check if an indexable lvalue (`ident`, `ident.path`, `ident[i]`, chained,
/// or a mixed `ident[i].field` — issue #674) is followed by an assignment
/// operator (=, +=, -=). Must not confuse `=` in `== knot ==` or bare `=` in
/// stitch headers.
fn is_assignment_ahead(p: &Parser<'_, '_>) -> bool {
    let mut i = 1; // nth(0) is already known to be IDENT by the caller
    while p.nth(i) == DOT && p.nth(i + 1) == IDENT {
        i += 2;
    }
    // A bare dotted-path prefix is already consumed above; any further
    // `[…]`/`.field` postfixes here only ever follow an index (an
    // unambiguous `arr[i].field` mixed chain — see `indexable_lvalue`'s
    // doc), so it's safe to interleave the two postfix kinds freely.
    loop {
        if p.nth(i) == L_BRACKET {
            i += 1;
            let mut depth = 1i32;
            loop {
                match p.nth(i) {
                    L_BRACKET => depth += 1,
                    R_BRACKET => depth -= 1,
                    EOF | NEWLINE => return false, // unterminated — not an assignment
                    _ => {}
                }
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            continue;
        }
        if p.nth(i) == DOT && p.nth(i + 1) == IDENT {
            i += 2;
            continue;
        }
        break;
    }
    let next = p.nth(i);
    matches!(next, EQ | PLUS_EQ | MINUS_EQ) && !(next == EQ && p.nth(i + 1) == EQ)
}

/// Parse `return expr?`. The value is absent when followed directly by a
/// statement terminator: end of line, end of block (`}`), or EOF.
fn return_statement(p: &mut Parser<'_, '_>) {
    p.start_node(RETURN_STMT);
    p.bump(); // KW_RETURN
    p.skip_ws();
    if !matches!(p.current(), NEWLINE | EOF | R_BRACE) {
        super::expression::expression(p);
    }
    p.finish_node();
}

/// Parse `await <cond>` — a `FlowFrame` suspension point
/// (docs/flow-suspension-spec.md §3). `await` is a contextual (soft) keyword,
/// already matched by the caller. Statement/logic position only; the
/// condition is an ordinary expression (mid-expression `await` is permanently
/// out — §3). Purity of the condition is enforced downstream (the effect-row
/// gate), and lowering to the VM is fenced until FS-3 lands — this only builds
/// the CST node.
fn await_statement(p: &mut Parser<'_, '_>) {
    p.start_node(AWAIT_STMT);
    p.bump(); // "await" (IDENT)
    p.skip_ws();
    if matches!(p.current(), NEWLINE | EOF | R_BRACE) {
        p.error("expected a condition expression after `await`".into());
    } else {
        super::expression::expression(p);
    }
    p.finish_node();
}

/// Parse `temp ident = expr`.
fn temp_declaration(p: &mut Parser<'_, '_>) {
    p.start_node(TEMP_DECL);
    p.bump(); // KW_TEMP
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    // Optional ascription (TM-2, docs/typed-mode-spec.md §3):
    // `~ temp name: type = expr`.
    if at_type_annotation(p) {
        type_annotation(p);
    }
    p.skip_ws();
    assignment_op(p);
    p.skip_ws();
    super::expression::expression(p);
    p.finish_node();
}

/// Parse `lvalue op= expr`, where `lvalue` is a (possibly indexed, T1b §4)
/// path: `ident`, `ident.path`, or `ident[i]`/chained `grid[y][x]`.
fn assignment(p: &mut Parser<'_, '_>) {
    p.start_node(ASSIGNMENT);
    indexable_lvalue(p);
    p.skip_ws();
    assignment_op(p);
    p.skip_ws();
    super::expression::expression(p);
    p.finish_node();
}

/// Parse a path, then any trailing postfix index/field chain (`a[0]`,
/// `grid[y][x]`, `arr[i].field` — issue #674) — the shared shape between an
/// expression-position postfix chain and an assignment lvalue (T1b §4,
/// extended for TM-4c mixed field/index writes).
///
/// A `.field` immediately after an `INDEX_EXPR` mirrors the general
/// expression grammar's `FIELD_ACCESS_EXPR` postfix (see
/// `expression::expression_bp`'s postfix loop doc) — unambiguous here for
/// the same reason it is there: a bare dotted `ident.ident…` prefix is
/// already consumed whole by `divert::path` above, so any `.field` this
/// loop sees only ever follows an index, never a plain path segment. LIR
/// still rejects the resulting `FieldAccessExpr` target as a chained/mixed
/// write (`E074`, the T1e boundary) — this only fixes the *grammar* so that
/// diagnostic is reachable instead of a generic parse error.
fn indexable_lvalue(p: &mut Parser<'_, '_>) {
    let checkpoint = p.checkpoint();
    super::divert::path(p);
    loop {
        if p.current() == L_BRACKET {
            p.start_node_at(checkpoint, INDEX_EXPR);
            super::expression::index_bracket(p);
            p.finish_node();
            continue;
        }
        if p.current() == DOT && p.nth(1) == IDENT {
            p.start_node_at(checkpoint, FIELD_ACCESS_EXPR);
            p.bump(); // .
            p.skip_ws();
            p.start_node(IDENTIFIER);
            p.bump(); // field name
            p.finish_node();
            p.finish_node();
            continue;
        }
        break;
    }
}

/// Consume an assignment operator: `=`, `+=`, or `-=`.
/// Bare `=` must not be `==`.
fn assignment_op(p: &mut Parser<'_, '_>) {
    match p.current() {
        PLUS_EQ | MINUS_EQ | EQ => {
            p.bump();
        }
        _ => {
            p.error("expected assignment operator".into());
        }
    }
}

// ─── T1b multi-line blocks (docs/t1b-surface-spec.md §2) ────────────────

/// Parse `{ stmt* }` — a braced statement list. Used for the top-level
/// `~ { … }` block body and every nested `if`/`while`/`for` body.
/// Depth-guarded like expression nesting (`if`/`while`/`for` bodies recurse
/// through `stmt_block`).
pub(super) fn stmt_block(p: &mut Parser<'_, '_>) {
    p.start_node(STMT_BLOCK);
    p.skip_ws();
    p.bump(); // {

    if p.at_depth_limit() {
        p.error("nesting depth limit exceeded".into());
        let mut depth = 1u32;
        while !p.at_eof() && depth > 0 {
            match p.current() {
                L_BRACE => {
                    depth += 1;
                    p.bump();
                }
                R_BRACE => {
                    depth -= 1;
                    if depth > 0 {
                        p.bump();
                    }
                }
                _ => p.bump(),
            }
        }
        if p.current() == R_BRACE {
            p.bump();
        }
        p.finish_node();
        return;
    }

    p.depth += 1;
    loop {
        p.skip_ws();
        while p.at(NEWLINE) {
            p.bump();
            p.skip_ws();
        }
        if p.at(R_BRACE) || p.at_eof() {
            break;
        }
        let before = p.pos();
        block_stmt(p);
        if p.pos() == before {
            p.error_recover("unexpected token in block");
        }
        p.skip_ws();
        if p.at(NEWLINE) {
            p.bump();
        }
    }
    p.skip_ws();
    p.expect(R_BRACE);
    p.depth -= 1;
    p.finish_node();
}

/// Dispatch a single statement inside a `~ { … }` block (T1b §2).
///
/// `if`/`while`/`for`/`break`/`continue`/`in` are contextual (soft)
/// keywords — plain `IDENT` tokens recognized only here by text, so they
/// stay ordinary identifiers everywhere else in the grammar (knot names,
/// variables, function names). `else` is a real, pre-existing ink keyword
/// (already reserved for conditional `else:` branches), reused as-is.
fn block_stmt(p: &mut Parser<'_, '_>) {
    match p.current() {
        KW_RETURN => return_statement(p),
        KW_TEMP => temp_declaration(p),
        IDENT if p.at_kw_text("await") && !is_assignment_ahead(p) => await_statement(p),
        IDENT if p.at_kw_text("if") => if_stmt(p),
        IDENT if p.at_kw_text("while") => while_stmt(p),
        IDENT if p.at_kw_text("for") => for_stmt(p),
        IDENT if p.at_kw_text("break") => {
            p.start_node(BREAK_STMT);
            p.bump();
            p.finish_node();
        }
        IDENT if p.at_kw_text("continue") => {
            p.start_node(CONTINUE_STMT);
            p.bump();
            p.finish_node();
        }
        IDENT if is_assignment_ahead(p) => assignment(p),
        _ => expr_stmt(p),
    }
}

/// `if cond { … } (else if cond { … })* (else { … })?`.
fn if_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(IF_STMT);
    p.bump(); // "if" (IDENT)
    p.skip_ws();
    super::expression::expression(p);
    p.skip_ws();
    stmt_block(p);
    p.skip_ws();
    if p.at(KW_ELSE) {
        p.start_node(ELSE_CLAUSE);
        p.bump(); // else
        p.skip_ws();
        if p.at_kw_text("if") {
            if_stmt(p); // else-if chain
        } else {
            stmt_block(p);
        }
        p.finish_node();
    }
    p.finish_node();
}

/// `while cond { … }`, or the persistent-await form `while await cond { … }`
/// (docs/flow-suspension-spec.md §3). The optional `await` keyword after
/// `while` is consumed here as an ordinary token inside the `WHILE_STMT` — the
/// AST/HIR reads its presence to distinguish the yield-with-policy desugar
/// from a plain loop. `await` stays a contextual soft keyword.
fn while_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(WHILE_STMT);
    p.bump(); // "while" (IDENT)
    p.skip_ws();
    // `while await cond { … }` — the persistent-await loop. Only treat `await`
    // as the marker when a condition expression follows it; `while await { … }`
    // (a plain loop over a variable literally named `await`) has `{` right
    // after, so `await` stays the ordinary loop condition there.
    if p.at_kw_text("await") && p.nth(1) != L_BRACE {
        p.bump(); // "await" (IDENT) — marks the persistent-await loop
        p.skip_ws();
    }
    super::expression::expression(p);
    p.skip_ws();
    stmt_block(p);
    p.finish_node();
}

/// `for name in expr { … }`.
fn for_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(FOR_STMT);
    p.bump(); // "for" (IDENT)
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    p.skip_ws();
    if p.at_kw_text("in") {
        p.bump();
    } else {
        p.error("expected 'in'".into());
    }
    p.skip_ws();
    super::expression::expression(p);
    p.skip_ws();
    stmt_block(p);
    p.finish_node();
}

/// A bare expression statement inside a block (function/external calls).
fn expr_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(EXPR_STMT);
    super::expression::expression(p);
    p.finish_node();
}
