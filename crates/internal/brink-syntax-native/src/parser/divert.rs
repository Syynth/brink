//! Diverts, tunnel calls, and return (charter §11 — all kept/respelled
//! verbatim from ink except the tunnel-return redirect, which trades
//! ink's `->-> x` for the self-explanatory `return -> x`).

use crate::SyntaxKind::{
    DIVERT, DIVERT_STMT, DIVERT_TARGET, KW_DONE, KW_END, NEWLINE, RETURN_REDIRECT, RETURN_STMT,
    TUNNEL_CALL,
};

use super::Parser;

/// `-> target` (a plain divert) or `-> place ->` (a tunnel call — divert,
/// target, divert, charter §11: "KEPT as `-> place ->`").
pub(crate) fn divert_or_tunnel(p: &mut Parser<'_, '_>) {
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
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}

/// `END` / `DONE` / a `PATH` — the divert target.
fn divert_target(p: &mut Parser<'_, '_>) {
    p.start_node(DIVERT_TARGET);
    if p.eat(KW_END) {
        // sentinel, no path
    } else if p.eat(KW_DONE) {
        // sentinel, no path
    } else {
        super::expr::path(p);
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
