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
    COLON, COMMA, CONST_DECL, EOF, EQ, EXTERN_DECL, FLAGS_DECL, FLAGS_MEMBER, FLAGS_MEMBER_LIST,
    FLOW_DECL, FN_DECL, GT, HASH, IDENT, IMPORT_DECL, KW_AS, KW_CONST, KW_EXTERN, KW_FLAGS,
    KW_FLOW, KW_FN, KW_MODULE, KW_PUB, KW_REF, KW_STRUCT, KW_VAR, L_BRACE, L_BRACKET, L_PAREN,
    MODULE_DECL, NEWLINE, PARAM, PARAM_LIST, R_BRACE, R_PAREN, STRUCT_DECL, STRUCT_FIELD, TILDE,
    USE_DECL, USE_TREE, USE_TREE_LIST, VAR_DECL,
};

use super::Parser;

// ── Lookahead guards ─────────────────────────────────────────────────

/// `HASH` joins `L_PAREN`/`L_BRACE` as an accepted third token (§8b.4,
/// issue #1715): a `flow` header may carry trailing `#tag`s
/// (`flow market #act1 { … }`) — container-level per-flow tags — and those
/// tags sit exactly where the body brace used to be the only legal
/// continuation.
///
/// The `#` alone is **not** enough evidence, and this is Finding #5's
/// firewall doing its job rather than a theoretical worry: a prose line
/// like `flow gently #1 and the river bends.` matches `KW_FLOW IDENT HASH`
/// exactly. So the tag form additionally requires a body opener later on
/// the same line ([`header_tags_precede_a_body`]) before committing —
/// the same "positive lookahead all the way to the token that must
/// follow" discipline the paren/brace forms already get for free.
pub(crate) fn at_flow_decl(p: &Parser<'_, '_>) -> bool {
    at_flow_decl_at(p, 0)
}

/// [`at_flow_decl`], shifted by `base` non-trivia tokens — `base == 1` is
/// the shape a leading `pub` (already seen but not yet consumed) commits
/// to (see [`at_pub_decl`]).
fn at_flow_decl_at(p: &Parser<'_, '_>, base: usize) -> bool {
    p.nth(base) == KW_FLOW
        && p.nth(base + 1) == IDENT
        && match p.nth(base + 2) {
            L_PAREN | L_BRACE => true,
            HASH => header_tags_precede_a_body(p, base + 2),
            _ => false,
        }
}

/// True when a body opener (`{`, including the `~{`/`>{` body-dialect
/// selectors' brace) is the last non-trivia token on the same line as the
/// `#`-tag run starting at non-trivia lookahead offset `n`.
///
/// The brace must be the *last* token on the line, not merely present
/// somewhere on it: a prose line can contain an interpolation or
/// alternation (`flow gently #1 and {gold} coins.`, `{river|stream}`)
/// whose own `{`/`}` pair would otherwise satisfy a bare "any brace on the
/// line" check and misparse the prose as a declaration header. No `.brink`
/// fixture in the repo uses a one-line `flow x { … }` body, so requiring
/// the brace to trail the line costs nothing real.
///
/// Bounded by the physical line: the scan stops at the first `NEWLINE` or
/// EOF, which also matches the grammar — `nth` skips trivia but never a
/// `NEWLINE`, so a declaration header has always been a single-line shape
/// (`flow x\n{ }` was never a declaration either).
fn header_tags_precede_a_body(p: &Parser<'_, '_>, n: usize) -> bool {
    let mut i = n;
    let mut last = None;
    loop {
        match p.nth(i) {
            NEWLINE | EOF => return last == Some(L_BRACE),
            kind => {
                last = Some(kind);
                i += 1;
            }
        }
    }
}

pub(crate) fn at_fn_decl(p: &Parser<'_, '_>) -> bool {
    at_fn_decl_at(p, 0)
}

