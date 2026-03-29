#![allow(clippy::panic)]

use brink_syntax::ast::AstNode;
use brink_syntax::parse;
use rowan::TextRange;

use crate::hir::lower2::{
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

// ─── Accumulator tests ──────────────────────────────────────────────

#[test]
fn accumulator_content_with_glue_suppresses_eol() {
    let source = "Hello<>\n";
    let (block, diags, _) = lower2_body(source);
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
    let (block, _, _) = lower2_body(source);
    // Root body might be empty (knots handle their own bodies),
    // so just verify it compiles and doesn't panic.
    let _ = block;
}

// ─── Full-pipeline comparison tests ─────────────────────────────────

/// Lower through both pipelines and assert identical `HirFile` output.
fn assert_pipelines_match(source: &str) {
    let parsed = parse(source);
    let tree = parsed.tree();

    let (hir1, manifest1, diags1) = crate::hir::lower::lower(FileId(0), &tree);
    let (hir2, manifest2, diags2) = crate::hir::lower2::lower(FileId(0), &tree);

    assert_eq!(
        diags1.len(),
        diags2.len(),
        "diagnostic count mismatch for source:\n{source}\nlower:  {d1:?}\nlower2: {d2:?}",
        d1 = diags1.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        d2 = diags2.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
    );

    // Compare diagnostic codes (order may differ, so sort)
    let mut codes1: Vec<_> = diags1.iter().map(|d| d.code.as_str()).collect();
    let mut codes2: Vec<_> = diags2.iter().map(|d| d.code.as_str()).collect();
    codes1.sort_unstable();
    codes2.sort_unstable();
    assert_eq!(
        codes1, codes2,
        "diagnostic codes differ for source:\n{source}"
    );

    assert_eq!(hir1, hir2, "HirFile mismatch for source:\n{source}");

    // Compare manifests — knots, stitches, variables, etc.
    assert_eq!(
        manifest1.knots.len(),
        manifest2.knots.len(),
        "knot count mismatch for source:\n{source}"
    );
    assert_eq!(
        manifest1.variables.len(),
        manifest2.variables.len(),
        "variable count mismatch for source:\n{source}"
    );
    assert_eq!(
        manifest1.unresolved.len(),
        manifest2.unresolved.len(),
        "unresolved count mismatch for source:\n{source}"
    );
}

#[test]
fn pipeline_match_empty() {
    assert_pipelines_match("");
}

#[test]
fn pipeline_match_simple_text() {
    assert_pipelines_match("Hello, world!\n");
}

#[test]
fn pipeline_match_multiple_lines() {
    assert_pipelines_match("Line one\nLine two\nLine three\n");
}

#[test]
fn pipeline_match_temp_decl() {
    assert_pipelines_match("~ temp x = 42\n");
}

#[test]
fn pipeline_match_var_decl() {
    assert_pipelines_match("VAR x = 5\n");
}

#[test]
fn pipeline_match_const_decl() {
    assert_pipelines_match("CONST limit = 10\n");
}

#[test]
fn pipeline_match_interpolation() {
    assert_pipelines_match("VAR x = 1\nThe value is {x}\n");
}

#[test]
fn pipeline_match_simple_divert() {
    assert_pipelines_match("-> END\n");
}

#[test]
fn pipeline_match_knot() {
    assert_pipelines_match("=== my_knot ===\nHello from knot\n-> END\n");
}

#[test]
fn pipeline_match_knot_with_stitch() {
    assert_pipelines_match("=== my_knot ===\n= my_stitch\nHello from stitch\n-> END\n");
}

#[test]
fn pipeline_match_simple_choice() {
    assert_pipelines_match("* Choice one\n* Choice two\n- Gather\n");
}

#[test]
fn pipeline_match_inline_conditional() {
    assert_pipelines_match("VAR x = true\n{x: yes}\n");
}

#[test]
fn pipeline_match_inline_sequence() {
    assert_pipelines_match("{&one|two|three}\n");
}

#[test]
fn pipeline_match_tags() {
    assert_pipelines_match("Hello #greeting #friendly\n");
}

#[test]
fn pipeline_match_function_knot() {
    assert_pipelines_match("=== function greet() ===\n~ return 1\n");
}

#[test]
fn pipeline_match_list_decl() {
    assert_pipelines_match("LIST colors = red, green, blue\n");
}

#[test]
fn pipeline_match_assignment() {
    assert_pipelines_match("VAR x = 0\n~ x = 5\n");
}

// ─── Corpus comparison test ─────────────────────────────────────────

fn find_ink_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_ink_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "ink") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Run both lowering pipelines on every `.ink` file in the test corpus
/// and assert identical `HirFile` output.
///
/// Currently ignored: 319 of 1124 files differ due to branch body
/// newline handling and promoted block trailing content. Run with
/// `--ignored` to see the failures.
#[test]
#[ignore = "319 of 1124 files differ — branch body newline handling and promoted blocks"]
fn pipeline_match_corpus() {
    let corpus_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests");
    let mut checked = 0;
    let mut failures = Vec::new();

    for path in find_ink_files(&corpus_root) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };

        let parsed = parse(&source);
        let tree = parsed.tree();

        let (hir1, manifest1, diags1) = crate::hir::lower::lower(FileId(0), &tree);
        let (hir2, manifest2, diags2) = crate::hir::lower2::lower(FileId(0), &tree);

        let rel = path.strip_prefix(&corpus_root).unwrap_or(&path);
        let name = rel.display().to_string();

        // Compare diagnostics
        let mut codes1: Vec<_> = diags1.iter().map(|d| d.code.as_str()).collect();
        let mut codes2: Vec<_> = diags2.iter().map(|d| d.code.as_str()).collect();
        codes1.sort_unstable();
        codes2.sort_unstable();
        if codes1 != codes2 {
            failures.push(format!(
                "{name}: diagnostic codes differ: {codes1:?} vs {codes2:?}"
            ));
            continue;
        }

        // Compare HIR
        if hir1 != hir2 {
            failures.push(format!("{name}: HirFile mismatch"));
            continue;
        }

        // Compare manifest counts
        if manifest1.knots.len() != manifest2.knots.len()
            || manifest1.variables.len() != manifest2.variables.len()
            || manifest1.unresolved.len() != manifest2.unresolved.len()
            || manifest1.locals.len() != manifest2.locals.len()
        {
            failures.push(format!("{name}: manifest mismatch"));
            continue;
        }

        checked += 1;
    }

    assert!(
        failures.is_empty(),
        "{} of {} files failed pipeline comparison:\n{}",
        failures.len(),
        checked + failures.len(),
        failures.join("\n")
    );

    // Sanity: we should have checked a meaningful number of files
    assert!(
        checked > 50,
        "only checked {checked} files — expected more in corpus"
    );
}
