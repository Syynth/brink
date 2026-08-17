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
//!
//! `L_BRACKET` is one exception to "purely syntactic, no lookahead into the
//! next construct": [`reject_bracket_after_type_name`] (issue #2792) reads
//! it to catch the retracted `Option[T]` spelling. See that function's doc
//! for the one calling position (the lambda's own return annotation) where
//! this module deliberately does NOT make that read — `expr.rs`'s module
//! doc claim that this module "never touches `L_BRACKET`" predates #2792
//! and is stale.

use crate::SyntaxKind::{
    COLON, COMMA, GT, IDENT, KW_FN, L_BRACKET, L_PAREN, LT, R_PAREN, TYPE_ANNOTATION, TYPE_EXPR,
    TYPE_FN, TYPE_GENERIC, TYPE_NAME,
};

use super::Parser;

/// Whether [`type_name_or_generic`]'s trailing-`[`-after-type-name check
/// (`reject_bracket_after_type_name`) applies to the outermost type an
/// entry point parses. See [`lambda_return_type_annotation`]'s doc for the
/// one position that passes `No`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RejectTrailingBracket {
    Yes,
    No,
}

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
    type_annotation_impl(p, RejectTrailingBracket::Yes);
}

/// Same as [`type_annotation`], except the OUTERMOST type this call parses
/// is exempt from [`reject_bracket_after_type_name`] — used solely by
/// `expr.rs::lambda_expr` for the lambda's own return annotation.
///
/// Issue #2792 review (BLOCKING false positive): every other annotation
/// position (params, `let`, `var`/`const`, struct fields, and the lambda's
/// own *params* via `expr.rs::lambda_param`) is followed by punctuation a
/// type name could never legally start, so a trailing `[` there is
/// unambiguously the retracted `Option[T]` mistake. The lambda's own return
/// annotation is different: it is immediately followed by the lambda
/// *body*, an expression, and `[` legally starts one (the array-literal
/// atom, #1490) — `|x: int|: List<int> [1, 2]` is a fully legal program
/// (return type `List<int>`, body `[1, 2]`), not a mistake. Verified in
/// review: at head `8db452d9` (this PR's own tip before the fix),
/// `var f = |x: int|: List<int> [1, 2]` and `|x: int|: Foo [1, 2]` both
/// regressed from zero errors (base `a9542235`) to the unified diagnostic,
/// with an identical, correct tree either way — the shared check cannot
/// tell "trailing bracket is a mistake" from "trailing bracket starts the
/// next legal construct" by looking at the type alone; only the calling
/// position knows what follows. Only the outermost type is exempted — a
/// generic argument nested inside `<…>` (`List<Option[int]>`) is never
/// followed by an expression position, so it keeps the normal check: the
/// recursive `type_expr` calls inside [`type_name_or_generic`]'s `<…>` loop
/// always go through the default (`Yes`) path, never this one.
pub(crate) fn lambda_return_type_annotation(p: &mut Parser<'_, '_>) {
    type_annotation_impl(p, RejectTrailingBracket::No);
}

fn type_annotation_impl(p: &mut Parser<'_, '_>, reject_trailing_bracket: RejectTrailingBracket) {
    p.start_node(TYPE_ANNOTATION);
    p.expect(COLON);
    // Flush the trivia *after* the colon into this node rather than letting
    // `type_expr`'s first `expect` pull it inside the `TYPE_EXPR` child.
    p.skip_ws();
    type_expr_impl(p, reject_trailing_bracket);
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
    type_expr_impl(p, RejectTrailingBracket::Yes);
}

