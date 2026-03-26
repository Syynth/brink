#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::wildcard_enum_match_arm,
    clippy::match_wildcard_for_single_variants,
    clippy::match_same_arms,
    clippy::too_many_lines,
    clippy::doc_markdown,
    unused_variables
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

// ═══════════════════════════════════════════════════════════════════════
// TheIntercept: glue inside conditional after gather
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn runtime_glue_inside_conditional_after_gather() {
    // TheIntercept pattern: after a choice, a gather has a conditional
    // block that starts with `<>` (glue) to append to the previous line.
    // The glue must connect through the conditional.
    let source = r#"VAR teacup = true
* [Agree]
    "Awkward," I reply
* [Disagree]
    "I don't see why," I reply
- { teacup:
    <>, sipping at my tea
  }
  <>.
- done
"#;
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    // First continue should present choices.
    let line = story.continue_single().expect("continue_single");
    match &line {
        brink_runtime::Line::Choices { .. } => story.choose(0).expect("choose"),
        other => panic!("expected Choices, got {other:?}"),
    }

    // After choosing "Agree", the output should be the reply + glued conditional + period.
    let line = story
        .continue_single()
        .expect("continue_single after choice");
    let text = match &line {
        brink_runtime::Line::Text { text, .. } | brink_runtime::Line::Done { text, .. } => {
            text.as_str()
        }
        other => panic!("expected text, got {other:?}"),
    };
    assert_eq!(
        text, "\"Awkward,\" I reply, sipping at my tea.\n",
        "glue inside conditional after gather should append to previous line",
    );
}

#[test]
fn runtime_intercept_glue_conditional_faithful() {
    // Closer reproduction of TheIntercept structure: nested choices,
    // multiple gathers, then the glue+conditional at the outer gather.
    let source = r#"VAR teacup = false
* [Take cup]
    ~ teacup = true
    I take a mug.
* [Leave it]
    I leave it.
- "Quite a difficult situation." "I'm sure you agree."
    * [Agree]
        "Awkward," I reply
    * [Disagree]
        "I don't see why," I reply
- { teacup:
    <>, sipping at my tea as though we were old friends
  }
  <>.
- done
"#;
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    // Choose "Take cup" (teacup = true)
    let line = story.continue_single().expect("step 1");
    match &line {
        brink_runtime::Line::Choices { .. } => story.choose(0).expect("choose 0"),
        other => panic!("expected Choices, got {other:?}"),
    }

    // "I take a mug.\n"
    let line = story.continue_single().expect("step 2");
    let text = match &line {
        brink_runtime::Line::Text { text, .. } => text.clone(),
        other => panic!("expected Text, got {other:?}"),
    };
    assert_eq!(text, "I take a mug.\n");

    // "Quite a difficult situation." ...
    let line = story.continue_single().expect("step 3");
    let text = match &line {
        brink_runtime::Line::Text { text, .. } => text.clone(),
        other => {
            println!("got: {other:?}");
            text.clone()
        }
    };
    println!("step 3: {text:?}");

    // Should present Agree/Disagree choices
    let line = story.continue_single().expect("step 4");
    match &line {
        brink_runtime::Line::Choices { .. } => story.choose(0).expect("choose Agree"),
        other => panic!("expected Choices for agree/disagree, got {other:?}"),
    }

    // After "Agree": should be reply + glued conditional + period
    let line = story.continue_single().expect("step 5 - reply with glue");
    let text = match &line {
        brink_runtime::Line::Text { text, .. } | brink_runtime::Line::Done { text, .. } => {
            text.as_str()
        }
        other => panic!("expected text, got {other:?}"),
    };
    println!("step 5 actual: {text:?}");
    assert_eq!(
        text, "\"Awkward,\" I reply, sipping at my tea as though we were old friends.\n",
        "TheIntercept pattern: glue inside conditional after nested gather",
    );
}