/// [`at_fn_decl`], shifted — see [`at_flow_decl_at`]'s doc.
fn at_fn_decl_at(p: &Parser<'_, '_>, base: usize) -> bool {
    p.nth(base) == KW_FN && p.nth(base + 1) == IDENT && matches!(p.nth(base + 2), L_PAREN | L_BRACE)
}

/// Shared guard for `var`/`const` — the caller has already checked
/// `p.at(KW_VAR)`/`p.at(KW_CONST)`.
///
/// `COLON` joins `EQ` as an accepted third token (NG-B, issue #1488): an
/// annotated binding (`var hp: int = 10`) puts the annotation where the
/// initializer's `=` used to be the only legal continuation. The
/// prose-collision risk Finding #5 guards against is unchanged in
/// character — neither `var` nor `const` is an English word a content line
/// plausibly opens with.
pub(crate) fn at_binding_decl(p: &Parser<'_, '_>) -> bool {
    at_binding_decl_at(p, 0)
}

/// [`at_binding_decl`], shifted — see [`at_flow_decl_at`]'s doc. The
/// caller still checks the `var`/`const` keyword itself at `base`
/// separately (this helper never did, even unshifted).
fn at_binding_decl_at(p: &Parser<'_, '_>, base: usize) -> bool {
    p.nth(base + 1) == IDENT && matches!(p.nth(base + 2), EQ | COLON)
}

pub(crate) fn at_flags_decl(p: &Parser<'_, '_>) -> bool {
    at_flags_decl_at(p, 0)
}

/// [`at_flags_decl`], shifted — see [`at_flow_decl_at`]'s doc.
fn at_flags_decl_at(p: &Parser<'_, '_>, base: usize) -> bool {
    p.nth(base) == KW_FLAGS && p.nth(base + 1) == IDENT && p.nth(base + 2) == EQ
}

pub(crate) fn at_struct_decl(p: &Parser<'_, '_>) -> bool {
    at_struct_decl_at(p, 0)
}

/// [`at_struct_decl`], shifted — see [`at_flow_decl_at`]'s doc.
fn at_struct_decl_at(p: &Parser<'_, '_>, base: usize) -> bool {
    p.nth(base) == KW_STRUCT && p.nth(base + 1) == IDENT && p.nth(base + 2) == L_BRACE
}

pub(crate) fn at_extern_decl(p: &Parser<'_, '_>) -> bool {
    at_extern_decl_at(p, 0)
}

/// [`at_extern_decl`], shifted — see [`at_flow_decl_at`]'s doc.
fn at_extern_decl_at(p: &Parser<'_, '_>, base: usize) -> bool {
    p.nth(base) == KW_EXTERN && p.nth(base + 1) == IDENT && p.nth(base + 2) == L_PAREN
}

// ── `pub`-prefixed lookaheads (issue #1582, RULED 2026-08-03) ────────
//
// One per-kind guard per form, each `p.at(KW_PUB) && <shifted shape check>`
// — mirrors every other declaration head's discipline (Finding #5): `pub`
// is now a hard-reserved keyword (tokenizes as `KW_PUB` regardless of
// position), so a prose line that happens to start with the bare word
// "pub" must not be swallowed unless a legal declaration shape actually
// follows it. `block::try_declaration` ORs each of these alongside its
// unprefixed sibling in the same routing check — `pub` itself is never
// bumped here or in `try_declaration`; each decl fn below consumes its own
// optional leading `pub` via `p.eat(KW_PUB)` right after opening its node,
// so the token lands as an ordinary child, never inside the `///`
// doc-comment wrap `doc_comment::open_with_doc` produces (`pub` and a doc
// run are two independent leading-position channels — combining their
// checkpoints would have doc-wrapped `pub` right along with the doc
// tokens, which is wrong whenever a doc comment is ALSO present).
//
/// Only the seven forms the ruling names carry a [`crate::VisibilityMark`]
/// slot to begin with: `flow`, `fn`, `var`, `const`, `flags`, `struct`,
/// `extern`. `import`/`use`/`module` are deliberately excluded — none of
/// their HIR shapes has a `visibility` field. `flow` covers both a
/// top-level container (lowers to `Knot`) and a nested one (lowers to
/// `Stitch`) — both carry `visibility` already, so `pub flow` reaches
/// either shape; only the ink dialect's own `===knot===`/`=stitch=`
/// grammar (a different crate, `brink-syntax`) is out of scope (the
/// decision log's "OWED" item 2).
pub(crate) fn at_pub_flow_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_PUB) && at_flow_decl_at(p, 1)
}

