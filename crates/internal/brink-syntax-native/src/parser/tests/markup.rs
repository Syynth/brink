//! Inline markup — XML-shaped spans, the escape set, and the nesting
//! doctrine (#1716; `docs/prose-dialect-spec.md` §4).

use super::*;

fn first_node(root: &SyntaxNode, kind: SyntaxKind) -> SyntaxNode {
    let found = root.descendants().find(|n| n.kind() == kind);
    assert!(found.is_some(), "no {kind:?} node in tree");
    found.expect("asserted present just above")
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

// ── Freeform by default (§4.2) ────────────────────────────────────────

#[test]
fn an_unknown_tag_name_is_not_a_parse_error() {
    // No manifest exists at the parser level — freeform by default means
    // an arbitrary tag name is only ever a grammar question here, never a
    // vocabulary question. Manifest validation is a separate, later pass.
    let src = "flow f() {\n  <totally_unrecognised_tag_name>text</totally_unrecognised_tag_name>\n}\n";
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
