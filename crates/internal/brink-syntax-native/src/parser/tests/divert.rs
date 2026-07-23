//! Diverts, tunnels, and splice. Family for #1196.
//!
//! Parity target: `brink-syntax/src/parser/tests/divert/mod.rs`. The native
//! grammar (`parser/divert.rs`, charter §11) is a deliberately narrower
//! reshape of ink's divert family — no `->->` tunnel-onwards node, no
//! `<-`-as-general-thread-start, and (per the probe below) exactly one hop
//! of tunnel-call arrow, not ink's arbitrary chain. Tests here assert the
//! ACTUAL native grammar's shape, not ink's, per the issue's own
//! instruction to "check `divert.rs`'s actual grammar first".

use super::*;

// ── Original family-split smoke tests (kept from the #1229 split) ───

#[test]
fn divert_and_tunnel_and_return() {
    let src = "flow a() {\n  -> b\n}\nflow b() {\n  -> c ->\n  return\n}\nflow c() {\n  return -> a\n  -> END\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn splice_inside_choice_point() {
    let src = "flow hub() {\n  {?\n    <- options()\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── N-1: inline diverts in content position ─────────────────────────

#[test]
fn divert_after_choice_bracket_text_is_a_divert_node_not_text() {
    // The exact shape from the exhibit/manual-stitch-v1 fixtures:
    // `* [text] -> target`. Before N-1's fix this parsed with zero errors
    // but folded `-> know_about_wager` into a literal TEXT run.
    let src = "flow f() {\n  {?\n    * [The wager.] -> know_about_wager\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice_inner = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    assert!(
        has_node_kind(&choice_inner, SyntaxKind::DIVERT_STMT),
        "expected a DIVERT_STMT inside CHOICE_INNER_CONTENT, tree: {choice_inner:#?}"
    );
    // The divert's target must be a real PATH, not swallowed text.
    let divert = choice_inner
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    let path = target.path().expect("path");
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["know_about_wager".to_string()]);
}

#[test]
fn divert_after_dotted_path_target_in_choice_text_parses() {
    // manual-stitch-v1's other shape: a dotted stitch-addressing target.
    let src = "flow f() {\n  {?\n    * [go] -> f.g\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let divert = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    let path = target.path().expect("path");
    assert!(!path.crosses_module_wall()); // `.` not `::`
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["f".to_string(), "g".to_string()]);
}

#[test]
fn divert_inside_multiline_choice_body_after_prose_is_a_divert_node() {
    // The sticky-choice shape: a `->` following prose on the SAME content
    // line inside a braced CHOICE_BODY (as opposed to a divert on its own
    // line, which was already recognized before this fix).
    let src = "flow f() {\n  {?\n    + [Eat] {\n      You eat another donut. -> f\n    }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let body = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BODY)
        .expect("CHOICE_BODY");
    let content_line = body
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    // The divert is a child of the same CONTENT_LINE as the preceding
    // prose, not a sibling body item.
    assert!(
        has_node_kind(&content_line, SyntaxKind::DIVERT_STMT),
        "expected DIVERT_STMT nested inside the CONTENT_LINE, tree: {content_line:#?}"
    );
    assert!(has_node_kind(&content_line, SyntaxKind::TEXT));
}

#[test]
fn tunnel_call_in_content_position_parses() {
    // `->->` in content position: still a TUNNEL_CALL, not a divert
    // followed by stray text.
    let src = "flow f() {\n  {?\n    * [go] visit -> place ->\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
}

#[test]
fn divert_to_end_in_content_position_parses() {
    let src = "flow f() {\n  {?\n    * [go] The end. -> END\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let divert = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    assert!(target.is_end());
}

// ── DIVERT_TARGET's three forms: END / DONE / PATH ──────────────────

#[test]
fn divert_target_end() {
    let src = "flow f() {\n  -> END\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let target = p
        .syntax()
        .descendants()
        .find_map(crate::ast::DivertTarget::cast)
        .expect("DIVERT_TARGET");
    assert!(target.is_end());
    assert!(!target.is_done());
    assert!(target.path().is_none());
}

