//! The code-ground control-flow layer: `if`/`else`, `while`, `for … in …`,
//! `until`.
//!
//! B0.8 Wave B (`docs/decision-log.md` 2026-07-23 "Code-ground sitting",
//! issue #1177). Rides Wave A's statement infrastructure verbatim —
//! `parser/stmt.rs`'s `statement()` dispatcher gains the four branches
//! below, and every construct's body reuses `stmt::stmt_block` unchanged
//! (no second block shape for control-flow bodies). Mirrors
//! `brink-syntax`'s `~ { … }` T1b grammar (`parser/logic.rs`'s
//! `if_stmt`/`while_stmt`/`for_stmt`) structurally — that grammar is this
//! one's differential partner (`crates/internal/brink-ir/tests/
//! b08_native_control_flow.rs`) — with one difference: native hard-reserves
//! `if`/`while`/`for`/`in`/`until` as real lexer keywords (`syntax_kind.rs`
//! Finding #1's "`RustScript` reserves globally" posture), where the
//! brink-dialect treats them as contextual soft keywords matched by IDENT
//! text. `until` has no brink-dialect counterpart at all — it is native's
//! sole condition-park spelling, retiring `await` (decision-log item 4).

use crate::SyntaxKind::{
    ELSE_CLAUSE, FOR_STMT, IDENT, IF_STMT, KW_ELSE, KW_IF, KW_IN, SEMICOLON, UNTIL_STMT, WHILE_STMT,
};

use super::Parser;

/// Parse a control-flow *head* expression — one directly followed by a `{`
/// that opens the construct's body, so a trailing `TypeName { … }`
/// construction literal (B5, issue #1464) must not swallow that brace.
/// Rust's `no-struct-literal` restriction is the precedent; `(…)` restores
/// the literal form (`while (Weighted { 1: a }) == w { … }`).
fn head_expression(p: &mut Parser<'_, '_>) {
    let saved = p.set_no_construct_literal(true);
    super::expr::expression(p);
    p.set_no_construct_literal(saved);
}

/// `if cond { … } (else if cond { … } | else { … })?`.
pub(crate) fn if_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(IF_STMT);
    p.bump(); // KW_IF
    p.skip_ws();
    head_expression(p);
    p.skip_ws();
    super::stmt::stmt_block(p);
    p.skip_ws();
    if p.at(KW_ELSE) {
        else_clause(p);
    }
    p.finish_node();
}

/// `else if cond { … }` (a chained `IF_STMT`, no `STMT_BLOCK` wrapper of
/// its own — mirrors `family.rs::else_branch`'s flat-chain precedent) or
/// `else { … }`.
fn else_clause(p: &mut Parser<'_, '_>) {
    p.start_node(ELSE_CLAUSE);
    p.expect(KW_ELSE);
    p.skip_ws();
    if p.at(KW_IF) {
        if_stmt(p);
    } else {
        super::stmt::stmt_block(p);
    }
    p.finish_node();
}

/// `while cond { … }`. Always a plain loop — native has no `await` keyword
/// to spell a persistent-await variant with (see module doc).
pub(crate) fn while_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(WHILE_STMT);
    p.bump(); // KW_WHILE
    p.skip_ws();
    head_expression(p);
    p.skip_ws();
    super::stmt::stmt_block(p);
    p.finish_node();
}

/// `for name in expr { … }` — single-binding iteration (the existing HIR
/// `ForStmt` shape; no destructuring).
pub(crate) fn for_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(FOR_STMT);
    p.bump(); // KW_FOR
    p.skip_ws();
    p.expect(IDENT);
    p.skip_ws();
    p.expect(KW_IN);
    p.skip_ws();
    head_expression(p);
    p.skip_ws();
    super::stmt::stmt_block(p);
    p.finish_node();
}

/// `until <cond>;` — the condition-park statement (decision-log item 4).
/// Always `;`-terminated, like every other code-ground statement
/// (`stmt.rs`'s `let_stmt`/`assign_stmt` precedent) — it has no body of its
/// own to delimit it, unlike `if`/`while`/`for`.
pub(crate) fn until_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(UNTIL_STMT);
    p.bump(); // KW_UNTIL
    p.skip_ws();
    super::expr::expression(p);
    p.skip_ws_and_newlines();
    p.expect(SEMICOLON);
    p.finish_node();
}
