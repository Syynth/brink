//! The code-ground statement layer — `let`/assignment/expression
//! statements, the `{ stmt; stmt; tail }` statement-block, and the
//! statement dispatcher. B0.8 Wave A, issue #1294
//! (`docs/decision-log.md` 2026-07-23 "Code-ground sitting").
//!
//! Entry point: every case below goes through `var name = { … }`, the
//! shortest reachable path from `source_file` to `stmt::stmt_block` — a
//! statement-block is itself an expression (blocks-as-values), so
//! `expr::atom`'s new `L_BRACE` case is what actually wires this layer in
//! (`parser/expr.rs`'s module doc). Mirrors `expression.rs`'s own
//! `var name = <expr>` entry-point convention.

use super::*;

fn stmt_block_of(p: &Parse) -> ast::StmtBlock {
    let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
    let var_decl: ast::VarDecl = find_child(file.syntax()).expect("var decl");
    let value = var_decl.value().expect("initializer node");
    ast::StmtBlock::cast(value).expect("STMT_BLOCK")
}

// ── A. STMT_BLOCK shape ─────────────────────────────────────────────

#[test]
fn empty_block_has_no_tail_and_no_errors() {
    let p = assert_lossless("var x = { }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    assert_eq!(block.items().count(), 0);
    assert!(block.tail().is_none());
}

#[test]
fn block_is_reachable_as_a_call_argument() {
    // A second reachable position, not just `var`/`const` initializers —
    // confirms the new `L_BRACE` atom case is a real expression-grammar
    // citizen, not special-cased to one call site.
    let p = assert_lossless("var x = foo({ 1 })\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STMT_BLOCK));
}

// ── B. LET_STMT ──────────────────────────────────────────────────────

#[test]
fn let_stmt_with_initializer() {
    let p = assert_lossless("var x = { let y = 1; y }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let items: Vec<_> = block.items().collect();
    assert_eq!(items.len(), 2, "LET_STMT + tail");
    assert_eq!(items[0].kind(), SyntaxKind::LET_STMT);
    let let_stmt = ast::LetStmt::cast(items[0].clone()).expect("LET_STMT");
    assert_eq!(
        let_stmt.name_token().map(|t| t.text().to_string()),
        Some("y".to_string())
    );
    assert_eq!(
        let_stmt.value().map(|n| n.kind()),
        Some(SyntaxKind::INTEGER_LIT)
    );
    let tail = block.tail().expect("tail");
    assert_eq!(tail.kind(), SyntaxKind::PATH_EXPR);
}

#[test]
fn let_stmt_without_initializer() {
    let p = assert_lossless("var x = { let y; y }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let let_stmt: ast::LetStmt = find_child(block.syntax()).expect("LET_STMT");
    assert_eq!(
        let_stmt.name_token().map(|t| t.text().to_string()),
        Some("y".to_string())
    );
    assert!(let_stmt.value().is_none());
}

// ── C. ASSIGN_STMT (including RMW field paths) ──────────────────────

#[test]
fn assign_stmt_simple_place() {
    let p = assert_lossless("var x = { y = 1; y }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let assign: ast::AssignStmt = find_child(block.syntax()).expect("ASSIGN_STMT");
    let place = assign.place().expect("place path");
    assert_eq!(
        place
            .segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["y".to_string()]
    );
    assert_eq!(
        assign.value().map(|n| n.kind()),
        Some(SyntaxKind::INTEGER_LIT)
    );
}

#[test]
fn assign_stmt_field_rmw_path() {
    let p = assert_lossless("var x = { player.hp = 10; player }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let assign: ast::AssignStmt = find_child(block.syntax()).expect("ASSIGN_STMT");
    let place = assign.place().expect("place path");
    assert_eq!(
        place
            .segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["player".to_string(), "hp".to_string()]
    );
}

#[test]
fn assign_stmt_rhs_is_a_full_expression() {
    let p = assert_lossless("var x = { y = 1 + 2 * 3; y }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let assign: ast::AssignStmt = find_child(block.syntax()).expect("ASSIGN_STMT");
    assert_eq!(
        assign.value().map(|n| n.kind()),
        Some(SyntaxKind::INFIX_EXPR)
    );
}

/// `a == b;` is a comparison expression statement, never mistaken for
/// assignment — `==` lexes as its own `EQ_EQ` token, distinct from `EQ`
/// (`stmt::at_assignment`'s doc comment).
#[test]
fn double_equals_is_not_an_assignment() {
    let p = assert_lossless("var x = { a == b; 1 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    assert!(!has_node_kind(block.syntax(), SyntaxKind::ASSIGN_STMT));
    let expr_stmt: ast::ExprStmt = find_child(block.syntax()).expect("EXPR_STMT");
    assert_eq!(
        expr_stmt.expr().map(|n| n.kind()),
        Some(SyntaxKind::INFIX_EXPR)
    );
}

// ── D. EXPR_STMT ─────────────────────────────────────────────────────

#[test]
fn expr_stmt_call() {
    let p = assert_lossless("var x = { foo(); 1 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let expr_stmt: ast::ExprStmt = find_child(block.syntax()).expect("EXPR_STMT");
    assert_eq!(
        expr_stmt.expr().map(|n| n.kind()),
        Some(SyntaxKind::CALL_EXPR)
    );
}

// ── E. Blocks-as-values: terminated statement vs. unterminated tail ──

#[test]
fn trailing_semicolon_makes_the_last_expression_a_statement_not_a_tail() {
    let p = assert_lossless("var x = { 1 + 2; }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    assert!(block.tail().is_none());
    let items: Vec<_> = block.items().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind(), SyntaxKind::EXPR_STMT);
}

#[test]
fn missing_trailing_semicolon_makes_the_last_expression_the_tail() {
    let p = assert_lossless("var x = { 1 + 2 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let tail = block.tail().expect("tail");
    assert_eq!(tail.kind(), SyntaxKind::INFIX_EXPR);
    let items: Vec<_> = block.items().collect();
    assert_eq!(items.len(), 1, "the tail is the block's only child here");
}

#[test]
fn single_bare_ident_tail() {
    let p = assert_lossless("var x = { y }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    assert_eq!(block.tail().map(|n| n.kind()), Some(SyntaxKind::PATH_EXPR));
}

// ── F. The dispatcher, combined ──────────────────────────────────────

#[test]
fn dispatcher_let_then_assign_then_expr_stmt_then_tail() {
    let p = assert_lossless("var x = { let a = 1; a = 2; foo(a); a }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let items: Vec<_> = block.items().collect();
    let kinds: Vec<_> = items.iter().map(rowan::SyntaxNode::kind).collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::LET_STMT,
            SyntaxKind::ASSIGN_STMT,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::PATH_EXPR,
        ]
    );
    let tail = block.tail().expect("tail");
    assert_eq!(tail.kind(), SyntaxKind::PATH_EXPR);
}

#[test]
fn nested_statement_block_as_a_let_initializer() {
    let p = assert_lossless("var x = { let a = { let b = 1; b }; a }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::STMT_BLOCK), 2);
    let block = stmt_block_of(&p);
    let let_stmt: ast::LetStmt = find_child(block.syntax()).expect("LET_STMT");
    assert_eq!(
        let_stmt.value().map(|n| n.kind()),
        Some(SyntaxKind::STMT_BLOCK)
    );
}

// ── G. Error recovery ────────────────────────────────────────────────

#[test]
fn error_let_missing_name_does_not_panic() {
    let src = "var x = { let = 1; 1 }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_assign_missing_rhs_does_not_panic() {
    let src = "var x = { a = ; 1 }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_garbage_token_inside_block_recovers() {
    let src = "var x = { @ 1 }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
}

#[test]
fn error_missing_semicolon_after_let_still_finds_a_tail() {
    let src = "var x = { let a = 1 a }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
    let block = stmt_block_of(&p);
    assert!(block.tail().is_some());
}

#[test]
fn error_unclosed_block_at_eof_does_not_panic() {
    let src = "var x = { let a = 1;";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn adversarial_deeply_nested_blocks_does_not_panic() {
    // 300 > MAX_DEPTH (256) — each `{` nesting level costs two depth units
    // (one from `expr::expression_bp`, one from `stmt::stmt_block`'s own
    // loop guard), so this comfortably exceeds the cap. Must not blow the
    // stack (CLAUDE.md "guard against unbounded growth") — reaching the
    // final assertion at all proves that.
    let src = format!("var x = {}1{}\n", "{".repeat(300), "}".repeat(300));
    let p = parse(&src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected a max-nesting-depth error, not silent success or a panic"
    );
}

#[test]
fn adversarial_many_statements_does_not_panic() {
    use std::fmt::Write as _;

    let mut src = "var x = { ".to_string();
    for i in 0..500 {
        let _ = writeln!(src, "let v{i} = {i};");
    }
    src.push_str("0 }\n");
    let p = parse(&src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    assert_eq!(block.items().count(), 501, "500 LET_STMTs + the `0` tail");
    assert_eq!(
        block.tail().map(|n| n.kind()),
        Some(SyntaxKind::INTEGER_LIT)
    );
}