fn type_expr_impl(p: &mut Parser<'_, '_>, reject_trailing_bracket: RejectTrailingBracket) {
    p.start_node(TYPE_EXPR);
    if p.enter_depth() {
        if p.at(KW_FN) {
            type_fn(p);
        } else if p.at(IDENT) {
            type_name_or_generic(p, reject_trailing_bracket);
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
///
/// `reject_trailing_bracket` gates only the check on THIS call's own type
/// name/generic — nested type arguments (the nested `type_expr` calls
/// inside the `<…>` loop below) always go through the default `Yes` path
/// regardless of what the caller passed, per [`lambda_return_type_annotation`]'s
/// doc.
fn type_name_or_generic(p: &mut Parser<'_, '_>, reject_trailing_bracket: RejectTrailingBracket) {
    let checkpoint = p.checkpoint();
    p.expect(IDENT);

    // Whether this type finished cleanly enough for a trailing `[` to mean
    // anything: a `TYPE_NAME` (no `<`) always does; a `TYPE_GENERIC` only
    // does if its own `>` was actually consumed. Guards issue #2792 review
    // finding 2 (duplicated diagnostic): without this, `List<Option[int]>`
    // reported the same "expected `<` or end of type name" message TWICE
    // for the identical `[` — once from the inner `Option` type name's own
    // check, then again from this call's unconditional check firing on the
    // same still-unconsumed `[` after the outer `expect(GT)` failed to
    // close the generic (it never had a `>` to eat, so `p.pos()` never
    // moved between the two checks). Only running this call's own check
    // when its `<…>` actually closed means the failure is reported once,
    // by the innermost position that actually saw it.
    let generic_closed;
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
        generic_closed = p.eat(GT);
        if !generic_closed {
            p.error(format!("expected {:?}, found {:?}", GT, p.current()));
        }
        p.finish_node();
    } else {
        p.start_node_at(checkpoint, TYPE_NAME);
        p.finish_node();
        generic_closed = true;
    }
    if reject_trailing_bracket == RejectTrailingBracket::Yes && generic_closed {
        reject_bracket_after_type_name(p);
    }
}

/// Issue #2792: `[` is not the type-argument delimiter — angle brackets are
/// RULED (`docs/decision-log.md` 2026-07-27, retracting the older
/// `Option[T]` spelling), and `[…]` is reserved for the array/sequence
/// literal (#1490). Every position in this grammar that reads a type name
/// funnels through [`type_name_or_generic`] (params, return clauses,
/// lambda params, `let`/`var`/`const` bindings, struct fields), so checking
/// here — right after a `TYPE_NAME`/successfully-closed `TYPE_GENERIC`
/// finishes — gives every one of them the same targeted diagnostic "for
/// free", instead of each position's own incidental fallout from whatever
/// hard `expect` (or worse, none at all) happened to sit next in that
/// position's grammar.
///
/// Before this, only `var`/`const` had an explicit check (#2781/#2785,
/// called separately right after `binding_annotation` in `decl.rs`) — every
/// other position either happened to fail loudly for an unrelated reason
/// (`fn`/`flow` params trip their own `expect(R_PAREN)`; lambda params trip
/// `expect(PIPE)`) with a generic, position-specific message, or — for a
/// lambda's own return annotation — didn't fail at all: `expression(p)`
/// (`parser/expr.rs::lambda_expr`) happily reads a leftover `[…]` as an
/// `ARRAY_LITERAL` lambda body (a silent data drop, CLAUDE.md: "always bugs
/// until proven otherwise" — the exact shape #2781 found for `var`/`const`'s
/// initializer, just in a different position). Centralizing the check here
/// fixes that silent drop for every position EXCEPT the lambda's own return
/// annotation — see [`lambda_return_type_annotation`]'s doc (#2792 review):
/// that one position calls this check with `RejectTrailingBracket::No`
/// instead, because unlike every other annotation position, an expression
/// (the lambda body) legally follows it, and `[` legally starts one. The
/// silent drop for `|y: int|: Option[int] { none }`-shaped mistakes is
/// therefore back at that one position, same as before #2792 — the false
/// positive against legal array-literal bodies (`|x: int|: List<int>
/// [1, 2]`) was judged the worse of the two failure modes.
///
/// Only fires when the `[` sits on the same source line as the type name
/// just parsed — `Parser::at` never crosses a `NEWLINE`, so a legitimate
/// array literal starting fresh on the next line is untouched. Only called
/// when this call's own type finished cleanly (a bare `TYPE_NAME`, or a
/// `TYPE_GENERIC` whose `>` was actually consumed) — see the
/// `generic_closed` guard in [`type_name_or_generic`], which exists to
/// avoid double-reporting the same `[` once from an inner nested type
/// argument's own check and again from an outer generic that never closed.
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
