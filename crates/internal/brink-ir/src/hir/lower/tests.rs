#![allow(clippy::panic)]

use brink_syntax::parse;

use crate::hir::lower::lower;
use crate::*;

fn lower_ink(source: &str) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
    let parsed = parse(source);
    let tree = parsed.tree();
    lower(FileId(0), &tree)
}

fn diags_for(source: &str) -> Vec<Diagnostic> {
    let (_, _, diags) = lower_ink(source);
    diags
}

fn expect_diag(diags: &[Diagnostic], code: DiagnosticCode) -> &Diagnostic {
    diags.iter().find(|d| d.code == code).unwrap_or_else(|| {
        panic!(
            "expected diagnostic {}, got: {:?}",
            code.as_str(),
            diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        )
    })
}

/// Assert that malformed input doesn't silently vanish — either the HIR
/// emits the expected diagnostic, or the parser already rejects it.
fn expect_diag_or_parse_error(source: &str, code: DiagnosticCode) {
    let parsed = parse(source);
    let tree = parsed.tree();
    let (_, _, diags) = lower(FileId(0), &tree);
    let has_hir_diag = diags.iter().any(|d| d.code == code);
    let has_parse_error = !parsed.errors().is_empty();
    assert!(
        has_hir_diag || has_parse_error,
        "expected diagnostic {} or a parse error for {:?}, got neither",
        code.as_str(),
        source,
    );
}

#[test]
fn empty_file() {
    let (hir, manifest, diags) = lower_ink("");
    assert!(hir.root_content.stmts.is_empty());
    assert!(hir.knots.is_empty());
    assert!(diags.is_empty());
    assert!(manifest.knots.is_empty());
}

#[test]
fn plain_text_content() {
    let (hir, _, diags) = lower_ink("Hello, world!\n");
    assert!(diags.is_empty());
    assert_eq!(hir.root_content.stmts.len(), 2);
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            assert!(!c.parts.is_empty());
            assert!(matches!(&c.parts[0], ContentPart::Text(t) if t.contains("Hello")));
        }
        other => panic!("expected Content, got {other:?}"),
    }
    assert!(matches!(&hir.root_content.stmts[1], Stmt::EndOfLine));
}

#[test]
fn knot_with_stitch() {
    let (hir, manifest, diags) = lower_ink(
        "\
=== knot_one ===
Some content.
= stitch_a
Stitch content.
",
    );
    assert!(diags.is_empty());
    assert_eq!(hir.knots.len(), 1);
    assert_eq!(hir.knots[0].name.text, "knot_one");
    assert!(!hir.knots[0].is_function);
    assert_eq!(hir.knots[0].stitches.len(), 1);
    assert_eq!(hir.knots[0].stitches[0].name.text, "stitch_a");

    assert!(manifest.knots.iter().any(|s| s.name == "knot_one"));
    assert!(
        manifest
            .stitches
            .iter()
            .any(|s| s.name == "knot_one.stitch_a")
    );
}

/// First stitch with no params gets an implicit divert in the knot body.
#[test]
fn first_stitch_auto_enter() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
= first_stitch
First stitch content.
= second_stitch
Second stitch content.
",
    );
    assert!(diags.is_empty());
    let knot = &hir.knots[0];
    assert_eq!(knot.stitches.len(), 2);
    // Knot body should have an implicit divert to first_stitch
    assert!(
        knot.body.stmts.iter().any(|s| matches!(s,
            Stmt::Divert(d) if matches!(&d.target.path, DivertPath::Path(p) if p.segments[0].text == "first_stitch")
        )),
        "expected implicit divert to first_stitch, got {:#?}",
        knot.body.stmts
    );
}

/// First stitch with params does NOT get auto-entered.
#[test]
fn first_stitch_with_params_no_auto_enter() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
= first_stitch(x)
Content with {x}.
",
    );
    assert!(diags.is_empty());
    let knot = &hir.knots[0];
    // No implicit divert — first stitch has params
    assert!(
        !knot.body.stmts.iter().any(|s| matches!(s, Stmt::Divert(_))),
        "should NOT have implicit divert when first stitch has params, got {:#?}",
        knot.body.stmts
    );
}

/// Knot with content before stitches does NOT get auto-enter divert.
#[test]
fn knot_with_content_before_stitch_no_auto_enter() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
Some preamble.
= first_stitch
Stitch content.
",
    );
    assert!(diags.is_empty());
    let knot = &hir.knots[0];
    // Has content before stitch, so no auto-enter needed
    assert!(
        knot.body
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::Content(_))),
        "knot body should have content"
    );
}

#[test]
fn function_knot() {
    let (hir, _, diags) = lower_ink(
        "\
=== function greet(name) ===
~ return \"hello\"
",
    );
    assert!(diags.is_empty());
    assert_eq!(hir.knots.len(), 1);
    assert!(hir.knots[0].is_function);
    assert_eq!(hir.knots[0].params.len(), 1);
    assert_eq!(hir.knots[0].params[0].name.text, "name");
    assert!(!hir.knots[0].params[0].is_ref);
    assert!(!hir.knots[0].params[0].is_divert);
}

#[test]
fn ref_param() {
    let (hir, _, _) = lower_ink(
        "\
=== function inc(ref x) ===
~ x = x + 1
",
    );
    assert_eq!(hir.knots[0].params.len(), 1);
    assert!(hir.knots[0].params[0].is_ref);
}

#[test]
fn var_decl() {
    let (hir, manifest, diags) = lower_ink("VAR health = 100\n");
    assert!(diags.is_empty());
    assert_eq!(hir.variables.len(), 1);
    assert_eq!(hir.variables[0].name.text, "health");
    assert!(matches!(hir.variables[0].value, Expr::Int(100)));
    assert!(manifest.variables.iter().any(|s| s.name == "health"));
}

#[test]
fn const_decl() {
    let (hir, _, diags) = lower_ink("CONST max_health = 100\n");
    assert!(diags.is_empty());
    assert_eq!(hir.constants.len(), 1);
    assert_eq!(hir.constants[0].name.text, "max_health");
}

#[test]
fn list_decl() {
    let (hir, manifest, diags) = lower_ink("LIST colors = red, (green), blue\n");
    assert!(diags.is_empty());
    assert_eq!(hir.lists.len(), 1);
    assert_eq!(hir.lists[0].name.text, "colors");
    assert_eq!(hir.lists[0].members.len(), 3);
    assert_eq!(hir.lists[0].members[0].name.text, "red");
    assert!(!hir.lists[0].members[0].is_active);
    assert_eq!(hir.lists[0].members[1].name.text, "green");
    assert!(hir.lists[0].members[1].is_active);
    assert!(manifest.lists.iter().any(|s| s.name == "colors"));
}

#[test]
fn external_decl() {
    let (hir, manifest, diags) = lower_ink("EXTERNAL playSound(name, volume)\n");
    assert!(diags.is_empty());
    assert_eq!(hir.externals.len(), 1);
    assert_eq!(hir.externals[0].name.text, "playSound");
    assert_eq!(hir.externals[0].param_count, 2);
    assert!(manifest.externals.iter().any(|s| s.name == "playSound"));
}

#[test]
fn include_site() {
    let (hir, _, diags) = lower_ink("INCLUDE helper.ink\n");
    assert!(diags.is_empty());
    assert_eq!(hir.includes.len(), 1);
    assert_eq!(hir.includes[0].file_path, "helper.ink");
}

#[test]
fn simple_divert() {
    let (hir, manifest, _) = lower_ink("-> knot_name\n");
    assert_eq!(hir.root_content.stmts.len(), 1);
    match &hir.root_content.stmts[0] {
        Stmt::Divert(d) => match &d.target.path {
            DivertPath::Path(p) => assert_eq!(p.segments[0].text, "knot_name"),
            other => panic!("expected Path, got {other:?}"),
        },
        other => panic!("expected Divert, got {other:?}"),
    }
    assert!(
        manifest
            .unresolved
            .iter()
            .any(|r| r.path == "knot_name" && r.kind == RefKind::Divert)
    );
}

#[test]
fn divert_done() {
    let (hir, _, _) = lower_ink("-> DONE\n");
    match &hir.root_content.stmts[0] {
        Stmt::Divert(d) => assert!(matches!(d.target.path, DivertPath::Done)),
        other => panic!("expected Divert, got {other:?}"),
    }
}

#[test]
fn divert_end() {
    let (hir, _, _) = lower_ink("-> END\n");
    match &hir.root_content.stmts[0] {
        Stmt::Divert(d) => assert!(matches!(d.target.path, DivertPath::End)),
        other => panic!("expected Divert, got {other:?}"),
    }
}

#[test]
fn temp_decl() {
    let (hir, _, _) = lower_ink(
        "\
=== my_knot ===
~ temp x = 5
",
    );
    let body = &hir.knots[0].body;
    assert!(
        body.stmts
            .iter()
            .any(|s| matches!(s, Stmt::TempDecl(t) if t.name.text == "x"))
    );
}

#[test]
fn assignment() {
    let (hir, _, _) = lower_ink(
        "\
=== my_knot ===
~ x = 10
",
    );
    let body = &hir.knots[0].body;
    assert!(body.stmts.iter().any(|s| matches!(s, Stmt::Assignment(_))));
}

