//! The native type-annotation grammar (NG-A/NG-B/NG-C, issues
//! #1487/#1488/#1489).
//!
//! **One spelling, every position** (`docs/decision-log.md` 2026-07-26
//! "NG-C ruled: `: type` returns everywhere"): a colon introduces a type
//! wherever one can be written — parameters (`fn f(g: Guest)`), bindings
//! (`let x: int = 1;`, `var hp: int = 10`), a `fn`/`flow` return clause
//! after the parameter list (`fn probability(g: Guest): float { … }`), and
//! lambdas (`|g: Guest|: bool { … }`, ratified 2026-07-23). The Rust arrow
//! was rejected: `->` lexes unconditionally as `DIVERT`
//! (`lexer/punctuation.rs`), so one arrow keeps one meaning.
//!
//! Structurally this mirrors the brink dialect's own TM-2 grammar
//! (`brink-syntax/src/parser/types.rs`, `docs/typed-mode-spec.md` §3) node
//! for node, so both frontends' lowering targets the same
//! `brink_ir::hir::TypeExpr` shape. Like that module, this one is **purely
//! syntactic**: any `IDENT` is accepted as a type name or generic head, and
//! recognizing the fixed nominal set (`int`, `float`, `bool`, `string`,
//! `List<L>`, `Map<K, V>`, `Array<T>`, `Option<T>`, `Weighted<T>`,
//! `Handle<K>`, declared struct names, …) is `brink-analyzer`'s job, never
//! this parser's. Non-primitive type names are Uppercase (issue #1552,
//! `docs/decision-log.md` 2026-07-27 "Type-name surface ruled").

use crate::SyntaxKind::{
    COLON, COMMA, GT, IDENT, KW_FN, L_BRACKET, L_PAREN, LT, R_PAREN, TYPE_ANNOTATION, TYPE_EXPR,
    TYPE_FN, TYPE_GENERIC, TYPE_NAME,
};

use super::Parser;

/// `true` when a `: type` annotation clause starts at the current position.
///
/// Read-only lookahead (`Parser::at` skips trivia but never `NEWLINE`), so
/// a colon on the *next* line never gets absorbed as an annotation.
pub(crate) fn at_type_annotation(p: &Parser<'_, '_>) -> bool {
    p.at(COLON)
}

/// Parse `: type_expr` into a [`TYPE_ANNOTATION`] node.
///
/// ```text
/// type_annotation = { ":" ~ type_expr }
/// ```
pub(crate) fn type_annotation(p: &mut Parser<'_, '_>) {
    p.start_node(TYPE_ANNOTATION);
    p.expect(COLON);
    // Flush the trivia *after* the colon into this node rather than letting
    // `type_expr`'s first `expect` pull it inside the `TYPE_EXPR` child.
    p.skip_ws();
    type_expr(p);
    p.finish_node();
}

/// Parse one type expression — a function type, a generic instantiation, or
/// a bare nominal name.
///
/// ```text
/// type_expr = { type_fn | type_name_or_generic }
/// ```
///
/// Depth-guarded like every other recursive rule in this parser: a
/// pathological `List<List<List<…>>>` records the nesting-depth diagnostic
/// and stops recursing rather than blowing the stack (CLAUDE.md "guard
/// against unbounded growth"). A depth-limited node is left childless — the
/// "absent data is legal" contract every optional AST child in this grammar
/// already has.
pub(crate) fn type_expr(p: &mut Parser<'_, '_>) {
    p.start_node(TYPE_EXPR);
    if p.enter_depth() {
        if p.at(KW_FN) {
            type_fn(p);
        } else if p.at(IDENT) {
            type_name_or_generic(p);
        } else {
            p.error(format!("expected a type, found {:?}", p.current()));
        }
        p.exit_depth();
    }
    p.finish_node();
}

