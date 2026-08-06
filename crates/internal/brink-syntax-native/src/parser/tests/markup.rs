//! Inline markup — XML-shaped spans, the escape set, and the nesting
//! doctrine (#1716; `docs/prose-dialect-spec.md` §4).

use super::*;

fn first_node(root: &SyntaxNode, kind: SyntaxKind) -> SyntaxNode {
    let found = root.descendants().find(|n| n.kind() == kind);
    assert!(found.is_some(), "no {kind:?} node in tree");
    found.expect("asserted present just above")
}

/// The literal character an `ESCAPE` node produces — its second token's
/// text (mirrors `hir::lower_native::body::push_escape`'s own reading of
/// the node, minus the `assert_lossless` ceremony this test module uses
/// instead of full lowering).
fn escape_literal(escape: &SyntaxNode) -> String {
    escape
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .nth(1)
        .map(|t| t.text().to_string())
        .unwrap_or_default()
}

// ── Basic spans (§4.1) ───────────────────────────────────────────────

#[test]
fn a_simple_span_parses_with_no_errors() {
    let src = "flow f() {\n  He hands you <item id=\"lantern\">the old lantern</item>.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("span name")
            .to_string(),
        "item"
    );
    let attr = find_child::<ast::SpanAttr>(span.syntax()).expect("one SPAN_ATTR");
    assert!(attr.to_string().contains("id=\"lantern\""));
    assert_eq!(text_run_concat(span.syntax()), "the old lantern");
}

#[test]
fn a_bare_less_than_that_is_not_span_shaped_stays_plain_text() {
    // "5 < 10" — no letter immediately after `<`, so per the blunt-lexing
    // rule (§4.1) this is never a tag attempt.
    let src = "flow f() {\n  The score was 5 < 10 that round.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SPAN));
}

#[test]
fn glue_and_splice_are_not_claimed_by_span_recognition() {
    // `<>` (glue) and `<-` (splice) are already distinct compound tokens at
    // the lexer — a bare `LT` reaching the span recognizer can never be
    // either, so this must parse exactly as it always has.
    let src = "flow f() {\n  \"Hello,\"\n  <> \"World!\"\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::GLUE_NODE));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SPAN));
}

// ── Hyphenated tag names (§4.1, RULED 2026-08-01, issue #1996) ───────

#[test]
fn a_hyphenated_span_name_parses_with_no_errors_and_matches_its_close_tag() {
    let src = "flow f() {\n  <fade-in>hello</fade-in>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("span name")
            .to_string(),
        "fade-in"
    );
    assert_eq!(text_run_concat(span.syntax()), "hello");
}

#[test]
fn a_hyphenated_self_closing_span_parses() {
    let src = "flow f() {\n  Bell tolls. <fade-in/> Door slams.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("name")
            .to_string(),
        "fade-in"
    );
}

#[test]
fn a_hyphen_continuation_segment_may_be_a_reserved_keyword_spelling() {
    // `fade-in` is this ruling's own worked example, and `in` is `KW_IN`
    // everywhere else in the grammar (`for k in m`) — native keywords are
    // reserved everywhere in code, but a tag name is freeform prose
    // vocabulary (§4.2). A continuation segment (the word after a `-`)
    // must accept a keyword spelling, or the ruling's own headline example
    // wouldn't parse. This is deliberately narrower than "a tag may be
    // *named* a bare keyword" — see `a_leading_hyphen_never_opens_a_span`'s
    // sibling concern is unaffected; only the continuation position widens.
    let src = "flow f() {\n  <fade-in>hello</fade-in>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("span name")
            .to_string(),
        "fade-in"
    );
}

#[test]
fn a_tag_name_may_chain_more_than_one_hyphen() {
    // Proves `tag_name_len`'s loop, not just a single MINUS/IDENT pair.
    let src = "flow f() {\n  <a-b-c>x</a-b-c>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("name")
            .to_string(),
        "a-b-c"
    );
}

#[test]
fn a_leading_hyphen_never_opens_a_span() {
    // `<-` is already claimed by `THREAD` (splice) at the lexer, so `<-x>`
    // never even reaches `LT` — pinning the "leading hyphen is illegal"
    // half of the ruling, and mirroring the existing
    // `glue_and_splice_are_not_claimed_by_span_recognition` precedent.
    let src = "flow f() {\n  text <-x> more text\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SPAN));
}

#[test]
fn a_leading_hyphen_never_closes_a_span() {
    // `</-x>` IS lexically distinguishable from `THREAD` (`SLASH` breaks
    // the `<-` pattern) — but `at_span_close` requires its own first name
    // token to be `IDENT`, not `MINUS`, so it is rejected here too, keeping
    // the ban symmetric across open/close by construction.
    let src = "flow f() {\n  <b>hi</-b>\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "`<b>` never sees a matching close (`</-b>` isn't one), so it's unclosed"
    );
}