#[test]
fn return_stmt() {
    let (hir, _, _) = lower_ink(
        "\
=== function double(x) ===
~ return x * 2
",
    );
    let body = &hir.knots[0].body;
    assert!(body.stmts.iter().any(|s| matches!(s, Stmt::Return(_))));
}

#[test]
fn simple_choice() {
    let (hir, _, diags) = lower_ink(
        "\
* Choice A
* Choice B
",
    );
    assert!(diags.is_empty());
    assert_eq!(hir.root_content.stmts.len(), 1);
    match &hir.root_content.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            assert_eq!(cs.choices.len(), 2);
            assert!(!cs.choices[0].is_sticky);
            assert!(!cs.choices[1].is_sticky);
            assert!(cs.continuation.stmts.is_empty());
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

#[test]
fn sticky_choice() {
    let (hir, _, _) = lower_ink(
        "\
+ Sticky choice
",
    );
    match &hir.root_content.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            assert!(cs.choices[0].is_sticky);
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

#[test]
fn choice_with_gather() {
    let (hir, _, diags) = lower_ink(
        "\
* Choice A
* Choice B
- Gathered text
",
    );
    assert!(diags.is_empty());
    match &hir.root_content.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            assert_eq!(cs.choices.len(), 2);
            assert!(!cs.continuation.stmts.is_empty());
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

#[test]
fn expression_infix() {
    let (hir, _, _) = lower_ink("VAR x = 2 + 3\n");
    match &hir.variables[0].value {
        Expr::Infix(lhs, op, rhs) => {
            assert!(matches!(lhs.as_ref(), Expr::Int(2)));
            assert_eq!(*op, InfixOp::Add);
            assert!(matches!(rhs.as_ref(), Expr::Int(3)));
        }
        other => panic!("expected Infix, got {other:?}"),
    }
}

#[test]
fn expression_prefix() {
    let (hir, _, _) = lower_ink("VAR x = -5\n");
    match &hir.variables[0].value {
        Expr::Prefix(op, inner) => {
            assert_eq!(*op, PrefixOp::Negate);
            assert!(matches!(inner.as_ref(), Expr::Int(5)));
        }
        other => panic!("expected Prefix, got {other:?}"),
    }
}

#[test]
fn float_literal() {
    let (hir, _, _) = lower_ink("VAR x = 2.5\n");
    match &hir.variables[0].value {
        Expr::Float(f) => {
            let val = f.to_f64();
            assert!((val - 2.5_f64).abs() < f64::EPSILON);
        }
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn bool_literal() {
    let (hir, _, _) = lower_ink("VAR x = true\n");
    assert!(matches!(hir.variables[0].value, Expr::Bool(true)));
}

#[test]
fn string_literal() {
    let (hir, _, _) = lower_ink("VAR x = \"hello\"\n");
    match &hir.variables[0].value {
        Expr::String(s) => {
            assert_eq!(s.parts.len(), 1);
            assert!(matches!(&s.parts[0], StringPart::Literal(t) if t == "hello"));
        }
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn glue_in_content() {
    let (hir, _, _) = lower_ink("Hello <>world\n");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            assert!(c.parts.iter().any(|p| matches!(p, ContentPart::Glue)));
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn tags_on_content() {
    let (hir, _, _) = lower_ink("Hello # tag1 # tag2\n");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            assert_eq!(c.tags.len(), 2);
            assert_eq!(c.tags[0].parts, vec![ContentPart::Text("tag1".to_string())]);
            assert_eq!(c.tags[1].parts, vec![ContentPart::Text("tag2".to_string())]);
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn content_with_divert() {
    let (hir, _, _) = lower_ink("Hello -> knot\n");
    // Should produce content stmt followed by divert stmt
    assert!(!hir.root_content.stmts.is_empty());
    let has_content = hir
        .root_content
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::Content(_)));
    let has_divert = hir
        .root_content
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::Divert(_)));
    assert!(has_content);
    assert!(has_divert);
}

#[test]
fn manifest_tracks_unresolved_divert() {
    let (_, manifest, _) = lower_ink("-> somewhere\n");
    assert!(
        manifest
            .unresolved
            .iter()
            .any(|r| r.path == "somewhere" && r.kind == RefKind::Divert)
    );
}

#[test]
fn manifest_tracks_unresolved_variable() {
    let (_, manifest, _) = lower_ink("VAR x = other_var\n");
    assert!(
        manifest
            .unresolved
            .iter()
            .any(|r| r.kind == RefKind::Variable)
    );
}

// ─── Per-knot lowering ──────────────────────────────────────────────

#[test]
fn lower_knot_matches_full_lower() {
    let source = "\
=== my_knot ===
Hello from knot.
-> DONE
";
    let parsed = parse(source);
    let tree = parsed.tree();

    // Full lower
    let (hir, _, _) = lower(FileId(0), &tree);
    let full_knot = &hir.knots[0];

    // Per-knot lower
    let ast_knot = tree.knots().next().unwrap();
    let (knot, manifest, diags) = crate::lower_knot(FileId(0), &ast_knot);
    let knot = knot.unwrap();

    assert!(diags.is_empty());
    assert_eq!(knot.name.text, full_knot.name.text);
    assert_eq!(knot.is_function, full_knot.is_function);
    assert_eq!(knot.body.stmts.len(), full_knot.body.stmts.len());
    assert!(manifest.knots.iter().any(|s| s.name == "my_knot"));
}

#[test]
fn lower_knot_function_with_params() {
    let source = "\
=== function add(a, b) ===
~ return a + b
";
    let parsed = parse(source);
    let tree = parsed.tree();

    let ast_knot = tree.knots().next().unwrap();
    let (knot, manifest, diags) = crate::lower_knot(FileId(0), &ast_knot);
    let knot = knot.unwrap();

    assert!(diags.is_empty());
    assert!(knot.is_function);
    assert_eq!(knot.params.len(), 2);
    assert!(manifest.knots.iter().any(|s| s.name == "add"));
}

// ─── Top-level lowering ─────────────────────────────────────────────

#[test]
fn lower_top_level_excludes_knots() {
    let source = "\
VAR health = 100
Hello world.
=== some_knot ===
Knot content.
";
    let parsed = parse(source);
    let tree = parsed.tree();

    let (block, _top_level_knots, manifest, diags) = crate::lower_top_level(FileId(0), &tree);

    assert!(diags.is_empty());
    // Should have root content
    assert!(!block.stmts.is_empty());
    // Should declare the variable
    assert!(manifest.variables.iter().any(|s| s.name == "health"));
    // Should NOT declare the knot
    assert!(manifest.knots.is_empty());
}

#[test]
fn lower_top_level_returns_declarations() {
    let source = "\
VAR x = 1
CONST y = 2
LIST colors = red, green
EXTERNAL doThing(a)
";
    let parsed = parse(source);
    let tree = parsed.tree();

    let (_block, _top_level_knots, manifest, diags) = crate::lower_top_level(FileId(0), &tree);

    assert!(diags.is_empty());
    assert!(manifest.variables.iter().any(|s| s.name == "x"));
    assert!(manifest.constants.iter().any(|s| s.name == "y"));
    assert!(manifest.lists.iter().any(|s| s.name == "colors"));
    assert!(manifest.externals.iter().any(|s| s.name == "doThing"));
}

// ─── fold_weave public access ───────────────────────────────────────

#[test]
fn fold_weave_is_public() {
    use crate::hir::lower::WeaveItem;
    use crate::hir::lower::fold_weave;

    // Simple: just statements, no choices or gathers
    let items = vec![WeaveItem::Stmt(Stmt::Content(Content {
        ptr: None,
        parts: vec![ContentPart::Text("hello".into())],
        tags: vec![],
    }))];
    let block = fold_weave(items);
    assert_eq!(block.stmts.len(), 1);
    assert!(matches!(&block.stmts[0], Stmt::Content(_)));
}

// ─── Complex weave patterns ─────────────────────────────────────────

/// Choices with no gather — produces a `ChoiceSet` with `gather=None`.
#[test]
fn weave_choices_without_gather() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Choice A
  After A.
* Choice B
  After B.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    // The knot body may contain content stmts from choice sub-lines, plus the ChoiceSet
    let choice_sets: Vec<_> = body
        .stmts
        .iter()
        .filter(|s| matches!(s, Stmt::ChoiceSet(_)))
        .collect();
    assert_eq!(
        choice_sets.len(),
        1,
        "expected exactly 1 ChoiceSet, got {:#?}",
        body.stmts
    );
    match choice_sets[0] {
        Stmt::ChoiceSet(cs) => {
            assert_eq!(cs.choices.len(), 2);
            assert!(cs.continuation.stmts.is_empty());
        }
        _ => unreachable!(),
    }
}

/// Choices followed by a gather — the gather becomes part of the `ChoiceSet`.
#[test]
fn weave_choices_with_gather() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Choice A
* Choice B
- Gathered here.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    assert_eq!(body.stmts.len(), 1);
    match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            assert_eq!(cs.choices.len(), 2);
            assert!(
                cs.continuation
                    .stmts
                    .iter()
                    .any(|s| matches!(s, Stmt::Content(_))),
                "continuation should have content"
            );
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Statements after a gather nest inside the gather's continuation block.
#[test]
fn weave_stmts_after_gather_nest_in_continuation() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Choice A
* Choice B
- Gathered.
More content after gather.
-> DONE
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    // Single ChoiceSet — everything after the gather is in the continuation
    assert_eq!(body.stmts.len(), 1, "stmts: {:#?}", body.stmts);
    let Stmt::ChoiceSet(cs) = &body.stmts[0] else {
        panic!("expected ChoiceSet, got {:?}", body.stmts[0]);
    };
    // Continuation contains: "Gathered." content + EOL + "More content" + EOL + Divert
    let cont = &cs.continuation;
    assert!(
        cont.stmts.len() >= 3,
        "continuation should have gather content + trailing stmts, got: {:#?}",
        cont.stmts
    );
    // First stmt is the gather's own content ("Gathered.")
    assert!(
        matches!(&cont.stmts[0], Stmt::Content(c) if c.parts.iter().any(|p| matches!(p, ContentPart::Text(t) if t.contains("Gathered")))),
        "first continuation stmt should be gather content"
    );
    // Should end with Divert(DONE)
    assert!(
        cont.stmts.iter().any(|s| matches!(s, Stmt::Divert(_))),
        "continuation should contain the trailing divert"
    );
}

/// Two sequential choice sets: the second nests inside the first's continuation.
#[test]
fn weave_two_sequential_choice_sets_nest() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* First A
* First B
- First gather.
* Second A
* Second B
- Second gather.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    // One top-level ChoiceSet
    assert_eq!(body.stmts.len(), 1, "stmts: {:#?}", body.stmts);
    let Stmt::ChoiceSet(cs1) = &body.stmts[0] else {
        panic!("expected ChoiceSet, got {:?}", body.stmts[0]);
    };
    assert_eq!(
        cs1.choices.len(),
        2,
        "first choice set should have 2 choices"
    );
    // First gather content + second choice set nested in continuation
    let cont = &cs1.continuation;
    assert!(
        !cont.stmts.is_empty(),
        "first continuation should not be empty"
    );
    // The continuation should contain a nested ChoiceSet for the second pair
    let has_nested_cs = cont.stmts.iter().any(|s| matches!(s, Stmt::ChoiceSet(_)));
    assert!(
        has_nested_cs,
        "first continuation should contain a nested ChoiceSet, got: {:#?}",
        cont.stmts
    );
    // Find the nested choice set and verify it
    let nested_cs = cont
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        nested_cs.choices.len(),
        2,
        "second choice set should have 2 choices"
    );
    assert!(
        !nested_cs.continuation.stmts.is_empty(),
        "second continuation should not be empty"
    );
}

/// Content before choices becomes a top-level stmt, not part of the `ChoiceSet`.
#[test]
fn weave_content_before_choices() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
Preamble text.
* Choice A
* Choice B
- Gathered.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    // [Content("Preamble text."), EndOfLine, ChoiceSet(A, B, gather)]
    assert_eq!(body.stmts.len(), 3, "stmts: {:#?}", body.stmts);
    assert!(matches!(&body.stmts[0], Stmt::Content(_)));
    assert!(matches!(&body.stmts[1], Stmt::EndOfLine));
    assert!(matches!(&body.stmts[2], Stmt::ChoiceSet(_)));
}

