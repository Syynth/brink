//! Declaration-header grammar: `flow`/`fn` + the decl keywords
//! (`var const flags struct extern import use module`).
//!
//! Every `at_*` lookahead below exists for one reason (Finding #5, recorded
//! in the B0.5 report): unlike ink's `===`-delimited knot headers, this
//! grammar has **no unambiguous declaration sigil** — a hard keyword
//! (`flow`, `var`, …) can collide with a prose line that happens to start
//! with that exact lowercase word (`"flow through the garden."`). Every
//! declaration head therefore requires a positive multi-token lookahead —
//! `KW IDENT` plus the token that *must* follow a real declaration name
//! (`(`/`{` for a body, `=` for an initializer) — before committing;
//! failing the check falls through to prose content instead of emitting a
//! spurious parse error on ordinary text. `import`/`use` keep the weaker
//! two-token check (no third-token shape to lean on — flagged as the
//! residual risk in the same finding).

use crate::SyntaxKind::{
    COLON, COMMA, CONST_DECL, EQ, EXTERN_DECL, FLAGS_DECL, FLAGS_MEMBER, FLAGS_MEMBER_LIST,
    FLOW_DECL, FN_DECL, IDENT, IMPORT_DECL, KW_AS, KW_EXTERN, KW_FLAGS, KW_FLOW, KW_FN, KW_MODULE,
    KW_REF, KW_STRUCT, L_BRACE, L_PAREN, MODULE_DECL, PARAM, PARAM_LIST, R_BRACE, R_PAREN,
    STRUCT_DECL, STRUCT_FIELD, USE_DECL, USE_TREE, USE_TREE_LIST, VAR_DECL,
};

use super::Parser;

// ── Lookahead guards ─────────────────────────────────────────────────

pub(crate) fn at_flow_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_FLOW) && p.nth(1) == IDENT && matches!(p.nth(2), L_PAREN | L_BRACE)
}

pub(crate) fn at_fn_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_FN) && p.nth(1) == IDENT && matches!(p.nth(2), L_PAREN | L_BRACE)
}

/// Shared guard for `var`/`const` — the caller has already checked
/// `p.at(KW_VAR)`/`p.at(KW_CONST)`.
pub(crate) fn at_binding_decl(p: &Parser<'_, '_>) -> bool {
    p.nth(1) == IDENT && p.nth(2) == EQ
}

pub(crate) fn at_flags_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_FLAGS) && p.nth(1) == IDENT && p.nth(2) == EQ
}

pub(crate) fn at_struct_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_STRUCT) && p.nth(1) == IDENT && p.nth(2) == L_BRACE
}

pub(crate) fn at_extern_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_EXTERN) && p.nth(1) == IDENT && p.nth(2) == L_PAREN
}

/// Caller has already checked `p.at(KW_IMPORT)`. Weaker check (Finding
/// #5): `import` has no required third-token shape to lean on.
pub(crate) fn at_import_decl(p: &Parser<'_, '_>) -> bool {
    p.nth(1) == IDENT
}

/// Caller has already checked `p.at(KW_USE)`. Weaker check (Finding #5):
/// same residual risk as `import`.
pub(crate) fn at_use_decl(p: &Parser<'_, '_>) -> bool {
    matches!(p.nth(1), IDENT | crate::SyntaxKind::COLON_COLON)
}

pub(crate) fn at_module_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_MODULE) && p.nth(1) == IDENT && p.nth(2) == L_BRACE
}

// ── flow / fn ─────────────────────────────────────────────────────────

/// `flow name(params) { … }` — nested `flow` = a stitch (charter §4). `doc`
/// is a leading `///` run already consumed by `block::item`
/// (`doc_comment::maybe_consume_leading_run`), threaded through so it
/// attaches as this node's leading `DOC_COMMENT` child (B0.6b).
pub(crate) fn flow_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, FLOW_DECL, doc);
    p.bump(); // KW_FLOW
    p.expect(IDENT);
    if p.at(L_PAREN) {
        param_list(p);
    }
    if p.at(L_BRACE) {
        super::block::block(p);
    } else {
        p.error("expected a braced body after the flow header".into());
    }
    p.finish_node();
}