#[test]
fn runtime_intercept_glue_conditional_trace() {
    // Trace every step to understand the flow.
    let source = r#"VAR teacup = false
VAR drugged = false
* [Take cup]
    ~ teacup = true
    I take a mug.
* [Leave it]
    I leave it.
- "I'm sure you agree."
    * [Agree]
        "Awkward," I reply
    * [Disagree]
        "I don't see why," I reply
- { teacup:
    ~ drugged = true
    <>, sipping at my tea as though we were old friends
  }
  <>.
- done
"#;
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let mut steps = Vec::new();
    for i in 0..20 {
        match story.continue_single() {
            Ok(brink_runtime::Line::Text { text, .. }) => {
                println!("step {i}: Text {text:?}");
                steps.push(format!("Text {text:?}"));
            }
            Ok(brink_runtime::Line::Done { text, .. }) => {
                println!("step {i}: Done {text:?}");
                steps.push(format!("Done {text:?}"));
                break;
            }
            Ok(brink_runtime::Line::Choices { text, choices, .. }) => {
                let names: Vec<_> = choices.iter().map(|c| c.text.as_str()).collect();
                println!("step {i}: Choices {text:?} {names:?} -> picking 0");
                steps.push(format!("Choices {names:?}"));
                story.choose(0).expect("choose");
            }
            Ok(brink_runtime::Line::End { text, .. }) => {
                println!("step {i}: End {text:?}");
                steps.push(format!("End {text:?}"));
                break;
            }
            Err(e) => {
                println!("step {i}: Error {e:?}");
                break;
            }
        }
    }

    // The key assertion: after choosing "Agree", the reply should
    // include the glued conditional text.
    // Find the step after the second choice that contains "Awkward"
    let awkward_step = steps.iter().find(|s| s.contains("Awkward"));
    assert!(
        awkward_step.is_some_and(|s| s.contains("sipping")),
        "reply should include glued conditional text; steps: {steps:#?}",
    );
}

#[test]
fn runtime_intercept_multi_level_weave_glue() {
    // Faithful TheIntercept reproduction: multi-level weave with
    // conditional glue between two gathers.
    let source = r#"VAR teacup = false
VAR forceful = 0
VAR drugged = false

* [Wait]
    I say nothing.
- He has brought two cups of tea.
    * (took) [Take one]
        ~ teacup = true
        I take a mug and warm my hands. It's <>
    * [Wait]
        He pushes one mug towards me: <>
- a small gesture of friendship.
    Enough to give me hope?
    * {teacup} [Drink]
        I raise the cup to my mouth but it's too hot to drink.
    * {teacup} [Wait]
        I wait.
- "Quite a difficult situation," he begins. "I'm sure you agree."
    * [Agree]
        "Awkward," I reply
    * (disagree) [Disagree]
        "I don't see why," I reply
    * [Lie] -> disagree
    * [Evade]
        "I'm sure you've handled worse," I reply casually
- { teacup:
    ~ drugged = true
    <>, sipping at my tea as though we were old friends
  }
  <>.
-
    * [Watch him]
        His face is telling me nothing.
    * [Wait]
        I wait.
- done
"#;
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let mut steps = Vec::new();
    for i in 0..30 {
        match story.continue_single() {
            Ok(brink_runtime::Line::Text { text, .. }) => {
                println!("step {i}: Text {text:?}");
                steps.push(format!("Text {text:?}"));
            }
            Ok(brink_runtime::Line::Done { text, .. }) => {
                println!("step {i}: Done {text:?}");
                steps.push(format!("Done {text:?}"));
                break;
            }
            Ok(brink_runtime::Line::Choices { text, choices, .. }) => {
                let names: Vec<_> = choices.iter().map(|c| c.text.as_str()).collect();
                println!("step {i}: Choices {text:?} {names:?} -> picking 0");
                steps.push(format!("Choices {names:?}"));
                story.choose(0).expect("choose");
            }
            Ok(brink_runtime::Line::End { text, .. }) => {
                println!("step {i}: End {text:?}");
                break;
            }
            Err(e) => {
                println!("step {i}: Error {e:?}");
                break;
            }
        }
    }

    let awkward_step = steps.iter().find(|s| s.contains("Awkward"));
    assert!(
        awkward_step.is_some_and(|s| s.contains("sipping")),
        "TheIntercept multi-level weave: reply should include glued conditional; steps: {steps:#?}",
    );
}

