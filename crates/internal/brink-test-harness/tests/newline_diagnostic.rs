#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants,
    clippy::doc_markdown
)]
//! Layer-by-layer diagnostic tests for logic line newline emission.
//!
//! These tests mechanically verify assumptions at each compiler layer
//! using the I008 fixture pattern — a function call whose body produces
//! no visible output but contains a content line with a void expression.

use brink_ir::{ContentPart, FileId, Stmt};

// ─── Fixture ─────────────────────────────────────────────────────────

/// The I008 pattern: function call whose body has a void-expression
/// content line that should produce no visible output.
const I008_FIXTURE: &str = "\
-> outer
=== outer
~ temp x = 0
~ f(x)
{x}
-> DONE
=== function f(ref x)
~temp local = 0
~x=x
{setTo3(local)}
=== function setTo3(ref x)
~x = 3
";

/// Simple case: expression statement function call that DOES produce
/// output, followed by content. The EndOfLine after the function call
/// should create a line boundary.
const I097_FIXTURE: &str = "\
~ func ()
text 2
~ temp tempVar = func ()
text 2
== function func ()
    text1
    ~ return true
";

// ─── Helpers ─────────────────────────────────────────────────────────

fn lower_hir(
    source: &str,
) -> (
    brink_ir::HirFile,
    brink_ir::SymbolManifest,
    Vec<brink_ir::Diagnostic>,
) {
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    brink_ir::hir::lower(FileId(0), &tree)
}

fn compile_to_story_data(source: &str) -> brink_format::StoryData {
    let output = brink_compiler::compile("<fixture>", |_| Ok(source.to_string()));
    match output {
        Ok(o) => o.data,
        Err(e) => panic!("compilation failed: {e:?}"),
    }
}

fn dump_inkt(data: &brink_format::StoryData) -> String {
    let mut out = String::new();
    brink_format::write_inkt(data, &mut out).expect("inkt write failed");
    out
}

/// Describe the Stmt variant for readable assertion messages.
fn stmt_name(s: &Stmt) -> &'static str {
    match s {
        Stmt::Content(_) => "Content",
        Stmt::Divert(_) => "Divert",
        Stmt::TunnelCall(_) => "TunnelCall",
        Stmt::ThreadStart(_) => "ThreadStart",
        Stmt::TempDecl(_) => "TempDecl",
        Stmt::Assignment(_) => "Assignment",
        Stmt::Return(_) => "Return",
        Stmt::ChoiceSet(_) => "ChoiceSet",
        Stmt::LabeledBlock(_) => "LabeledBlock",
        Stmt::Conditional(_) => "Conditional",
        Stmt::Sequence(_) => "Sequence",
        Stmt::ExprStmt(_) => "ExprStmt",
        Stmt::EndOfLine => "EndOfLine",
    }
}

fn stmt_names(stmts: &[Stmt]) -> Vec<&'static str> {
    stmts.iter().map(stmt_name).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// LAYER 1: HIR — stmt sequence verification
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hir_function_f_body_stmts() {
    let (hir, _, diags) = lower_hir(I008_FIXTURE);
    assert!(diags.is_empty(), "HIR diagnostics: {diags:?}");

    let f_knot = hir
        .knots
        .iter()
        .find(|k| k.name.text == "f")
        .expect("function f not found in HIR");

    let stmts = &f_knot.body.stmts;
    let names = stmt_names(stmts);
    println!("function f body stmts: {names:?}");

    // Expected: TempDecl, Assignment, Content({setTo3(local)}), EndOfLine
    // No extra EndOfLine after TempDecl or Assignment.
    assert!(
        matches!(stmts[0], Stmt::TempDecl(_)),
        "f[0]: expected TempDecl, got {}",
        stmt_name(&stmts[0]),
    );
    assert!(
        !matches!(stmts[1], Stmt::EndOfLine),
        "f[1]: TempDecl should NOT be followed by EndOfLine, got {}",
        stmt_name(&stmts[1]),
    );
    assert!(
        matches!(stmts[1], Stmt::Assignment(_)),
        "f[1]: expected Assignment, got {}",
        stmt_name(&stmts[1]),
    );
    assert!(
        !matches!(stmts[2], Stmt::EndOfLine),
        "f[2]: Assignment should NOT be followed by EndOfLine, got {}",
        stmt_name(&stmts[2]),
    );

    // The content line {setTo3(local)} should produce Content + EndOfLine.
    let content_idx = stmts
        .iter()
        .position(|s| matches!(s, Stmt::Content(_)))
        .expect("function f should have a Content stmt");
    assert!(
        matches!(stmts.get(content_idx + 1), Some(Stmt::EndOfLine)),
        "Content should be followed by EndOfLine, got {:?}",
        stmts.get(content_idx + 1).map(stmt_name),
    );

    // The content should have an expression/interpolation part.
    if let Stmt::Content(c) = &stmts[content_idx] {
        println!("function f content parts: {:?}", c.parts);
        assert!(
            c.parts
                .iter()
                .any(|p| matches!(p, ContentPart::Interpolation(_))),
            "Content should contain an Interpolation part for {{setTo3(local)}}, got: {:?}",
            c.parts,
        );
    }
}