/// Parse a bare type name, upgrading it to a [`TYPE_GENERIC`] if a `<`
/// follows on the same line.
///
/// ```text
/// type_name_or_generic = { IDENT ~ ("<" ~ type_expr ~ ("," ~ type_expr)* ~ ">")? }
/// ```
fn type_name_or_generic(p: &mut Parser<'_, '_>) {
    let checkpoint = p.checkpoint();
    p.expect(IDENT);

    if p.at(LT) {
        p.start_node_at(checkpoint, TYPE_GENERIC);
        p.expect(LT);
        p.skip_ws_and_newlines();
        while p.peek_skip_nl() != GT && !p.at_eof() {
            let before = p.pos();
            type_expr(p);
            if p.pos() == before {
                p.error_recover("unexpected token in type argument list");
                continue;
            }
            p.skip_ws_and_newlines();
            if !p.eat(COMMA) {
                break;
            }
            p.skip_ws_and_newlines();
        }
        p.expect(GT);
        p.finish_node();
    } else {
        p.start_node_at(checkpoint, TYPE_NAME);
        p.finish_node();
    }
    reject_bracket_after_type_name(p);
}

/// Issue #2792: `[` is not the type-argument delimiter — angle brackets are
/// RULED (`docs/decision-log.md` 2026-07-27, retracting the older
/// `Option[T]` spelling), and `[…]` is reserved for the array/sequence
/// literal (#1490). Every position in this grammar that reads a type name
/// funnels through [`type_name_or_generic`] (params, return clauses,
/// lambda params/returns, `let`/`var`/`const` bindings, struct fields), so
/// checking here — right after a `TYPE_NAME`/`TYPE_GENERIC` finishes — gives
/// every one of them the same targeted diagnostic "for free", instead of
/// each position's own incidental fallout from whatever hard `expect` (or
/// worse, none at all) happened to sit next in that position's grammar.
///
/// Before this, only `var`/`const` had an explicit check (#2781/#2785,
/// called separately right after `binding_annotation` in `decl.rs`) — every
/// other position either happened to fail loudly for an unrelated reason
/// (`fn`/`flow` params trip their own `expect(R_PAREN)`; lambda params trip
/// `expect(PIPE)`) with a generic, position-specific message, or — for a
/// lambda's own return annotation — didn't fail at all: `expression(p)`
/// (`parser/expr.rs::lambda_expr`) happily reads a leftover `[int]` as an
/// `ARRAY_LITERAL` lambda body, silently dropping the real body that
/// followed (a silent data drop, CLAUDE.md: "always bugs until proven
/// otherwise" — the exact shape #2781 found for `var`/`const`'s
/// initializer, just in a different position). Centralizing the check here
/// fixes that silent drop the same way it unifies the other three
/// messages, with no new per-position wiring to keep in sync.
///
/// Only fires when the `[` sits on the same source line as the type name
/// just parsed — `Parser::at` never crosses a `NEWLINE`, so a legitimate
/// array literal starting fresh on the next line is untouched.
///
/// Deliberately narrow, matching #2792's scope: this only ever *adds* the
/// targeted diagnostic ahead of whatever recovery already ran at a given
/// position; it never consumes the `[`, so a position with its own hard
/// `expect` immediately afterward (`fn`/`flow` params, lambda params) still
/// separately reports its prior generic message too, and the leftover
/// `[…]` still lands as parser-generated garbage exactly as it did before.
/// Unifying that recovery is a bigger, separate design question (#2792's
/// own text) — out of scope here.
fn reject_bracket_after_type_name(p: &mut Parser<'_, '_>) {
    if p.at(L_BRACKET) {
        p.error(format!(
            "expected `<` or end of type name, found {:?}",
            p.current()
        ));
    }
}

/// Parse `fn(type, …): type` — a function type.
///
/// ```text
/// type_fn = { "fn" ~ "(" ~ (type_expr ~ ("," ~ type_expr)*)? ~ ")" ~ ":" ~ type_expr }
/// ```
///
/// The return clause is required (a `fn(…)` with no `:` records a
/// diagnostic): `brink_ir::hir::TypeExpr::Fn` carries a non-optional
/// return type, matching the brink dialect's own `type_fn`.
fn type_fn(p: &mut Parser<'_, '_>) {
    p.start_node(TYPE_FN);
    p.expect(KW_FN);
    p.expect(L_PAREN);
    p.skip_ws_and_newlines();
    while p.peek_skip_nl() != R_PAREN && !p.at_eof() {
        let before = p.pos();
        type_expr(p);
        if p.pos() == before {
            p.error_recover("unexpected token in fn-type parameter list");
            continue;
        }
        p.skip_ws_and_newlines();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws_and_newlines();
    }
    p.expect(R_PAREN);
    p.expect(COLON);
    p.skip_ws();
    type_expr(p);
    p.finish_node();
}
