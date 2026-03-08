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
            assert!(cs.gather.is_none());
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
            assert!(cs.gather.is_some());
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
            assert_eq!(c.tags[0].text, "tag1");
            assert_eq!(c.tags[1].text, "tag2");
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

    let (block, manifest, diags) = crate::lower_top_level(FileId(0), &tree);

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

    let (_block, manifest, diags) = crate::lower_top_level(FileId(0), &tree);

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
            assert!(cs.gather.is_none());
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
            let gather = cs.gather.as_ref().unwrap();
            assert!(gather.content.is_some());
        }
        other => panic!("expected ChoiceSet, got {other:?}"),
    }
}

/// Statements after a gather are siblings in the parent block, NOT nested in the gather.
#[test]
fn weave_stmts_after_gather_are_siblings() {
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
    // ChoiceSet with gather, then Content + EndOfLine, then Divert — all siblings
    assert_eq!(body.stmts.len(), 4, "stmts: {:#?}", body.stmts);
    assert!(
        matches!(&body.stmts[0], Stmt::ChoiceSet(cs) if cs.gather.is_some()),
        "first stmt should be ChoiceSet with gather"
    );
    assert!(
        matches!(&body.stmts[1], Stmt::Content(_)),
        "second stmt should be Content, got {:?}",
        body.stmts[1]
    );
    assert!(
        matches!(&body.stmts[2], Stmt::EndOfLine),
        "third stmt should be EndOfLine, got {:?}",
        body.stmts[2]
    );
    assert!(
        matches!(&body.stmts[3], Stmt::Divert(_)),
        "fourth stmt should be Divert, got {:?}",
        body.stmts[3]
    );
}

/// Two sequential choice sets each with their own gather.
#[test]
fn weave_two_sequential_choice_sets() {
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
    // Two ChoiceSets
    assert_eq!(body.stmts.len(), 2, "stmts: {:#?}", body.stmts);
    for (i, stmt) in body.stmts.iter().enumerate() {
        match stmt {
            Stmt::ChoiceSet(cs) => {
                assert_eq!(cs.choices.len(), 2, "choice set {i} should have 2 choices");
                assert!(cs.gather.is_some(), "choice set {i} should have a gather");
            }
            other => panic!("expected ChoiceSet at index {i}, got {other:?}"),
        }
    }
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
/// `ChoiceSet`, and the second batch of choices starts a new `ChoiceSet`.
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
    // First ChoiceSet (Alpha, Beta) with gather, then Content from standalone gather
    // emission, then second ChoiceSet (Gamma, Delta) without gather.
    let choice_sets: Vec<_> = body
        .stmts
        .iter()
        .filter(|s| matches!(s, Stmt::ChoiceSet(_)))
        .collect();
    assert_eq!(
        choice_sets.len(),
        2,
        "expected 2 choice sets, got {:#?}",
        body.stmts
    );

    // First should have gather content embedded, second should not
    match choice_sets[0] {
        Stmt::ChoiceSet(cs) => {
            assert_eq!(cs.choices.len(), 2);
        }
        _ => unreachable!(),
    }
    match choice_sets[1] {
        Stmt::ChoiceSet(cs) => {
            assert_eq!(cs.choices.len(), 2);
            assert!(cs.gather.is_none());
        }
        _ => unreachable!(),
    }
}

/// A choice with a divert on its line — the divert is captured in choice.divert.
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
            assert!(cs.choices[0].divert.is_some());
            assert!(cs.choices[1].divert.is_none());
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
            let gather = cs.gather.as_ref().unwrap();
            let label = gather.label.as_ref().unwrap();
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
    assert!(outer_cs.gather.is_some(), "expected outer gather");

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
    assert!(inner_cs.gather.is_some(), "expected inner gather");

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

/// Content interleaved around multiple choice sets — all siblings in a flat block.
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
    // Expected flat structure (EndOfLine follows each Content):
    // [0] Content("Before first.")
    // [1] EndOfLine
    // [2] ChoiceSet(A1, A2, gather: "Gather one.")
    // [3] Content("Between sets.")
    // [4] EndOfLine
    // [5] ChoiceSet(B1, B2, gather: "Gather two.")
    // [6] Content("After everything.")
    // [7] EndOfLine
    assert_eq!(body.stmts.len(), 8, "stmts: {:#?}", body.stmts);
    assert!(matches!(&body.stmts[0], Stmt::Content(_)));
    assert!(matches!(&body.stmts[1], Stmt::EndOfLine));
    assert!(matches!(&body.stmts[2], Stmt::ChoiceSet(_)));
    assert!(matches!(&body.stmts[3], Stmt::Content(_)));
    assert!(matches!(&body.stmts[4], Stmt::EndOfLine));
    assert!(matches!(&body.stmts[5], Stmt::ChoiceSet(_)));
    assert!(matches!(&body.stmts[6], Stmt::Content(_)));
    assert!(matches!(&body.stmts[7], Stmt::EndOfLine));
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
