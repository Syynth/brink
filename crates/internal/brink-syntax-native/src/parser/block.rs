use crate::SyntaxKind::{
    self, AT, AT_L_BRACKET, BANG, BLOCK, COLON_COLON, DIVERT, EOF, HASH, KW_CONST, KW_IMPORT,
    KW_RETURN, KW_USE, KW_VAR, L_BRACE, L_PAREN, NEWLINE, R_BRACE, THREAD, TILDE,
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

    // A braced body always starts a fresh dialogue chain (`Parser::
    // cue_chain`, the prose dialect's chain rule) and restores the
    // enclosing one on the way out, so a `(delivery)`-shaped line in an
    // inner body is never read against a cue that lives outside it.
    let outer_chain = p.set_cue_chain(false);

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

    p.set_cue_chain(outer_chain);
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

    // A scene heading declares a stitch (`docs/prose-dialect-spec.md`
    // §3.2's structural exception), so it is dispatched here alongside the
    // keyword declarations rather than in `body_line` — it, and only it,
    // absorbs the following items into its own header-scoped body (§8b.2).
    if super::element::at_scene_heading(p) {
        p.set_cue_chain(false);
        super::element::scene_stitch(p, doc);
        return;
    }

    if try_declaration(p, doc) {
        // A declaration is never a link in a dialogue chain — anything
        // that is not a cue, a parenthetical, or that cue's own dialogue
        // breaks it (`brink_ir::dialect`'s chain semantics).
        p.set_cue_chain(false);
        return;
    }

    body_line(p);
}

/// The declaration half of [`item`]'s dispatch: parse one `flow`/`fn`/
/// `var`/`const`/`flags`/`struct`/`extern`/`import`/`use`/`module`
/// declaration if the lookahead commits to one, returning whether it did.
///
/// Split out of [`item`] so the chain-breaking bookkeeping the prose
/// dialect needs (`Parser::cue_chain`) has exactly one place to live
/// instead of one `set_cue_chain` call per declaration head. The lookahead
/// order and every guard below it are unchanged.
fn try_declaration(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) -> bool {
    // Every hard-keyword declaration head requires a positive lookahead
    // check before committing (Finding #5, see `decl.rs`): unlike ink's
    // `===`-delimited knots, this grammar has no unambiguous declaration
    // sigil, so a prose line that happens to start with a reserved word
    // (`flow through the garden.`) must fall through to prose, not get
    // mis-parsed as a declaration header.
    if super::decl::at_flow_decl(p) {
        super::decl::flow_decl(p, doc);
        return true;
    }
    if super::decl::at_fn_decl(p) {
        super::decl::fn_decl(p, doc);
        return true;
    }
    if p.at(KW_VAR) && super::decl::at_binding_decl(p) {
        super::decl::var_decl(p, doc);
        return true;
    }
    if p.at(KW_CONST) && super::decl::at_binding_decl(p) {
        super::decl::const_decl(p, doc);
        return true;
    }
    if super::decl::at_flags_decl(p) {
        super::decl::flags_decl(p, doc);
        return true;
    }
    if super::decl::at_struct_decl(p) {
        super::decl::struct_decl(p, doc);
        return true;
    }
    if super::decl::at_extern_decl(p) {
        super::decl::extern_decl(p, doc);
        return true;
    }
    if p.at(KW_IMPORT) && super::decl::at_import_decl(p) {
        super::decl::import_decl(p, doc);
        return true;
    }
    if p.at(KW_USE) && super::decl::at_use_decl(p) {
        super::decl::use_decl(p, doc);
        return true;
    }
    // issue #1285 review: `at_use_decl` no longer commits on a leading
    // `::` (Finding #5's weaker two-token guard), so a typo'd
    // `use ::foo;` now falls through to prose instead of partially
    // parsing as a malformed USE_DECL. Falling through silently would
    // turn the typo into player-facing prose with no signal, so emit a
    // targeted diagnostic here before falling through.
    if p.at(KW_USE) && p.nth(1) == COLON_COLON {
        p.error("a `use` path cannot start with `::`".into());
    }
    if super::decl::at_module_decl(p) {
        super::decl::module_decl(p, doc);
        return true;
    }
    false
}

