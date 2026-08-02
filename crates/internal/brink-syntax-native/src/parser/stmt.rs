//! The code-ground statement layer: `let`/assignment (plain and
//! compound)/expression/`return`/`break`/`continue` statements, `if`/
//! `while`/`for`/`until` control flow (`control_flow.rs`), and the
//! `{ stmt; stmt; tail }` statement-block shape.
//!
//! B0.8 Wave A (`docs/decision-log.md` 2026-07-23 "Code-ground sitting",
//! issue #1294) established the CST shape the sitting ruled (`RustScript`-
//! shaped statements, blocks-as-values) as **parser only, no HIR
//! lowering**, with `statement()`'s dispatcher deliberately left ready for
//! `if`/`while`/`for`/`until` to slot in as real branches rather than a
//! stub. B0.8 Wave B (issue #1177) is that slice: `control_flow.rs`'s four
//! constructs are dispatched from `statement()` below, and (unlike Wave A)
//! this wave also lowers to HIR (`brink-ir::hir::lower_native::
//! control_flow`) — see that module's doc for the NF-2 fence. B0.8 Wave B
//! *tail* (issue #1322) fills in the rest of the ruled surface #1177
//! didn't cover: `return e`/`break`/`continue` (below, this module) and
//! compound/RMW assignment (`at_assignment`/`assign_stmt`, below) — `#fn`
//! function values remain out of scope, and UFCS *resolution* is the
//! analyzer's (issue #1482), not this layer's; see
//! `hir::lower_native::expr`'s module doc. Rides `expr.rs`'s expression
//! skeleton — this is the statement *layer* over it, not a replacement.
//!
//! **Reached through the expression grammar**: a statement-block is itself
//! an expression (blocks-as-values ruled — `let x = { … };` is valid), so
//! `expr::atom`'s `L_BRACE` case dispatches here. `var x = { let y = 1; y +
//! 1 }` is the shortest path from `source_file` down to this module — the
//! same shape `expr.rs`'s own tests use to reach `expr::expression` through
//! `var name = <expr>`.
//!
//! **Also reached through `flow`/`fn` declaration bodies** (`parser/
//! decl.rs`'s `decl_body`, issue #1309, charter §4 RULED 2026-07-23): the
//! body-dialect seam's per-keyword default routes a `fn`'s plain `{ }` (and
//! a `flow`'s `~{ }` "Compound guard" override) through `stmt_block` below,
//! same grammar, same node kind — `flow`'s own plain-`{ }` default (and
//! `fn`'s `>{ }` override) still route through the content-ground `BLOCK`
//! (`block::block`).

use crate::SyntaxKind::{
    ASSIGN_STMT, BREAK_STMT, CONTINUE_STMT, DOT, EOF, EQ, EXPR_STMT, IDENT, KW_BREAK, KW_CONTINUE,
    KW_FOR, KW_IF, KW_LET, KW_RETURN, KW_UNTIL, KW_WHILE, L_BRACE, LET_STMT, LOGIC_LINE, MINUS_EQ,
    NEWLINE, PLUS_EQ, R_BRACE, RETURN_STMT, SEMICOLON, STMT_BLOCK,
};

use super::Parser;

/// `{ stmt* tail? }` — the code-ground body shape. Mirrors
/// `block::braced_item_list`'s depth-guard structure (adversarial/over-deep
/// nesting must never blow the stack, CLAUDE.md "guard against unbounded
/// growth"), but with statement dispatch instead of item/body-line dispatch.
pub(crate) fn stmt_block(p: &mut Parser<'_, '_>) {
    p.start_node(STMT_BLOCK);
    p.expect(L_BRACE);
    // Once inside a block body the `TypeName { … }` restriction a
    // control-flow head may have set no longer applies — a statement here
    // is not the head expression (`if a { let m = Map { "k": 1 }; }`).
    let saved = p.set_no_construct_literal(false);

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

    p.set_no_construct_literal(saved);
    p.expect(R_BRACE);
    p.finish_node();
}

/// Parse one statement (or the block's tail expression) at the current
/// position. Returns `true` if the block should keep looping (a `;`-
/// terminated `LET_STMT`/`ASSIGN_STMT`/`EXPR_STMT`/`UNTIL_STMT`/
/// `RETURN_STMT`/`BREAK_STMT`/`CONTINUE_STMT`, or a brace-delimited
/// `IF_STMT`/`WHILE_STMT`/`FOR_STMT`, was produced), `false` if what was
/// parsed is the tail — an unterminated expression, which must be the last
/// thing in the enclosing `STMT_BLOCK`. Control-flow constructs and the
/// three B0.8 Wave B tail additions never produce a value (no case for any
/// of them exists on the blocks-as-values tail position — see
/// `ast::StmtBlock::tail`'s doc), so they always return `true` here, same
/// as the `;`-terminated statements.
fn statement(p: &mut Parser<'_, '_>) -> bool {
    if p.at(KW_LET) {
        let_stmt(p);
        return true;
    }
    if p.at(KW_IF) {
        super::control_flow::if_stmt(p);
        return true;
    }
    if p.at(KW_WHILE) {
        super::control_flow::while_stmt(p);
        return true;
    }
    if p.at(KW_FOR) {
        super::control_flow::for_stmt(p);
        return true;
    }
    if p.at(KW_UNTIL) {
        super::control_flow::until_stmt(p);
        return true;
    }
    if p.at(KW_RETURN) {
        return_stmt(p);
        return true;
    }
    if p.at(KW_BREAK) {
        break_stmt(p);
        return true;
    }
    if p.at(KW_CONTINUE) {
        continue_stmt(p);
        return true;
    }
    if at_assignment(p) {
        assign_stmt(p);
        return true;
    }
    expr_or_tail_stmt(p)
}

