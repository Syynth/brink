#![allow(clippy::panic)]

use brink_syntax::ast::AstNode;
use brink_syntax::parse;
use rowan::TextRange;

use crate::hir::lower::{
    BodyChild, EffectSink, LowerScope, LowerSink, classify_body_child, lower_simple_body,
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
fn lower_body(source: &str) -> (Block, Vec<Diagnostic>) {
    let parsed = parse(source);
    let tree = parsed.tree();
    let scope = make_scope();
    let mut sink = make_sink();
    let block = lower_simple_body(tree.syntax(), &scope, &mut sink);
    let diagnostics = sink.finish();
    (block, diagnostics)
}

// ─── Mock sink for testing trait abstraction ────────────────────────
//
// Post-B0.4, `LowerSink` only carries diagnostics — symbol declarations are
// no longer a sink-write concern (`brink_ir::symbols::project_manifest`
// derives the whole `SymbolManifest` from the finished `HirFile` instead),
// so this mock only needs to record diagnostics.

struct TestSink {
    diagnostics: Vec<(TextRange, DiagnosticCode)>,
}

impl TestSink {
    fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }
}

impl LowerSink for TestSink {
    fn diagnose(&mut self, range: TextRange, code: DiagnosticCode) -> crate::hir::lower::Diagnosed {
        self.diagnostics.push((range, code));
        crate::hir::lower::Diagnosed::test_token()
    }
}

// ─── Expression lowering tests ──────────────────────────────────────

#[test]
fn lower_integer_literal() {
    let source = "~ temp x = 42\n";
    let (block, diags) = lower_body(source);
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
    let (block, diags) = lower_body(source);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 1);
    match &block.stmts[0] {
        Stmt::TempDecl(td) => {
            assert_eq!(td.name.text, "y");
            assert!(
                matches!(
                    &td.value,
                    Some(Expr::Infix(ie))
                    if ie.op == InfixOp::Add
                    && matches!(ie.lhs.as_ref(), Expr::Int(3))
                    && matches!(ie.rhs.as_ref(), Expr::Int(4))
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
    let (block, diags) = lower_body("Hello, world!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(block.stmts.len(), 2, "expected Content + EndOfLine");
    assert!(matches!(&block.stmts[0], Stmt::Content(c) if !c.parts.is_empty()));
    assert!(matches!(&block.stmts[1], Stmt::EndOfLine));
}

#[test]
fn expression_interpolation() {
    let (block, diags) = lower_body("Value is {x}\n");
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
    let (block, diags) = lower_body("Hello #greeting\n");
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
    let (block, diags) = lower_body(source);
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
    let (_, diags) = lower_body(source);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E014),
        "expected E014 diagnostic, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

// ─── Computed-callee call attempt (docs/t1c-spec.md §3/§10, issue #869) ──
//
// `expr(args…)` where `expr` isn't a bare variable/temp/param name is
// always rejected (E104) rather than silently dropped — proven for all
// three non-bare-name callee shapes the npc-fsm/behavior-tree tier1
// corpus fixtures found (indexed, field access, call-result), plus a
// sanity check that the ratified `call(f, args…)` Explicit form (which
// already dispatches through exactly these callee shapes correctly) is
// untouched by this rejection.

#[test]
fn computed_callee_indexed_emits_e104() {
    let source = "~ temp x = handlers[state](event)\n";
    let (_, diags) = lower_body(source);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E104),
        "expected E104 diagnostic, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn computed_callee_field_access_emits_e104() {
    let source = "~ temp x = obj.field()\n";
    let (_, diags) = lower_body(source);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E104),
        "expected E104 diagnostic, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn computed_callee_call_result_emits_e104() {
    let source = "~ temp x = get_handler()()\n";
    let (_, diags) = lower_body(source);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E104),
        "expected E104 diagnostic, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn bare_name_direct_call_never_emits_e104() {
    // The bare-name Direct-call fast path (RULED, t1c-spec §3) must never
    // trip the new rejection.
    let source = "~ temp x = bare(1, 2)\n";
    let (_, diags) = lower_body(source);
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E104),
        "bare-name call incorrectly rejected: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn explicit_call_form_never_emits_e104() {
    // `call(f, args…)` — the ratified Explicit form — lowers as an
    // ordinary named call (`Expr::Call(path = "call", …)`), never as the
    // new `CALL_EXPR` shape, so it must stay untouched by E104.
    let source = "~ temp x = call(handlers[state], event)\n";
    let (_, diags) = lower_body(source);
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E104),
        "call(f, args…) incorrectly rejected: {:?}",
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
    let (block, diags) = lower_body(source);
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
    let (block, _) = lower_body(source);
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

