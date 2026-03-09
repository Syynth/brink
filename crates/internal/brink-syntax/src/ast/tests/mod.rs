mod choice;
mod content;
mod decl;
mod divert;
mod expr;
mod gather;
mod invariants;

use super::*;

// ── Shared helpers ──────────────────────────────────────────────────

/// Parse source and return the typed [`SourceFile`] root.
///
/// Panics if the parse produces errors.
fn parse_tree(src: &str) -> SourceFile {
    let parse = crate::parse(src);
    assert!(
        parse.errors().is_empty(),
        "unexpected parse errors:\n{:#?}\n\ntree:\n{:#?}",
        parse.errors(),
        parse.syntax(),
    );
    parse.tree()
}

/// Find the first descendant of type `N` in a syntax tree.
///
/// Panics with a diagnostic message if no matching node is found.
fn first<N: AstNode>(root: &SyntaxNode) -> N {
    root.descendants()
        .find_map(N::cast)
        .expect("no matching descendant found")
}

/// Parse source and return the first descendant of type `N`.
fn parse_first<N: AstNode>(src: &str) -> N {
    let tree = parse_tree(src);
    first::<N>(tree.syntax())
}

/// Iterate all descendants of type `N`.
#[expect(dead_code, reason = "available for future test use")]
fn descendants<N: AstNode>(root: &SyntaxNode) -> impl Iterator<Item = N> {
    root.descendants().filter_map(N::cast)
}

// ── Parse::tree() ────────────────────────────────────────────────────

#[test]
fn source_file_cast_empty() {
    let parse = crate::parse("");
    let sf = SourceFile::cast(parse.syntax());
    assert!(sf.is_some());
}

#[test]
fn source_file_cast_wrong_kind() {
    let parse = crate::parse("Hello\n");
    let root = parse.syntax();
    let child = root.children().next().unwrap();
    assert!(SourceFile::cast(child).is_none());
}

#[test]
fn parse_tree_roundtrip() {
    let src = "Hello world\n";
    let tree = parse_tree(src);
    assert_eq!(tree.syntax().text().to_string(), src);
}

#[test]
fn parse_tree_with_knot() {
    let tree = parse_tree("=== myKnot ===\nSome content\n");
    assert_eq!(tree.knots().count(), 1);
}

// ── Tags ─────────────────────────────────────────────────────────────

#[test]
fn content_line_tags() {
    let tree = parse_tree("Hello #tag1 #tag2\n");
    let line = tree.content_lines().next().unwrap();
    let tags = line.tags().unwrap();
    let values: Vec<_> = tags.tags().map(|t| t.text()).collect();
    assert_eq!(values, vec!["tag1", "tag2"]);
}

// ── Identifier ───────────────────────────────────────────────────────

#[test]
fn identifier_name() {
    let tree = parse_tree("VAR myVar = 1\n");
    let decl = tree.var_decls().next().unwrap();
    let ident = decl.identifier().unwrap();
    assert_eq!(ident.name().as_deref(), Some("myVar"));
    assert!(ident.ident_token().is_some());
}

// ── Literals ─────────────────────────────────────────────────────────

#[test]
fn integer_lit_value() {
    let lit = parse_first::<IntegerLit>("VAR x = 42\n");
    assert_eq!(lit.value(), Some(42));
}

#[test]
fn float_lit_value() {
    let lit = parse_first::<FloatLit>("VAR x = 2.5\n");
    assert_eq!(lit.value(), Some(2.5));
}

#[test]
fn boolean_lit_true() {
    let lit = parse_first::<BooleanLit>("VAR x = true\n");
    assert_eq!(lit.value(), Some(true));
}

#[test]
fn boolean_lit_false() {
    let lit = parse_first::<BooleanLit>("VAR x = false\n");
    assert_eq!(lit.value(), Some(false));
}

#[test]
fn string_lit_raw_text() {
    let lit = parse_first::<StringLit>("VAR x = \"hello\"\n");
    assert_eq!(lit.raw_text(), "hello");
}
