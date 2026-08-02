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

#[test]
fn let_stmt_with_type_annotation_and_initializer() {
    // NG-B (issue #1488): the `: type` clause sits between the name and the
    // `=`, the same slot `var`/`const` use.
    let p = assert_lossless("var x = { let y: int = 1; y }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let let_stmt: ast::LetStmt = find_child(block.syntax()).expect("LET_STMT");
    let annotation = let_stmt.type_annotation().expect("`: int` annotation");
    let te = annotation.type_expr().expect("type expr");
    let Some(ast::TypeExprKind::Name(n)) = te.kind() else {
        unreachable!("expected a nominal type, tree: {:#?}", te.syntax())
    };
    assert_eq!(n.name(), Some("int".to_string()));
    // The annotation must not be picked up as the initializer.
    assert_eq!(
        let_stmt.value().map(|n| n.kind()),
        Some(SyntaxKind::INTEGER_LIT)
    );
}

#[test]
fn let_stmt_with_type_annotation_and_no_initializer() {
    let p = assert_lossless("var x = { let y: string; y }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let let_stmt: ast::LetStmt = find_child(block.syntax()).expect("LET_STMT");
    assert!(let_stmt.type_annotation().is_some());
    assert!(
        let_stmt.value().is_none(),
        "the annotation is not an initializer"
    );
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

/// `for k, v in m` — two-binding map iteration (B2, issue #1461,
/// docs/stdlib-spec.md §5/§9's F10 ruling). `val_name_token` is the second
/// direct `IDENT`; `name_token` (the key binding) is unaffected.
#[test]
fn for_stmt_two_binding_shape() {
    let p = assert_lossless("var x = { for k, v in m { foo(k, v); } 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let for_stmt: ast::ForStmt = find_child(block.syntax()).expect("FOR_STMT");
    assert_eq!(
        for_stmt.name_token().map(|t| t.text().to_string()),
        Some("k".to_string())
    );
    assert_eq!(
        for_stmt.val_name_token().map(|t| t.text().to_string()),
        Some("v".to_string())
    );
    assert_eq!(
        for_stmt.iterable().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
}

/// The single-binding form still has no second binding — `val_name_token`
/// stays `None`, not accidentally picking up an `IDENT` from inside the
/// iterable expression (which is a nested node, not a direct `FOR_STMT`
/// token).
#[test]
fn for_stmt_single_binding_has_no_val_name() {
    let p = assert_lossless("var x = { for item in items { foo(item); } 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let for_stmt: ast::ForStmt = find_child(block.syntax()).expect("FOR_STMT");
    assert!(for_stmt.val_name_token().is_none());
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

/// `> text` (charter §8.2, issue #1992) never produces a value either —
/// review finding F2. Before this, a `PROSE_LINE` as the last item in a
/// `STMT_BLOCK` was missing from `StmtBlock::tail`'s exclusion list, so it
/// was mistaken for the block's blocks-as-values tail expression exactly
/// like an `IF_STMT` used to be (the sibling case just above).
#[test]
fn prose_line_as_the_last_item_is_not_mistaken_for_a_tail() {
    let p = assert_lossless("var x = { > hi }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    assert!(block.tail().is_none());
    let items: Vec<_> = block.items().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind(), SyntaxKind::PROSE_LINE);
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

// ── I. The `as` binding (B1b, issue #1475) ──────────────────────────
//
// One construct, both condition positions — the template half lives in
// `brace_family.rs` (`{if EXPR as NAME: … else: …}`), the statement half
// here. `parser/binding.rs` is the shared rule.

#[test]
fn if_stmt_as_binding_is_a_sibling_of_the_condition() {
    let p = assert_lossless("var x = { if find(s, \"a\") as i { i; } 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let if_stmt: ast::IfStmt = find_child(block.syntax()).expect("IF_STMT");
    // The binding must NOT displace the condition accessor — it is a
    // trailing sibling node, and `condition()` skips it by kind.
    assert_eq!(
        if_stmt.condition().map(|n| n.kind()),
        Some(SyntaxKind::CALL_EXPR)
    );
    let binding = if_stmt.as_binding().expect("AS_BINDING");
    assert_eq!(
        binding.name_token().map(|t| t.text().to_string()),
        Some("i".to_string())
    );
    assert!(if_stmt.body().is_some());
}

#[test]
fn if_stmt_without_as_has_no_binding() {
    let p = assert_lossless("var x = { if a { 1; } 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let if_stmt: ast::IfStmt = find_child(block.syntax()).expect("IF_STMT");
    assert!(if_stmt.as_binding().is_none());
}

#[test]
fn while_stmt_as_binding_parses() {
    let p = assert_lossless("var x = { while pop(q) as item { consume(item); } 0 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let block = stmt_block_of(&p);
    let while_stmt: ast::WhileStmt = find_child(block.syntax()).expect("WHILE_STMT");
    assert_eq!(
        while_stmt.condition().map(|n| n.kind()),
        Some(SyntaxKind::CALL_EXPR)
    );
    assert_eq!(
        while_stmt
            .as_binding()
            .and_then(|b| b.name_token())
            .map(|t| t.text().to_string()),
        Some("item".to_string())
    );
}

/// The v1 whole-condition restriction, parser half: an operator directly
/// after the binding is refused by name, not with a generic
/// `expected L_BRACE`. (The mirror spelling — a binding over a `&&`
/// composition — parses fine here and is `brink-ir`'s `E145`.)
#[test]
fn as_binding_followed_by_an_operator_is_a_named_parse_error() {
    for src in [
        "var x = { if find(s, \"a\") as i && i > 0 { 1; } 0 }\n",
        "var x = { if find(s, \"a\") as i || true { 1; } 0 }\n",
        "var x = { if find(s, \"a\") as i or 3 { 1; } 0 }\n",
    ] {
        let p = parse(src);
        assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
        assert!(
            p.errors()
                .iter()
                .any(|e| e.message.contains("must be the entire condition")),
            "expected the whole-condition error for {src:?}, got: {:?}",
            p.errors()
        );
    }
}

#[test]
fn error_as_with_no_name_does_not_panic() {
    let src = "var x = { if a as { 1; } 0 }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

// ── J. The content-ground line escape: `~ stmt` (charter §8.2, RULED ─
// ── 2026-07-23, issue #1991) ──────────────────────────────────────────
//
// Ink's logic line, kept: `~ stmt` runs code inside an otherwise
// content-ground (prose) `flow`/`fn` body. Before this landed, a leading
// `~` on a content line was not recognized by `block::body_line`'s
// dispatch at all, so it fell through to the prose fallback and was
// swallowed into an ordinary `TEXT` run — compiling clean, with the `~`
// and the statement text both printed verbatim as story prose, and the
// statement itself never executed. These tests pin the fix at the CST
// level; `tests/tier1-native/logic-line-escape/` pins it end-to-end
// through a real compile+run, and `hir::lower_native::tests` pins the HIR
// lowering.

#[test]
fn logic_line_assignment_is_not_swallowed_as_prose() {
    let src = "flow greet() {\n~ n = 5\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = expect_prose_body(flow.body());
    // The whole point of the fix: no `TEXT` node anywhere in the body
    // (the pre-fix bug folded `~ n = 5` into exactly that).
    assert!(
        !has_node_kind(body.syntax(), SyntaxKind::TEXT),
        "the logic line must not be swallowed into a TEXT run"
    );
    let logic_line: ast::LogicLine = find_child(body.syntax()).expect("LOGIC_LINE");
    let assign = logic_line.assign_stmt().expect("ASSIGN_STMT child");
    assert!(logic_line.expr_stmt().is_none());
    let place: ast::Path = find_child(assign.syntax()).expect("place path");
    assert_eq!(
        place
            .segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["n".to_string()]
    );
    assert_eq!(
        assign.value().map(|n| n.kind()),
        Some(SyntaxKind::INTEGER_LIT)
    );
    assert!(assign.op_token().is_none_or(|t| t.kind() == SyntaxKind::EQ));
}

#[test]
fn logic_line_temp_decl_is_not_swallowed_as_prose() {
    // Issue #1972: the emitter-only `Assignment`/`ExprStmt` gap #1991
    // closed left `TempDecl` as the one bucket of the three named in the
    // corpus sweep still missing native grammar entirely — before this,
    // `~ let n = 5` had no `KW_LET` dispatch here at all and reached
    // `expr_stmt_line`'s `expr::expression`, which diagnoses `let` as an
    // unrecognized atom (same as `~ if`) rather than parsing it.
    let src = "flow greet() {\n~ let n = 5\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = expect_prose_body(flow.body());
    assert!(
        !has_node_kind(body.syntax(), SyntaxKind::TEXT),
        "the logic line must not be swallowed into a TEXT run"
    );
    let logic_line: ast::LogicLine = find_child(body.syntax()).expect("LOGIC_LINE");
    let let_stmt = logic_line.let_stmt().expect("LET_STMT child");
    assert!(logic_line.assign_stmt().is_none());
    assert!(logic_line.expr_stmt().is_none());
    assert_eq!(
        let_stmt.name_token().map(|t| t.text().to_string()),
        Some("n".to_string())
    );
    assert_eq!(
        let_stmt.value().map(|n| n.kind()),
        Some(SyntaxKind::INTEGER_LIT)
    );
}

#[test]
fn logic_line_temp_decl_with_annotation_and_no_initializer() {
    let src = "flow greet() {\n~ let n: int\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = expect_prose_body(flow.body());
    let logic_line: ast::LogicLine = find_child(body.syntax()).expect("LOGIC_LINE");
    let let_stmt = logic_line.let_stmt().expect("LET_STMT child");
    assert!(let_stmt.type_annotation().is_some());
    assert!(let_stmt.value().is_none());
}

#[test]
fn logic_line_temp_decl_precedes_ordinary_content_on_the_next_line() {
    // Mirrors `logic_line_precedes_ordinary_content_on_the_next_line` for
    // the assignment shape: the escape consumes exactly its own line.
    let src = "flow greet() {\n~ let n = 5\nValue is {n}.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = expect_prose_body(flow.body());
    let items: Vec<_> = body.syntax().children().collect();
    assert_eq!(items[0].kind(), SyntaxKind::LOGIC_LINE);
    assert_eq!(items[1].kind(), SyntaxKind::CONTENT_LINE);
}

#[test]
fn logic_line_compound_assignment_operators() {
    for (src_op, expected) in [("+=", SyntaxKind::PLUS_EQ), ("-=", SyntaxKind::MINUS_EQ)] {
        let src = format!("flow greet() {{\n~ n {src_op} 1\n}}\n");
        let p = assert_lossless(&src);
        assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
        let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
        let body = expect_prose_body(flow.body());
        let logic_line: ast::LogicLine = find_child(body.syntax()).expect("LOGIC_LINE");
        let assign = logic_line.assign_stmt().expect("ASSIGN_STMT child");
        assert_eq!(assign.op_token().map(|t| t.kind()), Some(expected));
    }
}

#[test]
fn logic_line_bare_expression_is_a_call() {
    let src = "flow greet() {\n~ bump()\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = expect_prose_body(flow.body());
    let logic_line: ast::LogicLine = find_child(body.syntax()).expect("LOGIC_LINE");
    assert!(logic_line.assign_stmt().is_none());
    let expr_stmt = logic_line.expr_stmt().expect("EXPR_STMT child");
    assert_eq!(
        expr_stmt.expr().map(|n| n.kind()),
        Some(SyntaxKind::CALL_EXPR)
    );
}

#[test]
fn logic_line_precedes_ordinary_content_on_the_next_line() {
    // The escape consumes exactly its own line — the content line right
    // after it parses completely normally, unaffected.
    let src = "flow greet() {\n~ n = 5\nValue is {n}.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = expect_prose_body(flow.body());
    let items: Vec<_> = body.syntax().children().collect();
    assert_eq!(items[0].kind(), SyntaxKind::LOGIC_LINE);
    assert_eq!(items[1].kind(), SyntaxKind::CONTENT_LINE);
    assert!(has_node_kind(&items[1], SyntaxKind::INTERPOLATION));
}

#[test]
fn logic_line_inside_choice_body_and_conditional_colon_body() {
    // `body_line`'s TILDE arm is shared by every body-shaped list that
    // reuses it (choice bodies via `braced_item_list`); `colon_body_line`
    // (`family.rs`) keeps its own copy in sync (see that function's doc) —
    // both must recognize the escape, not just the top-level flow body.
    let choice_src = "flow greet() {\n{?\n* [go] {\n~ n = 1\n}\n}\n}\n";
    let p = assert_lossless(choice_src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LOGIC_LINE));

    let colon_src = "flow greet() {\n{if n > 0: ~ n = 0}\n}\n";
    let p = assert_lossless(colon_src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LOGIC_LINE));
}

#[test]
fn logic_line_temp_decl_inside_choice_body_and_conditional_colon_body() {
    // Same two `TILDE`-dispatch sites `logic_line_inside_choice_body_and_
    // conditional_colon_body` already covers for the assignment shape,
    // exercised with `~ let` instead — the recovery/terminator awareness
    // (issue #1991 findings F2/F3) must hold for every logic-line shape,
    // not just assignment, or a `~ let` inside a braced/colon body could
    // desync the enclosing block (rule 12o).
    let choice_src = "flow greet() {\n{?\n* [go] {\n~ let n = 1\n}\n}\n}\n";
    let p = assert_lossless(choice_src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LOGIC_LINE));

    let colon_src = "flow greet() {\n{if n > 0: ~ let m = 0}\n}\n";
    let p = assert_lossless(colon_src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LOGIC_LINE));
}

#[test]
fn logic_line_temp_decl_partial_progress_is_not_swallowed_as_prose() {
    // Mirrors `logic_line_partial_progress_is_not_swallowed_as_prose` for
    // the temp-decl shape (rule 12o: a recovery loop must fire on partial
    // progress, not only zero progress). `~ let n 5` makes partial
    // progress — `let_line` consumes `KW_LET`/`IDENT` but `binding_
    // annotation` sees no `:` and `p.eat(EQ)` fails on `5` (not `=`), so
    // `let_line` stops with `5` unconsumed. Before a recovery loop existed
    // for this shape, that leftover would be handed back to `body_line`'s
    // prose scanner with zero diagnostics.
    let src = "flow greet() {\n~ let n 5\n}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "partial-progress leftover tokens must raise a real diagnostic"
    );
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::TEXT),
        "partial-progress leftover tokens must never be swallowed into TEXT prose"
    );
}

#[test]
fn logic_line_partial_progress_is_not_swallowed_as_prose() {
    // Issue #1991 finding F2: the recovery loop originally fired only on
    // ZERO token progress. `~ n *= 3` makes PARTIAL progress — `at_assignment`
    // doesn't recognize `*=` (only `=`/`+=`/`-=`), so `expr_stmt_line` parses
    // `n` alone as an `EXPR_STMT` and stops, leaving `*= 3` unconsumed. Before
    // the fix that leftover was handed back to `body_line`'s prose scanner
    // with zero diagnostics; it must now be recovered inside `LOGIC_LINE`
    // itself, loudly, and never fold into `TEXT`.
    let src = "flow greet() {\n~ n *= 3\n}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "partial-progress leftover tokens must raise a real diagnostic"
    );
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::TEXT),
        "partial-progress leftover tokens must never be swallowed into TEXT prose"
    );
}

#[test]
fn logic_line_recovery_does_not_consume_enclosing_close_brace() {
    // Issue #1991 finding F3: the recovery loop originally terminated only
    // on NEWLINE/EOF, so inside a braced/colon body (no NEWLINE before the
    // block's own `}`) it consumed the enclosing `R_BRACE` too, corrupting
    // the block structure. `~ if` is a zero-progress unsupported shape
    // (mirrors `logic_line_unsupported_shape_is_a_loud_diagnostic_never_silent_prose`),
    // reached here from a same-line colon body so its leftover recovery
    // has to stop at `}` rather than eat it. Before the fix, the recovery
    // loop consumed straight through `}` looking for a `NEWLINE`, so the
    // `CONDITIONAL_BLOCK` never closed, `after` was absorbed into the
    // still-open `IF_ARM`, and the parse ended with a spurious "expected
    // R_BRACE, found EOF" (this test's own source has no such content, so
    // that extra diagnostic — not present here — was the tell).
    let src = "flow greet() {\n{if n > 0: ~ if}\nafter\n}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert_eq!(
        p.errors().len(),
        2,
        "expected exactly the atom + logic-line diagnostics for `~ if`, no R_BRACE/EOF fallout: {:?}",
        p.errors()
    );
    // The conditional block must close cleanly, so the following content
    // line is a sibling of it — not absorbed into the still-open IF_ARM —
    // and the whole flow body reaches its own closing brace.
    let flow: ast::FlowDecl = find_child(&p.syntax()).expect("flow decl");
    let body = expect_prose_body(flow.body());
    let items: Vec<_> = body.syntax().children().collect();
    assert!(
        items
            .iter()
            .any(|n| n.kind() == SyntaxKind::CONDITIONAL_BLOCK),
        "expected a closed CONDITIONAL_BLOCK sibling, got: {:?}",
        items.iter().map(SyntaxNode::kind).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|n| n.kind() == SyntaxKind::CONTENT_LINE
            && n.text().to_string().contains("after")),
        "expected the following content line as a sibling, not absorbed into the block"
    );
}

