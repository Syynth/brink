//! Declarations — `flow`/`fn`/`var`/`const`/`flags`/`struct`/`extern`, plus
//! `use`/`import`/`module` and doc-comment attachment to declaration nodes.
//! Family for #1192.

use super::*;

#[test]
fn minimal_flow_decl() {
    let p = assert_lossless("flow greet() {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn flow_with_prose_body() {
    let p = assert_lossless("flow greet(name) {\n  Hello, {name}! <>\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn fn_decl_parses() {
    let p = assert_lossless("fn heal(hp) {\n  var x = hp + 1\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn use_and_import_and_module() {
    let src = "use story::npcs::{guard, merchant as trader};\nimport story::items\nmodule inner {\n  var secret = 1\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn use_decl_semicolon_is_consumed_by_the_decl_not_left_as_prose() {
    // `;` has no role anywhere else in the grammar — confirm it becomes a
    // token *inside* USE_DECL, not a stray token that just happens to
    // round-trip as unrelated adjacent prose text.
    let src = "use a::b;\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let use_decl = p
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::USE_DECL)
        .expect("USE_DECL");
    assert!(
        use_decl
            .children_with_tokens()
            .any(|t| t.kind() == SyntaxKind::SEMICOLON),
        "expected the `;` inside USE_DECL, tree: {use_decl:#?}"
    );
    // And nothing else at the top level — the `;` didn't spawn its own
    // stray CONTENT_LINE sibling.
    assert_eq!(p.syntax().children().count(), 1);
}

#[test]
fn use_decl_without_semicolon_still_parses() {
    let src = "use a::b\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn var_const_flags_struct_extern() {
    let src = "var hp = 10\nconst MAX = 100\nflags Mood = (calm), wary, hostile\nstruct Item {\n  name: string,\n  weight: int\n}\nextern log(msg)\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn nested_stitch_flow() {
    let src = "flow garden() {\n  flow gate() {\n    Creak.\n  }\n  -> gate\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── B0.6b: doc-comment CST attachment ────────────────────────────────

#[test]
fn leading_doc_comment_attaches_as_flow_decls_first_child() {
    let src = "/// Greets the player.\nflow greet() {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let first_child = flow
        .syntax()
        .children()
        .next()
        .expect("flow decl has a child node");
    assert_eq!(first_child.kind(), SyntaxKind::DOC_COMMENT);
    assert_eq!(first_child.text(), "/// Greets the player.\n");
}

#[test]
fn multiline_leading_doc_comment_is_one_doc_comment_node() {
    let src = "/// Line one.\n/// Line two.\nflow greet() {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    assert_eq!(count_node_kind(flow.syntax(), SyntaxKind::DOC_COMMENT), 1);
    let doc = flow.doc().expect("doc attached");
    assert_eq!(doc.lines().len(), 2);
}

#[test]
fn leading_doc_comment_attaches_to_var_const_flags_struct_extern() {
    for (src, kind) in [
        ("/// x\nvar x = 1\n", SyntaxKind::VAR_DECL),
        ("/// x\nconst x = 1\n", SyntaxKind::CONST_DECL),
        ("/// x\nflags F = a, b\n", SyntaxKind::FLAGS_DECL),
        ("/// x\nstruct S { f: int }\n", SyntaxKind::STRUCT_DECL),
        ("/// x\nextern e(a)\n", SyntaxKind::EXTERN_DECL),
    ] {
        let p = assert_lossless(src);
        assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
        let decl = p.syntax().children().find(|n| n.kind() == kind);
        assert!(decl.is_some(), "{kind:?} not found in {src:?}");
        let decl = decl.expect("checked above");
        let first_child = decl.children().next().expect("decl has a child node");
        assert_eq!(
            first_child.kind(),
            SyntaxKind::DOC_COMMENT,
            "{src:?}: doc did not attach as leading child"
        );
    }
}

#[test]
fn blank_line_breaks_the_leading_doc_run_unattached_falls_back_to_bare_tokens() {
    // The B0.6b judgment call: an unattached doc — separated from the
    // following declaration by a blank line — falls back to sitting bare
    // in the tree (no DOC_COMMENT node, no diagnostic), same as trivia.
    let src = "/// orphaned\n\nflow greet() {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::DOC_COMMENT));
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl still parses");
    assert!(flow.doc().is_none());
}

#[test]
fn doc_comment_with_no_following_declaration_falls_back_to_bare_tokens() {
    let src = "/// just some prose after this, no decl\nHello there.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::DOC_COMMENT));
}

#[test]
fn plain_comment_between_doc_lines_breaks_the_run() {
    // Matches the OLD ink parser's `collect_doc_lines` precedent
    // (`plain_comment_breaks_the_block`): a plain `//` line ends the doc
    // run entirely — the `/// kept` line above it does NOT attach to
    // whatever eventually follows the `//` comment either (there's no
    // partial-attachment concept; it falls back to bare/trivia-shaped
    // tokens like any other unattached run).
    let src = "/// kept\n// not a doc line\nflow greet() {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::DOC_COMMENT));
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    assert!(flow.doc().is_none());
}

#[test]
fn inner_doc_comment_attaches_to_flow_body_block() {
    let src = "flow greet() {\n//! Describes this flow from within.\nHi!\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = flow.body().expect("body block");
    let first_child = body
        .syntax()
        .children()
        .next()
        .expect("block has a child node");
    assert_eq!(first_child.kind(), SyntaxKind::DOC_COMMENT);
    let doc = body.doc().expect("inner doc accessor");
    assert!(doc.is_inner());
    assert_eq!(doc.lines().len(), 1);
}

#[test]
fn inner_doc_comment_attaches_to_source_file() {
    let src = "//! File-level doc.\nflow greet() {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let file = ast::SourceFile::cast(p.syntax()).expect("source file");
    let doc = file.doc().expect("file-level inner doc");
    assert!(doc.is_inner());
    assert_eq!(doc.lines()[0].0, "File-level doc.");
}

#[test]
fn inner_doc_tolerates_leading_blank_lines() {
    // "At the start of the body" tolerates leading blank lines — only
    // real content before the `//!` run disqualifies it (see the next
    // test), not blank formatting.
    let src = "flow greet() {\n\n//! still attaches\nHi!\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = flow.body().expect("body block");
    let doc = body
        .doc()
        .expect("inner doc still attaches past blank lines");
    assert!(doc.is_inner());
}

#[test]
fn inner_doc_after_real_content_does_not_attach_to_block() {
    // Only a `//!` run that is the body's very first REAL content
    // attaches (B0.6b judgment call) — one preceded by an actual content
    // line has no "start of body" to attach to. But it must NOT fall
    // through into visible narrative either: a doc-comment token is bumped
    // BARE by the content scanner (`content.rs`), never folded into a
    // `TEXT` node, so it stays invisible — the trivia-fallback the
    // unattached-`///` path already gives. The invariant this guards: a
    // doc-comment token must never become story prose.
    let src = "flow greet() {\nHi!\n//! not attached\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = flow.body().expect("body block");
    assert!(body.doc().is_none());
    assert!(!has_node_kind(body.syntax(), SyntaxKind::DOC_COMMENT));
    // The `//!` text must not leak into any visible `TEXT` run.
    let leaked = body
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .any(|n| n.text().to_string().contains("not attached"));
    assert!(!leaked, "doc-comment token folded into visible TEXT");
}

#[test]
fn outer_and_inner_doc_tokens_are_not_trivia_in_the_tree() {
    // Sanity check on the CST-node-attachment claim itself: unlike plain
    // `//` trivia, DOC_COMMENT_OUTER/INNER tokens are real (non-trivia)
    // tokens the parser dispatches on, wrapped in a real DOC_COMMENT node.
    let src = "/// doc\nflow greet() {\n}\n";
    let p = assert_lossless(src);
    let doc_node = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DOC_COMMENT)
        .expect("DOC_COMMENT node exists");
    let tok = doc_node
        .first_token()
        .expect("doc comment node has a token");
    assert_eq!(tok.kind(), SyntaxKind::DOC_COMMENT_OUTER);
    assert!(!tok.kind().is_trivia());
}