/// `let name: type = expr;` (both the annotation and the initializer
/// optional — `let name;` is legal too, the issue's own hedge on the ruled
/// `let x = e` shape). Distinct from `var`/`const` (`parser/decl.rs`):
/// those are declaration-layer and terminator-free; `let` is code-ground
/// and always `;`-terminated. The `: type` clause is the same one they
/// take, in the same slot (NG-B, issue #1488) — hence the shared
/// `decl::binding_annotation`.
fn let_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(LET_STMT);
    p.bump(); // KW_LET
    p.skip_ws();
    p.expect(IDENT);
    p.skip_ws();
    super::decl::binding_annotation(p);
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
/// place path — `IDENT (DOT IDENT)*` immediately followed by a bare `=`,
/// `+=`, or `-=` (not `==`, which lexes as its own `EQ_EQ` token, so no
/// ambiguity with a comparison expression statement). A call (`foo()`) or
/// any other expression shape never matches, since only a dotted place path
/// can be followed directly by an assignment operator under this check.
/// `+=`/`-=` (B0.8 Wave B tail, issue #1322, decision-log 2026-07-23
/// "Code-ground sitting": "compound/RMW assignment") mirror the
/// brink-dialect's own `is_assignment_ahead` lookahead
/// (`brink-syntax/src/parser/logic.rs`), which recognizes the same three
/// operators.
fn at_assignment(p: &Parser<'_, '_>) -> bool {
    if !p.at(IDENT) {
        return false;
    }
    let mut n = 1;
    while p.nth(n) == DOT && p.nth(n + 1) == IDENT {
        n += 2;
    }
    matches!(p.nth(n), EQ | PLUS_EQ | MINUS_EQ)
}

/// `x = expr;` / `x.field = expr;` (and the `+=`/`-=` compound-assign
/// forms) — a read-modify-write place path (decision-log 2026-07-23:
/// "assignment `x = e` / `x.field = e` (RMW paths)"; compound assignment
/// added B0.8 Wave B tail, issue #1322). The LHS reuses the expression
/// grammar's dotted `PATH` — no `::`-crossing, since `at_assignment`'s
/// lookahead only ever commits here on a `DOT`-only chain (an assignable
/// place is always local, never a module path). Whichever of `=`/`+=`/`-=`
/// `at_assignment` matched is bumped as-is — `ast::AssignStmt::op_token`
/// reads it back; a caller that reaches this function without one of those
/// three at the current position (shouldn't happen given the dispatcher's
/// `at_assignment` guard) falls back to `expect(EQ)`, which still records a
/// diagnostic and keeps the node shape well-formed.
fn assign_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(ASSIGN_STMT);
    super::expr::path(p);
    p.skip_ws();
    if matches!(p.current(), EQ | PLUS_EQ | MINUS_EQ) {
        p.bump();
    } else {
        p.expect(EQ);
    }
    p.skip_ws();
    super::expr::expression(p);
    p.skip_ws_and_newlines();
    p.expect(SEMICOLON);
    p.finish_node();
}

/// `return expr?;` — the code-ground value-return statement (B0.8 Wave B
/// tail, issue #1322, decision-log 2026-07-23 "Code-ground sitting" item
/// 1: "return e"). Reuses the SAME `RETURN_STMT` node kind the
/// content-ground bare `return`/`return -> x` already use
/// (`parser/divert.rs::return_stmt`) — see that `SyntaxKind` variant's doc
/// for why one node shape safely serves both grammars. Unlike the
/// content-ground form, this one is always `;`-terminated (the statement
/// layer's uniform terminator) and has no tunnel-redirect (`return -> x`)
/// counterpart — that respelling is a content-ground/tunnel concept with
/// no code-ground meaning.
fn return_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(RETURN_STMT);
    p.bump(); // KW_RETURN
    p.skip_ws();
    if !p.at(SEMICOLON) {
        super::expr::expression(p);
    }
    p.skip_ws_and_newlines();
    p.expect(SEMICOLON);
    p.finish_node();
}

/// `break;` — loop-exit statement (B0.8 Wave B tail, issue #1322). Legal
/// only inside a `while`/`for` body — enforcing that is `brink-analyzer`'s
/// job (E057), not this grammar's, mirroring the brink-dialect's own
/// `BreakStmt` (a bare keyword, no in-loop check at parse time either).
fn break_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(BREAK_STMT);
    p.bump(); // KW_BREAK
    p.skip_ws_and_newlines();
    p.expect(SEMICOLON);
    p.finish_node();
}

