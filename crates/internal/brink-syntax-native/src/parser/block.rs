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
    // Every hard-keyword declaration head requires a positive lookahead
    // check before committing (Finding #5, see `decl.rs`): unlike ink's
    // `===`-delimited knots, this grammar has no unambiguous declaration
    // sigil, so a prose line that happens to start with a reserved word
    // (`flow through the garden.`) must fall through to prose, not get
    // mis-parsed as a declaration header.
    if super::decl::at_flow_decl(p) {
        super::decl::flow_decl(p);
        return;
    }
    if super::decl::at_fn_decl(p) {
        super::decl::fn_decl(p);
        return;
    }
    if p.at(KW_VAR) && super::decl::at_binding_decl(p) {
        super::decl::var_decl(p);
        return;
    }
    if p.at(KW_CONST) && super::decl::at_binding_decl(p) {
        super::decl::const_decl(p);
        return;
    }
    if super::decl::at_flags_decl(p) {
        super::decl::flags_decl(p);
        return;
    }
    if super::decl::at_struct_decl(p) {
        super::decl::struct_decl(p);
        return;
    }
    if super::decl::at_extern_decl(p) {
        super::decl::extern_decl(p);
        return;
    }
    if p.at(KW_IMPORT) && super::decl::at_import_decl(p) {
        super::decl::import_decl(p);
        return;
    }
    if p.at(KW_USE) && super::decl::at_use_decl(p) {
        super::decl::use_decl(p);
        return;
    }
    if super::decl::at_module_decl(p) {
        super::decl::module_decl(p);
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