#[test]
fn divert_target_done() {
    let src = "flow f() {\n  -> DONE\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let target = p
        .syntax()
        .descendants()
        .find_map(crate::ast::DivertTarget::cast)
        .expect("DIVERT_TARGET");
    assert!(target.is_done());
    assert!(!target.is_end());
    assert!(target.path().is_none());
}

#[test]
fn divert_target_path() {
    let src = "flow f() {\n  -> knot\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let target = p
        .syntax()
        .descendants()
        .find_map(crate::ast::DivertTarget::cast)
        .expect("DIVERT_TARGET");
    assert!(!target.is_end());
    assert!(!target.is_done());
    let path = target.path().expect("PATH");
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["knot".to_string()]);
}

#[test]
fn divert_target_path_dotted() {
    let src = "flow f() {\n  -> knot.stitch\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let target = p
        .syntax()
        .descendants()
        .find_map(crate::ast::DivertTarget::cast)
        .expect("DIVERT_TARGET");
    let path = target.path().expect("PATH");
    assert!(!path.crosses_module_wall());
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["knot".to_string(), "stitch".to_string()]);
}

#[test]
fn divert_target_path_module_wall() {
    // `::` crosses a module wall (charter §13.2), distinct from `.`.
    let src = "flow f() {\n  -> a::b\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let target = p
        .syntax()
        .descendants()
        .find_map(crate::ast::DivertTarget::cast)
        .expect("DIVERT_TARGET");
    let path = target.path().expect("PATH");
    assert!(path.crosses_module_wall());
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["a".to_string(), "b".to_string()]);
}

// ── DIVERT_STMT ───────────────────────────────────────────────────────

#[test]
fn simple_divert_is_divert_stmt_not_tunnel_call() {
    let src = "flow f() {\n  -> knot\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
}

#[test]
fn content_then_divert_at_top_level() {
    // The content-position sibling grammar (`divert_in_content`), not just
    // the choice-only variant already covered elsewhere in this file.
    let src = "flow f() {\n  Hello -> knot\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let content_line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert!(has_node_kind(&content_line, SyntaxKind::TEXT));
    assert!(has_node_kind(&content_line, SyntaxKind::DIVERT_STMT));
    let divert = find_child::<crate::ast::DivertStmt>(&content_line)
        .or_else(|| {
            content_line
                .descendants()
                .find_map(crate::ast::DivertStmt::cast)
        })
        .expect("DIVERT_STMT");
    let target = divert.target().expect("DIVERT_TARGET");
    let segs: Vec<_> = target
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["knot".to_string()]);
}

// ── TUNNEL_CALL and its disambiguation from a plain DIVERT_STMT ──────

#[test]
fn tunnel_call_simple() {
    let src = "flow f() {\n  -> place ->\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call = p
        .syntax()
        .descendants()
        .find_map(crate::ast::TunnelCall::cast)
        .expect("TUNNEL_CALL");
    let target = call.target().expect("DIVERT_TARGET");
    let segs: Vec<_> = target
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["place".to_string()]);
}

#[test]
fn tunnel_call_dotted_target() {
    let src = "flow f() {\n  -> knot.stitch ->\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call = p
        .syntax()
        .descendants()
        .find_map(crate::ast::TunnelCall::cast)
        .expect("TUNNEL_CALL");
    let target = call.target().expect("DIVERT_TARGET");
    let segs: Vec<_> = target
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["knot".to_string(), "stitch".to_string()]);
}

#[test]
fn tunnel_call_to_end_target_parses_but_is_a_semantic_question() {
    // `-> END ->` is a syntactically valid TUNNEL_CALL — the grammar has no
    // opinion on whether an END-target tunnel call makes sense; that is the
    // analyzer's job, not the parser's (mirrors `divert_target`'s
    // `KW_END`/`KW_DONE`/`PATH` disjunction applying uniformly regardless
    // of the enclosing node).
    let src = "flow f() {\n  -> END ->\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call = p
        .syntax()
        .descendants()
        .find_map(crate::ast::TunnelCall::cast)
        .expect("TUNNEL_CALL");
    assert!(call.target().expect("DIVERT_TARGET").is_end());
}