/// `fn name(params) { … }`. See [`flow_decl`]'s doc comment for `doc`.
pub(crate) fn fn_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, FN_DECL, doc);
    p.bump(); // KW_FN
    p.expect(IDENT);
    if p.at(L_PAREN) {
        param_list(p);
    }
    if p.at(L_BRACE) {
        super::block::block(p);
    } else {
        p.error("expected a braced body after the fn header".into());
    }
    p.finish_node();
}

/// `(param, ref param, …)` — shared by `flow`/`fn`/`extern` headers. Types
/// are out of B0.5's scope (b0-sequencing lists them under B0.6, not
/// B0.5) — a bare `ref`? `IDENT` is the full shape here.
fn param_list(p: &mut Parser<'_, '_>) {
    p.start_node(PARAM_LIST);
    p.expect(L_PAREN);
    p.skip_ws_and_newlines();
    while p.peek_skip_nl() != R_PAREN && !p.at_eof() {
        let before = p.pos();
        param(p);
        if p.pos() == before {
            p.error_recover("unexpected token in parameter list");
            continue;
        }
        p.skip_ws_and_newlines();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws_and_newlines();
    }
    p.expect(R_PAREN);
    p.finish_node();
}

fn param(p: &mut Parser<'_, '_>) {
    p.start_node(PARAM);
    p.eat(KW_REF);
    p.expect(IDENT);
    p.finish_node();
}

// ── var / const ───────────────────────────────────────────────────────

/// `var name = expr`. No statement terminator is required (Finding #6:
/// sitting-2/code-ground statement termination is explicitly "own sitting
/// pending" per charter §7 — this skeleton treats `NEWLINE`/`}`/EOF as the
/// terminator, Rust-`;`-free, and flags the choice rather than inventing a
/// semicolon the charter never ruled).
pub(crate) fn var_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, VAR_DECL, doc);
    p.bump(); // KW_VAR
    p.expect(IDENT);
    if p.eat(crate::SyntaxKind::EQ) {
        super::expr::expression(p);
    }
    p.finish_node();
}

/// `const name = expr`.
pub(crate) fn const_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, CONST_DECL, doc);
    p.bump(); // KW_CONST
    p.expect(IDENT);
    if p.eat(crate::SyntaxKind::EQ) {
        super::expr::expression(p);
    }
    p.finish_node();
}

// ── flags ────────────────────────────────────────────────────────────

/// `flags Name = (member), member, …` (charter §11: renamed `LIST`).
pub(crate) fn flags_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, FLAGS_DECL, doc);
    p.bump(); // KW_FLAGS
    p.expect(IDENT);
    p.expect(crate::SyntaxKind::EQ);
    flags_member_list(p);
    p.finish_node();
}

fn flags_member_list(p: &mut Parser<'_, '_>) {
    p.start_node(FLAGS_MEMBER_LIST);
    // `flags F = ()` — the explicit empty set (LIST parity, ruled #1260 for
    // #1262). Checked before the member loop so it's never misread as a
    // parenthesized-member shape (`(name)`) with a missing `IDENT`.
    if p.at(L_PAREN) && p.nth(1) == R_PAREN {
        p.eat(L_PAREN);
        p.eat(R_PAREN);
        p.finish_node();
        return;
    }
    // Whether at least one real `FLAGS_MEMBER` has been parsed yet — a
    // completely empty list (bare `flags F =` with nothing member-shaped
    // following) is the one case that must error rather than silently
    // accept zero members (ruled #1260: every sibling recovery path here —
    // `param_list`, `struct_decl`'s body loop — calls `error_recover` on
    // zero progress; this was the sole exception).
    let mut has_member = false;
    loop {
        let before = p.pos();
        flags_member(p);
        if p.pos() == before {
            if !has_member {
                p.skip_ws();
                p.error_recover(
                    "expected a flags member after `=` (use `()` for an explicit empty set)",
                );
            }
            break;
        }
        has_member = true;
        if !p.eat(COMMA) {
            break;
        }
    }
    p.finish_node();
}

fn flags_member(p: &mut Parser<'_, '_>) {
    if !p.at(IDENT) && !p.at(L_PAREN) {
        return;
    }
    p.start_node(FLAGS_MEMBER);
    if p.eat(L_PAREN) {
        p.expect(IDENT);
        p.expect(R_PAREN);
    } else {
        p.expect(IDENT);
    }
    p.finish_node();
}

// ── struct ───────────────────────────────────────────────────────────