#[test]
fn a_trailing_hyphen_is_a_parse_error_not_folded_into_the_name() {
    // `<x->` — the trailing `-` (here, greedily lexed as one `DIVERT`
    // token alongside the following `>`) is never folded into the name;
    // pinning the "trailing hyphen is illegal" half of the ruling.
    let src = "flow f() {\n  <x-> more text\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "a trailing hyphen must be reported, not silently accepted"
    );
}

#[test]
fn a_close_tag_must_match_the_open_tags_full_hyphenated_name_not_a_prefix() {
    // Regression for comparing the FULL close-tag name, not just its first
    // `IDENT` segment: `</fade>` must NOT be accepted as the close for
    // `<fade-in>`, even though both start with the `fade` segment.
    let src = "flow f() {\n  <fade-in>hi</fade>\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "`</fade>` must not satisfy `<fade-in>`'s close tag"
    );
}

// ── Point markers (§8b.11) — self-closing spans ──────────────────────

#[test]
fn a_self_closing_span_is_a_point_marker_with_no_body_or_close_tag() {
    let src = "flow f() {\n  The bell tolls again. <pause/> Somewhere a door slams.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("name")
            .to_string(),
        "pause"
    );
    // No content, no nested SPAN/TEXT children carrying a body.
    assert!(find_child::<ast::SpanAttr>(span.syntax()).is_none());
}

#[test]
fn a_self_closing_span_with_an_attribute_parses() {
    let src = "flow f() {\n  @VENDOR: The curfew, kid. <sfx name=\"bell\"/> That.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("name")
            .to_string(),
        "sfx"
    );
    let attr = find_child::<ast::SpanAttr>(span.syntax()).expect("attr");
    assert!(attr.to_string().contains("name=\"bell\""));
}

// ── `<center>` is markup, not an element (§8d.3) ─────────────────────

#[test]
fn center_is_ordinary_markup_with_no_special_casing() {
    let src = "flow f() {\n  <center>LATER</center>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(
        find_child::<ast::SpanName>(span.syntax())
            .expect("name")
            .to_string(),
        "center"
    );
    assert_eq!(text_run_concat(span.syntax()), "LATER");
}

// ── Nesting doctrine (§4.3) ───────────────────────────────────────────

#[test]
fn a_span_may_contain_bare_interpolation() {
    let src = "flow f() {\n  var name = \"Fogg\"\n  <b>hello {name}</b>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert!(has_node_kind(span.syntax(), SyntaxKind::INTERPOLATION));
}

#[test]
fn a_conditional_branch_may_contain_a_fully_closed_span() {
    // Native's ruled conditional spelling is `{if cond: … else: …}`
    // (charter §6) — the ink-dialect bare `{cond: a|b}` shorthand the
    // spec's own worked examples use illustratively is not native surface.
    // The span opens and closes entirely inside the `if` branch, its own
    // fragment scope.
    let src = "flow f() {\n  {if hp > 0: <i>yawn</i> else: Ready.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONDITIONAL_BLOCK));
    let span = ast::Span::cast(first_node(&p.syntax(), SyntaxKind::SPAN)).expect("SPAN");
    assert_eq!(text_run_concat(span.syntax()), "yawn");
}

#[test]
fn a_span_opened_outside_a_conditional_cannot_close_inside_its_branch() {
    // The spec's own rejected shape (§4.3, `<b>hi {tired: there</b>|friend}`)
    // respelled onto native's ruled `{if cond: …}` syntax: `<b>` opens in
    // the outer scope, `</b>` appears inside the `if` branch. The branch's
    // own scan (whose stop set includes the branch boundary) reaches that
    // boundary before ever seeing `</b>`, so `<b>` is reported unclosed — a
    // diagnostic, not a panic or a silently-accepted cross-scope close.
    let src = "flow f() {\n  <b>hi {if hp > 0: there</b> else: friend}\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected a nesting-doctrine violation to be diagnosed"
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPAN));
}

#[test]
fn nested_spans_close_in_reverse_order() {
    let src = "flow f() {\n  <b><i>hi</i></b>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::SPAN), 2);
}

#[test]
fn an_unclosed_span_is_diagnosed_not_silently_accepted() {
    let src = "flow f() {\n  <b>hi\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPAN));
}

#[test]
fn a_stray_close_tag_with_no_open_is_diagnosed_and_does_not_hang() {
    let src = "flow f() {\n  surprise </b> more text after\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    // Forward progress: the text after the stray close tag must not be
    // dropped (CLAUDE.md: "flag silent data drops").
    assert!(text_run_concat(&p.syntax()).contains("more text after"));
}

#[test]
fn a_span_never_crosses_a_newline_it_is_line_scoped() {
    let src = "flow f() {\n  <b>hi\n  there</b>\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "unclosed on its own line");
}

// ── Escape set — final (§8d.6) ────────────────────────────────────────

#[test]
fn all_four_escapes_produce_literals_with_no_errors() {
    let src = "flow f() {\n  \\< \\{ \\# \\\\\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::ESCAPE), 4);
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SPAN));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::TAG));
}