#[test]
fn regular_divert_not_tunnel_call() {
    let src = "flow f() {\n  -> target\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT));
}

#[test]
fn divert_with_unrelated_trailing_content_is_not_a_tunnel_call() {
    // A `DIVERT_STMT` followed, on the same line, by unrelated content that
    // is NOT a second `->` — this must stay a plain divert plus separate
    // content, never a `TUNNEL_CALL` (which requires the second arrow
    // immediately, modulo whitespace, per `divert_or_tunnel_core`).
    let src = "flow f() {\n  -> knot extra text\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
    let divert = p
        .syntax()
        .descendants()
        .find_map(crate::ast::DivertStmt::cast)
        .expect("DIVERT_STMT");
    let segs: Vec<_> = divert
        .target()
        .expect("DIVERT_TARGET")
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(
        segs,
        vec!["knot".to_string()],
        "the divert target must not have swallowed `extra`/`text`"
    );
    assert!(
        p.syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CONTENT_LINE),
        "`extra text` must land in its own CONTENT_LINE"
    );
}

#[test]
fn second_arrow_not_immediately_after_target_is_still_a_tunnel_call() {
    // Whitespace between the target and the closing arrow is fine —
    // `divert_or_tunnel_core` does `p.skip_ws()` before checking for the
    // second `DIVERT`.
    let src = "flow f() {\n  -> place   ->\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
}

#[test]
fn no_third_arrow_native_grammar_has_no_divert_chaining() {
    // Ink's parity suite has a `divert_chain` test (`-> tunnel -> next`)
    // that stays a single ink DIVERT chained arbitrarily. The native
    // grammar has no such concept: the first `->`/target/`->` triple
    // always closes as a `TUNNEL_CALL`, and anything after the second
    // arrow is ordinary trailing content — here a bare-word CONTENT_LINE,
    // not a further divert.
    let src = "flow f() {\n  -> a -> b\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
    // Only ONE tunnel call / divert-stmt total — `b` is content, not a
    // second target.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL), 1);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT), 0);
    let call = p
        .syntax()
        .descendants()
        .find_map(crate::ast::TunnelCall::cast)
        .expect("TUNNEL_CALL");
    let segs: Vec<_> = call
        .target()
        .expect("DIVERT_TARGET")
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["a".to_string()]);
    assert!(
        p.syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CONTENT_LINE),
        "`b` must land in a trailing CONTENT_LINE, not a chained divert"
    );
}

// ── RETURN_STMT / RETURN_REDIRECT ────────────────────────────────────

#[test]
fn bare_return_stmt() {
    let src = "flow f() {\n  -> place ->\n  return\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::RETURN_STMT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::RETURN_REDIRECT));
}

#[test]
fn return_redirect_simple() {
    let src = "flow f() {\n  return -> x\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let redirect = p
        .syntax()
        .descendants()
        .find_map(crate::ast::ReturnRedirect::cast)
        .expect("RETURN_REDIRECT");
    let segs: Vec<_> = redirect
        .target()
        .expect("DIVERT_TARGET")
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["x".to_string()]);
    // A RETURN_REDIRECT is not also counted as a bare RETURN_STMT — they
    // are distinct node kinds, not one wrapping the other
    // (`return_stmt`'s `start_node_at` picks exactly one).
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::RETURN_STMT));
}

#[test]
fn return_redirect_to_end() {
    let src = "flow f() {\n  return -> END\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let redirect = p
        .syntax()
        .descendants()
        .find_map(crate::ast::ReturnRedirect::cast)
        .expect("RETURN_REDIRECT");
    assert!(redirect.target().expect("DIVERT_TARGET").is_end());
}

