use crate::SyntaxKind::{
    self, AT_L_BRACKET, BLOCK, DIVERT, EOF, HASH, KW_CONST, KW_IMPORT, KW_RETURN, KW_USE, KW_VAR,
    L_BRACE, NEWLINE, R_BRACE,
};

use super::Parser;

/// Parse a brace-delimited body: `{ item* }`, wrapped in a `BLOCK` node.
///
/// The universal body delimiter (charter §4) — used for `flow`/`fn`/
/// `module` bodies.
pub(crate) fn block(p: &mut Parser<'_, '_>) {
    braced_item_list(p, BLOCK);
}

/// Shared implementation: `{ item* }` wrapped in `kind`. Also used by
/// `choice.rs` for `CHOICE_BODY`/`ELSE_BRANCH`, which are braces-as-body in
/// exactly the same shape (charter §4: braces are the universal body
/// delimiter) and need the same depth guard against adversarial nesting.
pub(crate) fn braced_item_list(p: &mut Parser<'_, '_>, kind: SyntaxKind) {
    p.start_node(kind);
    p.expect(L_BRACE);

    // B0.6b inner-doc attachment: only `BLOCK` bodies (flow/fn/module),
    // not `CHOICE_BODY`/`ELSE_BRANCH` — a `//!` at the top of a choice
    // body has no "enclosing container" in the doc-comment sense the
    // ruling describes. Must run before the depth guard below so a
    // `//!` run is recognized even at `MAX_DEPTH`-adjacent nesting.
    //
    // "At the start of the body" tolerates leading blank lines (bare
    // `NEWLINE`s, including the one that ordinarily terminates the `{`
    // line itself) before the run — only real content disqualifies it;
    // every `NEWLINE` skipped here is bumped bare, the exact same shape
    // `body_line`'s own `NEWLINE => p.bump()` case would have produced one
    // main-loop iteration at a time, so this is not an observable CST
    // change for the blank-line tokens themselves.
    if kind == BLOCK {
        loop {
            p.skip_ws();
            if p.current() == NEWLINE {
                p.bump();
            } else {
                break;
            }
        }
        super::doc_comment::maybe_consume_inner_run(p);
    }

    if p.enter_depth() {
        loop {
            p.skip_ws();
            if p.at(R_BRACE) || p.at_eof() {
                break;
            }
            let before = p.pos();
            item(p);
            if p.pos() == before {
                p.error_recover("unexpected token inside block");
            }
        }
        p.exit_depth();
    } else {
        // Depth limit already recorded an error; still consume to the
        // matching close as best-effort, one token at a time, so the rest
        // of the file stays recoverable.
        while !p.at(R_BRACE) && !p.at_eof() {
            p.skip_ws();
            if p.at(R_BRACE) || p.at_eof() {
                break;
            }
            let before = p.pos();
            p.error_recover("skipped inside over-deep block");
            if p.pos() == before {
                break;
            }
        }
    }

    p.expect(R_BRACE);
    p.finish_node();
}

/// Dispatch a single top-level-or-in-block item: a declaration, or a body
/// line (`super::block::body_line`).
///
/// Shared by `source_file` (brace-less top level, charter §4's
/// "no one-flow-per-file constraint") and `block` (braced bodies) — a
/// `flow`/`fn` nested inside another `flow` is exactly how charter §4
/// spells stitches ("stitches are nested `flow`s").
pub(crate) fn item(p: &mut Parser<'_, '_>) {
    // B0.6b: a leading `///` run is consumed here, before decl lookahead,
    // regardless of what follows — `maybe_consume_leading_run` is a no-op
    // returning `None` when the current token isn't DOC_COMMENT_OUTER, so
    // this has zero effect on the overwhelmingly common non-doc-comment
    // case. If a declaration head *does* follow, `doc` is threaded into
    // that decl's node-opening call so it retroactively wraps as the
    // declaration's leading child (`doc_comment::open_with_doc`). If
    // nothing declaration-shaped follows, `doc`'s checkpoint is simply
    // never used — the run's tokens are already sitting bare in the tree
    // (the ruling's "unattached leading doc falls back to trivia, no
    // diagnostic" judgment call).
    let doc = super::doc_comment::maybe_consume_leading_run(p);

    // Every hard-keyword declaration head requires a positive lookahead
    // check before committing (Finding #5, see `decl.rs`): unlike ink's
    // `===`-delimited knots, this grammar has no unambiguous declaration
    // sigil, so a prose line that happens to start with a reserved word
    // (`flow through the garden.`) must fall through to prose, not get
    // mis-parsed as a declaration header.
    if super::decl::at_flow_decl(p) {
        super::decl::flow_decl(p, doc);
        return;
    }
    if super::decl::at_fn_decl(p) {
        super::decl::fn_decl(p, doc);
        return;
    }
    if p.at(KW_VAR) && super::decl::at_binding_decl(p) {
        super::decl::var_decl(p, doc);
        return;
    }
    if p.at(KW_CONST) && super::decl::at_binding_decl(p) {
        super::decl::const_decl(p, doc);
        return;
    }
    if super::decl::at_flags_decl(p) {
        super::decl::flags_decl(p, doc);
        return;
    }
    if super::decl::at_struct_decl(p) {
        super::decl::struct_decl(p, doc);
        return;
    }
    if super::decl::at_extern_decl(p) {
        super::decl::extern_decl(p, doc);
        return;
    }
    if p.at(KW_IMPORT) && super::decl::at_import_decl(p) {
        super::decl::import_decl(p, doc);
        return;
    }
    if p.at(KW_USE) && super::decl::at_use_decl(p) {
        super::decl::use_decl(p, doc);
        return;
    }
    if super::decl::at_module_decl(p) {
        super::decl::module_decl(p, doc);
        return;
    }
    body_line(p);
}

/// Parse a single non-declaration body line: annotations, tags, diverts,
/// return, the annotated-brace family, or prose content (the fallback).
pub(crate) fn body_line(p: &mut Parser<'_, '_>) {
    match p.current() {
        NEWLINE => {
            p.bump();
        }
        HASH => super::content::tag_line(p),
        AT_L_BRACKET => super::annotation::annotation_line(p),
        DIVERT => super::divert::divert_or_tunnel(p),
        KW_RETURN => super::divert::return_stmt(p),
        L_BRACE if super::family::at_choice_point(p) => super::choice::choice_point(p),
        L_BRACE if super::family::at_conditional(p) => super::family::conditional_block(p),
        L_BRACE if super::family::at_alternation(p) => super::family::alternation_block(p),
        EOF => {}
        _ => super::content::content_line(p),
    }
}