#[test]
fn runtime_intercept_exact_path_to_step23() {
    // Exact TheIntercept structure up to the glue divergence at step 23.
    // Traces the e0 path: always choose option 0.
    let source = r#"VAR forceful = 0
VAR evasive = 0
VAR teacup = false
VAR drugged = false
-> start

=== function lower(ref x)
~ x = x - 1

=== function raise(ref x)
~ x = x + 1

=== start ===
- They are keeping me waiting.
    * Hut 14[]. The door was locked after I sat down.
    I don't even have a pen to do any work.
    I am not a machine, whatever they say about me.

- (opts)
    {|I rattle my fingers on the field table.|}
    * (think) [Think]
        They suspect me to be a traitor.
        When they don't find it, {plan:then} they'll come back and demand I talk.
        -> opts
    * (plan) [Plan]
        {not think:What I am is|I am} a problem—solver. Good with figures, quick with crosswords, excellent at chess.
        But in this scenario — in this trap — what is the winning play?
        * * (cooperate) [Co—operate]
            I must co—operate. My credibility is my main asset.
            I must simply hope they do not ask the questions I do not want to answer.
            ~ lower(forceful)
        * * [Dissemble]
            Misinformation, then.
            ~ raise(forceful)
        * * (delay) [Divert]
            Avoidance and delay.
            ~ raise(evasive)
    * [Wait]
- -> waited

= waited
- Half an hour goes by before Commander Harris returns.
    "Well, then," he begins, awkwardly. This is an unseemly situation.
    * "Commander."
        He nods. <>
    * (tellme) {not start.delay} "Tell me what this is about."
        He shakes his head.
        "Now, don't let's pretend."
    * [Wait]
        I say nothing.
- He has brought two cups of tea in metal mugs: he sets them down on the tabletop between us.
    * {tellme} [Deny] "I'm not pretending anything."
        {cooperate:I'm lying already, despite my good intentions.}
        Harris looks disapproving. -> pushes_cup
    * (took) [Take one]
        ~ teacup = true
        I take a mug and warm my hands. It's <>
    * (what2) {not tellme} "What's going on?"
        "You know already."
        -> pushes_cup
    * [Wait]
        I wait for him to speak.
        - - (pushes_cup) He pushes one mug halfway towards me: <>
- a small gesture of friendship.
    Enough to give me hope?
    * (lift_up_cup) {not teacup} [Take it]
        I {took:lift the mug|take the mug,} and blow away the steam.
        Harris picks his own up and just holds it.
        ~ teacup = true
        ~ lower(forceful)
    * {not teacup} [Don't take it]
        Just a cup of insipid canteen tea. I leave it where it is.
        ~ raise(forceful)

    * {teacup} [Drink]
        I raise the cup to my mouth but it's too hot to drink.

    * {teacup} [Wait]
        I say nothing as -> lift_up_cup

- "Quite a difficult situation," {lift_up_cup:he|Harris} begins{forceful <= 0:, sternly}. I've seen him adopt this stiff tone of voice before, but only when talking to the brass. "I'm sure you agree."
    * [Agree]
        "Awkward," I reply
    * (disagree) [Disagree]
        "I don't see why," I reply
        ~ raise(forceful)
        ~ raise(evasive)
    * [Lie] -> disagree
    * [Evade]
        "I'm sure you've handled worse," I reply casually
        ~ raise(evasive)
- { teacup:
    ~ drugged = true
    <>, sipping at my tea as though we were old friends
  }
  <>.

-
    * [Watch him]
        His face is telling me nothing.
    * [Wait]
        I wait to see how he'll respond.
    * {not disagree} [Smile]
        I try a weak smile.
        ~ lower(forceful)
- done
"#;
    let data = compile_to_story_data(source);
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    let mut steps = Vec::new();
    for i in 0..30 {
        match story.continue_single() {
            Ok(brink_runtime::Line::Text { text, .. }) => {
                println!("step {i}: Text {text:?}");
                steps.push(format!("Text {text:?}"));
            }
            Ok(brink_runtime::Line::Done { text, .. }) => {
                println!("step {i}: Done {text:?}");
                steps.push(format!("Done {text:?}"));
                break;
            }
            Ok(brink_runtime::Line::Choices { text, choices, .. }) => {
                let names: Vec<_> = choices.iter().map(|c| c.text.as_str()).collect();
                println!("step {i}: Choices {text:?} {names:?} -> picking 0");
                steps.push(format!("Choices {text:?} {names:?}"));
                story.choose(0).expect("choose");
            }
            Ok(brink_runtime::Line::End { text, .. }) => {
                println!("step {i}: End {text:?}");
                break;
            }
            Err(e) => {
                println!("step {i}: Error {e:?}");
                break;
            }
        }
    }

    // The "Awkward" reply should include "sipping" via glue+conditional
    let has_glued_reply = steps
        .iter()
        .any(|s| s.contains("Awkward") && s.contains("sipping"));
    assert!(
        has_glued_reply,
        "exact TheIntercept path: reply should include glued conditional; steps: {steps:#?}",
    );
}

fn compile_intercept() -> brink_format::StoryData {
    use std::path::PathBuf;
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests");
    let intercept_path = tests_dir.join("tier3/misc/TheIntercept/story.ink");
    let full_source = std::fs::read_to_string(&intercept_path).expect("read TheIntercept");
    compile_to_story_data(&full_source)
}

#[test]
fn runtime_intercept_step23_glue_not_dropped() {
    // The full TheIntercept compiled from story.ink diverges at step 23:
    // after choosing "Agree", the reply should include glued conditional
    // text ("sipping at my tea...") but brink drops it entirely and
    // jumps to the wrong part of the story.
    let data = compile_intercept();
    let (program, line_tables) = brink_runtime::link(&data).expect("link failed");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(&program, line_tables);

    // Run e0 path (always pick choice 0), dumping state at the critical steps
    let mut last_text = String::new();
    for i in 0..24 {
        // Dump state before the critical region (steps 21-23)
        if (21..=23).contains(&i) {
            println!("\n--- BEFORE step {i} ---");
            println!("{}", story.debug_state());
        }
        match story.continue_single() {
            Ok(brink_runtime::Line::Text { text, .. }) => {
                if i >= 20 {
                    println!("step {i}: Text {text:?}");
                }
                last_text = text;
            }
            Ok(brink_runtime::Line::Done { text, .. }) => {
                println!("step {i}: Done {text:?}");
                last_text = text;
                break;
            }
            Ok(brink_runtime::Line::Choices { text, choices, .. }) => {
                if i >= 20 {
                    let names: Vec<_> = choices.iter().map(|c| c.text.as_str()).collect();
                    println!("step {i}: Choices {text:?} {names:?}");
                }
                last_text = text;
                story.choose(0).expect("choose");
            }
            Ok(brink_runtime::Line::End { text, .. }) => {
                println!("step {i}: End {text:?}");
                last_text = text;
                break;
            }
            _ => break,
        }
    }

    // Step 23 (oracle numbering) should be:
    //   "Awkward," I reply, sipping at my tea as though we were old friends.
    // NOT just:
    //   "Awkward," I reply
    assert!(
        last_text.contains("sipping"),
        "TheIntercept step 23: expected glued conditional text 'sipping...', \
         got: {last_text:?}. The Agree choice body's goto likely targets \
         the wrong gather container.",
    );
}

#[test]
fn intercept_agree_choice_diverts_to_correct_gather() {
    // The Agree choice body should divert to the gather that contains
    // the {teacup: <>, sipping...} conditional. In the .inkt, the
    // Agree body ends with `goto $container` and that container should
    // contain `emit_line 69` (the "sipping" text).
    //
    // This test compiles TheIntercept, dumps the .inkt, and verifies
    // that the goto target from the "Awkward" container leads to a
    // container that references the "sipping" line.
    let data = compile_intercept();
    let inkt = dump_inkt(&data);

    let lines: Vec<&str> = inkt.lines().collect();

    // Find the container with emit_line 62 ("Awkward," I reply)
    // and extract its goto target.
    let mut goto_target = None;
    let mut in_awkward_container = false;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.contains("emit_line 62 ") {
            in_awkward_container = true;
        }
        if in_awkward_container && trimmed.starts_with("goto ") {
            goto_target = trimmed.strip_prefix("goto ");
            break;
        }
        // Reset if we hit a new container before finding goto
        if in_awkward_container && trimmed.starts_with("(container ") {
            in_awkward_container = false;
        }
    }

    let target = goto_target.expect("Agree choice body should have a goto after emit_line 62");
    println!("Agree choice goto target: {target}");

    // Now find the target container and check it references emit_line 69
    // (the "sipping" text) or enters a sub-container that does.
    let target_header = format!("(container {target}");
    let mut in_target = false;
    let mut has_sipping_or_enters_conditional = false;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with(&target_header) {
            in_target = true;
            continue;
        }
        if in_target {
            if trimmed.contains("emit_line 69") {
                has_sipping_or_enters_conditional = true;
                break;
            }
            // The gather might enter_container the conditional body
            if trimmed.starts_with("enter_container ") {
                has_sipping_or_enters_conditional = true;
                break;
            }
            // Stop at next container definition
            if trimmed.starts_with("(container ") {
                break;
            }
        }
    }

    assert!(
        has_sipping_or_enters_conditional,
        "Agree choice goto target {target} should contain emit_line 69 (sipping) \
         or enter_container for the teacup conditional, but it doesn't. \
         The gather target is wrong — the choice diverts to the wrong place.",
    );
}
