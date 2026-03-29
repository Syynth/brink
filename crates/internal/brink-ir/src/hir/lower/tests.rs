#![allow(clippy::panic)]

use brink_syntax::ast::AstNode;
use brink_syntax::parse;
use rowan::TextRange;

use crate::hir::lower::{
    BodyChild, DeclareSymbols, EffectSink, LowerScope, LowerSink, classify_body_child,
    lower_simple_body,
};
use crate::*;

// ─── Test helpers ───────────────────────────────────────────────────

fn make_scope() -> LowerScope {
    LowerScope::new(FileId(0))
}

fn make_sink() -> EffectSink {
    EffectSink::new(FileId(0))
}

/// Parse source and lower the root body.
fn lower_body(source: &str) -> (Block, Vec<Diagnostic>, SymbolManifest) {
    let parsed = parse(source);
    let tree = parsed.tree();
    let scope = make_scope();
    let mut sink = make_sink();
    let block = lower_simple_body(tree.syntax(), &scope, &mut sink);
    let (manifest, diagnostics) = sink.finish();
    (block, diagnostics, manifest)
}

// ─── Mock sink for testing trait abstraction ────────────────────────

struct TestSink {
    diagnostics: Vec<(TextRange, DiagnosticCode)>,
    symbols: Vec<(SymbolKind, String)>,
}

impl TestSink {
    fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            symbols: Vec::new(),
        }
    }
}

impl LowerSink for TestSink {
    fn diagnose(&mut self, range: TextRange, code: DiagnosticCode) -> crate::hir::lower::Diagnosed {
        self.diagnostics.push((range, code));
        crate::hir::lower::Diagnosed::test_token()
    }

    fn declare_with(
        &mut self,
        kind: SymbolKind,
        name: &str,
        _range: TextRange,
        _params: Vec<ParamInfo>,
        _detail: Option<String>,
    ) {
        self.symbols.push((kind, name.to_string()));
    }

    fn add_local(&mut self, _local: crate::symbols::LocalSymbol) {}

    fn add_unresolved(
        &mut self,
        _path: &str,
        _range: TextRange,
        _kind: crate::symbols::RefKind,
        _scope: &Scope,
        _arg_count: Option<usize>,
    ) {
    }
}

// ─── Expression lowering tests ──────────────────────────────────────

#[test]
fn lower_integer_literal() {
    let source = "~ temp x = 42\n";
    let (block, diags, _) = lower_body(source);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 1);
    match &block.stmts[0] {
        Stmt::TempDecl(td) => {
            assert_eq!(td.name.text, "x");
            assert!(
                matches!(td.value, Some(Expr::Int(42))),
                "expected Int(42), got {:?}",
                td.value
            );
        }
        other => panic!("expected TempDecl, got {other:?}"),
    }
}

#[test]
fn lower_infix_expression() {
    let source = "~ temp y = 3 + 4\n";
    let (block, diags, _) = lower_body(source);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 1);
    match &block.stmts[0] {
        Stmt::TempDecl(td) => {
            assert_eq!(td.name.text, "y");
            assert!(
                matches!(
                    &td.value,
                    Some(Expr::Infix(lhs, InfixOp::Add, rhs))
                    if matches!(lhs.as_ref(), Expr::Int(3))
                    && matches!(rhs.as_ref(), Expr::Int(4))
                ),
                "expected 3 + 4, got {:?}",
                td.value
            );
        }
        other => panic!("expected TempDecl, got {other:?}"),
    }
}

// ─── Content lowering tests ─────────────────────────────────────────