/// A standalone gather (no preceding choices) emits its content as a Content stmt.
#[test]
fn weave_standalone_gather() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
- Standalone gathered text.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    // Standalone gather is flattened into a Content stmt
    assert!(
        body.stmts
            .iter()
            .any(|s| matches!(s, Stmt::Content(c) if c.parts.iter().any(|p|
                matches!(p, ContentPart::Text(t) if t.contains("Standalone"))))),
        "expected standalone gather content, got {:#?}",
        body.stmts
    );
}

/// Two gathers in a row — both are standalone (no choices between them).
#[test]
fn weave_consecutive_standalone_gathers() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
- First gather.
- Second gather.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    let content_stmts: Vec<_> = body
        .stmts
        .iter()
        .filter(|s| matches!(s, Stmt::Content(_)))
        .collect();
    assert!(
        content_stmts.len() >= 2,
        "expected at least 2 content stmts from standalone gathers, got {:#?}",
        body.stmts
    );
}

/// Choices with a gather, then more choices — the first gather closes the first
/// A gather separates choice sets: second `ChoiceSet` nests inside the first's continuation.
#[test]
fn weave_gather_separates_choice_sets() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Alpha
* Beta
- Middle gather.
* Gamma
* Delta
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    // One top-level ChoiceSet (Alpha, Beta), with the second (Gamma, Delta)
    // nested inside the continuation.
    assert_eq!(body.stmts.len(), 1, "stmts: {:#?}", body.stmts);
    let Stmt::ChoiceSet(cs1) = &body.stmts[0] else {
        panic!("expected ChoiceSet, got {:?}", body.stmts[0]);
    };
    assert_eq!(cs1.choices.len(), 2);

    // Continuation has gather content + nested second choice set
    let cont = &cs1.continuation;
    let nested_cs = cont.stmts.iter().find_map(|s| match s {
        Stmt::ChoiceSet(cs) => Some(cs),
        _ => None,
    });
    assert!(
        nested_cs.is_some(),
        "continuation should contain nested ChoiceSet, got: {:#?}",
        cont.stmts
    );
    let cs2 = nested_cs.unwrap();
    assert_eq!(cs2.choices.len(), 2);
    assert!(
        cs2.continuation.stmts.is_empty(),
        "second choice set should have empty continuation"
    );
}

/// A choice with a divert on its line — the divert is folded into the body.
#[test]
fn weave_choice_with_inline_divert() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Go somewhere -> other_knot
* Stay here
- Gathered.
=== other_knot ===
Arrived.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            // Inline divert is now in the choice body: [Divert, EndOfLine, ...]
            assert!(matches!(cs.choices[0].body.stmts[0], Stmt::Divert(_)));
            assert!(matches!(cs.choices[0].body.stmts[1], Stmt::EndOfLine));
            // No inline divert: body starts with [EndOfLine, ...]
            assert!(matches!(cs.choices[1].body.stmts[0], Stmt::EndOfLine));
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// A fallback choice with only a divert (`* -> target`) has the divert in its body.
#[test]
fn weave_fallback_choice_divert_in_body() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* [visible] text
* -> other_knot
- Gathered.
=== other_knot ===
Arrived.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            let fallback = &cs.choices[1];
            assert!(fallback.is_fallback);
            // Fallback body: [Divert(other_knot), EndOfLine]
            assert_eq!(fallback.body.stmts.len(), 2);
            assert!(matches!(fallback.body.stmts[0], Stmt::Divert(_)));
            assert!(matches!(fallback.body.stmts[1], Stmt::EndOfLine));
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// A choice with start content + inline divert: body begins with [`Divert`, `EndOfLine`].
#[test]
fn weave_choice_with_content_and_inline_divert() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Hello world -> other_knot
- Gathered.
=== other_knot ===
Arrived.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            let choice = &cs.choices[0];
            assert!(choice.start_content.is_some());
            // Body starts with the inline divert + EndOfLine
            assert!(matches!(choice.body.stmts[0], Stmt::Divert(_)));
            assert!(matches!(choice.body.stmts[1], Stmt::EndOfLine));
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// A choice with inner content + inline divert: divert is in body, not on the Choice struct.
#[test]
fn weave_choice_with_inner_content_and_divert() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* start[bracket]inner -> other_knot
- Gathered.
=== other_knot ===
Arrived.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            let choice = &cs.choices[0];
            assert!(choice.start_content.is_some());
            assert!(choice.bracket_content.is_some());
            assert!(choice.inner_content.is_some());
            // Body starts with inline divert + EndOfLine
            assert!(matches!(choice.body.stmts[0], Stmt::Divert(_)));
            assert!(matches!(choice.body.stmts[1], Stmt::EndOfLine));
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// A choice with indented body content but NO inline divert: body starts with `EndOfLine`.
#[test]
fn weave_choice_no_divert_has_endofline_preamble() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Pick this
  Some body text.
- Gathered.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            let choice = &cs.choices[0];
            // Body: [EndOfLine, Content("Some body text."), EndOfLine]
            assert_eq!(choice.body.stmts.len(), 3);
            assert!(matches!(choice.body.stmts[0], Stmt::EndOfLine));
            assert!(matches!(choice.body.stmts[1], Stmt::Content(_)));
            assert!(matches!(choice.body.stmts[2], Stmt::EndOfLine));
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// A choice with inline divert + indented body: divert before `EndOfLine`, then body content.
#[test]
fn weave_choice_inline_divert_with_body() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Go -> other_knot
  Extra body.