pub(crate) fn at_pub_fn_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_PUB) && at_fn_decl_at(p, 1)
}

pub(crate) fn at_pub_var_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_PUB) && p.nth(1) == KW_VAR && at_binding_decl_at(p, 1)
}

pub(crate) fn at_pub_const_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_PUB) && p.nth(1) == KW_CONST && at_binding_decl_at(p, 1)
}

pub(crate) fn at_pub_flags_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_PUB) && at_flags_decl_at(p, 1)
}

pub(crate) fn at_pub_struct_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_PUB) && at_struct_decl_at(p, 1)
}

pub(crate) fn at_pub_extern_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_PUB) && at_extern_decl_at(p, 1)
}

/// Caller has already checked `p.at(KW_IMPORT)`. Weaker check (Finding
/// #5): `import` has no required third-token shape to lean on.
pub(crate) fn at_import_decl(p: &Parser<'_, '_>) -> bool {
    p.nth(1) == IDENT
}

/// Caller has already checked `p.at(KW_USE)`. Weaker check (Finding #5):
/// same residual risk as `import`. Only commits to `USE_DECL` if the next
/// token is an identifier (issue #1285: a leading `::` with no first segment
/// should not commit — `use ::foo;` falls through to prose instead of
/// partially parsing as a malformed `USE_DECL`).
pub(crate) fn at_use_decl(p: &Parser<'_, '_>) -> bool {
    p.nth(1) == IDENT
}

pub(crate) fn at_module_decl(p: &Parser<'_, '_>) -> bool {
    p.at(KW_MODULE) && p.nth(1) == IDENT && p.nth(2) == L_BRACE
}

/// Consume an optional leading `pub` (issue #1582) as the next child of
/// whichever decl node is currently open — a no-op (nothing bumped, no
/// trivia disturbed) when it isn't there. Called by each of the seven
/// `pub`-eligible decl fns immediately after `doc_comment::open_with_doc`,
/// so `KW_PUB` lands as an ordinary child of the declaration node, never
/// inside the `DOC_COMMENT` wrap (which `open_with_doc` has already
/// closed by the time this runs, if a doc run preceded).
///
/// `p.eat` flushes trivia before checking but not after consuming — the
/// caller's very next step is always a raw `p.bump()` for its own leading
/// keyword (safe only with zero pending trivia at that point, exactly like
/// `decl_body`'s `~`/`>` body-dialect selector consumption), so the
/// trailing `skip_ws` here is required, not cosmetic.
fn eat_pub(p: &mut Parser<'_, '_>) {
    p.eat(KW_PUB);
    p.skip_ws();
}

// ── flow / fn ─────────────────────────────────────────────────────────

/// `flow name(params) { … }` — nested `flow` = a stitch (charter §4). `doc`
/// is a leading `///` run already consumed by `block::item`
/// (`doc_comment::maybe_consume_leading_run`), threaded through so it
/// attaches as this node's leading `DOC_COMMENT` child (B0.6b).
///
/// Body-dialect default (charter §4, RULED 2026-07-23): `flow` is
/// prose-ground — see [`decl_body`].
pub(crate) fn flow_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, FLOW_DECL, doc);
    eat_pub(p);
    p.bump(); // KW_FLOW
    p.expect(IDENT);
    if p.at(L_PAREN) {
        param_list(p);
    }
    return_type_clause(p);
    header_tags(p);
    decl_body(p, false, "expected a braced body after the flow header");
    p.finish_node();
}