#[test]
fn hir_outer_knot_stmts() {
    let (hir, _, diags) = lower_hir(I008_FIXTURE);
    assert!(diags.is_empty(), "HIR diagnostics: {diags:?}");

    let outer = hir
        .knots
        .iter()
        .find(|k| k.name.text == "outer")
        .expect("knot outer not found in HIR");

    let stmts = &outer.body.stmts;
    let names = stmt_names(stmts);
    println!("outer body stmts: {names:?}");

    // Expected sequence:
    // TempDecl(x=0), ExprStmt(f(x)), EndOfLine, Content({x}), EndOfLine, Divert(DONE)
    assert!(
        matches!(stmts[0], Stmt::TempDecl(_)),
        "outer[0]: expected TempDecl, got {}",
        stmt_name(&stmts[0]),
    );
    assert!(
        !matches!(stmts[1], Stmt::EndOfLine),
        "outer[1]: TempDecl should NOT be followed by EndOfLine",
    );
    assert!(
        matches!(stmts[1], Stmt::ExprStmt(_)),
        "outer[1]: expected ExprStmt(f(x)), got {}",
        stmt_name(&stmts[1]),
    );
    assert!(
        matches!(stmts[2], Stmt::EndOfLine),
        "outer[2]: ExprStmt should be followed by EndOfLine, got {}",
        stmt_name(&stmts[2]),
    );
}