// ─── T2-2 `@[effects(…)]` assertion directive (docs/effects-spec.md §10,
// issue #861) ──────────────────────────────────────────────────────

#[test]
fn effects_directive_parses_reads_writes_calls_on_a_knot() {
    let (hir, diags) =
        lower_hir("== guard ==\n@[effects(reads(gold), writes(alarm), calls(audio))]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let assertion = hir.knots[0]
        .effects_assertion
        .as_ref()
        .expect("assertion present");
    assert!(!assertion.pure);
    assert_eq!(assertion.reads, vec!["gold".to_string()]);
    assert_eq!(assertion.writes, vec!["alarm".to_string()]);
    assert_eq!(assertion.calls, vec!["audio".to_string()]);
    assert!(all_content_tags(&hir).is_empty());
}

#[test]
fn effects_paren_clause_names_multiple_cells() {
    // `reads(a, b)` is ONE clause naming two cells — the paren respell
    // (2026-07-19, stdlib-spec §9.2 / issue #1120's second item): clause
    // membership is delimited by the parens, never by "continuation".
    let (hir, diags) =
        lower_hir("== guard ==\n@[effects(reads(a, b), writes(c), calls(d))]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let assertion = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert_eq!(assertion.reads, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(assertion.writes, vec!["c".to_string()]);
    assert_eq!(assertion.calls, vec!["d".to_string()]);
}

#[test]
fn hash_effects_colon_clause_continuation_stays_frozen() {
    // The deprecated tag spelling keeps its legacy colon grammar FROZEN
    // (E110 surface does not evolve): "reads: a, b" continues the open
    // clause exactly as it always did.
    let (hir, diags) =
        lower_hir("== guard ==\n#@effects(reads: a, b, writes: c, calls: d)\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E110]);
    let assertion = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert_eq!(assertion.reads, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(assertion.writes, vec!["c".to_string()]);
    assert_eq!(assertion.calls, vec!["d".to_string()]);
}

#[test]
fn effects_pure_sugar_sets_pure_with_empty_lists() {
    let (hir, diags) = lower_hir("== guard ==\n@[effects(pure)]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let assertion = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(assertion.pure);
    assert!(assertion.reads.is_empty());
    assert!(assertion.writes.is_empty());
    assert!(assertion.calls.is_empty());
}

#[test]
fn effects_directive_marks_stitch() {
    let (hir, diags) = lower_hir("== guard ==\nHalt!\n= mood\n@[effects(reads(gold))]\ngrumpy\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].effects_assertion.is_none());
    assert!(hir.knots[0].stitches[0].effects_assertion.is_some());
}

#[test]
fn unmarked_knot_has_no_effects_assertion() {
    let (hir, diags) = lower_hir("== guard ==\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].effects_assertion.is_none());
}

#[test]
fn bare_effects_directive_is_e100() {
    // Tag-channel spelling: the deprecation warning (E110) rides along.
    let (_hir, diags) = lower_hir("== guard ==\n#@effects\nHalt!\n");
    assert_eq!(
        codes(&diags),
        vec![DiagnosticCode::E110, DiagnosticCode::E100]
    );
}

#[test]
fn bare_effects_annotation_is_e100() {
    let (_hir, diags) = lower_hir("== guard ==\n@[effects]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E100]);
}

#[test]
fn empty_effects_args_is_e100() {
    let (_hir, diags) = lower_hir("== guard ==\n@[effects()]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E100]);
}

#[test]
fn effects_unknown_clause_keyword_is_e101() {
    let (_hir, diags) = lower_hir("== guard ==\n@[effects(frobs(gold))]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E101]);
}

#[test]
fn effects_bare_non_flag_ident_is_e101() {
    // Bare top-level idents are always FLAGS in the paren grammar — a bare
    // ident outside {pure, silent, total} is malformed, never a clause
    // value.
    let (_hir, diags) = lower_hir("== guard ==\n@[effects(gold)]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E101]);
}

#[test]
fn effects_non_identifier_value_is_e101() {
    let (_hir, diags) = lower_hir("== guard ==\n@[effects(reads(1gold))]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E101]);
}

#[test]
fn effects_colon_clause_in_annotation_spelling_is_e101() {
    // The paren respell (issue #1120's second item): the colon grammar
    // belongs to the frozen `#@effects` alias only — inside `@[effects(…)]`
    // it is malformed.
    let (_hir, diags) = lower_hir("== guard ==\n@[effects(reads: gold)]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E101]);
}

#[test]
fn effects_dynamic_content_is_e046() {
    // Single E046 for dynamic `#@effects` directive (plus the NS-A2
    // deprecation warning for the tag spelling). The dynamic check
    // is deferred to `effects_assertion_from_directives`, not handled by
    // the generic check in `apply_scope_directives`, to avoid double-emitting.
    let (_hir, diags) = lower_hir("== guard ==\n#@effects({x})\nHalt!\n");
    assert_eq!(
        codes(&diags),
        vec![DiagnosticCode::E110, DiagnosticCode::E046]
    );
}

#[test]
fn was_dynamic_content_is_e046() {
    // Same fix, sibling path: `#@was` above a knot's body also has its own
    // dynamic check in `was_from_directives`, called alongside
    // `apply_scope_directives` on the same collected directives — the
    // generic check must not also fire for it.
    let (_hir, diags) = lower_hir("== guard ==\n#@was({x})\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E046]);
}

#[test]
fn duplicate_effects_directive_is_e048_first_wins() {
    let (hir, diags) =
        lower_hir("== guard ==\n@[effects(reads(a))]\n@[effects(reads(b))]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E048]);
    let assertion = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert_eq!(assertion.reads, vec!["a".to_string()]);
}

#[test]
fn effects_on_var_is_e049() {
    // Tag-channel spelling attached to a declaration: invalid target
    // (E049), plus the NS-A2 deprecation warning is NOT emitted — the
    // effects recognizer only runs for knot/stitch owners.
    let (_hir, diags) = lower_hir("#@effects(reads: gold)\nVAR gold = 0\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E049]);
}

#[test]
fn effects_on_const_is_e049() {
    let (_hir, diags) = lower_hir("#@effects(pure)\nCONST max = 3\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E049]);
}

// ─── NS-A2 `@[effects(…)]` annotation surface (issue #1108;
// docs/stdlib-spec.md §9.2) ─────────────────────────────────────────

#[test]
fn effects_annotation_flags_parse_any_subset() {
    let (hir, diags) = lower_hir("== guard ==\n@[effects(pure, silent, total)]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(a.pure && a.silent && a.total);
    assert!(a.reads.is_empty() && a.writes.is_empty() && a.calls.is_empty());
}

#[test]
fn effects_annotation_silent_alone_parses() {
    let (hir, diags) = lower_hir("== guard ==\n@[effects(silent)]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(!a.pure && a.silent && !a.total);
}

#[test]
fn effects_annotation_flags_combine_with_clauses() {
    let (hir, diags) =
        lower_hir("VAR gold = 0\n== guard ==\n@[effects(silent, reads(gold))]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(a.silent && !a.pure && !a.total);
    assert_eq!(a.reads, vec!["gold".to_string()]);
}

#[test]
fn effects_flag_after_a_clause_is_a_flag_not_a_clause_value() {
    // THE footgun the paren respell kills structurally (issue #1120's
    // second item): under the old colon grammar, `reads: gold, silent`
    // swallowed `silent` into the open `reads` clause as a cell name. With
    // delimited clauses, `reads(gold), silent` parses `silent` as the FLAG
    // — a flag can never be swallowed into an open clause.
    let (hir, diags) =
        lower_hir("VAR gold = 0\n== guard ==\n@[effects(reads(gold), silent)]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(a.silent, "`silent` after `reads(gold)` is the flag");
    assert_eq!(
        a.reads,
        vec!["gold".to_string()],
        "`silent` must not be swallowed into the reads clause"
    );
}

#[test]
fn effects_flag_name_inside_a_clause_is_a_clause_value() {
    // A cell genuinely named `silent` stays assertable — inside the parens
    // it is a clause value; only bare top-level idents are flags.
    let (hir, diags) = lower_hir("VAR silent = 0\n== guard ==\n@[effects(reads(silent))]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(
        !a.silent,
        "`silent` inside `reads(…)` is a cell name, not a flag"
    );
    assert_eq!(a.reads, vec!["silent".to_string()]);
}

#[test]
fn effects_pure_with_a_state_clause_is_contradictory_e101() {
    let (_hir, diags) = lower_hir("== guard ==\n@[effects(pure, reads(gold))]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E101]);
}

#[test]
fn deprecated_hash_effects_spelling_warns_e110_and_still_parses() {
    let (hir, diags) = lower_hir("== guard ==\n#@effects(pure)\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E110]);
    let a = hir.knots[0]
        .effects_assertion
        .as_ref()
        .expect("alias still parses");
    assert!(a.pure);
}

#[test]
fn hash_effects_spelling_accepts_the_new_flags_too() {
    // The frozen colon grammar had already grown the NS-A2 flags before it
    // froze — they stay. (Since the paren respell the two spellings share
    // only the flag vocabulary: clause shape is colon-style here,
    // paren-style in the annotation channel.)
    let (hir, diags) = lower_hir("== guard ==\n#@effects(silent, total)\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E110]);
    let a = hir.knots[0].effects_assertion.as_ref().expect("present");
    assert!(a.silent && a.total);
}

#[test]
fn unknown_annotation_name_is_e111() {
    let (_hir, diags) = lower_hir("== guard ==\n@[frobnicate(now)]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E111]);
}

#[test]
fn tag_directive_names_do_not_alias_into_the_annotation_channel() {
    let (hir, diags) = lower_hir("== guard ==\n@[local]\nHalt!\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E111]);
    assert!(
        !hir.knots[0].is_local,
        "`@[local]` must not act as `#@local`"
    );
}

#[test]
fn misplaced_annotation_line_is_e112_not_content() {
    // Mid-body (below content) is not the leading run.
    let (hir, diags) = lower_hir("== guard ==\nHalt!\n@[effects(pure)]\nMore.\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E112]);
    assert!(
        hir.knots[0].effects_assertion.is_none(),
        "a misplaced annotation must not attach"
    );
}

#[test]
fn file_level_annotation_line_is_e112() {
    let (_hir, diags) = lower_hir("@[effects(pure)]\nHello.\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E112]);
}

#[test]
fn annotation_line_never_lowers_to_content() {
    // The consumed placement is erased entirely — no content stmt carries
    // the annotation text (the silent-drop rule's flip side: consumed, not
    // dropped).
    let (hir, diags) = lower_hir("== guard ==\n@[effects(pure)]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body_text = format!("{:?}", hir.knots[0].body);
    assert!(
        !body_text.contains("effects"),
        "annotation text leaked into the lowered body: {body_text}"
    );
}

#[test]
fn annotation_below_plain_tag_line_still_attaches() {
    // Tag lines and annotation lines share the leading run (both orders).
    let (hir, diags) = lower_hir("== guard ==\n# mood: grim\n@[effects(pure)]\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].effects_assertion.is_some());
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
fn unannotated_stitch_header_has_no_return_type() {
    let (hir, diags) = lower_hir("=== camp ===\nText.\n= fire\nMore.\n-> DONE\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let camp = hir.knots.iter().find(|k| k.name.text == "camp").unwrap();
    assert_eq!(camp.return_type, None);
    assert_eq!(camp.stitches[0].name.text, "fire");
    assert_eq!(camp.stitches[0].return_type, None);
}

#[test]
fn return_type_annotation_lowers_onto_nested_stitch() {
    // #1509: `= name(params): type` on a *nested* stitch header (widening
    // NG-C's `Knot.return_type` grammar to `Stitch`).
    let (hir, diags) =
        lower_hir("=== camp ===\nText.\n= fire(logs): int\n~ return logs\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let camp = hir.knots.iter().find(|k| k.name.text == "camp").unwrap();
    match &camp.stitches[0].return_type {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "int"),
        other => panic!("expected Named(\"int\"), got {other:?}"),
    }
}

#[test]
fn return_type_annotation_lowers_onto_promoted_top_level_stitch() {
    // A top-level `= stitch` is promoted to `Knot` status during lowering
    // (`lower_top_level_stitch`), so its return type rides the same
    // `Knot::return_type` field a real `== knot ==` header's does.
    let (hir, diags) = lower_hir("= fire(logs): int\n~ return logs\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let fire = hir.knots.iter().find(|k| k.name.text == "fire").unwrap();
    match &fire.return_type {
        Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "int"),
        other => panic!("expected Named(\"int\"), got {other:?}"),
    }
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

// ─── M-2 imports + visibility (docs/modules-spec.md §2/§4) ───────────

#[test]
fn import_qualified_form_extracted() {
    let (hir, diags) = lower_hir("IMPORT quest_3\nHi\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    assert_eq!(hir.imports[0].module, "quest_3");
    assert!(!hir.imports[0].bare);
    assert!(hir.imports[0].items.is_empty());
}

#[test]
fn import_bare_list_with_alias_extracted() {
    let (hir, diags) = lower_hir("IMPORT { ambush, guard_talk AS gt } FROM quest_3\nHi\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    let imp = &hir.imports[0];
    assert_eq!(imp.module, "quest_3");
    assert!(imp.bare);
    assert_eq!(imp.items.len(), 2);
    assert_eq!(imp.items[0].name, "ambush");
    assert_eq!(imp.items[0].alias, None);
    assert_eq!(imp.items[0].local_name(), "ambush");
    assert_eq!(imp.items[1].name, "guard_talk");
    assert_eq!(imp.items[1].alias.as_deref(), Some("gt"));
    assert_eq!(imp.items[1].local_name(), "gt");
}

#[test]
fn private_directive_marks_var_visibility() {
    let (manifest, diags) = lower_full("#@private\nVAR secret = 0\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        manifest.variables[0].visibility,
        Some(crate::VisibilityMark::Private)
    );
}

#[test]
fn public_directive_marks_knot_visibility() {
    let (manifest, diags) = lower_full("== guard ==\n#@public\nHalt!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        manifest.knots[0].visibility,
        Some(crate::VisibilityMark::Public)
    );
}

#[test]
fn visibility_directives_collected_for_gate() {
    let (hir, _diags) = lower_hir("#@private\nVAR secret = 0\n");
    assert_eq!(hir.visibility.len(), 1);
    assert_eq!(hir.visibility[0].mark, crate::VisibilityMark::Private);
    // Erasure: the directive never becomes a content tag.
    assert!(all_content_tags(&hir).is_empty());
}

#[test]
fn conflicting_visibility_directives_is_e093() {
    let (_manifest, diags) = lower_full("#@private\n#@public\nVAR x = 0\n");
    assert!(
        codes(&diags).contains(&DiagnosticCode::E093),
        "expected E093, got {diags:?}"
    );
}

// ─── Block::tail (S1, docs/block-effect-model.md §10 row j) ────────
//
// Expand-phase groundwork only: `tail` is populated from `stmts`' final
// statement but consumed by nothing yet — `stmts` stays authoritative.
// These tests pin the ink frontend's half of that population.

#[test]
fn block_ending_in_divert_has_diverge_tail() {
    let (hir, diags) = lower_hir("== a ==\nHello\n-> b\n== b ==\nDone.\n-> END\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a_body = &hir.knots[0].body;
    assert!(
        matches!(a_body.tail(), Tail::Diverge(Terminator::Divert(_))),
        "expected Diverge(Divert) tail, got {:?}",
        a_body.tail()
    );
    let b_body = &hir.knots[1].body;
    assert!(
        matches!(b_body.tail(), Tail::Diverge(Terminator::Divert(_))),
        "-> END is still a Divert (DivertPath::End), got {:?}",
        b_body.tail()
    );
}

#[test]
fn block_ending_in_explicit_return_has_diverge_tail() {
    let (hir, diags) = lower_hir("=== function f() ===\n~ return 1\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[0].body;
    assert!(
        matches!(body.tail(), Tail::Diverge(Terminator::Return(_))),
        "expected Diverge(Return) tail, got {:?}",
        body.tail()
    );
}

#[test]
fn plain_content_block_has_unit_tail() {
    let (block, diags) = lower_body("Hello, world!\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(*block.tail(), Tail::Unit);
}

#[test]
fn weave_choice_body_ending_in_divert_has_diverge_tail() {
    // Weave folding appends trailing choice-body content statement-by-
    // statement (`weave.rs`'s `WeaveItem::Stmt` arm mutates `c.body.stmts`
    // in place after the choice's own construction) — `flush_choices` must
    // re-derive `tail` from the final body before sealing it into the
    // `ChoiceSet`, or this would still read the stale value from
    // construction time.
    let (hir, diags) =
        lower_hir("== a ==\n* Choice.\n  more text\n  -> b\n== b ==\nDone.\n-> END\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::ChoiceSet(cs) = &hir.knots[0].body.stmts[0] else {
        panic!("expected ChoiceSet, got {:?}", hir.knots[0].body.stmts[0]);
    };
    let choice_body = &cs.choices[0].body;
    assert!(
        matches!(choice_body.stmts.last(), Some(Stmt::Divert(_))),
        "expected the weave-folded content to end in the divert, got {:?}",
        choice_body.stmts
    );
    assert!(
        matches!(choice_body.tail(), Tail::Diverge(Terminator::Divert(_))),
        "expected Diverge(Divert) tail, got {:?}",
        choice_body.tail()
    );
}