/// Trailing `#tag`s on a `flow` header line — **container-level per-flow
/// tags** (`docs/prose-dialect-spec.md` §8b.4, RULED 2026-07-25):
/// `flow market #act1 #tense { … }`. The scene-heading spelling
/// (`parser::element`) is the other half of the same ruling; both produce
/// ordinary `TAG` children of the declaring node.
///
/// Only `flow` carries them: the ruling names *per-flow* tags, and `fn` is
/// an expression-time container with no story-time identity to tag.
///
/// The tags run through `content::header_tag_tail`, not the plain
/// `tag_line_tail` a content line uses: an ordinary tag's text runs to the
/// end of its line, but a header's body opener (`{`, or a `~`/`>`
/// body-dialect selector) follows the tags on that same line and must not
/// be swallowed into the last tag's text.
fn header_tags(p: &mut Parser<'_, '_>) {
    if p.at(HASH) {
        super::content::header_tag_tail(p);
    }
}

/// `fn name(params) { … }`. See [`flow_decl`]'s doc comment for `doc`.
///
/// Body-dialect default (charter §4, RULED 2026-07-23): `fn` is
/// code-ground — see [`decl_body`].
pub(crate) fn fn_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, FN_DECL, doc);
    eat_pub(p);
    p.bump(); // KW_FN
    p.expect(IDENT);
    if p.at(L_PAREN) {
        param_list(p);
    }
    return_type_clause(p);
    decl_body(p, true, "expected a braced body after the fn header");
    p.finish_node();
}

/// The optional `: type` return clause on a `flow`/`fn` header — RULED
/// 2026-07-26 (`docs/decision-log.md` "NG-C ruled: `: type` returns
/// everywhere", issue #1489): `fn probability(g: Guest): float { … }`,
/// matching the lambda convention so the language has exactly one
/// return-annotation spelling.
///
/// **A return-typed header needs an explicit parameter list**, even an
/// empty one (`flow quest(): QuestResult { … }`, not `flow quest: …`). The
/// declaration-head lookahead (`at_flow_decl`/`at_fn_decl`, Finding #5)
/// only commits on `KW IDENT (` or `KW IDENT {`, and widening it to accept
/// `KW IDENT :` would start claiming ordinary prose lines
/// (`flow onwards: the river bends.`) as declarations. Requiring the parens
/// keeps the guard's prose firewall intact and reads the same as the
/// `RustScript` north star's own `fn f() -> T`.
fn return_type_clause(p: &mut Parser<'_, '_>) {
    if super::types::at_type_annotation(p) {
        super::types::type_annotation(p);
    }
}

/// Parse a `flow`/`fn` declaration's body, honoring the body-dialect
/// selector on the opening brace (charter §4, RULED 2026-07-23 — see
/// `docs/decision-log.md` "Native interleaving & body-dialect spelling"):
/// plain `{ … }` is the per-keyword default (`code_default`: `fn` → code,
/// `flow` → prose); a `~` immediately before the brace forces a code-ground
/// `STMT_BLOCK` body (statements directly — a code-bodied `flow` is §3's
/// "Compound guard"); a `>` immediately before the brace forces a
/// prose-ground `BLOCK` body (a prose-bodied `fn`). Sigil mnemonics: `~` =
/// enter code, `>` = emit prose — the same two sigils §8.2's line-escape
/// grains reuse at finer granularity (not built by this function; see
/// `docs/decision-log.md`'s follow-up note on issue #1309).
///
/// The selector token, when present, is bumped as a bare child of the
/// enclosing `FLOW_DECL`/`FN_DECL` node — a leading sibling of the
/// `BLOCK`/`STMT_BLOCK` body node it selects.
fn decl_body(p: &mut Parser<'_, '_>, code_default: bool, missing_body_msg: &str) {
    if p.at(TILDE) && p.nth(1) == L_BRACE {
        p.eat(TILDE); // flushes any pending trivia first, unlike a raw `bump`
        super::stmt::stmt_block(p);
    } else if p.at(GT) && p.nth(1) == L_BRACE {
        p.eat(GT); // flushes any pending trivia first, unlike a raw `bump`
        super::block::block(p);
    } else if p.at(L_BRACE) {
        if code_default {
            super::stmt::stmt_block(p);
        } else {
            super::block::block(p);
        }
    } else {
        p.error(missing_body_msg.into());
    }
}