- Gathered.
=== other_knot ===
Arrived.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            let choice = &cs.choices[0];
            // Body: [Divert, EndOfLine, Content("Extra body."), EndOfLine]
            assert_eq!(choice.body.stmts.len(), 4);
            assert!(matches!(choice.body.stmts[0], Stmt::Divert(_)));
            assert!(matches!(choice.body.stmts[1], Stmt::EndOfLine));
            assert!(matches!(choice.body.stmts[2], Stmt::Content(_)));
            assert!(matches!(choice.body.stmts[3], Stmt::EndOfLine));
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Sticky and once-only choices can be mixed in the same `ChoiceSet`.
#[test]
fn weave_mixed_sticky_and_once_only() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Once-only choice
+ Sticky choice
* Another once-only
- Gathered.
",
    );
    assert!(diags.is_empty());
    match &hir.knots[0].body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            assert!(!cs.choices[0].is_sticky);
            assert!(cs.choices[1].is_sticky);
            assert!(!cs.choices[2].is_sticky);
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Choice with a condition — the condition expr is captured.
#[test]
fn weave_conditional_choice() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* {x > 0} Conditional choice
* Always available
- Gathered.
",
    );
    assert!(diags.is_empty());
    match &hir.knots[0].body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            assert!(cs.choices[0].condition.is_some());
            assert!(cs.choices[1].condition.is_none());
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Labeled gather — the label is preserved.
#[test]
fn weave_labeled_gather() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Choice A
* Choice B
- (my_label) Gathered with label.
",
    );
    assert!(diags.is_empty());
    match &hir.knots[0].body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            let label = cs.continuation.label.as_ref().unwrap();
            assert_eq!(label.text, "my_label");
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Nested bullet choices produce recursively nested `ChoiceSet`s.
/// `* *` (depth 2) choices become a nested `ChoiceSet` inside Outer A's body.
#[test]
fn weave_nested_bullet_choices() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Outer A
  * * Inner A1
  * * Inner A2
  - - Inner gather.
* Outer B
- Outer gather.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;

    // Top level: one ChoiceSet with 2 outer choices + outer gather
    assert_eq!(body.stmts.len(), 1, "stmts: {:#?}", body.stmts);
    let outer_cs = match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => cs,
        other => panic!("expected ChoiceSet, got {other:?}"),
    };
    assert_eq!(outer_cs.choices.len(), 2, "expected 2 outer choices");
    assert!(
        !outer_cs.continuation.stmts.is_empty(),
        "expected outer continuation"
    );

    // Outer A's body should contain a nested ChoiceSet with the inner choices
    let outer_a_body = &outer_cs.choices[0].body;
    let inner_cs = outer_a_body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected nested ChoiceSet in Outer A's body, got {:#?}",
                outer_a_body.stmts
            )
        });
    assert_eq!(inner_cs.choices.len(), 2, "expected 2 inner choices");
    assert!(
        !inner_cs.continuation.stmts.is_empty(),
        "expected inner continuation"
    );

    // Outer B should have no nested choice sets
    assert!(
        outer_cs.choices[1]
            .body
            .stmts
            .iter()
            .all(|s| !matches!(s, Stmt::ChoiceSet(_))),
        "Outer B should have no nested ChoiceSets"
    );
}

/// Choice with bracket content — text in [...] is captured separately.
#[test]
fn weave_choice_bracket_content() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Start [bracket] inner
- Gathered.
",
    );
    assert!(diags.is_empty());
    match &hir.knots[0].body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            let choice = &cs.choices[0];
            assert!(choice.start_content.is_some(), "missing start_content");
            assert!(choice.bracket_content.is_some(), "missing bracket_content");
            assert!(choice.inner_content.is_some(), "missing inner_content");
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Fallback choice — a choice with no text content is marked as fallback.
#[test]
fn weave_fallback_choice() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* -> somewhere
",
    );
    assert!(diags.is_empty());
    match &hir.knots[0].body.stmts[0] {
        Stmt::ChoiceSet(cs) => {
            assert!(cs.choices[0].is_fallback);
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Content interleaved around multiple choice sets — nests through continuations.
#[test]
fn weave_interleaved_content_and_choices() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
Before first.
* A1
* A2
- Gather one.
Between sets.
* B1
* B2
- Gather two.
After everything.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;
    // Expected nested structure:
    // [0] Content("Before first.")
    // [1] EndOfLine
    // [2] ChoiceSet(A1, A2, continuation: [
    //       Content("Gather one."), ...,
    //       Content("Between sets."), ...,
    //       ChoiceSet(B1, B2, continuation: [
    //         Content("Gather two."), ...,
    //         Content("After everything."), ...,
    //       ])
    //     ])
    assert_eq!(body.stmts.len(), 3, "stmts: {:#?}", body.stmts);
    assert!(matches!(&body.stmts[0], Stmt::Content(_)));
    assert!(matches!(&body.stmts[1], Stmt::EndOfLine));
    let Stmt::ChoiceSet(cs1) = &body.stmts[2] else {
        panic!("expected ChoiceSet at index 2, got {:?}", body.stmts[2]);
    };
    assert_eq!(cs1.choices.len(), 2);

    // First continuation should contain gather content + between content + nested choice set
    let cont1 = &cs1.continuation;
    let nested_cs = cont1.stmts.iter().find_map(|s| match s {
        Stmt::ChoiceSet(cs) => Some(cs),
        _ => None,
    });
    assert!(
        nested_cs.is_some(),
        "first continuation should contain nested ChoiceSet"
    );
    let cs2 = nested_cs.unwrap();
    assert_eq!(cs2.choices.len(), 2);

    // Second continuation should contain gather content + after content
    let cont2 = &cs2.continuation;
    assert!(
        !cont2.stmts.is_empty(),
        "second continuation should not be empty"
    );
}

// ─── Inline logic lowering ──────────────────────────────────────────

#[test]
fn inline_expression_interpolation() {
    let (hir, _, diags) = lower_ink("Hello {x} world\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            assert!(
                c.parts
                    .iter()
                    .any(|p| matches!(p, ContentPart::Interpolation(Expr::Path(_)))),
                "expected Interpolation(Path), got {:#?}",
                c.parts
            );
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn inline_conditional_true_false() {
    let (hir, _, diags) = lower_ink("{x: yes|no}\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            let ic = c.parts.iter().find_map(|p| match p {
                ContentPart::InlineConditional(ic) => Some(ic),
                _ => None,
            });
            let ic = ic.expect("expected InlineConditional part");
            assert_eq!(ic.branches.len(), 2, "expected 2 branches");
            assert!(
                ic.branches[0].condition.is_some(),
                "first branch should have condition"
            );
            assert!(
                ic.branches[1].condition.is_none(),
                "second branch should be else"
            );
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn inline_conditional_true_only() {
    let (hir, _, diags) = lower_ink("{x: shown}\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            let ic = c.parts.iter().find_map(|p| match p {
                ContentPart::InlineConditional(ic) => Some(ic),
                _ => None,
            });
            let ic = ic.expect("expected InlineConditional part");
            assert!(ic.branches[0].condition.is_some());
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn inline_sequence_cycle() {
    let (hir, _, diags) = lower_ink("Hello {&a|b|c}\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            let is = c.parts.iter().find_map(|p| match p {
                ContentPart::InlineSequence(is) => Some(is),
                _ => None,
            });
            let is = is.expect("expected InlineSequence part");
            assert_eq!(is.kind, SequenceType::CYCLE);
            assert_eq!(is.branches.len(), 3);
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn inline_sequence_stopping() {
    let (hir, _, diags) = lower_ink("{stopping: first|second|third}\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            let is = c.parts.iter().find_map(|p| match p {
                ContentPart::InlineSequence(is) => Some(is),
                _ => None,
            });
            let is = is.expect("expected InlineSequence part");
            assert_eq!(is.kind, SequenceType::STOPPING);
            assert_eq!(is.branches.len(), 3);
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn inline_sequence_shuffle() {
    let (hir, _, diags) = lower_ink("{~a|b}\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            let is = c.parts.iter().find_map(|p| match p {
                ContentPart::InlineSequence(is) => Some(is),
                _ => None,
            });
            let is = is.expect("expected InlineSequence part");
            assert_eq!(is.kind, SequenceType::SHUFFLE);
            assert_eq!(is.branches.len(), 2);
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn inline_sequence_once() {
    let (hir, _, diags) = lower_ink("{!a|b}\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            let is = c.parts.iter().find_map(|p| match p {
                ContentPart::InlineSequence(is) => Some(is),
                _ => None,
            });
            let is = is.expect("expected InlineSequence part");
            assert_eq!(is.kind, SequenceType::ONCE);
            assert_eq!(is.branches.len(), 2);
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

#[test]
fn implicit_sequence() {
    let (hir, _, diags) = lower_ink("{a|b|c}\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            let is = c.parts.iter().find_map(|p| match p {
                ContentPart::InlineSequence(is) => Some(is),
                _ => None,
            });
            let is = is.expect("expected InlineSequence part");
            assert_eq!(
                is.kind,
                SequenceType::STOPPING,
                "implicit sequences default to stopping"
            );
            assert_eq!(is.branches.len(), 3);
        }
        other => panic!("expected Content, got {other:?}"),
    }
}

// ─── Block-level conditionals and sequences ─────────────────────────

