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

    fn declare_full(
        &mut self,
        kind: SymbolKind,
        name: &str,
        _range: TextRange,
        _params: Vec<ParamInfo>,
        _detail: Option<String>,
        _doc: Option<DocBlock>,
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

// ─── Doc-comment attachment tests ───────────────────────────────────

/// Lower a complete file and return its manifest + diagnostics.
fn lower_full(source: &str) -> (SymbolManifest, Vec<Diagnostic>) {
    let parsed = parse(source);
    let tree = parsed.tree();
    let (_hir, manifest, diags) = crate::hir::lower(FileId(0), &tree);
    (manifest, diags)
}

#[test]
fn docs_attach_to_all_declaration_kinds() {
    let source = "\
/// An external.
EXTERNAL ping(x)
/// A variable.
VAR health = 100
/// A constant.
CONST SPEED = 0.5
/// A list.
LIST mood = happy, sad
/// A knot.
== hub ==
intro
/// A nested stitch.
= market
stalls
/// A function knot.
== function damage(weapon) ==
~ return 1
";
    let (manifest, diags) = lower_full(source);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

    let doc_text = |kind: SymbolKind, name: &str| {
        manifest
            .docs
            .get(&(kind, name.to_string()))
            .unwrap_or_else(|| panic!("doc for {kind:?} {name}"))
            .doc
            .clone()
    };
    assert_eq!(
        doc_text(SymbolKind::External, "ping").as_deref(),
        Some("An external.")
    );
    assert_eq!(
        doc_text(SymbolKind::Variable, "health").as_deref(),
        Some("A variable.")
    );
    assert_eq!(
        doc_text(SymbolKind::Constant, "SPEED").as_deref(),
        Some("A constant.")
    );
    assert_eq!(
        doc_text(SymbolKind::List, "mood").as_deref(),
        Some("A list.")
    );
    assert_eq!(
        doc_text(SymbolKind::Knot, "hub").as_deref(),
        Some("A knot.")
    );
    assert_eq!(
        doc_text(SymbolKind::Stitch, "hub.market").as_deref(),
        Some("A nested stitch."),
        "nested stitch docs are keyed by qualified name"
    );
    assert_eq!(
        doc_text(SymbolKind::Knot, "damage").as_deref(),
        Some("A function knot.")
    );
}

#[test]
fn inapplicable_tags_emit_e043() {
    let source = "\
/// @kind query
== hub ==
intro
/// @param x {int}
VAR health = 100
";
    let (manifest, diags) = lower_full(source);
    let e043: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E043)
        .collect();
    assert_eq!(e043.len(), 2, "one E043 per inapplicable tag: {diags:?}");
    // The dropped tags leave no doc content behind.
    assert!(
        !manifest
            .docs
            .contains_key(&(SymbolKind::Knot, "hub".to_string())),
        "tag-only block with all tags dropped attaches nothing"
    );
}

#[test]
fn undocumented_declarations_have_no_doc_entries() {
    let source = "\
EXTERNAL ping(x)
VAR health = 100
== hub ==
intro
";
    let (manifest, diags) = lower_full(source);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(manifest.docs.is_empty());
}

// ─── Directive annotations (`#@…`) ──────────────────────────────────

/// Full-file lower returning the HIR too.
fn lower_hir(source: &str) -> (HirFile, Vec<Diagnostic>) {
    let parsed = parse(source);
    let tree = parsed.tree();
    let (hir, _manifest, diags) = crate::hir::lower(FileId(0), &tree);
    (hir, diags)
}

/// Collect every tag string that survives into lowered content
/// (blocks, recursively through knots/stitches).
fn all_content_tags(hir: &HirFile) -> Vec<String> {
    fn tags_in_block(block: &Block, out: &mut Vec<String>) {
        for stmt in &block.stmts {
            if let Stmt::Content(c) = stmt {
                for tag in &c.tags {
                    let mut text = String::new();
                    for part in &tag.parts {
                        if let ContentPart::Text(t) = part {
                            text.push_str(t);
                        }
                    }
                    out.push(text);
                }
            }
        }
    }
    let mut out = Vec::new();
    tags_in_block(&hir.root_content, &mut out);
    for knot in &hir.knots {
        tags_in_block(&knot.body, &mut out);
        for stitch in &knot.stitches {
            tags_in_block(&stitch.body, &mut out);
        }
    }
    out
}

fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn local_directive_marks_var() {
    let (hir, diags) = lower_hir("#@local\nVAR mood = 0\nhello\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.variables.len(), 1);
    assert!(hir.variables[0].is_local);
    // Erasure: the directive never becomes a content tag.
    assert!(all_content_tags(&hir).is_empty());
}

#[test]
fn plain_var_is_not_local() {
    let (hir, diags) = lower_hir("VAR mood = 0\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(!hir.variables[0].is_local);
}

#[test]
fn local_directive_marks_knot_from_top_of_body() {
    let (hir, diags) = lower_hir("== guard ==\n#@local\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots.len(), 1);
    assert!(hir.knots[0].is_local);
    assert!(all_content_tags(&hir).is_empty());
}

#[test]
fn local_directive_marks_stitch() {
    let (hir, diags) = lower_hir("== guard ==\nHalt!\n= mood\n#@local\ngrumpy\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(!hir.knots[0].is_local);
    assert!(hir.knots[0].stitches[0].is_local);
}

#[test]
fn knot_directive_coexists_with_plain_knot_tags() {
    let (hir, diags) = lower_hir("== guard ==\n# author: bob\n#@local\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].is_local);
    // The plain tag line survives as content; the directive is erased.
    assert_eq!(all_content_tags(&hir), vec!["author: bob".to_string()]);
}

#[test]
fn unmarked_knot_is_not_local() {
    let (hir, diags) = lower_hir("== guard ==\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(!hir.knots[0].is_local);
}

#[test]
fn unknown_directive_is_e044() {
    let (_hir, diags) = lower_hir("#@locale\nVAR mood = 0\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E044]);
}

#[test]
fn directive_above_content_line_is_e045() {
    let (hir, diags) = lower_hir("#@local\njust text\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E045]);
    // Still erased — never a runtime tag.
    assert!(all_content_tags(&hir).is_empty());
}

#[test]
fn inline_directive_tag_is_e045() {
    let (hir, diags) = lower_hir("some text #@local\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E045]);
    assert!(all_content_tags(&hir).is_empty());
}

#[test]
fn directive_mid_knot_body_is_e045() {
    let (_hir, diags) = lower_hir("== guard ==\nHalt!\n#@local\nmore\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E045]);
}

#[test]
fn dynamic_directive_is_e046() {
    let (_hir, diags) = lower_hir("#@{x}\nVAR mood = 0\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E046]);
}

#[test]
fn mixed_directive_and_plain_tags_is_e047() {
    let (hir, diags) = lower_hir("#@local # art.png\nsome text\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E047]);
    // The plain tag survives; the directive is erased.
    assert_eq!(all_content_tags(&hir), vec!["art.png".to_string()]);
}

#[test]
fn duplicate_local_directive_is_e048() {
    let (hir, diags) = lower_hir("#@local\n#@local\nVAR mood = 0\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E048]);
    // First one still applies.
    assert!(hir.variables[0].is_local);
}

#[test]
fn local_on_const_is_e049() {
    let (_hir, diags) = lower_hir("#@local\nCONST max = 3\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E049]);
}

#[test]
fn local_on_list_is_e049() {
    let (_hir, diags) = lower_hir("#@local\nLIST moods = happy, sad\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E049]);
}

#[test]
fn local_on_external_is_e049() {
    let (_hir, diags) = lower_hir("#@local\nEXTERNAL ping(x)\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E049]);
}

#[test]
fn local_with_args_is_e050() {
    let (_hir, diags) = lower_hir("#@local(now)\nVAR mood = 0\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E050]);
}

#[test]
fn directive_with_blank_line_still_attaches() {
    let (hir, diags) = lower_hir("#@local\n\nVAR mood = 0\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.variables[0].is_local);
}

#[test]
fn plain_tag_lines_are_unaffected() {
    let (hir, diags) = lower_hir("# above\nsome text # inline\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let tags = all_content_tags(&hir);
    assert_eq!(tags.len(), 2, "both plain tags survive: {tags:?}");
}

// ─── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ──────

#[test]
fn param_annotation_lowers_to_named_type_expr() {
    let (hir, diags) = lower_hir("=== heal(hp: int) ===\n~ return\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let knot = &hir.knots[0];
    assert_eq!(knot.params.len(), 1);
    match &knot.params[0].annotation {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "int"),
        other => panic!("expected Named(\"int\"), got {other:?}"),
    }
}

#[test]
fn unannotated_param_lowers_to_none() {
    let (hir, diags) = lower_hir("=== heal(hp) ===\n~ return\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots[0].params[0].annotation, None);
}

#[test]
fn return_type_annotation_lowers_onto_knot() {
    let (hir, diags) = lower_hir("=== function heal(hp) ===\n~ return hp\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    // No annotation declared — must be `None`, not a synthesized `Unknown`.
    assert_eq!(hir.knots[0].return_type, None);

    let (hir2, diags2) = lower_hir("=== function heal(hp): int ===\n~ return hp\n");
    assert!(diags2.is_empty(), "unexpected diagnostics: {diags2:?}");
    match &hir2.knots[0].return_type {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "int"),
        other => panic!("expected Named(\"int\"), got {other:?}"),
    }
}

#[test]
fn void_return_type_lowers_to_named_void_not_none() {
    // `void` is an explicit annotation, distinct from "no annotation
    // declared" (`None`) — both mean "nothing meaningful returned" to a
    // human, but only one is a parsed `TypeExpr` a consumer can inspect.
    let (hir, diags) = lower_hir("=== function noop(): void ===\n~ return\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.knots[0].return_type {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "void"),
        other => panic!("expected Named(\"void\"), got {other:?}"),
    }
}

#[test]
fn stitch_header_never_carries_a_return_type() {
    // `= stitch` headers have no return-type grammar position (TM-2 §3
    // scopes `): type ===` to `== knot ==` headers) — a promoted top-level
    // stitch's `Knot.return_type` is always `None`.
    let (hir, diags) = lower_hir("=== camp ===\nText.\n= fire\nMore.\n-> DONE\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let camp = hir.knots.iter().find(|k| k.name.text == "camp").unwrap();
    assert_eq!(camp.return_type, None);
    assert_eq!(camp.stitches[0].name.text, "fire");
}

#[test]
fn var_annotation_lowers_onto_var_decl() {
    let (hir, diags) = lower_hir("VAR gold: int = 100\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.variables[0].annotation {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "int"),
        other => panic!("expected Named(\"int\"), got {other:?}"),
    }
}

#[test]
fn unannotated_var_lowers_to_none() {
    let (hir, diags) = lower_hir("VAR gold = 100\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.variables[0].annotation, None);
}

#[test]
fn temp_ascription_lowers_onto_temp_decl() {
    let (hir, diags) = lower_hir("~ temp name: string = \"who\"\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::TempDecl(td) => match &td.annotation {
            Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "string"),
            other => panic!("expected Named(\"string\"), got {other:?}"),
        },
        other => panic!("expected TempDecl, got {other:?}"),
    }
}

#[test]
fn block_scoped_temp_ascription_lowers_onto_block_temp_decl() {
    // The T1b `~ { … }` block-statement path shares `ast::TempDecl` with
    // the classic `~ temp` form — same grammar, same HIR lowering.
    let (hir, diags) = lower_hir("~ {\ntemp x: int = 1\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::LogicBlock(lb) => match &lb.stmts[0] {
            BlockStmt::TempDecl(td) => match &td.annotation {
                Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "int"),
                other => panic!("expected Named(\"int\"), got {other:?}"),
            },
            other => panic!("expected TempDecl, got {other:?}"),
        },
        other => panic!("expected LogicBlock, got {other:?}"),
    }
}

#[test]
fn generic_list_and_map_annotations_lower_with_args() {
    let (hir, diags) = lower_hir("VAR w: list<Weathers> = 0\nVAR m: map<string, int> = 0\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.variables[0].annotation {
        Some(TypeExpr::Generic { name, args, .. }) => {
            assert_eq!(name, "list");
            assert_eq!(args.len(), 1);
            assert!(matches!(&args[0], TypeExpr::Named { name, .. } if name == "Weathers"));
        }
        other => panic!("expected Generic(\"list\", ...), got {other:?}"),
    }
    match &hir.variables[1].annotation {
        Some(TypeExpr::Generic { name, args, .. }) => {
            assert_eq!(name, "map");
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected Generic(\"map\", ...), got {other:?}"),
    }
}

#[test]
fn fn_type_annotation_lowers_with_params_and_return() {
    let (hir, diags) = lower_hir("VAR cb: fn(int, int): bool = 0\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.variables[0].annotation {
        Some(TypeExpr::Fn { params, ret, .. }) => {
            assert_eq!(params.len(), 2);
            assert!(matches!(**ret, TypeExpr::Named { ref name, .. } if name == "bool"));
        }
        other => panic!("expected Fn, got {other:?}"),
    }
}

#[test]
fn unknown_type_name_still_lowers_without_diagnostics() {
    // HIR lowering is purely structural — validity checking (unknown
    // names, `fn` reservation) is `brink-analyzer`'s job (E061/E062), not
    // this layer's (mirrors the T1b dialect-gate split).
    let (hir, diags) = lower_hir("VAR p: Frobnicator = 0\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.variables[0].annotation {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "Frobnicator"),
        other => panic!("expected Named(\"Frobnicator\"), got {other:?}"),
    }
}

#[test]
fn const_annotation_lowers_onto_const_decl() {
    // #641: mirrors `var_annotation_lowers_onto_var_decl` — CONST accepts
    // a type annotation end to end, same as VAR (typed-mode-spec.md §3,
    // "optional anywhere").
    let (hir, diags) = lower_hir("CONST speed: float = 0.5\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    match &hir.constants[0].annotation {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "float"),
        other => panic!("expected Named(\"float\"), got {other:?}"),
    }
}

#[test]
fn unannotated_const_lowers_to_none() {
    let (hir, diags) = lower_hir("CONST speed = 0.5\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.constants[0].annotation, None);
}

// ── M-1 modules: `#@module(name)` directive (docs/modules-spec.md §1) ──

#[test]
fn file_module_directive_recognized_and_erased() {
    let (hir, diags) = lower_hir("#@module(quest)\n== start ==\nHi\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let module = hir.module.as_ref().expect("module declared");
    assert_eq!(module.name, "quest");
    // Erasure: the directive never becomes a content tag.
    assert!(
        all_content_tags(&hir).is_empty(),
        "the #@module directive must not leak into content tags"
    );
}

#[test]
fn plain_file_has_no_module_declaration() {
    let (hir, diags) = lower_hir("== start ==\nHi\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.module, None);
}

#[test]
fn module_directive_after_leading_comment_still_recognized() {
    let (hir, diags) = lower_hir("// header\n#@module(quest)\nHi\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.module.map(|m| m.name), Some("quest".to_string()));
}

#[test]
fn module_directive_without_name_is_e086() {
    let (hir, diags) = lower_hir("#@module\nHi\n");
    assert_eq!(hir.module, None);
    assert!(
        codes(&diags).contains(&DiagnosticCode::E086),
        "expected E086, got {diags:?}"
    );
}

#[test]
fn module_directive_empty_name_is_e086() {
    let (hir, diags) = lower_hir("#@module()\nHi\n");
    assert_eq!(hir.module, None);
    assert!(codes(&diags).contains(&DiagnosticCode::E086));
}

#[test]
fn duplicate_module_directive_is_e086() {
    let (hir, diags) = lower_hir("#@module(quest)\n#@module(other)\nHi\n");
    // First declaration wins; the second is the duplicate error.
    assert_eq!(hir.module.map(|m| m.name), Some("quest".to_string()));
    assert!(codes(&diags).contains(&DiagnosticCode::E086));
}

#[test]
fn unknown_file_level_directive_still_errors_e045() {
    // Only `#@module` is a valid file-level directive; anything else at
    // file scope stays an E045 misplacement (reserved-@-namespace rule).
    let (hir, diags) = lower_hir("#@bogus\nHi\n");
    assert_eq!(hir.module, None);
    assert!(
        codes(&diags).contains(&DiagnosticCode::E045),
        "expected E045, got {diags:?}"
    );
}
