//! `@[name(args)]` — the annotation-line channel, paren-clause grammar
//! (charter §11 / NS-A2 lineage, `docs/directive-annotations-spec.md`
//! §5b's `@[effects(pure, silent, reads(gold, hp))]`).

use crate::SyntaxKind::{
    ANNOTATION_ARG, ANNOTATION_ARGS, ANNOTATION_LINE, AT_L_BRACKET, COLON_COLON, COMMA, EQ, FLOAT,
    FLOAT_LIT, IDENT, INTEGER, INTEGER_LIT, L_BRACKET, L_PAREN, NEWLINE, QUOTE, R_BRACKET, R_PAREN,
    STRING_ESCAPE, STRING_LIT, STRING_TEXT,
};

use super::Parser;

pub(crate) fn annotation_line(p: &mut Parser<'_, '_>) {
    p.start_node(ANNOTATION_LINE);
    p.expect(AT_L_BRACKET);
    p.expect(IDENT);
    if p.at(L_PAREN) {
        annotation_args(p);
    }
    p.expect(R_BRACKET);
    // Anything else on the line is unexpected — consume to newline so the
    // parser stays line-synchronized (mirrors `brink-syntax`'s
    // `annotation_line` recovery).
    let mut trailing = false;
    while !p.at_eof() && p.nth_raw(0) != NEWLINE {
        if !matches!(p.nth_raw(0), crate::SyntaxKind::WHITESPACE) {
            trailing = true;
        }
        p.bump();
    }
    if trailing {
        p.error("unexpected text after `]` on an annotation line".into());
    }
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

fn annotation_args(p: &mut Parser<'_, '_>) {
    p.start_node(ANNOTATION_ARGS);
    p.expect(L_PAREN);
    if p.enter_depth() {
        p.skip_ws_and_newlines();
        while p.peek_skip_nl() != R_PAREN && !p.at_eof() {
            let before = p.pos();
            annotation_arg(p);
            if p.pos() == before {
                p.error_recover("unexpected token in annotation arguments");
                p.skip_ws_and_newlines();
                continue;
            }
            p.skip_ws_and_newlines();
            if !p.eat(COMMA) {
                break;
            }
            p.skip_ws_and_newlines();
        }
        p.exit_depth();
    }
    p.expect(R_PAREN);
    p.finish_node();
}

/// One argument: a bare `IDENT`, `IDENT(ANNOTATION_ARGS)` (the nested
/// paren-clause form), `IDENT = STRING` (the key/value clause form —
/// issue #1719, `@[element(args = "…")]` / `@[style(chan = "…")]`), an
/// unquoted `::`-separated module `PATH`
/// (`story::old::path` — issue #1349, companion to the closed #1286's
/// `@[was(...)]` native rename migration; reuses `expr::path`'s
/// `PATH`/`PATH_SEGMENT` production verbatim rather than inventing a
/// second one, so `Path::segments`/`Path::crosses_module_wall` work the
/// same here as everywhere else `PATH` appears), or a literal
/// (`INTEGER`/`FLOAT`/`STRING` — the future `@[maxlen(80)]`-shaped tenants
/// `directive-annotations-spec.md` §6 names).
fn annotation_arg(p: &mut Parser<'_, '_>) {
    match p.current() {
        // A second non-trivia token of `::` commits to the path
        // production instead of a bare ident — `nth(1)` skips trivia, so
        // `story ::old` (unlikely but not forbidden) still resolves to a
        // path rather than leaving a dangling `::old` for the caller's
        // comma/`)` loop to choke on.
        IDENT if p.nth(1) == COLON_COLON => {
            p.start_node(ANNOTATION_ARG);
            super::expr::path(p);
            p.finish_node();
        }
        IDENT => {
            p.start_node(ANNOTATION_ARG);
            p.expect(IDENT);
            if p.at(L_PAREN) {
                annotation_args(p);
            } else if p.at(EQ) {
                // The key/value clause form (issue #1719): `key = "value"`.
                // Only a string-literal value is accepted — the ruled
                // spellings (`@[element(args = "…")]`, `@[style(chan =
                // "…")]`) never carry a bare-ident or numeric value on the
                // right of `=`, so nothing else is attempted here. A
                // malformed right-hand side is NOT recovered: it leaves no
                // `STRING_LIT` child for `AnnotationArg::eq_value` to find
                // (the lowering-side reader diagnoses that as a missing
                // value), but the token itself is left unconsumed, which
                // desyncs the enclosing `annotation_args`/`annotation_line`
                // loops — expect a real parser error (`expected R_PAREN`,
                // then `expected R_BRACKET`, then a trailing-text error on
                // the line) for a non-string right-hand side, not a single
                // clean diagnostic.
                p.expect(EQ);
                if p.at(QUOTE) {
                    annotation_string_value(p);
                }
            }
            p.finish_node();
        }
        INTEGER => {
            p.start_node(ANNOTATION_ARG);
            p.start_node(INTEGER_LIT);
            p.expect(INTEGER);
            p.finish_node();
            p.finish_node();
        }
        FLOAT => {
            p.start_node(ANNOTATION_ARG);
            p.start_node(FLOAT_LIT);
            p.expect(FLOAT);
            p.finish_node();
            p.finish_node();
        }
        QUOTE => {
            p.start_node(ANNOTATION_ARG);
            super::expr::string_lit(p);
            p.finish_node();
        }
        _ => {}
    }
}

/// A `STRING_LIT` for an annotation clause's `= "value"` right-hand side
/// (issue #1719) — deliberately **not** `expr::string_lit`.
///
/// String-mode lexing (`lexer::lex_string_token`) always emits `[`/`]` as
/// their own `L_BRACKET`/`R_BRACKET` tokens rather than folding them into
/// `STRING_TEXT`, so choice-bracket boundaries (charter §5's `[]`
/// display-split) stay visible inside dialogue-quoted content. `expr::
/// string_lit`'s loop only accepts `STRING_TEXT`/`STRING_ESCAPE`/`{expr}`
/// interpolation, so it breaks out the instant it meets one of those
/// bracket tokens — fatal for a regex pattern with a character class
/// (`@[element(args = "^(?<chan>[A-Z0-9-]+): (?<text>.+)$")]`, the spec's
/// own §3.5b fixture). An annotation clause value is never dialogue
/// choice-text, so this variant folds `L_BRACKET`/`R_BRACKET` back in as
/// literal content instead of breaking on them — the one behavioral
/// difference from `expr::string_lit`. It still builds a `STRING_LIT`
/// node (same `SyntaxKind`), so `AnnotationArg::eq_value`'s
/// `support::child::<StringLit>` cast and every other `STRING_LIT`
/// consumer keep working unchanged.
///
/// `{…}` interpolation is deliberately **not** accepted here (unlike
/// `expr::string_lit`): an annotation value is never dialogue text, so
/// there is no locale-resolution reason to parse an interpolation node —
/// and the lowering-side reader (`hir::lower_native::annotation::
/// eq_value_text`) only folds tokens, not nodes, so a `{…}` silently
/// vanished from the value (and from a regex pattern, silently truncated
/// it) when this was accepted. Hitting `{` here just ends the loop, the
/// same as any other unrecognized token, and `p.expect(QUOTE)` below
/// reports the resulting mismatch.
fn annotation_string_value(p: &mut Parser<'_, '_>) {
    p.start_node(STRING_LIT);
    p.expect(QUOTE);
    loop {
        match p.current() {
            STRING_TEXT | STRING_ESCAPE | L_BRACKET | R_BRACKET => p.bump(),
            _ => break,
        }
    }
    p.expect(QUOTE);
    p.finish_node();
}
