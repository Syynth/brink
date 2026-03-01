mod cst;

use super::check;
use crate::parse;

#[test]
fn include_basic() {
    check("INCLUDE story.ink\n");
}

#[test]
fn include_path_with_slashes() {
    check("INCLUDE path/to/story.ink\n");
}

#[test]
fn external_no_params() {
    check("EXTERNAL myFunc()\n");
}

#[test]
fn external_with_params() {
    check("EXTERNAL myFunc(a, b, c)\n");
}

#[test]
fn var_declaration() {
    check("VAR x = 5\n");
}

#[test]
fn const_declaration() {
    check("CONST PI = 3\n");
}

#[test]
fn list_simple() {
    check("LIST colors = red, green, blue\n");
}

#[test]
fn list_with_values() {
    check("LIST items = (sword = 1), shield, (potion = 3)\n");
}

/// Ink keywords are contextual — `or`, `and`, `not`, `mod`, `has`, `hasnt`,
/// `true`, `false`, `else`, `done`, `end`, `ref`, etc. may all appear as
/// list member names.
#[test]
fn list_keyword_member_off() {
    check("LIST items = or, and, not\n");
}

#[test]
fn list_keyword_member_on() {
    check("LIST items = (or), (and), (not)\n");
}

#[test]
fn list_mixed_keyword_members() {
    check("LIST items = (true), false, (mod = 1), has\n");
}

/// Exact corpus pattern from Jonkeevy `FUNC_NameGenerator.ink`
#[test]
fn list_corpus_midsyll_demonic() {
    check("LIST Midsyll_Demonic = (ng), (ik), (yek), (roth), (och), (ra), (or), (gor)\n");
}

#[test]
fn insta_include() {
    let p = parse("INCLUDE story.ink\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_external() {
    let p = parse("EXTERNAL myFunc(a, b)\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_list() {
    let p = parse("LIST items = (sword = 1), shield\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
