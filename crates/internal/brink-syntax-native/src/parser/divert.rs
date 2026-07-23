//! Diverts, tunnel calls, and return (charter §11 — all kept/respelled
//! verbatim from ink except the tunnel-return redirect, which trades
//! ink's `->-> x` for the self-explanatory `return -> x`).

use crate::SyntaxKind::{
    DIVERT, DIVERT_STMT, DIVERT_TARGET, KW_DONE, KW_END, L_PAREN, NEWLINE, RETURN_REDIRECT,
    RETURN_STMT, TUNNEL_CALL,
};

use super::Parser;

/// `-> target` (a plain divert) or `-> place ->` (a tunnel call — divert,
/// target, divert, charter §11: "KEPT as `-> place ->`"). Statement
/// position — a line whose first token is `->` (`block::body_line`'s
/// dispatch) — also consumes a trailing `NEWLINE`, since this call owns
/// terminating the line. [`divert_in_content`] is the content-position
/// sibling (N-1): a `->` that follows prose on the same content run
/// shares this exact node-shape logic but must NOT consume the `NEWLINE`
/// itself — the enclosing content loop (`content::content_items_until`)
/// owns line termination there, same as it does for any other content
/// item.
pub(crate) fn divert_or_tunnel(p: &mut Parser<'_, '_>) {
    divert_or_tunnel_core(p);
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}

/// The content-position sibling of [`divert_or_tunnel`] (N-1, charter
/// §11: diverts are "kept verbatim" including in content position — the
/// Fogg exhibit spells `* [The wager.] -> know_about_wager` this way).
/// Called from `content::content_items_until` whenever a `DIVERT` token
/// appears anywhere in a content run, not only as a line's first token.
/// Does not consume a trailing `NEWLINE` — see the doc comment above.
pub(crate) fn divert_in_content(p: &mut Parser<'_, '_>) {
    divert_or_tunnel_core(p);
}

/// Shared grammar: `-> target` or `-> place ->`, without any
/// newline-consumption policy (that differs by call site, see
/// [`divert_or_tunnel`]/[`divert_in_content`]'s doc comments).
fn divert_or_tunnel_core(p: &mut Parser<'_, '_>) {
    let checkpoint = p.checkpoint();
    p.bump(); // DIVERT (->)
    divert_target(p);
    p.skip_ws();
    if p.at(DIVERT) {
        p.start_node_at(checkpoint, TUNNEL_CALL);
        p.bump(); // second ->
        p.finish_node();
    } else {
        p.start_node_at(checkpoint, DIVERT_STMT);
        p.finish_node();
    }
}

/// `END` / `DONE` / a `PATH` (optionally followed by a call-style
/// `(args)`) — the divert target. Charter §11 keeps `-> knot(args)`
/// verbatim from ink, so a `PATH` target routes through the same
/// call-capable grammar as an ordinary expression path
/// (`expr::path_or_call`'s `(args)` half, `expr::arg_list`) rather than
/// stopping at the bare path — otherwise the parenthesized args parse
/// with zero errors but orphan into an unrelated sibling `CONTENT_LINE`
/// (bug #1196). The `ARG_LIST`, when present, is captured as a direct
/// sibling of `PATH` under `DIVERT_TARGET` rather than wrapped in a
/// `CALL_EXPR` — a divert target is not itself an expression, and the
/// existing `PATH` shape is a public accessor contract
/// (`DivertTarget::path`) shared by every call site (`divert_or_tunnel`,
/// `tunnel_call`, `return_stmt`'s redirect).
fn divert_target(p: &mut Parser<'_, '_>) {
    p.start_node(DIVERT_TARGET);
    // `END`/`DONE` are sentinels with no path; either can be eaten.
    if !p.eat(KW_END) && !p.eat(KW_DONE) {
        super::expr::path(p);
        p.skip_ws();
        if p.at(L_PAREN) {
            super::expr::arg_list(p);
        }
    }
    p.finish_node();
}

/// `return` — leave this container. `return -> x` is the tunnel-return
/// respelling (charter §11): pop the obligation, then go.
pub(crate) fn return_stmt(p: &mut Parser<'_, '_>) {
    let checkpoint = p.checkpoint();
    p.bump(); // KW_RETURN
    p.skip_ws();
    if p.at(DIVERT) {
        p.start_node_at(checkpoint, RETURN_REDIRECT);
        p.bump(); // ->
        divert_target(p);
        p.finish_node();
    } else {
        p.start_node_at(checkpoint, RETURN_STMT);
        p.finish_node();
    }
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}
