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

// ── #1192: exhaustive per-shape coverage ──────────────────────────────
//
// Parity target: `brink-syntax/src/parser/tests/declaration/mod.rs` (28
// tests) + `brink-syntax/src/parser/tests/knot/mod.rs` (13 tests) —
// `FLOW_DECL`'s nested form is the native analog of ink's knot/stitch
// nesting. The tests above already exercise doc-comment CST attachment
// (B0.6b); everything below fills the remaining per-node-kind, param,
// use-tree, and error-recovery gaps the issue calls out.

// ── flow / fn: params, ref, nested stitches ────────────────────────────

#[test]
fn flow_decl_single_param() {
    let p = assert_lossless("flow greet(name) {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let params: Vec<_> = flow.param_list().expect("param list").params().collect();
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name_token().expect("param name").text(), "name");
    assert!(!params[0].is_ref());
}

#[test]
fn flow_decl_multiple_params_mixed_ref() {
    let p = assert_lossless("flow modify(ref x, y, ref z) {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let params: Vec<_> = flow.param_list().expect("param list").params().collect();
    assert_eq!(params.len(), 3);
    let shape: Vec<(String, bool)> = params
        .iter()
        .map(|p| (p.name_token().expect("name").text().to_string(), p.is_ref()))
        .collect();
    assert_eq!(
        shape,
        vec![
            ("x".to_string(), true),
            ("y".to_string(), false),
            ("z".to_string(), true),
        ]
    );
}

#[test]
fn fn_decl_ref_param() {
    let p = assert_lossless("fn tweak(ref amount) {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let f: ast::FnDecl = find_child(&p.syntax()).expect("fn decl");
    let params: Vec<_> = f.param_list().expect("param list").params().collect();
    assert_eq!(params.len(), 1);
    assert!(params[0].is_ref());
}

#[test]
fn flow_decl_empty_param_list() {
    // `flow name() { … }` — param list present but empty (distinct shape
    // from no param list at all, e.g. `flow name { … }`).
    let p = assert_lossless("flow greet() {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    assert_eq!(flow.param_list().expect("param list").params().count(), 0);
}

#[test]
fn flow_decl_no_param_list_at_all() {
    // `flow name { … }` — no `(` at all is a distinct valid shape from
    // `flow name() { … }` (`flow_decl` only calls `param_list` when
    // `p.at(L_PAREN)`).
    let p = assert_lossless("flow greet {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    assert!(flow.param_list().is_none());
}

#[test]
fn multiple_sibling_stitches_under_one_flow() {
    let src =
        "flow garden() {\n  flow gate() {\n    Creak.\n  }\n  flow shed() {\n    Dusty.\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let stitches: Vec<_> = flow.stitches().collect();
    assert_eq!(stitches.len(), 2);
    assert_eq!(stitches[0].name_token().expect("name").text(), "gate");
    assert_eq!(stitches[1].name_token().expect("name").text(), "shed");
}

#[test]
fn doubly_nested_stitch_flow() {
    // A stitch may itself nest a stitch — `FLOW_DECL` nesting has no
    // charter-imposed depth limit (only the parser's general `MAX_DEPTH`
    // guard, exercised separately by `trivia.rs`'s adversarial-depth test).
    let src = "flow outer() {\n  flow middle() {\n    flow inner() {\n      Deep.\n    }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 3);
}

// ── var / const: value accessor, no-initializer prose fallback ────────

#[test]
fn var_decl_value_accessor_returns_the_initializer_node() {
    let p = assert_lossless("var hp = 10\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let v: ast::VarDecl = find_child(&p.syntax()).expect("var decl");
    assert_eq!(v.name_token().expect("name").text(), "hp");
    let value = v.value().expect("initializer");
    assert_eq!(value.kind(), SyntaxKind::INTEGER_LIT);
}

#[test]
fn const_decl_value_accessor_returns_the_initializer_node() {
    let p = assert_lossless("const MAX = 100\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let c: ast::ConstDecl = find_child(&p.syntax()).expect("const decl");
    assert_eq!(c.name_token().expect("name").text(), "MAX");
    let value = c.value().expect("initializer");
    assert_eq!(value.kind(), SyntaxKind::INTEGER_LIT);
}

#[test]
fn var_without_initializer_shape_is_prose_not_a_decl() {
    // `at_binding_decl` requires `IDENT EQ` after the keyword — bare `var
    // hp` (no `=`) fails that lookahead and falls through to ordinary
    // prose (Finding #5's disambiguation pattern, exercised here for
    // `var`/`const` specifically rather than `flow`, which `trivia.rs`
    // already covers).
    let src = "var hp on the wall.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::VAR_DECL));
}

#[test]
fn const_without_initializer_shape_is_prose_not_a_decl() {
    let src = "const answers are hard to find.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONST_DECL));
}

// ── flags: active-marker accessor, dangling/malformed shapes ──────────

#[test]
fn flags_member_is_active_distinguishes_parenthesized_members() {
    let p = assert_lossless("flags Mood = (calm), wary, (hostile)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::FlagsDecl = find_child(&p.syntax()).expect("flags decl");
    let members: Vec<_> = decl.member_list().expect("member list").members().collect();
    assert_eq!(members.len(), 3);
    let shape: Vec<(String, bool)> = members
        .iter()
        .map(|m| {
            (
                m.name_token().expect("name").text().to_string(),
                m.is_active(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            ("calm".to_string(), true),
            ("wary".to_string(), false),
            ("hostile".to_string(), true),
        ]
    );
}

#[test]
fn flags_member_all_bare_no_active_markers() {
    let p = assert_lossless("flags Colors = red, green, blue\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::FlagsDecl = find_child(&p.syntax()).expect("flags decl");
    let members: Vec<_> = decl.member_list().expect("member list").members().collect();
    assert_eq!(members.len(), 3);
    assert!(members.iter().all(|m| !m.is_active()));
}

#[test]
fn flags_member_all_parenthesized() {
    let p = assert_lossless("flags Colors = (red), (green), (blue)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::FlagsDecl = find_child(&p.syntax()).expect("flags decl");
    let members: Vec<_> = decl.member_list().expect("member list").members().collect();
    assert!(members.iter().all(ast::FlagsMember::is_active));
}

#[test]
fn flags_decl_single_member() {
    let p = assert_lossless("flags Solo = only\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::FlagsDecl = find_child(&p.syntax()).expect("flags decl");
    assert_eq!(
        decl.member_list().expect("member list").members().count(),
        1
    );
}

#[test]
fn flags_decl_dangling_no_members_after_eq_parses_with_empty_member_list() {
    // Ruled #1260 (LIST parity) for #1262: a bare `flags F =` with nothing
    // member-shaped following is now a parse error, like every sibling
    // zero-progress recovery path in this file (`param_list`, the
    // `struct_decl` body loop). `flags_member_list` still recovers — the
    // decl and an empty `FLAGS_MEMBER_LIST` are still produced — but a
    // diagnostic is now emitted instead of the old silent `break`. Use
    // `flags F = ()` (below) to spell an explicit empty set without an
    // error.
    let src = "flags F =\n";
    let p = assert_lossless(src);
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("flags member")),
        "expected a 'flags member' error, got: {:?}",
        p.errors()
    );
    let decl: ast::FlagsDecl = find_child(&p.syntax()).expect("flags decl still parses");
    assert_eq!(
        decl.member_list()
            .expect("member list node still present")
            .members()
            .count(),
        0
    );
}

#[test]
fn flags_decl_explicit_empty_parens_is_the_empty_set_no_error() {
    // Ruled #1260: `flags F = ()` is the explicit empty-set spelling (LIST
    // parity) — it must parse clean, with an empty `FLAGS_MEMBER_LIST` and
    // no diagnostic, distinct from the bare-`=` error case above.
    let src = "flags F = ()\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::FlagsDecl = find_child(&p.syntax()).expect("flags decl");
    assert_eq!(
        decl.member_list()
            .expect("member list node present")
            .members()
            .count(),
        0
    );
}

#[test]
fn flags_without_eq_shape_is_prose_not_a_decl() {
    // `at_flags_decl` requires `IDENT EQ` after `flags` — without the `=`
    // it falls through to prose, same disambiguation as `var`/`const`.
    let src = "flags are usually red or white.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::FLAGS_DECL));
}

// ── struct: field shapes, generic-type gap, malformed bodies ──────────

#[test]
fn struct_decl_single_field() {
    let p = assert_lossless("struct Wrapper {\n  value: int\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::StructDecl = find_child(&p.syntax()).expect("struct decl");
    let fields: Vec<_> = decl.fields().collect();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name_token().expect("name").text(), "value");
    let ty = fields[0].type_path().expect("type path");
    assert_eq!(
        ty.segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["int"]
    );
}

#[test]
fn struct_decl_multiple_fields() {
    let p = assert_lossless("struct Item {\n  name: string,\n  weight: int\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::StructDecl = find_child(&p.syntax()).expect("struct decl");
    let names: Vec<_> = decl
        .fields()
        .map(|f| f.name_token().expect("name").text().to_string())
        .collect();
    assert_eq!(names, vec!["name", "weight"]);
}

#[test]
fn struct_decl_empty_body() {
    let p = assert_lossless("struct Empty {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::StructDecl = find_child(&p.syntax()).expect("struct decl");
    assert_eq!(decl.fields().count(), 0);
}

#[test]
fn struct_field_dotted_type_path() {
    // `path()` accepts `.`/`::`-joined segments — a struct field's type may
    // be a module-qualified path, not just a bare `IDENT`.
    let p = assert_lossless("struct Wrapper {\n  loc: geo::Point\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::StructDecl = find_child(&p.syntax()).expect("struct decl");
    let field = decl.fields().next().expect("field");
    let segs: Vec<_> = field
        .type_path()
        .expect("type path")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["geo", "Point"]);
}

#[test]
fn struct_decl_missing_colon_recovers() {
    // Missing `:` between field name and type: `struct_field` records an
    // error but still parses the following token as the type path (no
    // `error_recover`/zero-progress path here since `expect(IDENT)` for
    // the name already made forward progress) — documents actual recovery
    // shape, not asserting it's the "right" shape.
    let src = "struct S {\n  name int\n}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty(), "expected a missing-colon error");
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STRUCT_DECL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STRUCT_FIELD));
}

#[test]
fn struct_decl_unexpected_token_in_body_recovers() {
    // A digit where a field name is expected: `struct_field` returns
    // immediately (zero progress) on a non-`IDENT` token, so the body loop's
    // `error_recover` wraps it as an `ERROR` node and keeps scanning.
    let src = "struct S {\n  1\n  ok: int\n}\n";
    let p = assert_lossless(src);
    assert!(
        p.errors().iter().any(|e| e.message.contains("struct body")),
        "expected 'unexpected token in struct body' error, got: {:?}",
        p.errors()
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
    let decl: ast::StructDecl = find_child(&p.syntax()).expect("struct decl still recovers");
    assert_eq!(
        decl.fields().count(),
        1,
        "the well-formed field after the garbage still parses"
    );
}

#[test]
fn struct_decl_generic_field_type_is_a_documented_gap_not_a_panic() {
    // `struct_field`'s own doc comment already scopes this out: "a bare
    // dotted path in this skeleton grammar (no generics/fn-types)". A
    // generic field type (`array<int>`, the brink-syntax parity target's
    // `struct_decl_generic_field_type` shape) is therefore NOT a supported
    // shape here — this is a documented scope gap, not a bug this
    // TEST-ONLY issue should paper over. Only asserting the resilience
    // property: no panic, lossless round-trip, and at least one recorded
    // error surfacing the gap.
    let src = "struct Bag {\n  items: array<int>\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected the unsupported `<...>` shape to surface at least one error"
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STRUCT_DECL));
}

#[test]
fn struct_without_brace_shape_is_prose_not_a_decl() {
    let src = "struct is just a word in this sentence.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::STRUCT_DECL));
}

// ── extern: params, missing close-paren, keyword-lookahead prose ──────

#[test]
fn extern_decl_no_params() {
    let p = assert_lossless("extern log()\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::ExternDecl = find_child(&p.syntax()).expect("extern decl");
    assert_eq!(decl.name_token().expect("name").text(), "log");
    assert_eq!(decl.param_list().expect("param list").params().count(), 0);
}

#[test]
fn extern_decl_multiple_params() {
    let p = assert_lossless("extern setBrightness(x, y, ref z)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::ExternDecl = find_child(&p.syntax()).expect("extern decl");
    assert_eq!(decl.param_list().expect("param list").params().count(), 3);
}

#[test]
fn extern_decl_bare_name_with_no_parens_is_prose_not_a_decl() {
    // Unlike `flow`/`fn` (whose guard accepts EITHER `(` or `{` as the
    // third token), `at_extern_decl` requires `L_PAREN` specifically — an
    // `extern name` with no `(` at all never satisfies the lookahead and
    // falls through to prose (mirrors the ink precedent: `EXTERNAL` always
    // requires `()`, even for zero params — `brink-syntax`'s
    // `external_no_params` test still writes `EXTERNAL myFunc()\n`).
    // `extern_decl`'s own `if p.at(L_PAREN) { param_list(p) }` guard is
    // therefore only ever taken (this is not dead code with no params at
    // all reachable — `extern log()` still hits it with an empty list).
    let src = "extern log\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::EXTERN_DECL));
}

#[test]
fn extern_without_paren_shape_is_prose_not_a_decl() {
    // `at_extern_decl` requires `IDENT L_PAREN` — without the paren it's
    // prose, same disambiguation pattern.
    let src = "extern circumstances prevented it.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::EXTERN_DECL));
}

// ── param-list / param error recovery (shared by flow/fn/extern) ──────

#[test]
fn missing_param_close_paren_recovers() {
    // No `)` before the body's `{` — `param_list` breaks its inner loop
    // (no comma follows `b`), `p.expect(R_PAREN)` records an error without
    // consuming, and `flow_decl` still finds `L_BRACE` next and parses the
    // body normally.
    let src = "flow greet(a, b {\n}\n";
    let p = assert_lossless(src);
    assert!(
        p.errors().iter().any(|e| e.message.contains("R_PAREN")),
        "expected an R_PAREN error, got: {:?}",
        p.errors()
    );
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl still recovers");
    assert!(
        flow.body().is_some(),
        "body still parses after the missing `)`"
    );
}

#[test]
fn param_ref_without_following_name_records_error_but_recovers() {
    // `ref` consumed, then `expect(IDENT)` fails — `param` still makes
    // forward progress (the `ref` token), so the outer param-list loop
    // does NOT treat this as zero-progress; it just carries the recorded
    // error forward and keeps parsing normally.
    let src = "flow f(ref) {\n}\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected a missing-param-name error"
    );
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl still recovers");
    assert!(flow.body().is_some());
}

#[test]
fn unexpected_token_in_param_list_recovers() {
    // A leading comma with nothing before it: `param` makes zero progress
    // (its `IDENT` expectation fails immediately), so the outer loop's
    // `error_recover` wraps the comma itself in an `ERROR` node and the
    // list still closes on `)`.
    //
    // But `param` calls `p.start_node(PARAM)` *before* checking `IDENT`
    // (unlike `flags_member`/`struct_field`, which return before
    // `start_node` on a bad lookahead) — so the zero-progress attempt
    // still leaves behind a real, zero-width, nameless `PARAM` sibling
    // ahead of the well-formed one and the `ERROR`-wrapped comma. That
    // reaches the typed AST as a nameless `Param` `hir::lower_native`
    // would iterate over; filtering it out of `params()` here (as a prior
    // version of this test did) would hide the artifact instead of
    // asserting it, so both params — and their exact text — are asserted
    // explicitly below. #1192 gap: same class as
    // `use_tree_list_unexpected_token_recovers`'s empty-`USE_TREE`
    // artifact.
    let src = "flow f(, a) {\n}\n";
    let p = assert_lossless(src);
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("parameter list")),
        "expected 'unexpected token in parameter list' error, got: {:?}",
        p.errors()
    );
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl still recovers");
    let params: Vec<_> = flow.param_list().expect("param list").params().collect();
    assert_eq!(
        params.len(),
        2,
        "the zero-width nameless PARAM left by the garbage comma is a real \
         sibling of the well-formed param, not absorbed by it: {:?}",
        params
            .iter()
            .map(|p| p.syntax().text().to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        params[0].name_token().is_none(),
        "the garbage comma produced a nameless PARAM"
    );
    assert_eq!(
        params[1]
            .name_token()
            .expect("second param has a name")
            .text(),
        "a"
    );
    assert_eq!(
        params
            .iter()
            .map(|p| p.syntax().text().to_string())
            .collect::<Vec<_>>(),
        vec![String::new(), " a".to_string()],
        "the nameless PARAM is zero-width; the well-formed one still carries its leading space"
    );
}

// ── import: path shapes, doc attachment ────────────────────────────────

#[test]
fn import_decl_single_segment_path() {
    let p = assert_lossless("import items\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::ImportDecl = find_child(&p.syntax()).expect("import decl");
    let segs: Vec<_> = decl
        .path()
        .expect("path")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(segs, vec!["items"]);
}

#[test]
fn import_decl_dotted_path() {
    let p = assert_lossless("import story::items.detail\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::ImportDecl = find_child(&p.syntax()).expect("import decl");
    let path = decl.path().expect("path");
    assert!(path.crosses_module_wall());
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["story", "items", "detail"]);
}

#[test]
fn leading_doc_comment_attaches_to_import_decl() {
    let src = "/// what this pulls in\nimport story::items\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::ImportDecl = find_child(&p.syntax()).expect("import decl");
    assert!(decl.doc().is_some());
}

// ── use: bare / aliased / nested-group forms, malformed trees ─────────

#[test]
fn use_decl_bare_path_no_alias_no_group() {
    let p = assert_lossless("use story::npcs::guard;\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::UseDecl = find_child(&p.syntax()).expect("use decl");
    let tree = decl.tree().expect("use tree");
    let segs: Vec<_> = tree.path_segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["story", "npcs", "guard"]);
    assert!(tree.alias_token().is_none());
    assert!(tree.nested_list().is_none());
}

#[test]
fn use_decl_aliased_form() {
    let p = assert_lossless("use story::npcs::merchant as trader;\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::UseDecl = find_child(&p.syntax()).expect("use decl");
    let tree = decl.tree().expect("use tree");
    assert_eq!(tree.alias_token().expect("alias").text(), "trader");
}

#[test]
fn use_decl_nested_group_form() {
    let p = assert_lossless("use story::npcs::{guard, merchant as trader};\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::UseDecl = find_child(&p.syntax()).expect("use decl");
    let tree = decl.tree().expect("use tree");
    let group = tree.nested_list().expect("nested group");
    let members: Vec<_> = group.trees().collect();
    assert_eq!(members.len(), 2);
    assert_eq!(
        members[0]
            .path_segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["guard"]
    );
    assert_eq!(members[1].alias_token().expect("alias").text(), "trader");
}

#[test]
fn use_decl_nested_group_of_groups() {
    // A group member may itself be a nested group — `use_tree` recurses
    // into `use_tree_list` whenever `::` is immediately followed by `{`,
    // regardless of depth.
    let p = assert_lossless("use a::{b::{c, d}, e};\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::USE_TREE_LIST), 2);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::USE_TREE), 5);
}

#[test]
fn use_decl_bare_group_with_no_leading_path_is_a_parse_error() {
    // Ruled 2026-07-22 (#1275, closing #1256's last rulings-needed item):
    // a `use` with no leading path — a bare group like `use {a, b};` — is
    // a parse error, not a valid import; a `use`-tree group is always the
    // *tail* of a path, never the whole tree. #1277 pruned `use_tree`'s
    // dead bare-group branch (see the doc comment on its `else` arm) but
    // deliberately did NOT widen `at_use_decl` to route `L_BRACE` into
    // `USE_DECL` — that was explicitly rejected, since a bare group has no
    // module to select from. So this exact top-level shape still never
    // satisfies the `USE_DECL` lookahead (`at_use_decl` only accepts
    // `IDENT`/`::` after `use`): `use` is bumped bare as leftover prose,
    // and the `{a, b}` that follows misparses as bare-brace `{expr}`
    // interpolation (`a, b` isn't a valid expression, hence the errors
    // below). This is no longer an open TODO — it's the ruled, intentional
    // outcome for this shape; see `use_tree_list_nested_bare_group_with_no_leading_path_errors`
    // below for the shape the prune actually reaches and cleanly errors.
    let src = "use {a, b};\n";
    let p = assert_lossless(src);
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::USE_DECL));
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("expected R_BRACE")),
        "documents the misparse-as-interpolation fallout, got: {:?}",
        p.errors()
    );
}

#[test]
fn use_tree_list_nested_bare_group_with_no_leading_path_errors() {
    // The bare-group `use_tree` branch #1277 pruned was dead at the top
    // level (see the test above) but genuinely LIVE for nested list
    // entries: `use_tree_list`'s loop calls `use_tree` for each member
    // without first checking for `IDENT`, so a nested bare group like the
    // `{b, c}` here used to hit the (now-removed) `p.at(L_BRACE) {
    // use_tree_list(p) }` branch and parse with ZERO errors — silently
    // accepting a group with no module to select `b`/`c` from. It now
    // falls through to the shared `else` arm and reports the clear
    // diagnostic instead.
    let src = "use a::{ {b, c} };\n";
    let p = assert_lossless(src);
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("needs a module path")),
        "expected the use-tree error, got: {:?}",
        p.errors()
    );
}

#[test]
fn use_tree_malformed_missing_path_recovers() {
    // `use ::foo;` — `at_use_decl`'s weaker two-token check (Finding #5's
    // documented residual risk: `nth(1)` is `COLON_COLON`, which passes the
    // guard) commits to `USE_DECL`, but `use_tree` itself sees no `IDENT`
    // first and records "a `use` needs a module path" without consuming
    // anything. `USE_DECL` still closes (with just `use` as its content);
    // the leftover `::foo;` falls through to the next `item()` call as
    // ordinary prose on its own line — it is NOT silently absorbed into
    // the `USE_DECL`.
    let src = "use ::foo;\n";
    let p = assert_lossless(src);
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("needs a module path")),
        "expected the use-tree error, got: {:?}",
        p.errors()
    );
    let decl: ast::UseDecl = find_child(&p.syntax()).expect("use decl still recovers");
    assert!(
        decl.tree().is_some(),
        "USE_TREE node still exists, just empty"
    );
    assert_eq!(
        decl.tree().expect("checked above").path_segments().count(),
        0
    );
    // The leftover `::foo;` is not swallowed into USE_DECL's text.
    assert!(!decl.syntax().text().to_string().contains("foo"));
}

#[test]
fn use_tree_list_unexpected_token_recovers() {
    // Two garbage tokens (`1`, then the stray `,` it leaves behind) inside
    // a use-tree group, followed by a well-formed member — each garbage
    // token is wrapped in its own `ERROR` node by `use_tree_list`'s
    // zero-progress `error_recover`, and the well-formed `b` after them
    // still parses as a real `USE_TREE`.
    //
    // But `use_tree` calls `p.start_node(USE_TREE)` before checking
    // whether anything path-shaped follows, so each garbage token also
    // leaves behind an empty `USE_TREE` sibling — the nested group ends up
    // with 3 `USE_TREE` children (2 empty + `b`), for 4 total in the whole
    // decl once the outer `a::{...}` tree is counted. A `flat_map` straight
    // to path segments (as a prior version of this test did) would hide
    // that shape instead of asserting it — filtered results are `["b"]`
    // either way, so the test couldn't fail if the extra empty nodes
    // vanished OR multiplied. Asserted explicitly below. #1192 gap: same
    // class as `unexpected_token_in_param_list_recovers`'s nameless
    // `PARAM` artifact.
    let src = "use a::{1, b};\n";
    let p = assert_lossless(src);
    assert!(
        p.errors().len() >= 2,
        "expected at least 2 recovery errors, got: {:?}",
        p.errors()
    );
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::USE_TREE),
        4,
        "outer a::{{...}} tree + 2 empty garbage-token artifacts + the real `b`"
    );
    let decl: ast::UseDecl = find_child(&p.syntax()).expect("use decl still recovers");
    let group = decl
        .tree()
        .expect("use tree")
        .nested_list()
        .expect("nested group");
    let members: Vec<_> = group.trees().collect();
    assert_eq!(
        members.len(),
        3,
        "2 empty USE_TREE artifacts from the garbage `1` and `,`, plus the real `b`"
    );
    let names: Vec<_> = members
        .iter()
        .flat_map(|t| {
            t.path_segments()
                .map(|tok| tok.text().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(names, vec!["b"]);
}

#[test]
fn use_decl_optional_semicolon_both_present_and_absent_round_trip() {
    // Exercises both branches of `use_decl`'s `p.eat(SEMICOLON)` in one
    // test — complements `use_decl_semicolon_is_consumed_by_the_decl_not_left_as_prose`
    // and `use_decl_without_semicolon_still_parses` above with an explicit
    // side-by-side on the same member shape.
    for src in ["use a::b;\n", "use a::b\n"] {
        let p = assert_lossless(src);
        assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
        assert!(has_node_kind(&p.syntax(), SyntaxKind::USE_DECL));
    }
}

#[test]
fn leading_doc_comment_attaches_to_use_decl() {
    let src = "/// bring these into scope\nuse a::b;\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::UseDecl = find_child(&p.syntax()).expect("use decl");
    assert!(decl.doc().is_some());
}

// ── module: nested body, doc attachment, malformed shape ──────────────

#[test]
fn module_decl_basic() {
    let p = assert_lossless("module inner {\n  var secret = 1\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::ModuleDecl = find_child(&p.syntax()).expect("module decl");
    assert_eq!(decl.name_token().expect("name").text(), "inner");
    let body = decl.body().expect("module body");
    assert!(has_node_kind(body.syntax(), SyntaxKind::VAR_DECL));
}

#[test]
fn module_decl_nested_module() {
    let p = assert_lossless("module outer {\n  module inner {\n    var x = 1\n  }\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::MODULE_DECL), 2);
}

#[test]
fn leading_doc_comment_attaches_to_module_decl() {
    let src = "/// groups the secret stuff\nmodule inner {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl: ast::ModuleDecl = find_child(&p.syntax()).expect("module decl");
    assert!(decl.doc().is_some());
}

#[test]
fn module_without_brace_shape_is_prose_not_a_decl() {
    let src = "module citizens gathered in the square.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::MODULE_DECL));
}

#[test]
fn module_decl_missing_closing_brace_recovers_via_block_eof() {
    // NOT `module_decl`'s own `else` branch (that name was wrong — no
    // brace is "missing" here, the CLOSING one is): `at_module_decl`
    // requires `L_BRACE` at position 2 before `module_decl` is ever
    // called, so by the time we're inside it the opening `{` is always
    // already there. That makes `module_decl`'s own
    // "expected a braced body after the module header" `else` arm
    // unreachable dead code (same class of reachability observation as
    // the inline note on `extern_decl_bare_name_with_no_parens_is_prose_not_a_decl`
    // above, and reported as a #1192 gap alongside it) — confirmed by
    // `module_without_brace_shape_is_prose_not_a_decl` just below, which
    // shows a bodyless `module` header falls through to prose instead of
    // ever reaching `module_decl` at all.
    //
    // What this test actually documents: an unterminated body.
    // `super::block::block(p)` opens its `BLOCK` node, the item loop runs
    // out of input before finding `R_BRACE`, and `p.expect(R_BRACE)`
    // records exactly one error — "expected R_BRACE, found EOF" — then
    // the `BLOCK` node still closes best-effort so `module_decl` still
    // finishes.
    let src = "module inner {\n  var x = 1\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1, "errors: {:?}", p.errors());
    assert!(
        p.errors()[0].message.contains("R_BRACE") && p.errors()[0].message.contains("EOF"),
        "expected the block's missing-R_BRACE-at-EOF error, got: {:?}",
        p.errors()
    );
    let decl: ast::ModuleDecl = find_child(&p.syntax()).expect("module decl still recovers");
    assert!(
        decl.body().is_some(),
        "the unterminated body still parses as BLOCK"
    );
}

// ── Adversarial: property-based coverage scoped to declarations ───────
//
// Mirrors `tests/proptest_native.rs`'s generator style (studied from that
// file) but scoped to the declaration shapes this issue owns — per the
// wave instructions, a proptest generator for #1192 lives in this family
// file, not in the shared `proptest_native.rs` integration test (owned by
// #1199 this wave).

mod prop {
    use proptest::prelude::*;

    const NUM_CASES: u32 = 256;

    const KEYWORDS: &[&str] = &[
        "flow", "fn", "var", "const", "flags", "struct", "extern", "import", "use", "module",
        "return", "ref", "if", "match", "else", "as", "true", "false", "END", "DONE",
    ];

    fn arb_ident() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,7}"
            .prop_filter("must not be a keyword", |s| !KEYWORDS.contains(&s.as_str()))
    }

    fn arb_type_ident() -> impl Strategy<Value = String> {
        "[A-Z][A-Za-z0-9_]{0,7}"
    }

    fn arb_const_decl() -> impl Strategy<Value = String> {
        (arb_type_ident(), 0..10_000u32).prop_map(|(name, n)| format!("const {name} = {n}\n"))
    }

    fn arb_flags_member() -> impl Strategy<Value = String> {
        (arb_ident(), prop::bool::ANY)
            .prop_map(|(name, active)| if active { format!("({name})") } else { name })
    }

    fn arb_flags_decl() -> impl Strategy<Value = String> {
        (
            arb_type_ident(),
            prop::collection::vec(arb_flags_member(), 1..=5),
        )
            .prop_map(|(name, members)| format!("flags {name} = {}\n", members.join(", ")))
    }

    fn arb_struct_field() -> impl Strategy<Value = String> {
        (arb_ident(), arb_ident()).prop_map(|(name, ty)| format!("  {name}: {ty}"))
    }

    fn arb_struct_decl() -> impl Strategy<Value = String> {
        (
            arb_type_ident(),
            prop::collection::vec(arb_struct_field(), 0..=4),
        )
            .prop_map(|(name, fields)| format!("struct {name} {{\n{}\n}}\n", fields.join(",\n")))
    }

    fn arb_param() -> impl Strategy<Value = String> {
        (prop::bool::ANY, arb_ident())
            .prop_map(|(is_ref, name)| if is_ref { format!("ref {name}") } else { name })
    }

    // `flow`/`fn`/`PARAM_LIST` are the first node kinds #1192 names — the
    // generators above (const/flags/struct/extern/import/use/module)
    // omitted them entirely. `arb_param` already covers the shared
    // `PARAM_LIST`/`PARAM` shape; these two round it out with the
    // declaration headers that actually own a param list and a body.
    fn arb_flow_decl() -> impl Strategy<Value = String> {
        (arb_ident(), prop::collection::vec(arb_param(), 0..=4))
            .prop_map(|(name, params)| format!("flow {name}({}) {{\n}}\n", params.join(", ")))
    }

    fn arb_fn_decl() -> impl Strategy<Value = String> {
        (arb_ident(), prop::collection::vec(arb_param(), 0..=4))
            .prop_map(|(name, params)| format!("fn {name}({}) {{\n}}\n", params.join(", ")))
    }

    fn arb_extern_decl() -> impl Strategy<Value = String> {
        (arb_ident(), prop::collection::vec(arb_param(), 0..=4))
            .prop_map(|(name, params)| format!("extern {name}({})\n", params.join(", ")))
    }

    fn arb_import_decl() -> impl Strategy<Value = String> {
        prop::collection::vec(arb_ident(), 1..=3)
            .prop_map(|segs| format!("import {}\n", segs.join("::")))
    }

    fn arb_use_tree() -> impl Strategy<Value = String> {
        (
            prop::collection::vec(arb_ident(), 1..=3),
            prop::option::of(arb_ident()),
        )
            .prop_map(|(segs, alias)| {
                let path = segs.join("::");
                match alias {
                    Some(a) => format!("{path} as {a}"),
                    None => path,
                }
            })
    }

    fn arb_use_decl() -> impl Strategy<Value = String> {
        // Always keep a real leading `IDENT` path segment before any
        // nested group — `at_use_decl`'s lookahead only recognizes
        // `IDENT`/`COLON_COLON` as the second token (never a bare
        // `L_BRACE`; see `use_decl_bare_group_with_no_leading_path_is_a_parse_error`
        // above), so a bare-group `use { … };` with no leading path is not
        // a reachable `USE_DECL` shape and must not be generated here.
        (arb_ident(), prop::collection::vec(arb_use_tree(), 1..=3)).prop_map(|(prefix, trees)| {
            if trees.len() == 1 {
                format!("use {prefix}::{};\n", trees[0])
            } else {
                format!("use {prefix}::{{{}}};\n", trees.join(", "))
            }
        })
    }

    fn arb_module_decl() -> impl Strategy<Value = String> {
        (arb_ident(), arb_const_decl())
            .prop_map(|(name, inner)| format!("module {name} {{\n  {inner}}}\n"))
    }

    /// Truncate the source at a byte-safe boundary — simulates an
    /// unterminated declaration body/param-list/use-tree.
    fn truncated(s: &str, cut_ratio: u32) -> String {
        let target = (s.len() as u64 * u64::from(cut_ratio) / 100) as usize;
        let mut end = target.min(s.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }

    fn parse_ok_roundtrip(input: &str) -> bool {
        let parsed = crate::parse(input);
        parsed.syntax().text() == input
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(NUM_CASES))]

        #[test]
        fn const_decl_roundtrip(input in arb_const_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn const_decl_no_errors(input in arb_const_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        #[test]
        fn flags_decl_roundtrip(input in arb_flags_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn flags_decl_no_errors(input in arb_flags_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        #[test]
        fn struct_decl_roundtrip(input in arb_struct_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn struct_decl_no_errors(input in arb_struct_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        #[test]
        fn flow_decl_roundtrip(input in arb_flow_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn flow_decl_no_errors(input in arb_flow_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        #[test]
        fn fn_decl_roundtrip(input in arb_fn_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn fn_decl_no_errors(input in arb_fn_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        #[test]
        fn extern_decl_roundtrip(input in arb_extern_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn extern_decl_no_errors(input in arb_extern_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        #[test]
        fn import_decl_roundtrip(input in arb_import_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn use_decl_roundtrip(input in arb_use_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn use_decl_no_errors(input in arb_use_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        #[test]
        fn module_decl_roundtrip(input in arb_module_decl()) {
            prop_assert!(parse_ok_roundtrip(&input));
        }

        #[test]
        fn module_decl_no_errors(input in arb_module_decl()) {
            let parsed = crate::parse(&input);
            prop_assert!(parsed.errors().is_empty(), "input: {input:?} errors: {:?}", parsed.errors());
        }

        // ── Adversarial: truncated declarations never panic, stay lossless ─

        #[test]
        fn truncated_flow_decl_never_panics(input in arb_flow_decl(), cut in 0u32..100) {
            let mutated = truncated(&input, cut);
            let parsed = crate::parse(&mutated);
            prop_assert_eq!(parsed.syntax().text().to_string(), mutated);
        }

        #[test]
        fn truncated_struct_decl_never_panics(input in arb_struct_decl(), cut in 0u32..100) {
            let mutated = truncated(&input, cut);
            let parsed = crate::parse(&mutated);
            prop_assert_eq!(parsed.syntax().text().to_string(), mutated);
        }

        #[test]
        fn truncated_module_decl_never_panics(input in arb_module_decl(), cut in 0u32..100) {
            let mutated = truncated(&input, cut);
            let parsed = crate::parse(&mutated);
            prop_assert_eq!(parsed.syntax().text().to_string(), mutated);
        }

        #[test]
        fn truncated_use_decl_never_panics(input in arb_use_decl(), cut in 0u32..100) {
            let mutated = truncated(&input, cut);
            let parsed = crate::parse(&mutated);
            prop_assert_eq!(parsed.syntax().text().to_string(), mutated);
        }

        #[test]
        fn truncated_extern_decl_never_panics(input in arb_extern_decl(), cut in 0u32..100) {
            let mutated = truncated(&input, cut);
            let parsed = crate::parse(&mutated);
            prop_assert_eq!(parsed.syntax().text().to_string(), mutated);
        }
    }
}