#[test]
fn logic_line_unsupported_shape_is_a_loud_diagnostic_never_silent_prose() {
    // Issue #1991's own hedge: if a shape reachable at code-ground
    // statement position has no content-ground meaning (`if`, here), it
    // must be diagnosed, never silently accepted as prose. It must NOT
    // fold into a `TEXT` run either way — including the recovery-loop
    // guard that keeps a fully-unrecognized shape's leftover tokens from
    // being handed back to `body_line`'s next iteration and re-dispatched
    // as a fresh (prose) line.
    let src = "flow greet() {\n~ if\n}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "an unsupported logic-line shape must raise a real diagnostic"
    );
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::TEXT),
        "an unsupported logic-line shape must never be swallowed into TEXT prose"
    );
}

// ── K. The code-ground line escape: `> text` (charter §8.2, RULED ────
// ── 2026-07-23, issue #1992) ───────────────────────────────────────────
//
// The mirror image of section J at the opposite ground: `> text` emits a
// prose line inside an otherwise code-ground `flow`/`fn` body (`fn`'s
// default, or a `flow`'s `~{ }` "Compound guard" override). Dispatched
// from `stmt::statement()`'s `GT` arm — reachable everywhere a
// code-ground `STMT_BLOCK` statement is parsed, including nested
// `if`/`while`/`for` bodies (all of which reuse `stmt_block`/`statement()`
// verbatim). Unlike `LOGIC_LINE`, `PROSE_LINE` wraps a `CONTENT_LINE` — the
// content-ground line layer's own node, reused unmodified — and needs no
// bespoke recovery loop of its own: `content_line` already owns its own
// termination discipline (stops at `NEWLINE`/EOF, never consumes a bare
// `R_BRACE`).