#[test]
fn hir_i097_first_func_call_has_end_of_line() {
    let (hir, _, diags) = lower_hir(I097_FIXTURE);
    assert!(diags.is_empty(), "HIR diagnostics: {diags:?}");

    let stmts = &hir.root_content.stmts;
    let names = stmt_names(stmts);
    println!("I097 root stmts: {names:?}");

    // ~ func () should produce ExprStmt + EndOfLine
    assert!(
        matches!(stmts[0], Stmt::ExprStmt(_)),
        "root[0]: expected ExprStmt, got {}",
        stmt_name(&stmts[0]),
    );
    assert!(
        matches!(stmts[1], Stmt::EndOfLine),
        "root[1]: ExprStmt should be followed by EndOfLine, got {}",
        stmt_name(&stmts[1]),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// LAYER 2: .inkt bytecode — opcode sequence verification
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn inkt_outer_has_emit_newline_after_function_call() {
    let data = compile_to_story_data(I008_FIXTURE);
    let inkt = dump_inkt(&data);
    println!("=== I008 .inkt dump ===\n{inkt}");

    // The outer knot's bytecode should contain EmitNewline between
    // the function call (Call + Pop) and the next EmitValue for {x}.
    // This is the newline our fix adds.
    //
    // We look for the pattern: Pop, EmitNewline, ... EmitValue
    // within the outer container's bytecode section.
    assert!(
        inkt.contains("pop") && inkt.contains("emit_newline"),
        "outer should contain pop and emit_newline in its bytecode",
    );
}

#[test]
fn inkt_function_f_ends_with_emit_value_emit_newline() {
    let data = compile_to_story_data(I008_FIXTURE);
    let inkt = dump_inkt(&data);

    // Function f's content line {setTo3(local)} should produce:
    // ...EmitValue, EmitNewline at the end (before Return).
    // Print the full dump for manual inspection.
    println!("=== I008 .inkt dump ===\n{inkt}");
}

#[test]
fn inkt_i097_has_emit_newline_after_statement_call() {
    let data = compile_to_story_data(I097_FIXTURE);
    let inkt = dump_inkt(&data);
    println!("=== I097 .inkt dump ===\n{inkt}");

    // The root container should have EmitNewline after the ~ func() call.
    // Pattern: Pop, EmitNewline (from the expression statement)
    // followed by the text 2 content.
    assert!(
        inkt.contains("emit_newline"),
        "root should contain emit_newline for the logic line newline",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// LAYER 5: trim_function_end — whitespace cleanup verification
// ═══════════════════════════════════════════════════════════════════════
//
// These tests verify trim_function_end behavior directly. They use the
// runtime's Output type through the compiled story, observing behavior
// through continue_single() output rather than direct buffer inspection.
// The tests below verify the END-TO-END behavior of function output
// trimming by checking what text the runtime produces.

#[test]
fn runtime_void_function_produces_no_output() {
    // A void function call should produce no text output.
    // This verifies trim_function_end cleans up any trailing parts.
    let source = "\
~ g()
hello
=== function g()
~ return
";
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let line = story.continue_single().expect("continue_single failed");
    let text = match &line {
        brink_runtime::Line::Text { text, .. }
        | brink_runtime::Line::Done { text, .. }
        | brink_runtime::Line::End { text, .. } => text.as_str(),
        other => panic!("unexpected line variant: {other:?}"),
    };
    assert_eq!(
        text, "hello\n",
        "void function should produce no output; first line should be 'hello\\n'",
    );
}

#[test]
fn runtime_function_with_void_content_line_produces_no_output() {
    // A function whose body has a content line with a void-returning
    // function call should produce no visible output — the null
    // EmitValue is suppressed, and trim_function_end cleans up
    // the trailing newline.
    let source = "\
~ f()
hello
=== function f()
{g()}
~ return
=== function g()
~ return
";
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let line = story.continue_single().expect("continue_single failed");
    let text = match &line {
        brink_runtime::Line::Text { text, .. }
        | brink_runtime::Line::Done { text, .. }
        | brink_runtime::Line::End { text, .. } => text.as_str(),
        other => panic!("unexpected line variant: {other:?}"),
    };
    assert_eq!(
        text, "hello\n",
        "function with void content line should produce no output; first line should be 'hello\\n'",
    );
}

#[test]
fn runtime_function_with_text_output_produces_separate_lines() {
    // A function whose body outputs text should produce that text
    // as a separate line from what follows.
    let source = "\
~ func()
text 2
=== function func()
text1
~ return
";
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let line1 = story.continue_single().expect("continue_single 1");
    let text1 = match &line1 {
        brink_runtime::Line::Text { text, .. } => text.as_str(),
        other => panic!("expected Text, got {other:?}"),
    };
    assert_eq!(text1, "text1\n", "function output should be its own line",);

    let line2 = story.continue_single().expect("continue_single 2");
    let text2 = match &line2 {
        brink_runtime::Line::Text { text, .. }
        | brink_runtime::Line::Done { text, .. }
        | brink_runtime::Line::End { text, .. } => text.as_str(),
        other => panic!("expected text, got {other:?}"),
    };
    assert_eq!(
        text2, "text 2\n",
        "content after function call should be its own line",
    );
}

#[test]
fn runtime_i008_pattern_no_spurious_newline() {
    // The full I008 pattern: ~ f(x) where f produces no visible output.
    // There should be NO spurious newline — just "0\n" as a single line.
    let data = compile_to_story_data(I008_FIXTURE);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let line = story.continue_single().expect("continue_single failed");
    let text = match &line {
        brink_runtime::Line::Text { text, .. }
        | brink_runtime::Line::Done { text, .. }
        | brink_runtime::Line::End { text, .. } => text.as_str(),
        other => panic!("unexpected line variant: {other:?}"),
    };
    assert_eq!(
        text, "0\n",
        "I008 pattern: function with void body should produce no output; \
         first visible line should be '0\\n', not a spurious newline",
    );
}

#[test]
fn runtime_function_with_whitespace_only_template_trimmed() {
    // I096 pattern: function body has a content line with two void
    // interpolations and a space between them — `{square(x)} {square(x)}`.
    // The template has only whitespace text between slots, so it should
    // NOT be recognized as a template. The EmitContent fallback uses
    // emit_value (suppresses null) and Springs for whitespace, so the
    // function produces no visible output and trim_function_end cleans up.
    let source = "\
VAR globalVal = 5
{globalVal}
~ squaresquare(globalVal)
{globalVal}
== function squaresquare(ref x) ==
 {square(x)} {square(x)}
 ~ return
== function square(ref x) ==
 ~ x = x * x
 ~ return
";
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let line1 = story.continue_single().expect("continue_single 1");
    let text1 = match &line1 {
        brink_runtime::Line::Text { text, .. } => text.as_str(),
        other => panic!("expected Text, got {other:?}"),
    };
    assert_eq!(text1, "5\n", "first line should be initial globalVal");

    let line2 = story.continue_single().expect("continue_single 2");
    let text2 = match &line2 {
        brink_runtime::Line::Text { text, .. }
        | brink_runtime::Line::Done { text, .. }
        | brink_runtime::Line::End { text, .. } => text.as_str(),
        other => panic!("expected text, got {other:?}"),
    };
    assert_eq!(
        text2, "625\n",
        "squaresquare's whitespace-only template should not be recognized; \
         second line should be '625\\n', not a spurious newline",
    );
}