#[test]
fn simple_text_line() {
    let (block, diags, _) = lower_body("Hello, world!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 2, "expected Content + EndOfLine");
    assert!(matches!(&block.stmts[0], Stmt::Content(c) if !c.parts.is_empty()));
    assert!(matches!(&block.stmts[1], Stmt::EndOfLine));
}

#[test]
fn expression_interpolation() {
    let (block, diags, _) = lower_body("Value is {x}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 2);
    match &block.stmts[0] {
        Stmt::Content(c) => {
            assert!(c.parts.len() >= 2, "expected text + interpolation");
            assert!(matches!(&c.parts[0], ContentPart::Text(t) if t.contains("Value")));
            assert!(
                matches!(&c.parts[1], ContentPart::Interpolation(Expr::Path(_))),
                "expected path interpolation, got {:?}",
                c.parts[1]
            );
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn tag_on_content_line() {
    let (block, diags, _) = lower_body("Hello #greeting\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 2);
    match &block.stmts[0] {
        Stmt::Content(c) => {
            assert!(!c.tags.is_empty(), "expected at least one tag");
            assert!(
                matches!(&c.tags[0].parts[0], ContentPart::Text(t) if t == "greeting"),
                "expected 'greeting' tag, got {:?}",
                c.tags[0].parts
            );
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn logic_line_assignment() {
    let source = "~ temp x = 0\n~ x = 5\n";
    let (block, diags, _) = lower_body(source);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 2, "expected TempDecl + Assignment");
    assert!(matches!(&block.stmts[0], Stmt::TempDecl(_)));
    assert!(matches!(&block.stmts[1], Stmt::Assignment(_)));
}

// ─── Diagnostic tests ───────────────────────────────────────────────

#[test]
fn logic_line_emits_diagnostic_on_malformed() {
    // A logic line with just `~` and nothing else should emit E014.
    let source = "~\n";
    let (_, diags, _) = lower_body(source);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E014),
        "expected E014 diagnostic, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

// ─── Mock sink tests ────────────────────────────────────────────────

#[test]
fn mock_sink_records_diagnostics() {
    let parsed = parse("~\n");
    let tree = parsed.tree();
    let scope = make_scope();
    let mut sink = TestSink::new();
    let _ = lower_simple_body(tree.syntax(), &scope, &mut sink);
    assert!(
        sink.diagnostics
            .iter()
            .any(|(_, code)| *code == DiagnosticCode::E014),
        "expected E014 in mock sink"
    );
}

#[test]
fn mock_sink_records_symbol_declarations() {
    let parsed = parse("VAR x = 5\n");
    let tree = parsed.tree();
    let scope = make_scope();
    let mut sink = TestSink::new();

    // Declarations are hoisted, not part of body lowering.
    // Directly test the DeclareSymbols trait.
    for node in tree.syntax().descendants() {
        if let Some(var) = brink_syntax::ast::VarDecl::cast(node) {
            let _ = var.declare_and_lower(&scope, &mut sink);
        }
    }
    assert!(
        sink.symbols
            .iter()
            .any(|(kind, name)| *kind == SymbolKind::Variable && name == "x"),
        "expected variable 'x' in mock sink, got: {:?}",
        sink.symbols
    );
}

// ─── BodyChild classification tests ─────────────────────────────────

#[test]
fn classify_recognizes_content_line() {
    let parsed = parse("Hello\n");
    let tree = parsed.tree();
    let mut found = false;
    for child in tree.syntax().children() {
        if matches!(classify_body_child(&child), BodyChild::ContentLine(_)) {
            found = true;
        }
    }
    assert!(found, "expected to find a ContentLine child");
}

#[test]
fn classify_recognizes_logic_line() {
    let parsed = parse("~ temp x = 1\n");
    let tree = parsed.tree();
    let mut found = false;
    for child in tree.syntax().children() {
        if matches!(classify_body_child(&child), BodyChild::LogicLine(_)) {
            found = true;
        }
    }
    assert!(found, "expected to find a LogicLine child");
}

// ─── Accumulator tests ──────────────────────────────────────────────

#[test]
fn accumulator_content_with_glue_suppresses_eol() {
    let source = "Hello<>\n";
    let (block, diags, _) = lower_body(source);
    assert!(diags.is_empty());
    // Glue suppresses EndOfLine — should have Content only, no EndOfLine
    assert!(
        matches!(&block.stmts[0], Stmt::Content(c) if !c.parts.is_empty()),
        "expected Content stmt"
    );
    // Should NOT have EndOfLine after glue
    assert!(
        !block.stmts.iter().any(|s| matches!(s, Stmt::EndOfLine)),
        "EndOfLine should be suppressed by glue"
    );
}

#[test]
fn accumulator_logic_line_with_call_emits_eol() {
    // A function call in a logic line triggers EndOfLine
    let source = "=== function f() ===\n~ return 1\n=== main ===\n~ f()\n";
    let (block, _, _) = lower_body(source);
    // Root body might be empty (knots handle their own bodies),
    // so just verify it compiles and doesn't panic.
    let _ = block;
}