/// Parse a single non-declaration body line: prose block elements (cues,
/// parentheticals), annotations, tags, diverts, return, the
/// annotated-brace family, or prose content (the fallback).
///
/// Every arm also settles the dialogue-chain state (`Parser::cue_chain`,
/// `docs/prose-dialect-spec.md` §3.1): a cue **opens** a chain, a
/// parenthetical and a plain content line (the cue's dialogue) **carry**
/// it, and everything else — a blank line above all, per
/// `brink_ir::dialect`'s "blank lines always break a chain" — closes it.
pub(crate) fn body_line(p: &mut Parser<'_, '_>) {
    match p.current() {
        NEWLINE => {
            p.bump();
            p.set_cue_chain(false);
        }
        HASH => {
            super::content::tag_line(p);
            p.set_cue_chain(false);
        }
        AT_L_BRACKET => {
            super::annotation::annotation_line(p);
            p.set_cue_chain(false);
        }
        // `@NAME` / `@NAME: text` — the two ruled cue patterns (§8b.9).
        // Checked before the `AT`-in-prose fallback, and adjacency-guarded
        // so a lone `@` still folds into `TEXT` (`SyntaxKind::AT`'s doc).
        AT if super::element::at_cue(p) => {
            super::element::cue_line(p);
            p.set_cue_chain(true);
        }
        // `!name` — the self-announcing annotation-element dispatch sigil
        // (§3.5b, issue #2004). Adjacency-guarded the same way the `@NAME`
        // cue above is, so a lone `!` (or one followed by a gap, ordinary
        // exclamation-mark prose) falls through to `TEXT` unchanged. Never
        // a link in a dialogue chain — a dispatched line stands alone, like
        // a divert or a logic line.
        BANG if super::element::at_bang_dispatch(p) => {
            super::element::bang_dispatch(p);
            p.set_cue_chain(false);
        }
        // `(hushed)` — only inside a live chain, so G-1's `(label)`
        // content-line spelling is untouched everywhere else
        // (`element::at_parenthetical`'s doc has the full rationale).
        L_PAREN if super::element::at_parenthetical(p) => super::element::parenthetical(p),
        DIVERT => {
            super::divert::divert_or_tunnel(p);
            p.set_cue_chain(false);
        }
        KW_RETURN => {
            super::divert::return_stmt(p);
            p.set_cue_chain(false);
        }
        // `~ stmt` — the content-ground logic-line escape into code
        // (charter §8.2, RULED 2026-07-23, issue #1991: ink's logic line,
        // kept). Checked before the prose fallback so a leading `~` is
        // never swallowed into `TEXT` — see `SyntaxKind::LOGIC_LINE`'s doc
        // and `stmt::logic_line`. Never a link in a dialogue chain, same as
        // every other non-prose body line.
        TILDE => {
            super::stmt::logic_line(p);
            p.set_cue_chain(false);
        }
        // `<-` outside a choice point (issue #1263, ruled #1260): not a
        // structural splice here (only `choice::choice_point`'s loop
        // recognizes `THREAD`), but not silent either — warn, then fall
        // through to ordinary content the same way it always has.
        THREAD => {
            super::choice::splice_outside_choice_point(p);
            p.set_cue_chain(false);
        }
        L_BRACE if super::family::at_choice_point(p) => {
            super::choice::choice_point(p);
            p.set_cue_chain(false);
        }
        L_BRACE if super::family::at_conditional(p) => {
            super::family::conditional_block(p);
            p.set_cue_chain(false);
        }
        L_BRACE if super::family::at_alternation(p) => {
            super::family::alternation_block(p);
            p.set_cue_chain(false);
        }
        EOF => {}
        // Prose. Inside a chain this is the cue's dialogue, so the chain
        // stays live across it (the inventory's "chain: after cue or
        // dialogue").
        _ => super::content::content_line(p),
    }
}