#[test]
fn return_redirect_dotted_target() {
    let src = "flow f() {\n  return -> knot.stitch\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let redirect = p
        .syntax()
        .descendants()
        .find_map(crate::ast::ReturnRedirect::cast)
        .expect("RETURN_REDIRECT");
    let segs: Vec<_> = redirect
        .target()
        .expect("DIVERT_TARGET")
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["knot".to_string(), "stitch".to_string()]);
}

#[test]
fn return_followed_by_unrelated_content_is_bare_return_stmt() {
    // `return` not immediately followed by `->` must NOT become a
    // RETURN_REDIRECT — the trailing content is a separate item, exactly
    // the divert/tunnel-call disambiguation's sibling case.
    let src = "flow f() {\n  return home\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::RETURN_STMT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::RETURN_REDIRECT));
    assert!(
        p.syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CONTENT_LINE),
        "`home` must land in its own CONTENT_LINE, not get folded into RETURN_STMT"
    );
}

// ── SPLICE — valid inside a CHOICE_POINT ─────────────────────────────

#[test]
fn splice_basic() {
    let src = "flow f() {\n  {?\n    <- side_thread\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let splice = p
        .syntax()
        .descendants()
        .find_map(crate::ast::Splice::cast)
        .expect("SPLICE");
    let segs: Vec<_> = splice
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["side_thread".to_string()]);
    assert!(splice.arg_list().is_none());
}

#[test]
fn splice_with_args() {
    let src = "flow f() {\n  {?\n    <- options(gold, 2)\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let splice = p
        .syntax()
        .descendants()
        .find_map(crate::ast::Splice::cast)
        .expect("SPLICE");
    let arg_list = splice.arg_list().expect("ARG_LIST");
    assert!(arg_list.is_open());
}

#[test]
fn splice_with_dotted_path() {
    let src = "flow f() {\n  {?\n    <- hub.options\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let splice = p
        .syntax()
        .descendants()
        .find_map(crate::ast::Splice::cast)
        .expect("SPLICE");
    let segs: Vec<_> = splice
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["hub".to_string(), "options".to_string()]);
}

#[test]
fn splice_coexists_with_choice_lines() {
    let src = "flow f() {\n  {?\n    * [Look] You look around.\n    <- extra_choices\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CHOICE));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPLICE));
}

#[test]
fn splice_outside_choice_point_is_not_a_splice_node() {
    // Charter §11: "no native spelling for general `<-`; only the scoped
    // splice inside choice points survives." Outside a `CHOICE_POINT`,
    // `choice.rs::splice` is never called — `<-` lexes to a bare `THREAD`
    // token that `block::body_line` now dispatches to
    // `choice::splice_outside_choice_point` (issue #1263, ruled #1260 on
    // #1256), which folds it into an ordinary `TEXT` run exactly as
    // before, but ALSO raises a warning-severity diagnostic — not a hard
    // error (`<-` can be literal dialogue punctuation) and not silent
    // either (a real gap the original version of this test documented and
    // pinned as a TODO). `side_thread` is a bare identifier with nothing
    // else on the line, so this is the high-confidence "looks like a
    // misremembered ink thread" shape.
    let src = "flow f() {\n  <- side_thread\n}\n";
    let p = assert_lossless(src);
    assert_eq!(
        p.errors().len(),
        1,
        "expected exactly one warning-severity diagnostic for `<-` outside \
         a choice point; errors: {:?}",
        p.errors()
    );
    let diag = &p.errors()[0];
    assert_eq!(
        diag.severity,
        ParseSeverity::Warning,
        "a splice outside a choice point must warn, never hard-error — \
         `<-` can be literal dialogue (issue #1263); diagnostic: {diag:?}"
    );
    assert!(
        diag.message.contains("knot/flow reference"),
        "`<- side_thread` is shaped exactly like a real flow reference (a \
         bare identifier, nothing else on the line) — the diagnostic \
         should be the higher-confidence variant; message: {}",
        diag.message
    );
    // `<-` (2 bytes) starts at byte 13 of `src` ("flow f() {\n  " is 13
    // bytes: `flow f() {\n` is 11, plus the two leading spaces of `  <-`).
    assert_eq!(diag.range, rowan::TextRange::at(13.into(), 2.into()));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SPLICE));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TEXT));
}