/// `continue;` — loop-skip statement (B0.8 Wave B tail, issue #1322). See
/// [`break_stmt`]'s doc for the same in-loop caveat.
fn continue_stmt(p: &mut Parser<'_, '_>) {
    p.start_node(CONTINUE_STMT);
    p.bump(); // KW_CONTINUE
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

// ── The content-ground line escape: `~ stmt` (charter §8.2, RULED ────
// ── 2026-07-23, issue #1991) ──────────────────────────────────────────
//
// Ink's logic line, kept: `~ stmt` runs code inside an otherwise
// content-ground (prose) body. Reached from `block::body_line`'s (and
// `family::colon_body_line`'s — kept in sync, see that function's doc)
// `TILDE` dispatch arm — never reached from `stmt_block`'s own per-
// statement loop, which recognizes a bare `~` nowhere in its dispatch.

/// `~ stmt` — parse one logic line. Only assignment (`at_assignment`,
/// above) and a bare expression (evaluated for its side effect — e.g. a
/// function call) have content-ground meaning here; every other code-ground
/// statement shape (`let`, `if`, `while`, `for`, `until`, `return`, `break`,
/// `continue`) either already has its own content-ground spelling reachable
/// without a `~` (bare `return`/diverts are `body_line` keywords in their
/// own right) or has no content-ground meaning at all — reaching for one of
/// those here falls through to [`expr_stmt_line`], whose `expr::expression`
/// call diagnoses an unrecognized leading keyword loudly (`expr::atom`'s
/// "expected an expression" fallback) rather than silently swallowing it,
/// per issue #1991's own hedge ("if the decision is instead ... it must be
/// a diagnostic, never silent prose").
///
/// The `NEWLINE`/EOF that ends the line is left **unconsumed** by the two
/// callees below and bumped here, as a sibling of `LOGIC_LINE`, exactly
/// like `content::content_line`'s own trailing-newline handling — the line
/// escape is a content-ground line, terminated by end-of-line, never by the
/// code-ground `;` (`ASSIGN_STMT`/`EXPR_STMT` are reused unmodified from
/// `stmt_block`'s grammar above — see `SyntaxKind::RETURN_STMT`'s doc for
/// the established one-node-two-grammars precedent this follows).
pub(crate) fn logic_line(p: &mut Parser<'_, '_>) {
    p.start_node(LOGIC_LINE);
    p.bump(); // TILDE
    p.skip_ws();
    let before = p.pos();
    if at_assignment(p) {
        assign_line(p);
    } else {
        expr_stmt_line(p);
    }
    // A statement/expression grammar that recognized NOTHING at all (an
    // unsupported shape — `if`/`while`/`for`/`until`/`let`/`return`/
    // `break`/`continue`, none of which `expr::expression`'s atom accepts,
    // already raised a diagnostic there) makes zero progress past `before`.
    // Left alone, the still-unconsumed tokens would be handed back to
    // `body_line`'s next loop iteration and re-dispatched — for anything
    // that isn't itself a fresh structural item, that fallback is the
    // prose scanner, which is exactly the silent swallow issue #1991 is
    // about. Consume the rest of THIS physical line here instead, inside
    // `LOGIC_LINE` itself, so a malformed logic line is always loud (one
    // `error_recover` diagnostic per stray token, the same idiom
    // `stmt_block`'s own recovery loop above uses) and never silently
    // reclassified as story text.
    if p.pos() == before {
        while !matches!(p.current(), NEWLINE | EOF) {
            let stuck = p.pos();
            p.error_recover("unsupported logic-line shape");
            if p.pos() == stuck {
                break;
            }
        }
    }
    p.finish_node();
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}

/// `~ x = expr` / `~ x += expr` / `~ x -= expr` — identical to
/// [`assign_stmt`] except for the terminator: this is a content-ground
/// line, so it stops at `NEWLINE`/EOF, never `;` (see [`logic_line`]'s
/// doc).
fn assign_line(p: &mut Parser<'_, '_>) {
    p.start_node(ASSIGN_STMT);
    super::expr::path(p);
    p.skip_ws();
    if matches!(p.current(), EQ | PLUS_EQ | MINUS_EQ) {
        p.bump();
    } else {
        p.expect(EQ);
    }
    p.skip_ws();
    super::expr::expression(p);
    p.finish_node();
}

/// `~ expr` — an expression evaluated for its side effect (a function call
/// being the overwhelmingly common case). The content-ground counterpart of
/// [`expr_or_tail_stmt`]'s `EXPR_STMT` case, minus the `;` terminator (see
/// [`logic_line`]'s doc) — every `~ stmt` that isn't recognized as an
/// assignment reaches here, so a malformed or unsupported logic line still
/// gets a real (if generic) diagnostic from `expr::expression` rather than
/// silently falling through to nothing.
fn expr_stmt_line(p: &mut Parser<'_, '_>) {
    p.start_node(EXPR_STMT);
    super::expr::expression(p);
    p.finish_node();
}