/// `struct Name { field: Type, … }`.
pub(crate) fn struct_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, STRUCT_DECL, doc);
    p.bump(); // KW_STRUCT
    p.expect(IDENT);
    p.expect(L_BRACE);
    p.skip_ws_and_newlines();
    while p.peek_skip_nl() != R_BRACE && !p.at_eof() {
        let before = p.pos();
        struct_field(p);
        if p.pos() == before {
            p.error_recover("unexpected token in struct body");
            p.skip_ws_and_newlines();
            continue;
        }
        p.skip_ws_and_newlines();
        p.eat(COMMA);
        p.skip_ws_and_newlines();
    }
    p.expect(R_BRACE);
    p.finish_node();
}

fn struct_field(p: &mut Parser<'_, '_>) {
    if !p.at(IDENT) {
        return;
    }
    p.start_node(STRUCT_FIELD);
    p.expect(IDENT);
    p.expect(COLON);
    super::expr::path(p);
    p.finish_node();
}

// ── extern ───────────────────────────────────────────────────────────

/// `extern name(params)` — no body, ever (kept from ink's `EXTERNAL`).
pub(crate) fn extern_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, EXTERN_DECL, doc);
    p.bump(); // KW_EXTERN
    p.expect(IDENT);
    if p.at(L_PAREN) {
        param_list(p);
    }
    p.finish_node();
}

// ── import ───────────────────────────────────────────────────────────

/// `import path` (Finding #3: the charter doesn't separately spell an
/// `import` grammar distinct from `use` — this is the minimal reasonable
/// shape for the token-set bullet that lists it as its own keyword).
pub(crate) fn import_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, IMPORT_DECL, doc);
    p.bump(); // KW_IMPORT
    super::expr::path(p);
    p.finish_node();
}

// ── use ──────────────────────────────────────────────────────────────

/// `use path::{a, b as c};` — Rust's `use` syntax lifted verbatim (charter
/// §13.2's literal example includes the trailing `;`). Unlike every other
/// declaration in this skeleton (Finding #6: no statement terminator
/// required elsewhere), `use` recognizes an *optional* trailing `;` —
/// optional rather than required because this skeleton has no semantic
/// gate to reject its absence yet, but recognized (not left to fall
/// through as unrelated prose, which a semicolon with no other role in the
/// grammar would otherwise silently do).
pub(crate) fn use_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, USE_DECL, doc);
    p.bump(); // KW_USE
    use_tree(p);
    p.eat(crate::SyntaxKind::SEMICOLON);
    p.finish_node();
}

fn use_tree(p: &mut Parser<'_, '_>) {
    p.start_node(USE_TREE);
    if p.at(IDENT) {
        p.expect(IDENT);
        loop {
            if p.at(crate::SyntaxKind::COLON_COLON) {
                if p.nth(1) == L_BRACE {
                    p.expect(crate::SyntaxKind::COLON_COLON);
                    use_tree_list(p);
                    break;
                }
                p.expect(crate::SyntaxKind::COLON_COLON);
                p.expect(IDENT);
            } else {
                break;
            }
        }
        if p.eat(KW_AS) {
            p.expect(IDENT);
        }
    } else if p.at(L_BRACE) {
        use_tree_list(p);
    } else {
        p.error("expected a path or `{` in a use tree".into());
    }
    p.finish_node();
}

fn use_tree_list(p: &mut Parser<'_, '_>) {
    p.start_node(USE_TREE_LIST);
    p.expect(L_BRACE);
    p.skip_ws_and_newlines();
    while p.peek_skip_nl() != R_BRACE && !p.at_eof() {
        let before = p.pos();
        use_tree(p);
        if p.pos() == before {
            p.error_recover("unexpected token in use-tree list");
            p.skip_ws_and_newlines();
            continue;
        }
        p.skip_ws_and_newlines();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws_and_newlines();
    }
    p.expect(R_BRACE);
    p.finish_node();
}

// ── module ───────────────────────────────────────────────────────────

/// `module name { … }` — a nested module block (charter §13.2).
pub(crate) fn module_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, MODULE_DECL, doc);
    p.bump(); // KW_MODULE
    p.expect(IDENT);
    if p.at(L_BRACE) {
        super::block::block(p);
    } else {
        p.error("expected a braced body after the module header".into());
    }
    p.finish_node();
}