#[test]
fn splice_outside_choice_point_low_confidence_when_not_reference_shaped() {
    // Same warning, but the tail after `<-` doesn't look like a bare
    // knot/flow reference (a quoted string, not an `IDENT`-led path) — the
    // lower-confidence message variant, still a warning, never an error.
    let src = "flow f() {\n  <- \"not a reference\"\n}\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1, "errors: {:?}", p.errors());
    let diag = &p.errors()[0];
    assert_eq!(diag.severity, ParseSeverity::Warning);
    assert!(
        !diag.message.contains("knot/flow reference"),
        "message should be the generic (low-confidence) variant: {}",
        diag.message
    );
}

#[test]
fn splice_outside_choice_point_high_confidence_with_call_args() {
    // `<- name(args)` — reference-shaped even with a call arg list.
    let src = "flow f() {\n  <- options(true)\n}\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1, "errors: {:?}", p.errors());
    let diag = &p.errors()[0];
    assert_eq!(diag.severity, ParseSeverity::Warning);
    assert!(
        diag.message.contains("knot/flow reference"),
        "message: {}",
        diag.message
    );
}

// ── Divert targets with call-style args (#1265, fixed bug #1196) ──────

#[test]
fn divert_target_with_call_args_captures_the_args_bug_1196() {
    // Charter §11 lists "-> knot(args)" as KEPT VERBATIM from ink
    // ("Diverts — KEPT verbatim (`->`, `-> knot(args)`, ...)").
    // `divert::divert_target` now routes a `PATH` target through the same
    // call-capable grammar `expr::path_or_call` uses (`expr::arg_list`),
    // so the parenthesized args are captured as an `ARG_LIST` sibling of
    // `PATH` under `DIVERT_TARGET`, with zero parse errors and nothing
    // left orphaned in a sibling `CONTENT_LINE`. Formerly (bug #1196) the
    // args parsed with zero errors but were dropped entirely, floating
    // into an unrelated `CONTENT_LINE`.
    let src = "flow f() {\n  -> greet(\"hello\")\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let divert = p
        .syntax()
        .descendants()
        .find_map(crate::ast::DivertStmt::cast)
        .expect("DIVERT_STMT");
    let target = divert.target().expect("DIVERT_TARGET");
    let segs: Vec<_> = target
        .path()
        .expect("PATH")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["greet".to_string()]);

    let arg_list = target
        .call_args()
        .expect("ARG_LIST captured under DIVERT_TARGET");
    assert_eq!(
        arg_list.syntax().parent().expect("DIVERT_TARGET").kind(),
        SyntaxKind::DIVERT_TARGET,
        "the ARG_LIST must be a direct child of DIVERT_TARGET, not wrapped \
         in a CALL_EXPR — a divert target is not itself an expression"
    );
    let args_text = arg_list.syntax().text().to_string();
    assert!(
        args_text.contains("hello"),
        "expected the string literal argument inside ARG_LIST, got: {args_text:?}"
    );

    // No more orphaned CONTENT_LINE holding the args — the whole flow body
    // is just the one DIVERT_STMT.
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE),
        "the args must not leak into a sibling CONTENT_LINE anymore"
    );
}

// ── Error recovery: malformed input must not panic, stays lossless ──

#[test]
fn divert_with_no_target_recovers() {
    let src = "flow f() {\n  -> \n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "a target-less divert should record a diagnostic"
    );
    // Still a well-formed (if error-carrying) DIVERT_STMT with an empty
    // DIVERT_TARGET — no panic, no dropped bytes.
    assert!(has_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::DIVERT_TARGET));
}

