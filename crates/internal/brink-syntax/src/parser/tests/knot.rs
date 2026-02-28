use super::check;
use crate::parse;

#[test]
fn basic_knot() {
    check("== myKnot ==\nHello.\n");
}

#[test]
fn knot_triple_equals() {
    check("=== myKnot ===\nHello.\n");
}

#[test]
fn knot_no_trailing_equals() {
    check("== myKnot\nHello.\n");
}

#[test]
fn function_knot() {
    check("== function greet ==\nHi!\n");
}

#[test]
fn knot_with_params() {
    check("== greet(name) ==\nHi {name}.\n");
}

#[test]
fn knot_with_ref_param() {
    check("== modify(ref x, y) ==\nDone.\n");
}

#[test]
fn stitch() {
    check("== myKnot ==\n= myStitch\nContent.\n");
}

#[test]
fn multiple_stitches() {
    check("== myKnot ==\n= stitch1\nA.\n= stitch2\nB.\n");
}

#[test]
fn knot_terminates_at_external() {
    // EXTERNAL after a knot should NOT be absorbed into the knot body.
    let p = parse("=== function SetBrightness(x) ===\n~ return\nEXTERNAL GetGreeting()\n");
    let src = p.syntax().text().to_string();
    assert_eq!(
        src, "=== function SetBrightness(x) ===\n~ return\nEXTERNAL GetGreeting()\n",
        "lossless"
    );
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let dbg = format!("{:#?}", p.syntax());
    assert!(
        dbg.contains("EXTERNAL_DECL"),
        "EXTERNAL should be a top-level declaration, not content inside knot body"
    );
}

#[test]
fn knot_absorbs_var_decl() {
    // VAR inside a knot body should be absorbed into the body, not ejected.
    let p = parse("=== myKnot ===\nHello.\nVAR x = 5\n");
    let dbg = format!("{:#?}", p.syntax());
    assert!(
        dbg.contains("VAR_DECL"),
        "VAR_DECL should be present in the tree"
    );
    let root = p.syntax();
    let var_decl = root
        .descendants()
        .find(|n| n.kind() == crate::SyntaxKind::VAR_DECL)
        .expect("VAR_DECL not found");
    let parent = var_decl.parent().expect("VAR_DECL has no parent");
    assert_eq!(
        parent.kind(),
        crate::SyntaxKind::KNOT_BODY,
        "VAR_DECL should be inside KNOT_BODY, found in {:?}",
        parent.kind()
    );
}

#[test]
fn insta_knot() {
    let p = parse("=== myKnot ===\nHello.\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_function_knot() {
    let p = parse("== function greet(ref name) ==\nHi!\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_stitch() {
    let p = parse("== myKnot ==\n= myStitch\nContent.\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