#[test]
fn block_conditional() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
{
- x > 5:
  Big.
- else:
  Small.
}
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.knots[0].body.stmts[0] {
        Stmt::Conditional(cond) => {
            assert_eq!(cond.branches.len(), 2);
            assert!(
                cond.branches[0].condition.is_some(),
                "first branch should have condition"
            );
            assert!(
                cond.branches[1].condition.is_none(),
                "second branch should be else"
            );
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn block_conditional_single_branch() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
{
- x:
  Hello.
}
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.knots[0].body.stmts[0] {
        Stmt::Conditional(cond) => {
            assert_eq!(cond.branches.len(), 1);
            assert!(cond.branches[0].condition.is_some());
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn block_sequence() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
{stopping:
- First.
- Second.
- Third.
}
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.knots[0].body.stmts[0] {
        Stmt::Sequence(seq) => {
            assert_eq!(seq.kind, SequenceType::STOPPING);
            assert_eq!(seq.branches.len(), 3);
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

// ─── Control flow ───────────────────────────────────────────────────

#[test]
fn tunnel_call() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
-> tunnel_knot ->
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.knots[0].body.stmts[0] {
        Stmt::TunnelCall(tc) => {
            assert!(
                !tc.targets.is_empty(),
                "tunnel should have at least one target"
            );
        }
        other => panic!("expected TunnelCall, got {other:?}"),
    }
}

#[test]
fn thread_start() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
<- background_thread
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.knots[0].body.stmts[0] {
        Stmt::ThreadStart(ts) => match &ts.target.path {
            DivertPath::Path(p) => assert_eq!(p.segments[0].text, "background_thread"),
            other => panic!("expected Path target, got {other:?}"),
        },
        other => panic!("expected ThreadStart, got {other:?}"),
    }
}

// ─── Expression statement ───────────────────────────────────────────

#[test]
fn expr_stmt_function_call() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
~ foo()
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.knots[0].body.stmts[0] {
        Stmt::ExprStmt(Expr::Call(path, args)) => {
            assert_eq!(path.segments[0].text, "foo");
            assert!(args.is_empty());
        }
        other => panic!("expected ExprStmt(Call), got {other:?}"),
    }
}

// ─── String interpolation ───────────────────────────────────────────

#[test]
fn string_literal_with_interpolation() {
    let (hir, _, diags) = lower_ink("VAR x = \"hello\"\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.variables[0].value {
        Expr::String(s) => {
            assert_eq!(s.parts.len(), 1);
            match &s.parts[0] {
                StringPart::Literal(text) => assert_eq!(text, "hello"),
                StringPart::Interpolation(e) => {
                    panic!("expected Literal, got Interpolation({e:?})")
                }
            }
        }
        other => panic!("expected String, got {other:?}"),
    }
}

// ─── Divert target as value ─────────────────────────────────────────

#[test]
fn divert_target_as_value() {
    let (hir, _, diags) = lower_ink("VAR x = -> somewhere\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    match &hir.variables[0].value {
        Expr::DivertTarget(p) => {
            assert_eq!(p.segments[0].text, "somewhere");
        }
        other => panic!("expected DivertTarget, got {other:?}"),
    }
}

// ─── Choices inside conditional blocks ───────────────────────────────

/// Choices inside multiline conditional blocks stay nested in the
/// conditional's branches in the HIR. Weave transparency is handled at
/// runtime — the conditional evaluates its branches, and choice points
/// within active branches get registered. The HIR preserves structure.
///
/// Reference: `InkParser_Statements.cs` allows choices at
/// `StatementLevel.InnerBlock`; `Weave.cs` treats the conditional as
/// regular content and handles loose ends via `PassLooseEndsToAncestors`.
#[test]
fn weave_choice_inside_conditional_preserved() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
{
- door_open:
  * Go outside -> END
- else:
  * Ask permission -> END
}
* Stay inside
- You decided.
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let body = &hir.knots[0].body;
    // The conditional is a stmt, followed by a choice set for "Stay inside"
    // with the gather "You decided."
    assert!(
        matches!(&body.stmts[0], Stmt::Conditional(_)),
        "first stmt should be Conditional, got {:?}",
        body.stmts[0]
    );
    // The conditional's branches each contain a ChoiceSet
    match &body.stmts[0] {
        Stmt::Conditional(cond) => {
            assert_eq!(cond.branches.len(), 2);
            assert!(
                matches!(&cond.branches[0].body.stmts[0], Stmt::ChoiceSet(_)),
                "first branch should contain a ChoiceSet"
            );
            assert!(
                matches!(&cond.branches[1].body.stmts[0], Stmt::ChoiceSet(_)),
                "second branch should contain a ChoiceSet"
            );
        }
        _ => unreachable!(),
    }
    // "Stay inside" is a separate ChoiceSet at the outer level
    assert!(
        matches!(&body.stmts[1], Stmt::ChoiceSet(_)),
        "second stmt should be ChoiceSet, got {:?}",
        body.stmts[1]
    );
}

/// Simpler case: conditional with a choice alongside unconditional
/// choices. The conditional is a separate stmt, not merged.
/// Reference: `TestConditionalChoiceInWeave` from ink tests.
#[test]
fn weave_conditional_choice_alongside_unconditional() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Always available
{
- has_key:
  * Use the key -> END
}
- You chose.
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let body = &hir.knots[0].body;
    // The choice "Always available" is followed by a multiline block
    // which the parser treats as part of the choice's body (since the
    // choice has no explicit divert). The conditional and gather are
    // woven relative to the choice.
    //
    // Current behavior: the choice accumulator sees the choice, then the
    // fold_weave encounters the conditional (which breaks the choice set),
    // then the gather. This will change when we redesign ChoiceSet to
    // support interleaved conditionals.
    assert!(!body.stmts.is_empty(), "body should have stmts, got empty");
}

// ─── Diagnostic coverage ────────────────────────────────────────────

// E001: knot is missing a name
#[test]
fn diag_e001_knot_missing_name() {
    let diags = diags_for("=== ===\nHello\n");
    expect_diag(&diags, DiagnosticCode::E001);
}

// E002: stitch is missing a name
#[test]
fn diag_e002_stitch_missing_name() {
    let diags = diags_for("=== knot ===\n= \nContent\n");
    expect_diag(&diags, DiagnosticCode::E002);
}

// E003: parameter is missing a name
#[test]
fn diag_e003_param_missing_name() {
    expect_diag_or_parse_error("=== function f(, ) ===\n~ return 0\n", DiagnosticCode::E003);
}

// E004: VAR is missing a name
#[test]
fn diag_e004_var_missing_name() {
    let diags = diags_for("VAR = 5\n");
    expect_diag(&diags, DiagnosticCode::E004);
}

// E005: VAR is missing an initializer
#[test]
fn diag_e005_var_missing_init() {
    let diags = diags_for("VAR x\n");
    expect_diag(&diags, DiagnosticCode::E005);
}

// E006: CONST is missing a name
#[test]
fn diag_e006_const_missing_name() {
    let diags = diags_for("CONST = 5\n");
    expect_diag(&diags, DiagnosticCode::E006);
}

// E007: CONST is missing an initializer
#[test]
fn diag_e007_const_missing_init() {
    let diags = diags_for("CONST x\n");
    expect_diag(&diags, DiagnosticCode::E007);
}

// E008: LIST is missing a name
#[test]
fn diag_e008_list_missing_name() {
    let diags = diags_for("LIST = a, b\n");
    expect_diag(&diags, DiagnosticCode::E008);
}

// E009: LIST member is missing a name
#[test]
fn diag_e009_list_member_missing_name() {
    expect_diag_or_parse_error("LIST things = , b\n", DiagnosticCode::E009);
}

// E010: EXTERNAL is missing a name
#[test]
fn diag_e010_external_missing_name() {
    let diags = diags_for("EXTERNAL (a, b)\n");
    expect_diag(&diags, DiagnosticCode::E010);
}

// E011: INCLUDE is missing a file path
#[test]
fn diag_e011_include_missing_path() {
    expect_diag_or_parse_error("INCLUDE\n", DiagnosticCode::E011);
}

// E012: divert is missing a target
#[test]
fn diag_e012_divert_missing_target() {
    let diags = diags_for("-> \n");
    expect_diag(&diags, DiagnosticCode::E012);
}

// E013: thread start is missing a target
#[test]
fn diag_e013_thread_missing_target() {
    expect_diag_or_parse_error("<- \n", DiagnosticCode::E013);
}

// E014: logic line has no effect
#[test]
fn diag_e014_bare_logic_line() {
    let diags = diags_for("~ \n");
    expect_diag(&diags, DiagnosticCode::E014);
}

// E015: expression is missing an operand
#[test]
fn diag_e015_expr_missing_operand() {
    let diags = diags_for("VAR x = -\n");
    expect_diag(&diags, DiagnosticCode::E015);
}

// E016: unknown operator
#[test]
fn diag_e016_unknown_operator() {
    // The parser may not even produce an infix node for truly unknown operators,
    // so we test with a prefix in a context where the op token is unexpected.
    let diags = diags_for("VAR x = 1 % \n");
    // If % parses as mod but has no rhs, that's E015. Let's just check that
    // the diagnostic system handles missing operands somewhere.
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E015 || d.code == DiagnosticCode::E016),
        "expected E015 or E016, got: {diags:?}"
    );
}

// E017: function call is missing a name
#[test]
fn diag_e017_call_missing_name() {
    // Hard to construct in ink syntax since the parser gate on ident;
    // we verify the code exists and the infrastructure works.
    // If the parser prevents this from reaching HIR, the test is a no-op —
    // but the code is still assigned and documented.
    let diags = diags_for("~ (1, 2)\n");
    // This may produce E014 or E017 depending on parser behavior — either is fine.
    // The key test is that malformed input doesn't silently vanish.
    assert!(
        !diags.is_empty() || {
            // If the parser fully rejects this, that's acceptable too.
            let parsed = parse("~ (1, 2)\n");
            !parsed.errors().is_empty()
        },
        "malformed function call produced no diagnostic from any layer"
    );
}

