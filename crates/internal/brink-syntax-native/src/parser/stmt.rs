//! The code-ground statement layer: `let`/assignment/expression statements
//! and the `{ stmt; stmt; tail }` statement-block shape.
//!
//! B0.8 Wave A (`docs/decision-log.md` 2026-07-23 "Code-ground sitting",
//! issue #1294): **parser only, no HIR lowering** — this establishes the
//! CST shape the sitting ruled (RustScript-shaped statements, blocks-as-
//! values) so `if`/`while`/`for`/`until` (Wave B) slot into a real
//! dispatcher instead of a stub. Rides `expr.rs`'s expression skeleton —
//! this is the statement *layer* over it, not a replacement.
//!
//! **Reached through the expression grammar**: a statement-block is itself
//! an expression (blocks-as-values ruled — `let x = { … };` is valid), so
//! `expr::atom`'s `L_BRACE` case dispatches here. `var x = { let y = 1; y +
//! 1 }` is the shortest path from `source_file` down to this module — the
//! same shape `expr.rs`'s own tests use to reach `expr::expression` through
//! `var name = <expr>`. This deliberately does **not** touch `flow`/`fn`
//! declaration bodies (`parser/decl.rs`'s `block::block` call) — whether
//! `fn` bodies switch from the content-ground `BLOCK` to `STMT_BLOCK` is
//! the body-dialect seam's own call (`docs/b0-sequencing.md`'s B0.5 note),
//! left to a later wave so this one stays a pure grammar addition with no
//! blast radius on the existing content-ground body tests.

use crate::SyntaxKind::{
    ASSIGN_STMT, DOT, EQ, EXPR_STMT, IDENT, KW_LET, L_BRACE, LET_STMT, R_BRACE, SEMICOLON,
    STMT_BLOCK,
};

use super::Parser;

/// `{ stmt* tail? }` — the code-ground body shape. Mirrors
/// `block::braced_item_list`'s depth-guard structure (adversarial/over-deep
/// nesting must never blow the stack, CLAUDE.md "guard against unbounded
/// growth"), but with statement dispatch instead of item/body-line dispatch.
pub(crate) fn stmt_block(p: &mut Parser<'_, '_>) {
    p.start_node(STMT_BLOCK);
    p.expect(L_BRACE);

    if p.enter_depth() {
        loop {
            p.skip_ws_and_newlines();
            if p.at(R_BRACE) || p.at_eof() {
                break;
            }
            let before = p.pos();
            let continues = statement(p);
            if p.pos() == before {
                p.error_recover("unexpected token in statement block");
                continue;
            }
            if !continues {
                // An unterminated expression — the block's tail. Nothing
                // else can follow it (blocks-as-values: the tail is always
                // last); stop without consuming `R_BRACE` itself.
                break;
            }
        }
        p.exit_depth();
    } else {
        // Depth limit already recorded an error; still consume to the
        // matching close as best-effort, one token at a time (mirrors
        // `block::braced_item_list`'s own over-deep fallback).
        while !p.at(R_BRACE) && !p.at_eof() {
            p.skip_ws_and_newlines();
            if p.at(R_BRACE) || p.at_eof() {
                break;
            }
            let before = p.pos();
            p.error_recover("skipped inside over-deep statement block");
            if p.pos() == before {
                break;
            }
        }
    }

    p.expect(R_BRACE);
    p.finish_node();
}

/// Parse one statement (or the block's tail expression) at the current
/// position. Returns `true` if the block should keep looping (a `;`-
/// terminated `LET_STMT`/`ASSIGN_STMT`/`EXPR_STMT` was produced), `false`
/// if what was parsed is the tail — an unterminated expression, which must
/// be the last thing in the enclosing `STMT_BLOCK`.
fn statement(p: &mut Parser<'_, '_>) -> bool {
    if p.at(KW_LET) {
        let_stmt(p);
        return true;
    }
    if at_assignment(p) {
        assign_stmt(p);
        return true;
    }
    expr_or_tail_stmt(p)
}

/// `let name = expr;` (initializer optional — `let name;` is legal too, the
/// issue's own hedge on the ruled `let x = e` shape). Distinct from
/// `var`/`const` (`parser/decl.rs`): those are declaration-layer and
/// terminator-free; `let` is code-ground and always `;`-terminated.
fn let_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(LET_STMT);
    p.bump(); // KW_LET
    p.skip_ws();
    p.expect(IDENT);
    p.skip_ws();
    if p.eat(EQ) {
        p.skip_ws();
        super::expr::expression(p);
    }
    p.skip_ws_and_newlines();
    p.expect(SEMICOLON);
    p.finish_node();
}

/// Lookahead-only: `true` if the current position starts an assignment's
/// place path — `IDENT (DOT IDENT)*` immediately followed by a bare `=`
/// (not `==`, which lexes as its own `EQ_EQ` token, so no ambiguity with a
/// comparison expression statement). A call (`foo()`) or any other
/// expression shape never matches, since only a dotted place path can be
/// followed directly by `=` under this check.
fn at_assignment(p: &Parser<'_, '_>) -> bool {
    if !p.at(IDENT) {
        return false;
    }
    let mut n = 1;
    while p.nth(n) == DOT && p.nth(n + 1) == IDENT {
        n += 2;
    }
    p.nth(n) == EQ
}

/// `x = expr;` / `x.field = expr;` — a read-modify-write place path
/// (decision-log 2026-07-23: "assignment `x = e` / `x.field = e` (RMW
/// paths)"). The LHS reuses the expression grammar's dotted `PATH` — no
/// `::`-crossing, since `at_assignment`'s lookahead only ever commits here
/// on a `DOT`-only chain (an assignable place is always local, never a
/// module path).
fn assign_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(ASSIGN_STMT);
    super::expr::path(p);
    p.skip_ws();
    p.expect(EQ);
    p.skip_ws();
    super::expr::expression(p);
    p.skip_ws_and_newlines();
    p.expect(SEMICOLON);
    p.finish_node();
}

/// A bare expression at statement position. `<expr>;` wraps as `EXPR_STMT`
/// and the block keeps going; `<expr>` with no trailing `;` is the block's
/// tail value (blocks-as-values) — left as a bare, unwrapped child node
/// (mirrors how `ElseBranch`/`MatchArm` already leave their own bare-
/// expression children unwrapped in this grammar), and the caller's loop
/// stops. Returns `true` for the `EXPR_STMT` case, `false` for the tail.
fn expr_or_tail_stmt(p: &mut Parser<'_, '_>) -> bool {
    let checkpoint = p.checkpoint();
    super::expr::expression(p);
    p.skip_ws_and_newlines();
    if p.eat(SEMICOLON) {
        p.start_node_at(checkpoint, EXPR_STMT);
        p.finish_node();
        true
    } else {
        false
    }
}