/// `(param, ref param: Type, …)` — shared by `flow`/`fn`/`extern` headers.
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

/// `ref`? `IDENT` (`:` type)? — one parameter (NG-A, issue #1487). The
/// annotation is optional in the grammar for every header kind; whether an
/// *un*annotated escaping parameter is an error is `brink-analyzer`'s
/// strict-mode call (`strict.rs`'s `E065` Unknown-escape and its
/// annotation firewall), never this parser's.
fn param(p: &mut Parser<'_, '_>) {
    p.start_node(PARAM);
    p.eat(KW_REF);
    p.expect(IDENT);
    if super::types::at_type_annotation(p) {
        super::types::type_annotation(p);
    }
    p.finish_node();
}

// ── var / const ───────────────────────────────────────────────────────

/// `var name = expr`. No statement terminator is required (Finding #6).
/// The code-ground sitting has since ruled statement termination for the
/// *new* statement layer (`;`, `docs/decision-log.md` 2026-07-23 —
/// `parser/stmt.rs`'s `LET_STMT`/`ASSIGN_STMT`/`EXPR_STMT`), but that
/// ruling applies to code-ground statements, not this declaration-layer
/// keyword — `var`/`const` keep their existing `NEWLINE`/`}`/EOF-terminated
/// shape unchanged; only `let` (the new binding keyword) is `;`-terminated.
///
/// `var name: type = expr` — the optional annotation sits between the name
/// and the initializer (NG-B, issue #1488), the same slot `let` uses.
pub(crate) fn var_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, VAR_DECL, doc);
    eat_pub(p);
    p.bump(); // KW_VAR
    p.expect(IDENT);
    binding_annotation(p);
    reject_bracket_after_annotation(p);
    if p.eat(crate::SyntaxKind::EQ) {
        super::expr::expression(p);
    }
    p.finish_node();
}

/// `const name = expr` / `const name: type = expr`.
pub(crate) fn const_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, CONST_DECL, doc);
    eat_pub(p);
    p.bump(); // KW_CONST
    p.expect(IDENT);
    binding_annotation(p);
    reject_bracket_after_annotation(p);
    if p.eat(crate::SyntaxKind::EQ) {
        super::expr::expression(p);
    }
    p.finish_node();
}

/// The optional `: type` clause between a binding's name and its `=`
/// initializer (NG-B, issue #1488). Shared by `var`/`const` here and by
/// `let` (`parser/stmt.rs`), which is why it lives as its own helper.
pub(super) fn binding_annotation(p: &mut Parser<'_, '_>) {
    if super::types::at_type_annotation(p) {
        super::types::type_annotation(p);
    }
}

