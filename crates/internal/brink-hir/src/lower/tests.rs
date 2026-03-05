#![allow(clippy::panic)]

use brink_syntax::parse;

use crate::lower::lower;
use crate::*;

fn lower_ink(source: &str) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
    let parsed = parse(source);
    let tree = parsed.tree();
    lower(&tree)
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
    assert_eq!(hir.root_content.stmts.len(), 1);
    match &hir.root_content.stmts[0] {
        Stmt::Content(c) => {
            assert!(!c.parts.is_empty());
            assert!(matches!(&c.parts[0], ContentPart::Text(t) if t.contains("Hello")));
        }
        other => panic!("expected Content, got {other:?}"),
    }
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