#[test]
fn a_backslash_before_anything_else_is_a_compile_error() {
    let src = "flow f() {\n  \\n not an escape\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "the escape set is final (\\< \\{{ \\# \\\\) — \\n must error, not extend it"
    );
    // Forward progress: the rest of the line still parses as text.
    assert!(text_run_concat(&p.syntax()).contains("not an escape"));
}

#[test]
fn an_escaped_angle_bracket_does_not_open_a_span() {
    // `\>` is not in the escape set (only `\< \{ \# \\` are, §8d.6) — a
    // bare `>` needs no escaping in the first place, since `GT` alone
    // never opens or closes anything structural.
    let src = "flow f() {\n  Hello \\<world>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SPAN));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::ESCAPE), 1);
}

// ── Line-start escapes `\!` `\@` (§8d.6, issue #1744) ────────────────

#[test]
fn a_leading_backslash_at_escapes_to_a_literal_at_not_a_cue() {
    // Bare `@NAME` at item position opens a CUE (§8b.9); `\@NAME` must
    // produce a literal `@NAME` text run instead — the line-start escape
    // §8d.6 rules alongside the four inline ones.
    let src = "flow f() {\n  \\@VENDOR is just a mention, not a cue.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CUE));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::ESCAPE), 1);
    let escape = first_node(&p.syntax(), SyntaxKind::ESCAPE);
    assert_eq!(escape_literal(&escape), "@");
    assert!(text_run_concat(&p.syntax()).contains("VENDOR"));
}

#[test]
fn a_leading_backslash_bang_escapes_to_a_literal_bang() {
    // `\!` at line start produces a literal `!` (§8d.6's second line-start
    // escape, alongside `\@`) instead of a `BANG_DISPATCH` — the
    // annotation-element `!name` dispatch sigil §3.5b now implements
    // (issue #2004).
    let src = "flow f() {\n  \\!radio TAC-2: not a handler dispatch.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::BANG_DISPATCH));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::ESCAPE), 1);
    let escape = first_node(&p.syntax(), SyntaxKind::ESCAPE);
    assert_eq!(escape_literal(&escape), "!");
    assert!(text_run_concat(&p.syntax()).contains("radio"));
}

#[test]
fn a_compact_cue_dialogue_line_start_backslash_at_escapes_correctly() {
    // Regression for the trivia-flush bug in `line_start_escape`: the
    // fused dialogue line after `@KID:` (`COMPACT_CUE`, `element::cue_line`)
    // reaches `content::content_line` via a *raw* `bump()` on `COLON` with
    // no trivia flush in between, so the space after the colon is still
    // pending in the raw token stream when `line_start_escape` opens its
    // node. Before the fix, `line_start_escape`'s own raw `bump()` calls
    // consumed that pending `WHITESPACE` as if it were the `\`, leaving the
    // real `\` and `@` split across `ESCAPE[WHITESPACE, BACKSLASH]` +
    // `TEXT[AT, IDENT "VENDOR", …]` — the escape did the opposite of its
    // job and the `@` re-opened as a would-be cue sigil in the leftover
    // text. `line_start_escape` now flushes trivia itself before opening
    // the node, so the fix holds regardless of whether the caller already
    // flushed.
    let src = "flow f() {\n  @KID: \\@VENDOR waves.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::COMPACT_CUE));
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::CUE),
        "the escaped @ must not open a second, nested cue"
    );
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::ESCAPE), 1);
    let escape = first_node(&p.syntax(), SyntaxKind::ESCAPE);
    assert_eq!(escape_literal(&escape), "@");
    let escape_tokens: Vec<_> = escape
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .collect();
    assert_eq!(
        escape_tokens.len(),
        2,
        "ESCAPE must hold exactly `\\` + `@`, no leaked whitespace: {escape_tokens:?}"
    );
    assert_eq!(escape_tokens[0].text(), "\\");
    assert!(text_run_concat(&p.syntax()).contains("VENDOR waves"));
}

#[test]
fn a_mid_line_backslash_bang_or_at_is_still_a_compile_error() {
    // The line-start escapes are exactly that — line-start only. A `\!`/
    // `\@` anywhere else in a line is not in the four-char inline escape
    // set and remains the ordinary "backslash before anything else" error.
    let src = "flow f() {\n  text \\! and \\@ here\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "mid-line \\! / \\@ must still error — only line-start use is escaped"
    );
}

// ── Freeform by default (§4.2) ────────────────────────────────────────

#[test]
fn an_unknown_tag_name_is_not_a_parse_error() {
    // No manifest exists at the parser level — freeform by default means
    // an arbitrary tag name is only ever a grammar question here, never a
    // vocabulary question. Manifest validation is a separate, later pass.
    let src =
        "flow f() {\n  <totally_unrecognised_tag_name>text</totally_unrecognised_tag_name>\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPAN));
}

// ── Worked case — the complement-pass page fragment (spec §8d) ────────

#[test]
fn the_spec_worked_line_parses_clean() {
    let src = concat!(
        "flow f() {\n",
        "  The bell tolls again. <pause/> Somewhere above, a door slams.\n",
        "}\n",
    );
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPAN));
}