/// Issue #2781: `[` is not the type-argument delimiter (`docs/decision-log.md`
/// 2026-07-27 retracted `Option[T]` — angle brackets are RULED, `[…]` is
/// reserved for array literals, #1490). `type_name_or_generic`
/// (`parser/types.rs`) reads a bare type name and simply stops before a
/// `[` it doesn't recognize, since `[` is not part of the type-annotation
/// grammar; nothing else in `var`/`const`'s header requires a specific next
/// token, so without this check the trailing `[int] = none` silently falls
/// out to `var_decl`'s own end and gets reinterpreted by the caller as an
/// unrelated `CONTENT_LINE` (narrative prose) — a silent data drop with
/// zero diagnostics (CLAUDE.md: "flag silent data drops... silent drops
/// are always bugs until proven otherwise").
///
/// `fn`/`flow` params, return types, and lambda params never needed this:
/// each of those has its own hard `expect` for the very next token
/// (`R_PAREN`, `PIPE`) that fires for free when `[` shows up in its place
/// (`expected R_PAREN, found L_BRACKET` / `expected PIPE, found
/// L_BRACKET`, pinned by
/// `lambda_param_square_bracket_generic_fails_in_lambda_param_fn_param_and_return_position`).
/// `var`/`const`'s initializer is optional (`p.eat(EQ)`, not
/// `p.expect(EQ)`), so there is no natural "expect" call left to catch it —
/// this is the one annotation position that needs an explicit check.
///
/// Deliberately **not** folded into `binding_annotation` itself: that
/// helper is also `let`'s entry point (`parser/stmt.rs`), and `let` already
/// errors loudly on the same input for a different, pre-existing reason —
/// it is `;`-terminated, so a leftover `[int]` before the required
/// `SEMICOLON` already produces `expected SEMICOLON, found L_BRACKET`
/// there. Adding a second, redundant diagnostic to an already-correct path
/// is unwarranted scope.
///
/// Only fires when the `[` sits on the same source line as the annotation
/// just parsed — `Parser::at` never crosses a `NEWLINE` (`at_type_annotation`
/// leans on the same property), so a legitimate array-literal `CONTENT_LINE`
/// starting fresh on the *next* line is untouched.
fn reject_bracket_after_annotation(p: &mut Parser<'_, '_>) {
    if p.at(L_BRACKET) {
        p.error(format!(
            "expected `<` or end of type name, found {:?}",
            p.current()
        ));
    }
}

// ── flags ────────────────────────────────────────────────────────────

/// `flags Name = (member), member, …` (charter §11: renamed `LIST`).
pub(crate) fn flags_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, FLAGS_DECL, doc);
    eat_pub(p);
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
    eat_pub(p);
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

/// `IDENT ~ ":" ~ type_expr` — a struct field's name and declared type
/// (NG-E, issue #1505). The type clause is the shared `TYPE_ANNOTATION`
/// wrapper (`super::types::type_annotation`), the same node every other
/// `: type` position in this grammar produces (`param`, `binding_annotation`,
/// `return_type_clause`), so a field's type may now be a generic
/// instantiation (`List<int>`, `Map<K, V>`) or a function type
/// (`fn(int): bool`) — not just a bare dotted path — unblocking
/// function-typed and container-typed struct fields (#1482, #1487).
fn struct_field(p: &mut Parser<'_, '_>) {
    if !p.at(IDENT) {
        return;
    }
    p.start_node(STRUCT_FIELD);
    p.expect(IDENT);
    super::types::type_annotation(p);
    p.finish_node();
}

// ── extern ───────────────────────────────────────────────────────────

/// `extern name(params)` — no body, ever (kept from ink's `EXTERNAL`).
pub(crate) fn extern_decl(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, EXTERN_DECL, doc);
    eat_pub(p);
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
    } else {
        // Ruled 2026-07-22 (#1275, decision log): a `use` with no leading
        // path — `use { a, b };` at the top level, or a nested bare group
        // inside a list (`use a::{ {b, c} };`) — is a parse error, not a
        // valid import. A `use`-tree group is always the *tail* of a path
        // (`use story::m::{a, b}`), never the whole tree: with no module
        // named there is nothing to select from. This used to have a
        // `p.at(L_BRACE) { use_tree_list(p) }` branch that *accepted* a
        // bare group here — dead at the top level (`at_use_decl`'s
        // lookahead never routes `use {…}` into `USE_DECL` in the first
        // place) but silently live for nested list entries, so
        // `use a::{ {b, c} };` parsed with zero errors. Pruned; every
        // path-less `use_tree` now reports this diagnostic instead.
        p.error("a `use` needs a module path".into());
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