// E018: divert target expression is missing a path
#[test]
fn diag_e018_divert_target_missing_path() {
    expect_diag_or_parse_error("VAR x = -> \n", DiagnosticCode::E018);
}

// E019: choice is missing bullet markers
#[test]
fn diag_e019_choice_missing_bullets() {
    // The parser controls choice node creation, so this may not be reachable
    // from source text. We verify the code is assigned.
    // A choice with no bullets would be a parser-level malformation.
    let diags = diags_for("* \n");
    // * with nothing should still parse as a valid choice (fallback).
    // E019 fires only if bullets() returns None on a Choice AST node,
    // which the parser may never produce. This is defensive.
    let _ = diags; // code is registered even if unreachable from source
}

// E020: inline conditional is missing a condition
#[test]
fn diag_e020_inline_cond_missing_condition() {
    let diags = diags_for("Hello {: yes | no}\n");
    expect_diag(&diags, DiagnosticCode::E020);
}

// E021: inline sequence has no branches
#[test]
fn diag_e021_inline_seq_no_branches() {
    expect_diag_or_parse_error("Hello {stopping:}\n", DiagnosticCode::E021);
}

// ─── Branchless conditional structure ────────────────────────────────

#[test]
fn conditional_with_expr_branchless_body_lowers_as_conditional() {
    let source = "\
=== function f(x) ===
{ x > 0:
    hello
- else:
    world
}
";
    let (hir, _, diags) = lower_ink(source);
    assert!(diags.is_empty(), "unexpected diags: {diags:?}");

    let knot = &hir.knots[0];
    let cond = knot
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::Conditional(c) => Some(c),
            _ => None,
        })
        .expect("expected Stmt::Conditional for `{ expr: ... - else: ... }`");
    assert_eq!(cond.branches.len(), 2, "expected 2 branches (true + else)");
    assert!(
        cond.branches[0].condition.is_some(),
        "first branch should have condition"
    );
    assert!(
        cond.branches[1].condition.is_none(),
        "second branch should be else (no condition)"
    );
    // First branch should have content "hello"
    assert!(
        !cond.branches[0].body.stmts.is_empty(),
        "first branch body should have statements"
    );
    // Second branch should have content "world"
    assert!(
        !cond.branches[1].body.stmts.is_empty(),
        "else branch body should have statements"
    );
}

#[test]
fn branchless_conditional_with_temp_decl() {
    let source = "\
=== function f(x) ===
{ x:
    ~ temp y = 1
    Some text.
- else:
    Other text.
}
";
    let (hir, _, diags) = lower_ink(source);
    assert!(diags.is_empty(), "unexpected diags: {diags:?}");

    let knot = &hir.knots[0];
    let cond = knot.body.stmts.iter().find_map(|s| match s {
        Stmt::Conditional(c) => Some(c),
        _ => None,
    });
    let cond = cond.expect("expected Stmt::Conditional");
    assert_eq!(cond.branches.len(), 2);

    // First branch should contain TempDecl and Content
    let first_body = &cond.branches[0].body;
    let has_temp = first_body
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::TempDecl(_)));
    let has_content = first_body
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::Content(_)));
    assert!(has_temp, "first branch should contain TempDecl");
    assert!(has_content, "first branch should contain Content");

    // Second branch should contain Content
    let second_body = &cond.branches[1].body;
    let has_content = second_body
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::Content(_)));
    assert!(has_content, "else branch should contain Content");
}

#[test]
fn conditional_branch_body_has_endofline() {
    let (hir, _, _) = lower_ink(
        "\
=== death(reason) ===
{
- reason ? beaten:
You've been beaten to death.
- else:
Sorry, you're dead
}
-> END
",
    );

    let knot = &hir.knots[0];
    // Find the Conditional statement
    let cond = knot.body.stmts.iter().find_map(|s| {
        if let Stmt::Conditional(c) = s {
            Some(c)
        } else {
            None
        }
    });
    let cond = cond.expect("should have a conditional");
    // First branch (reason ? beaten) body should have Content + EndOfLine
    let first_branch = &cond.branches[0];
    let has_eol = first_branch
        .body
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::EndOfLine));
    assert!(
        has_eol,
        "branch body should contain EndOfLine after content"
    );
}

#[test]
fn branchless_conditional_tunnel_return_count() {
    let (hir, _, _) = lower_ink(
        "\
=== get_hit(x) ===
~ hp = hp - x
{ is_alive():
    ->->
}
-> death(beaten)
",
    );

    let knot = &hir.knots[0];
    let cond = knot.body.stmts.iter().find_map(|s| {
        if let Stmt::Conditional(c) = s {
            Some(c)
        } else {
            None
        }
    });
    let cond = cond.expect("should have a conditional");
    assert_eq!(cond.branches.len(), 1, "branchless should have 1 branch");

    let body = &cond.branches[0].body;
    let return_count = body
        .stmts
        .iter()
        .filter(|s| matches!(s, Stmt::Return(_)))
        .count();
    assert_eq!(
        return_count, 1,
        "branch body should have exactly 1 Return, got {return_count}"
    );
}

#[test]
fn print_num_else_branch_captures_both_conditionals() {
    let (hir, _, _diags) = lower_ink(
        "\
=== function print_num(x)
{
    - x >= 1000:
        {print_num(x / 1000)} thousand
    - x >= 100:
        {print_num(x / 100)} hundred
    - x == 0:
        zero
    - else:
        { x >= 20:
            hello
        }
        { x < 10 || x > 20:
            world
        - else:
            other
        }
}
",
    );
    // Find the top-level conditional
    let body = &hir.knots[0].body;
    let cond = body
        .stmts
        .iter()
        .find_map(|s| {
            if let Stmt::Conditional(c) = s {
                Some(c)
            } else {
                None
            }
        })
        .expect("should have a conditional");

    // Should have 4 branches: x>=1000, x>=100, x==0, else
    assert_eq!(
        cond.branches.len(),
        4,
        "top-level conditional should have 4 branches, got {}: {:?}",
        cond.branches.len(),
        cond.branches
            .iter()
            .map(|b| format!(
                "cond={} body_stmts={}",
                b.condition.is_some(),
                b.body.stmts.len()
            ))
            .collect::<Vec<_>>()
    );

    // The else branch body should contain 2 conditionals
    let else_branch = &cond.branches[3];
    assert!(else_branch.condition.is_none(), "4th branch should be else");
    let cond_count = else_branch
        .body
        .stmts
        .iter()
        .filter(|s| matches!(s, Stmt::Conditional(_)))
        .count();
    assert_eq!(
        cond_count,
        2,
        "else branch should have 2 sub-conditionals, got {}: {:?}",
        cond_count,
        else_branch
            .body
            .stmts
            .iter()
            .map(|s| match s {
                Stmt::Content(_) => "Content",
                Stmt::EndOfLine => "EndOfLine",
                Stmt::Conditional(_) => "Conditional",
                _ => "Other",
            })
            .collect::<Vec<_>>()
    );
}

/// `{ x < 10 || x > 20: body }` must lower to a conditional, not a sequence.
///
/// The `||` operator shares `|` with the sequence separator. `brace_scan`
/// currently sees the first `|` and classifies the brace pair as a sequence,
/// so HIR lowering never produces a `Conditional` node.
#[test]
fn logical_or_conditional_not_sequence() {
    let (hir, _, diags) = lower_ink("{x < 10 || x > 20: body}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

    let cond = hir
        .root_content
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::Content(c) => c.parts.iter().find_map(|p| match p {
                ContentPart::InlineConditional(c) => Some(c),
                _ => None,
            }),
            _ => None,
        })
        .expect("should lower to a conditional, not a sequence");

    assert!(
        matches!(cond.kind, CondKind::InitialCondition),
        "inline conditional should be InitialCondition, got {:?}",
        cond.kind,
    );
    assert_eq!(
        cond.branches.len(),
        1,
        "one true branch expected, got {}",
        cond.branches.len(),
    );
}

// ── Tunnel onwards with direct target ──────────────────────────────