#[test]
fn prose_line_wraps_a_content_line_with_zero_errors() {
    let src = "fn radio() {\n> hi\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let f: ast::FnDecl = find_child(&p.syntax()).expect("fn decl");
    let body = expect_code_body(f.body());
    let items: Vec<_> = body.items().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind(), SyntaxKind::PROSE_LINE);
    let prose_line: ast::ProseLine = find_child(body.syntax()).expect("PROSE_LINE");
    let content_line = prose_line.content_line().expect("CONTENT_LINE child");
    assert!(has_node_kind(content_line.syntax(), SyntaxKind::TEXT));
}

#[test]
fn prose_line_carries_interpolation_like_any_content_line() {
    // The issue's own repro: `> [{chan}] {text}` — interpolation works
    // identically to the whole-body `>{ }` form, since both route through
    // the same `content_line` grammar.
    let src = "fn radio(chan: string, text: string) {\n> [{chan}] {text}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let f: ast::FnDecl = find_child(&p.syntax()).expect("fn decl");
    let body = expect_code_body(f.body());
    let prose_line: ast::ProseLine = find_child(body.syntax()).expect("PROSE_LINE");
    let content_line = prose_line.content_line().expect("CONTENT_LINE child");
    let interpolations: Vec<_> = content_line
        .syntax()
        .children()
        .filter(|n| n.kind() == SyntaxKind::INTERPOLATION)
        .collect();
    assert_eq!(interpolations.len(), 2, "one per `{{…}}` interpolation");
}

