//! The code-ground statement layer — `let`/assignment/expression
//! statements, the `{ stmt; stmt; tail }` statement-block, the statement
//! dispatcher (B0.8 Wave A, issue #1294), and `if`/`while`/`for`/`until`
//! control flow (B0.8 Wave B, issue #1177 — section H below)
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

// ── H. Control flow: if/else, while, for-in, until (B0.8 Wave B) ────

#[test]
fn if_stmt_no_else() {
    let p = assert_lossless("var x = { if a { 1; } 2 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let if_stmt: ast::IfStmt = find_child(block.syntax()).expect("IF_STMT");
    assert_eq!(
        if_stmt.condition().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
    assert!(if_stmt.body().is_some());
    assert!(if_stmt.else_clause().is_none());
}

#[test]
fn if_else_stmt() {
    let p = assert_lossless("var x = { if a { 1; } else { 2; } 3 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let if_stmt: ast::IfStmt = find_child(block.syntax()).expect("IF_STMT");
    let else_clause = if_stmt.else_clause().expect("ELSE_CLAUSE");
    assert!(else_clause.body().is_some());
    assert!(else_clause.if_stmt().is_none());
}

#[test]
fn else_if_chain_is_a_nested_if_stmt_with_no_extra_stmt_block() {
    let p = assert_lossless("var x = { if a { 1; } else if b { 2; } 3 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let if_stmt: ast::IfStmt = find_child(block.syntax()).expect("IF_STMT");
    let else_clause = if_stmt.else_clause().expect("ELSE_CLAUSE");
    let nested = else_clause.if_stmt().expect("chained IF_STMT");
    assert_eq!(
        nested.condition().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
    assert!(else_clause.body().is_none());
}

#[test]
fn while_stmt_shape() {
    let p = assert_lossless("var x = { while a { b = b + 1; } 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let while_stmt: ast::WhileStmt = find_child(block.syntax()).expect("WHILE_STMT");
    assert_eq!(
        while_stmt.condition().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
    let body = while_stmt.body().expect("body");
    assert!(has_node_kind(body.syntax(), SyntaxKind::ASSIGN_STMT));
}

#[test]
fn for_stmt_shape() {
    let p = assert_lossless("var x = { for item in items { foo(item); } 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let for_stmt: ast::ForStmt = find_child(block.syntax()).expect("FOR_STMT");
    assert_eq!(
        for_stmt.name_token().map(|t| t.text().to_string()),
        Some("item".to_string())
    );
    assert_eq!(
        for_stmt.iterable().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
    let body = for_stmt.body().expect("body");
    assert!(has_node_kind(body.syntax(), SyntaxKind::EXPR_STMT));
}

#[test]
fn until_stmt_shape() {
    let p = assert_lossless("var x = { until door_open; 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let until_stmt: ast::UntilStmt = find_child(block.syntax()).expect("UNTIL_STMT");
    assert_eq!(
        until_stmt.condition().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
}

/// `until` is native's sole condition-park spelling — `await` is not a
/// keyword at all on this surface (decision-log item 4), so it parses as a
/// perfectly ordinary identifier/call.
#[test]
fn await_is_not_a_native_keyword() {
    let p = assert_lossless("var x = { let await = 1; await }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::UNTIL_STMT));
}

/// `if`/`while`/`for`/`until` never produce a value — a control-flow
/// construct as the last item in a block is a statement, not the tail,
/// even with no trailing `;` of its own (`StmtBlock::tail`'s updated
/// exclusion list).
#[test]
fn if_stmt_as_the_last_item_is_not_mistaken_for_a_tail() {
    let p = assert_lossless("var x = { if a { 1; } }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    assert!(block.tail().is_none());
    let items: Vec<_> = block.items().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind(), SyntaxKind::IF_STMT);
}

#[test]
fn control_flow_bodies_nest_and_recurse() {
    let p = assert_lossless(
        "var x = { for i in xs { while a { if b { c = 1; } else { c = 2; } } } 0 }\n",
    );
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::FOR_STMT));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::WHILE_STMT));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::IF_STMT));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ELSE_CLAUSE));
}

#[test]
fn error_if_missing_condition_does_not_panic() {
    let src = "var x = { if { 1; } 0 }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_for_missing_in_does_not_panic() {
    let src = "var x = { for item items { 0; } 0 }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_until_missing_semicolon_does_not_panic() {
    let src = "var x = { until a 0 }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}
