#![allow(clippy::panic)]

use brink_syntax::ast::AstNode;
use brink_syntax::parse;
use rowan::TextRange;

use crate::hir::lower2::{
    BodyChild, ContentAccumulator, ContentLineOutput, DeclareSymbols, EffectSink, Integrate,
    LogicLineOutput, LowerScope, LowerSink, classify_body_child, lower_simple_body,
};
use crate::*;

// ─── Test helpers ───────────────────────────────────────────────────

fn make_scope() -> LowerScope {
    LowerScope::new(FileId(0))
}

fn make_sink() -> EffectSink {
    EffectSink::new(FileId(0))
}

/// Parse source and lower the root body through lower2.
fn lower2_body(source: &str) -> (Block, Vec<Diagnostic>, SymbolManifest) {
    let parsed = parse(source);
    let tree = parsed.tree();
    let scope = make_scope();
    let mut sink = make_sink();
    let block = lower_simple_body(tree.syntax(), &scope, &mut sink);
    let (manifest, diagnostics) = sink.finish();
    (block, diagnostics, manifest)
}

/// Parse source and lower through the original lower module.
fn lower1_body(source: &str) -> (Block, Vec<Diagnostic>) {
    let parsed = parse(source);
    let tree = parsed.tree();
    let (hir, _, diags) = crate::hir::lower::lower(FileId(0), &tree);
    (hir.root_content, diags)
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
    fn diagnose(
        &mut self,
        range: TextRange,
        code: DiagnosticCode,
    ) -> crate::hir::lower2::Diagnosed {
        self.diagnostics.push((range, code));
        crate::hir::lower2::Diagnosed::test_token()
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
    let (block, diags, _) = lower2_body(source);
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
    let (block, diags, _) = lower2_body(source);
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
    let (block, diags, _) = lower2_body("Hello, world!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 2, "expected Content + EndOfLine");
    assert!(matches!(&block.stmts[0], Stmt::Content(c) if !c.parts.is_empty()));
    assert!(matches!(&block.stmts[1], Stmt::EndOfLine));
}

#[test]
fn expression_interpolation() {
    let (block, diags, _) = lower2_body("Value is {x}\n");
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
    let (block, diags, _) = lower2_body("Hello #greeting\n");
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
    let (block, diags, _) = lower2_body(source);
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
    let (_, diags, _) = lower2_body(source);
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

// ─── Comparison tests (lower vs lower2) ─────────────────────────────

#[test]
fn compare_simple_text_line() {
    let source = "Hello, world!\n";
    let (block1, _) = lower1_body(source);
    let (block2, _, _) = lower2_body(source);
    assert_eq!(
        block1.stmts.len(),
        block2.stmts.len(),
        "statement count mismatch: lower={}, lower2={}",
        block1.stmts.len(),
        block2.stmts.len()
    );
    // Both should have Content + EndOfLine
    assert!(matches!(&block1.stmts[0], Stmt::Content(_)));
    assert!(matches!(&block2.stmts[0], Stmt::Content(_)));
    assert!(matches!(&block1.stmts[1], Stmt::EndOfLine));
    assert!(matches!(&block2.stmts[1], Stmt::EndOfLine));
}

#[test]
fn compare_temp_decl() {
    let source = "~ temp x = 42\n";
    let (block1, _) = lower1_body(source);
    let (block2, _, _) = lower2_body(source);
    assert_eq!(block1.stmts.len(), block2.stmts.len());
    match (&block1.stmts[0], &block2.stmts[0]) {
        (Stmt::TempDecl(td1), Stmt::TempDecl(td2)) => {
            assert_eq!(td1.name.text, td2.name.text);
            assert_eq!(format!("{:?}", td1.value), format!("{:?}", td2.value));
        }
        _ => panic!(
            "expected TempDecl from both, got {:?} vs {:?}",
            block1.stmts[0], block2.stmts[0]
        ),
    }
}

#[test]
fn compare_multiple_content_lines() {
    let source = "Line one\nLine two\nLine three\n";
    let (block1, _) = lower1_body(source);
    let (block2, _, _) = lower2_body(source);
    assert_eq!(
        block1.stmts.len(),
        block2.stmts.len(),
        "statement count mismatch for multi-line content"
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

// ─── Integrate tests ────────────────────────────────────────────────

#[test]
fn integrate_content_line_with_glue() {
    let mut acc = ContentAccumulator::new();
    acc.integrate(ContentLineOutput::Content {
        content: Content {
            ptr: None,
            parts: vec![ContentPart::Text("hello".into()), ContentPart::Glue],
            tags: Vec::new(),
        },
        divert: None,
        ends_with_glue: true,
    });
    let stmts = acc.finish();
    // Glue suppresses EndOfLine
    assert_eq!(
        stmts.len(),
        1,
        "expected only Content (no EndOfLine after glue)"
    );
    assert!(matches!(&stmts[0], Stmt::Content(_)));
}

#[test]
fn integrate_logic_line_with_call() {
    let mut acc = ContentAccumulator::new();
    // ExprStmt with a Call triggers EndOfLine
    acc.integrate(LogicLineOutput::ExprStmt(Expr::Call(
        Path {
            segments: vec![Name {
                text: "f".into(),
                range: TextRange::default(),
            }],
            range: TextRange::default(),
        },
        Vec::new(),
    )));
    let stmts = acc.finish();
    assert_eq!(stmts.len(), 2, "expected ExprStmt + EndOfLine");
    assert!(matches!(&stmts[0], Stmt::ExprStmt(_)));
    assert!(matches!(&stmts[1], Stmt::EndOfLine));
}