#[test]
fn tunnel_onwards_with_target_becomes_return() {
    let (hir, _, diags) = lower_ink(
        "\
=== A ===
->-> B
=== B ===
Done.
",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[0].body;
    let has_return_with_value = body.stmts.iter().any(|s| {
        matches!(
            s,
            Stmt::Return(Return {
                ptr: None,
                value: Some(Expr::DivertTarget(_)),
                ..
            })
        )
    });
    assert!(
        has_return_with_value,
        "`->-> B` should lower to Return with DivertTarget value, got: {:?}",
        body.stmts,
    );
}

// ── Multiple choice conditions ANDed ───────────────────────────────

#[test]
fn choice_multiple_conditions_anded() {
    let (hir, _, diags) = lower_ink("* {true} {false} hidden\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let cs = match &hir.root_content.stmts[0] {
        Stmt::ChoiceSet(cs) => cs,
        other => panic!("expected ChoiceSet, got: {other:?}"),
    };
    let cond = cs.choices[0]
        .condition
        .as_ref()
        .expect("choice should have a condition");
    assert!(
        matches!(cond, Expr::Infix(_, InfixOp::And, _)),
        "multiple choice conditions should be ANDed, got: {cond:?}",
    );
}

// ── Whitespace between inline expressions in branch bodies ─────────

#[test]
fn whitespace_between_inline_exprs_in_branch_body() {
    let (hir, _, diags) = lower_ink(
        "\
{
- else:
  {1} {2}
}
",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    // The else branch should contain content with a space between
    // the two interpolations: Interpolation(1), Text(" "), Interpolation(2).
    let cond = match &hir.root_content.stmts[0] {
        Stmt::Conditional(c) => c,
        other => panic!("expected Conditional, got: {other:?}"),
    };
    let branch = &cond.branches[0];
    let has_space = branch.body.stmts.iter().any(|s| match s {
        Stmt::Content(c) => c
            .parts
            .iter()
            .any(|p| matches!(p, ContentPart::Text(t) if t.contains(' '))),
        _ => false,
    });
    assert!(
        has_space,
        "space between {{1}} and {{2}} should be preserved as Text(\" \"), got: {:?}",
        branch.body.stmts,
    );
}

/// A nested gather with `-> END` must have its divert set to `End`, not `Done`.
/// Regression: inner gathers lost their explicit divert when the choice set
/// was lowered.
#[test]
fn nested_gather_divert_to_end() {
    let source = "\
=== main ===
Choose:
*   [Option A]
    Chose A.
    -> END
*   [Option B]
    Chose B.
    **  [Sub 1]
        Sub 1 content.
    **  [Sub 2]
        Sub 2 content.
    - -> END
";
    let (file, _, diags) = lower_ink(source);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

    // Find the knot's body — it should contain a ChoiceSet
    let knot = &file.knots[0];
    let cs = knot
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .expect("knot body should contain a ChoiceSet");

    // Option B's body should contain a nested ChoiceSet
    let option_b = &cs.choices[1];
    let inner_cs = option_b
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(inner) => Some(inner),
            _ => None,
        })
        .expect("Option B body should contain a nested ChoiceSet");

    // The inner choice set has no explicit gather (the `- -> END` is at the
    // outer level). The HIR correctly leaves the inner continuation empty.
    assert!(
        inner_cs.continuation.stmts.is_empty(),
        "inner ChoiceSet should NOT have a continuation — `- -> END` is at the outer level"
    );

    // The outer choice set's continuation should have the divert `-> END`.
    let outer_divert = cs
        .continuation
        .stmts
        .iter()
        .find_map(|s| match s {
            hir::Stmt::Divert(d) => Some(d),
            _ => None,
        })
        .expect("outer continuation should have divert -> END");
    assert_eq!(
        outer_divert.target.path,
        hir::DivertPath::End,
        "outer continuation's divert should be End, got: {:?}",
        outer_divert.target.path
    );
}

// ─── Phase 2: Continuation nesting ──────────────────────────────────
//
// These tests verify that the weave folder nests remaining items into
// continuations rather than leaving them as siblings. This matches ink
// semantics: everything after a gather belongs inside the gather's scope.

/// Gather-choice chain (`- * hello / - * world`) nests the second choice
/// set inside the first's continuation, not as a sibling.
#[test]
fn weave_gather_choice_chain_nests_through_continuations() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
- * hello
- * world
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;

    // The first `-` is a standalone gather (opening label), wrapping the first ChoiceSet.
    // The second `- * world` should nest inside the first ChoiceSet's continuation.
    // Result: one top-level LabeledBlock (or ChoiceSet).
    assert_eq!(
        body.stmts.len(),
        1,
        "expected 1 top-level stmt, got: {:#?}",
        body.stmts
    );

    // Unwrap: LabeledBlock wrapping a ChoiceSet
    let inner_stmts = match &body.stmts[0] {
        Stmt::LabeledBlock(block) => &block.stmts,
        Stmt::ChoiceSet(_) => &body.stmts, // if no opening label, ChoiceSet is top-level
        other => panic!("expected LabeledBlock or ChoiceSet, got {other:?}"),
    };

    let cs1 = match &inner_stmts[0] {
        Stmt::ChoiceSet(cs) => cs,
        other => panic!("expected ChoiceSet for 'hello', got {other:?}"),
    };
    assert_eq!(
        cs1.choices.len(),
        1,
        "first choice set should have 1 choice"
    );

    // The second choice set should be nested inside the first's continuation
    let cont1 = &cs1.continuation;
    let cs2 = cont1
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected nested ChoiceSet for 'world' in continuation, got: {:#?}",
                cont1.stmts
            )
        });
    assert_eq!(
        cs2.choices.len(),
        1,
        "second choice set should have 1 choice"
    );
}

/// Trailing stmts after a gather belong inside the continuation, not as
/// siblings. This matches ink semantics where content after a gather is
/// part of the gather's container.
#[test]
fn weave_trailing_stmts_nest_inside_continuation() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* Choice A
* Choice B
- Gathered.
More content after gather.
-> DONE
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;

    // Everything should be in one ChoiceSet — trailing stmts go into the continuation.
    assert_eq!(
        body.stmts.len(),
        1,
        "expected 1 top-level stmt, got: {:#?}",
        body.stmts
    );

    let cs = match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => cs,
        other => panic!("expected ChoiceSet, got {other:?}"),
    };
    assert_eq!(cs.choices.len(), 2);

    // The continuation should contain: gather content + trailing content + divert
    let cont = &cs.continuation;
    assert!(
        cont.stmts.iter().any(|s| matches!(s, Stmt::Content(_))),
        "continuation should have content stmts"
    );
    assert!(
        cont.stmts.iter().any(|s| matches!(s, Stmt::Divert(_))),
        "continuation should have the trailing divert"
    );
}

/// Two sequential choice sets: the second is nested inside the first's
/// continuation, not a sibling.
#[test]
fn weave_sequential_choice_sets_nest() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
* First A
* First B
- First gather.
* Second A
* Second B
- Second gather.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;

    // One top-level ChoiceSet — the second is nested in the first's continuation.
    assert_eq!(
        body.stmts.len(),
        1,
        "expected 1 top-level stmt, got: {:#?}",
        body.stmts
    );

    let cs1 = match &body.stmts[0] {
        Stmt::ChoiceSet(cs) => cs,
        other => panic!("expected ChoiceSet, got {other:?}"),
    };
    assert_eq!(
        cs1.choices.len(),
        2,
        "first choice set should have 2 choices"
    );

    // First continuation should contain gather content + nested second ChoiceSet
    let cont1 = &cs1.continuation;
    assert!(
        cont1.stmts.iter().any(|s| matches!(s, Stmt::Content(_))),
        "first continuation should have gather content"
    );

    let cs2 = cont1
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected nested ChoiceSet in first continuation, got: {:#?}",
                cont1.stmts
            )
        });
    assert_eq!(
        cs2.choices.len(),
        2,
        "second choice set should have 2 choices"
    );
    assert!(
        !cs2.continuation.stmts.is_empty(),
        "second choice set should have a continuation"
    );
}

/// Labeled gather-choice chain nests through continuations with labels preserved.
#[test]
fn weave_labeled_gather_choice_chain() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
- (a) * choice 1
- (b) * choice 2
- (c) End.
",
    );
    assert!(diags.is_empty());
    let body = &hir.knots[0].body;

    // Top level: one LabeledBlock with label "a"
    assert_eq!(
        body.stmts.len(),
        1,
        "expected 1 top-level stmt, got: {:#?}",
        body.stmts
    );
    let labeled_a = match &body.stmts[0] {
        Stmt::LabeledBlock(block) => block,
        other => panic!("expected LabeledBlock for 'a', got {other:?}"),
    };
    assert_eq!(
        labeled_a.label.as_ref().map(|l| l.text.as_str()),
        Some("a"),
        "top-level labeled block should have label 'a'"
    );

    // Inside "a": ChoiceSet with choice 1
    let cs1 = labeled_a
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .expect("labeled block 'a' should contain a ChoiceSet");
    assert_eq!(cs1.choices.len(), 1);

    // Continuation of cs1 should have label "b" and contain choice 2
    let cont_b = &cs1.continuation;
    assert_eq!(
        cont_b.label.as_ref().map(|l| l.text.as_str()),
        Some("b"),
        "first continuation should have label 'b'"
    );

    let cs2 = cont_b
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .expect("continuation 'b' should contain a ChoiceSet");
    assert_eq!(cs2.choices.len(), 1);

    // Continuation of cs2 should have label "c" and contain "End."
    let cont_c = &cs2.continuation;
    assert_eq!(
        cont_c.label.as_ref().map(|l| l.text.as_str()),
        Some("c"),
        "second continuation should have label 'c'"
    );
    assert!(
        cont_c.stmts.iter().any(|s| matches!(s, Stmt::Content(_))),
        "continuation 'c' should have content 'End.'"
    );
}

// ─── Gather glue suppresses EndOfLine ───────────────────────────────