#[test]
fn tunnel_call_with_no_target_recovers() {
    // `->->` with no space and no target between the arrows — the second
    // `->` is greedily consumed as the (missing) target's first token,
    // producing an error, then reinterpreted as the closing arrow.
    let src = "flow f() {\n  ->->\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
}

#[test]
fn return_redirect_with_no_target_recovers() {
    let src = "flow f() {\n  return -> \n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::RETURN_REDIRECT));
}

#[test]
fn divert_at_eof_with_no_newline_recovers() {
    // No trailing NEWLINE at all — `divert_or_tunnel`'s `if p.at(NEWLINE)`
    // guard must not choke on EOF.
    let src = "-> knot";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT));
}

#[test]
fn tunnel_call_at_eof_with_no_newline_recovers() {
    let src = "-> place ->";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
}

#[test]
fn splice_with_no_target_recovers() {
    let src = "flow f() {\n  {?\n    <- \n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPLICE));
}

#[test]
fn splice_with_unterminated_arg_list_recovers() {
    let src = "flow f() {\n  {?\n    <- options(gold\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPLICE));
}

#[test]
fn bare_arrow_soup_never_panics() {
    // Adversarial: a run of arrows with no valid target anywhere, deep
    // enough to exercise repeated zero-progress recovery.
    let src = "flow f() {\n  -> -> -> -> -> ->\n}\n";
    let p = assert_lossless(src);
    // Not asserting on error count — only that it stays lossless and does
    // not panic/hang (`assert_lossless` itself is the load-bearing check).
    let _ = p.errors();
}