#[test]
fn prose_line_precedes_ordinary_code_on_the_next_line() {
    // The escape consumes exactly its own line — an ordinary code
    // statement right after it parses completely normally, unaffected.
    // Mirrors `logic_line_precedes_ordinary_content_on_the_next_line`'s
    // opposite-ground shape.
    let src = "fn radio() {\n> hi\nn = 1;\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let f: ast::FnDecl = find_child(&p.syntax()).expect("fn decl");
    let body = expect_code_body(f.body());
    let items: Vec<_> = body.items().collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind(), SyntaxKind::PROSE_LINE);
    assert_eq!(items[1].kind(), SyntaxKind::ASSIGN_STMT);
}

#[test]
fn prose_line_reachable_inside_nested_if_body() {
    // Rule 12o: verify a construct dispatched from the shared per-statement
    // loop also parses safely one level down, inside a nested control-flow
    // body — `if`/`while`/`for` bodies all reuse `stmt_block`/`statement()`
    // verbatim, so this is the same dispatch table, not a special case.
    let src = "fn radio() {\nif true {\n> hi\n}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PROSE_LINE));
}

#[test]
fn prose_line_recovery_does_not_consume_enclosing_close_brace() {
    // Rule 12o's second half: a `> text` line nested inside an `if` body,
    // followed by an ordinary statement in the *enclosing* block, must not
    // have its own closing `}` swallowed — the following statement must
    // land as the `IF_STMT`'s own sibling, not get absorbed into a
    // still-open `if` body. `content_line` already stops cleanly at
    // `NEWLINE`, so this is a plain reachability/structure check, not a
    // fresh recovery-loop probe the way `LOGIC_LINE`'s own version of this
    // test (`logic_line_recovery_does_not_consume_enclosing_close_brace`)
    // needed to be for its own, more permissive statement grammar.
    let src = "fn radio() {\nif true {\n> hi\n}\nafter();\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let f: ast::FnDecl = find_child(&p.syntax()).expect("fn decl");
    let body = expect_code_body(f.body());
    let items: Vec<_> = body.items().collect();
    assert_eq!(
        items.len(),
        2,
        "expected IF_STMT + the trailing EXPR_STMT as siblings, got: {:?}",
        items.iter().map(SyntaxNode::kind).collect::<Vec<_>>()
    );
    assert_eq!(items[0].kind(), SyntaxKind::IF_STMT);
    assert_eq!(items[1].kind(), SyntaxKind::EXPR_STMT);
}

#[test]
fn prose_line_is_reachable_from_a_flows_compound_guard_body_too() {
    // `~{ }` selects the code-ground `STMT_BLOCK` body for a `flow`
    // (charter §4's "Compound guard") — the same node/grammar an `fn`'s
    // default body uses, so `> text` must work there too.
    let src = "flow greet() ~{\n> hi\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PROSE_LINE));
}