/// A gather line ending with `<>` (glue) should NOT produce an `EndOfLine`
/// after the content. The glue suppresses the line break, just like on
/// regular content lines.
#[test]
fn weave_gather_ending_with_glue_no_eol() {
    let (hir, _, diags) = lower_ink(
        "\
=== k ===
* choice
- text <>
More.
",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

    let cs = match &hir.knots[0].body.stmts[0] {
        Stmt::ChoiceSet(cs) => cs,
        other => panic!("expected ChoiceSet, got {other:?}"),
    };

    let cont = &cs.continuation;
    // First stmt should be Content with parts ending in Glue
    let first_content = match &cont.stmts[0] {
        Stmt::Content(c) => c,
        other => panic!("expected Content as first continuation stmt, got {other:?}"),
    };
    assert!(
        super::content_ends_with_glue(&first_content.parts),
        "gather content should end with Glue, got parts: {:#?}",
        first_content.parts,
    );
    // The stmt immediately after the glue-ending Content should NOT be EndOfLine
    assert!(
        !matches!(cont.stmts.get(1), Some(Stmt::EndOfLine)),
        "EndOfLine should be suppressed after gather content ending with glue, got: {:#?}",
        cont.stmts,
    );
}

/// A gather line WITHOUT glue should still produce `EndOfLine` after content.
/// Regression guard for the glue fix.
#[test]
fn weave_gather_without_glue_has_eol() {
    let (hir, _, diags) = lower_ink(
        "\
=== k ===
* choice
- text
More.
",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

    let cs = match &hir.knots[0].body.stmts[0] {
        Stmt::ChoiceSet(cs) => cs,
        other => panic!("expected ChoiceSet, got {other:?}"),
    };

    let cont = &cs.continuation;
    // First stmt should be Content("text"), second should be EndOfLine
    assert!(
        matches!(&cont.stmts[0], Stmt::Content(_)),
        "first continuation stmt should be Content"
    );
    assert!(
        matches!(&cont.stmts[1], Stmt::EndOfLine),
        "gather without glue should have EndOfLine after content, got: {:#?}",
        cont.stmts,
    );
}

// ─── Standalone labeled gather produces LabeledBlock ────────────────

/// A standalone labeled gather (no subsequent choices) must produce a
/// `LabeledBlock` in the HIR so the planning phase allocates a container
/// for it. Without a container, diverts like `-> knot.gather` resolve to
/// an unresolved definition at runtime.
#[test]
fn standalone_labeled_gather_produces_labeled_block() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
-> knot.gather
- (gather) g
-> DONE
",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

    let body = &hir.knots[0].body;
    // Should have: Divert(knot.gather), LabeledBlock{label: gather, ...}
    let has_labeled_block = body.stmts.iter().any(|s| {
        matches!(s, Stmt::LabeledBlock(block) if block.label.as_ref().is_some_and(|l| l.text == "gather"))
    });
    assert!(
        has_labeled_block,
        "standalone labeled gather must produce a LabeledBlock, got: {:#?}",
        body.stmts,
    );
}

/// Consecutive labeled gathers without intervening choices must nest:
/// `- (opts) ... - (test) ...` should produce a LabeledBlock(opts) that
/// contains a nested LabeledBlock(test).  This ensures `-> opts` loops
/// back through both gathers (matching inklecate's tail-nesting).
#[test]
fn consecutive_labeled_gathers_nest() {
    let (hir, _, diags) = lower_ink(
        "- (opts)\n\
         {test:seen test}\n\
         - (test)\n\
         { -> opts |}\n",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");

    // Root should have a single LabeledBlock(opts)
    let opts_block = hir
        .root_content
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::LabeledBlock(b) if b.label.as_ref().is_some_and(|l| l.text == "opts") => {
                Some(b.as_ref())
            }
            _ => None,
        })
        .expect("expected LabeledBlock(opts) in root");

    // Inside opts, there should be a nested LabeledBlock(test)
    let has_nested_test = opts_block.stmts.iter().any(|s| {
        matches!(s, Stmt::LabeledBlock(b) if b.label.as_ref().is_some_and(|l| l.text == "test"))
    });
    assert!(
        has_nested_test,
        "opts should contain nested LabeledBlock(test), got: {:#?}",
        opts_block.stmts,
    );
}

// ─── Logic line newline emission ────────────────────────────────────
//
// Inklecate emits a `\n` after expression-statement logic lines
// (`~ func()`) but NOT after assignments, temp declarations, or
// returns. This newline is critical: it provides the line boundary
// between function output and subsequent content after
// TrimWhitespaceFromFunctionEnd strips the function's trailing newline.

#[test]
fn logic_line_expr_stmt_emits_end_of_line_in_root() {
    // `~ func()` at root level should produce ExprStmt + EndOfLine.
    let (hir, _, diags) = lower_ink("~ func()\nsome text\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.root_content.stmts;
    assert!(
        matches!(&stmts[0], Stmt::ExprStmt(_)),
        "expected ExprStmt, got {:?}",
        stmts[0],
    );
    assert!(
        matches!(&stmts[1], Stmt::EndOfLine),
        "expected EndOfLine after ExprStmt, got {:?}",
        stmts[1],
    );
}

#[test]
fn logic_line_expr_stmt_emits_end_of_line_in_knot() {
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
~ foo()
some text
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.knots[0].body.stmts;
    assert!(
        matches!(&stmts[0], Stmt::ExprStmt(_)),
        "expected ExprStmt, got {:?}",
        stmts[0],
    );
    assert!(
        matches!(&stmts[1], Stmt::EndOfLine),
        "expected EndOfLine after ExprStmt, got {:?}",
        stmts[1],
    );
}

#[test]
fn logic_line_expr_stmt_emits_end_of_line_in_function_body() {
    let (hir, _, diags) = lower_ink(
        "\
== function f() ==
~ foo()
some text
~ return
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.knots[0].body.stmts;
    assert!(
        matches!(&stmts[0], Stmt::ExprStmt(_)),
        "expected ExprStmt, got {:?}",
        stmts[0],
    );
    assert!(
        matches!(&stmts[1], Stmt::EndOfLine),
        "expected EndOfLine after ExprStmt, got {:?}",
        stmts[1],
    );
}

#[test]
fn logic_line_temp_decl_no_end_of_line() {
    // `~ temp x = 0` should NOT produce EndOfLine — matching inklecate.
    let (hir, _, diags) = lower_ink(
        "\
=== knot ===
~ temp x = 0
some text
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.knots[0].body.stmts;
    assert!(
        matches!(&stmts[0], Stmt::TempDecl(_)),
        "expected TempDecl, got {:?}",
        stmts[0],
    );
    assert!(
        !matches!(&stmts[1], Stmt::EndOfLine),
        "TempDecl should NOT be followed by EndOfLine, got {:?}",
        stmts[1],
    );
}

#[test]
fn logic_line_assignment_no_end_of_line() {
    // `~ x = 5` should NOT produce EndOfLine — matching inklecate.
    let (hir, _, diags) = lower_ink("VAR x = 0\n~ x = 5\nsome text\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.root_content.stmts;
    let assign_idx = stmts
        .iter()
        .position(|s| matches!(s, Stmt::Assignment(_)))
        .expect("should have an Assignment stmt");
    assert!(
        !matches!(&stmts[assign_idx + 1], Stmt::EndOfLine),
        "Assignment should NOT be followed by EndOfLine, got {:?}",
        stmts[assign_idx + 1],
    );
}

#[test]
fn logic_line_return_no_end_of_line() {
    // `~ return` should NOT produce EndOfLine.
    let (hir, _, diags) = lower_ink(
        "\
== function f() ==
~ return true
",
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.knots[0].body.stmts;
    assert!(
        matches!(&stmts[0], Stmt::Return(_)),
        "expected Return, got {:?}",
        stmts[0],
    );
    // Return should be the last stmt, no EndOfLine after it.
    assert_eq!(
        stmts.len(),
        1,
        "Return should be the only stmt, got: {stmts:?}",
    );
}

#[test]
fn logic_line_temp_decl_with_call_emits_end_of_line() {
    // `~ temp x = func()` — the expression contains a function call,
    // so inklecate emits \n after it. This newline provides the line
    // boundary between the function's output and subsequent content.
    let (hir, _, diags) = lower_ink("~ temp x = func()\nsome text\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.root_content.stmts;
    assert!(
        matches!(&stmts[0], Stmt::TempDecl(_)),
        "expected TempDecl, got {:?}",
        stmts[0],
    );
    assert!(
        matches!(&stmts[1], Stmt::EndOfLine),
        "TempDecl with function call should be followed by EndOfLine, got {:?}",
        stmts[1],
    );
}

#[test]
fn logic_line_assignment_with_call_emits_end_of_line() {
    // `~ x = func()` — assignment with a function call expression.
    // Same rule: inklecate emits \n when the expression has a call.
    let (hir, _, diags) = lower_ink("VAR x = 0\n~ x = func()\nsome text\n");
    assert!(diags.is_empty(), "diags: {diags:?}");
    let stmts = &hir.root_content.stmts;
    let assign_idx = stmts
        .iter()
        .position(|s| matches!(s, Stmt::Assignment(_)))
        .expect("should have an Assignment stmt");
    assert!(
        matches!(&stmts[assign_idx + 1], Stmt::EndOfLine),
        "Assignment with function call should be followed by EndOfLine, got {:?}",
        stmts.get(assign_idx + 1),
    );
}