#[test]
fn return_redirect_chained_arrows_never_panics() {
    let src = "flow f() {\n  return -> -> ->\n}\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

#[test]
fn mismatched_tunnel_arrows_across_lines_recovers() {
    // A tunnel call's second arrow, when missing, must not eat the
    // following line's unrelated divert.
    let src = "flow f() {\n  -> place\n  -> next\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT), 2);
}

#[test]
fn splice_immediately_followed_by_r_brace_recovers() {
    let src = "flow f() {\n  {?\n    <-\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::SPLICE));
}

#[test]
fn deeply_nested_divert_targets_do_not_overflow() {
    // Adversarial depth: a long dotted path is flat (not recursive) in
    // this grammar, but exercise a long chain anyway to confirm the
    // `path()` loop has no quadratic/stack-depth surprise.
    let mut src = String::from("flow f() {\n  -> ");
    for i in 0..500 {
        if i > 0 {
            src.push('.');
        }
        src.push('a');
    }
    src.push('\n');
    src.push_str("}\n");
    let p = assert_lossless(&src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── Insta snapshots for full-tree shape ──────────────────────────────

#[test]
fn insta_tunnel_call() {
    let p = parse("flow f() {\n  -> place ->\n}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_return_redirect() {
    let p = parse("flow f() {\n  return -> x\n}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_splice_with_args() {
    let p = parse("flow f() {\n  {?\n    <- options(gold, 2)\n  }\n}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

// ── Proptest round-trip generators (family-local, per #1196's hint —
// `tests/proptest_native.rs` is #1199's this wave) ────────────────────

mod proptest_divert {
    use proptest::prelude::*;

    use crate::parse;

    const NUM_CASES: u32 = 256;

    /// Mirrors `tests/proptest_native.rs`'s own `KEYWORDS` list (this
    /// family's generator is intentionally independent — see #1196's
    /// scoping note about not touching that file this wave — but the
    /// keyword set it must avoid is the same grammar).
    const KEYWORDS: &[&str] = &[
        "flow", "fn", "var", "const", "let", "flags", "struct", "extern", "import", "use",
        "module", "return", "ref", "if", "match", "else", "as", "in", "true", "false", "END", "DONE",
    ];

    fn arb_ident() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,7}"
            .prop_filter("must not be a keyword", |s| !KEYWORDS.contains(&s.as_str()))
    }

    fn arb_dotted_path() -> impl Strategy<Value = String> {
        prop::collection::vec(arb_ident(), 1..=4).prop_map(|segs| segs.join("."))
    }

    fn arb_divert_target() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("END".to_string()),
            Just("DONE".to_string()),
            arb_dotted_path(),
        ]
    }

    fn arb_flow_wrapped_divert() -> impl Strategy<Value = String> {
        arb_divert_target().prop_map(|target| format!("flow f() {{\n  -> {target}\n}}\n"))
    }

    fn arb_flow_wrapped_tunnel_call() -> impl Strategy<Value = String> {
        arb_divert_target().prop_map(|target| format!("flow f() {{\n  -> {target} ->\n}}\n"))
    }

    fn arb_flow_wrapped_return_redirect() -> impl Strategy<Value = String> {
        arb_divert_target().prop_map(|target| format!("flow f() {{\n  return -> {target}\n}}\n"))
    }

    fn arb_flow_wrapped_splice() -> impl Strategy<Value = String> {
        (arb_dotted_path(), prop::collection::vec(arb_ident(), 0..=3)).prop_map(|(path, args)| {
            let call = if args.is_empty() {
                path
            } else {
                format!("{path}({})", args.join(", "))
            };
            format!("flow f() {{\n  {{?\n    <- {call}\n  }}\n}}\n")
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(NUM_CASES))]

        #[test]
        fn divert_roundtrips(input in arb_flow_wrapped_divert()) {
            let parsed = parse(&input);
            prop_assert_eq!(parsed.syntax().text().to_string(), input.clone());
            prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
        }

        #[test]
        fn tunnel_call_roundtrips(input in arb_flow_wrapped_tunnel_call()) {
            let parsed = parse(&input);
            prop_assert_eq!(parsed.syntax().text().to_string(), input.clone());
            prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
            prop_assert!(
                parsed.syntax().descendants().any(|n| n.kind() == crate::SyntaxKind::TUNNEL_CALL)
            );
        }

        #[test]
        fn return_redirect_roundtrips(input in arb_flow_wrapped_return_redirect()) {
            let parsed = parse(&input);
            prop_assert_eq!(parsed.syntax().text().to_string(), input.clone());
            prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
            prop_assert!(
                parsed.syntax().descendants().any(|n| n.kind() == crate::SyntaxKind::RETURN_REDIRECT)
            );
        }

        #[test]
        fn splice_roundtrips(input in arb_flow_wrapped_splice()) {
            let parsed = parse(&input);
            prop_assert_eq!(parsed.syntax().text().to_string(), input.clone());
            prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
            prop_assert!(
                parsed.syntax().descendants().any(|n| n.kind() == crate::SyntaxKind::SPLICE)
            );
        }

        // ── Adversarial: never panics, always lossless ─────────────────

        #[test]
        fn arrow_soup_never_panics(n in 1usize..12) {
            let arrows = "-> ".repeat(n);
            let src = format!("flow f() {{\n  {arrows}\n}}\n");
            let parsed = parse(&src);
            prop_assert_eq!(parsed.syntax().text().to_string(), src);
        }

        #[test]
        fn truncated_divert_never_panics(target in arb_dotted_path(), cut in 0u32..100) {
            let full = format!("flow f() {{\n  -> {target} ->\n}}\n");
            let target_len = (full.len() as u64 * u64::from(cut) / 100) as usize;
            let mut end = target_len.min(full.len());
            while end > 0 && !full.is_char_boundary(end) {
                end -= 1;
            }
            let truncated = &full[..end];
            let parsed = parse(truncated);
            prop_assert_eq!(parsed.syntax().text().to_string(), truncated);
        }

        #[test]
        fn splice_outside_choice_point_never_panics(path in arb_dotted_path()) {
            let src = format!("flow f() {{\n  <- {path}\n}}\n");
            let parsed = parse(&src);
            prop_assert_eq!(parsed.syntax().text().to_string(), src);
        }
    }
}
