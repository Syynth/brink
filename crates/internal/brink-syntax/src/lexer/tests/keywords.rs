use super::*;
use SyntaxKind::*;

#[test]
fn all_keywords() {
    assert_eq!(kinds("INCLUDE"), vec![KW_INCLUDE]);
    assert_eq!(kinds("EXTERNAL"), vec![KW_EXTERNAL]);
    assert_eq!(kinds("VAR"), vec![KW_VAR]);
    assert_eq!(kinds("CONST"), vec![KW_CONST]);
    assert_eq!(kinds("LIST"), vec![KW_LIST]);
    assert_eq!(kinds("temp"), vec![KW_TEMP]);
    assert_eq!(kinds("return"), vec![KW_RETURN]);
    assert_eq!(kinds("ref"), vec![KW_REF]);
    assert_eq!(kinds("true"), vec![KW_TRUE]);
    assert_eq!(kinds("false"), vec![KW_FALSE]);
    assert_eq!(kinds("not"), vec![KW_NOT]);
    assert_eq!(kinds("and"), vec![KW_AND]);
    assert_eq!(kinds("or"), vec![KW_OR]);
    assert_eq!(kinds("mod"), vec![KW_MOD]);
    assert_eq!(kinds("has"), vec![KW_HAS]);
    assert_eq!(kinds("hasnt"), vec![KW_HASNT]);
    assert_eq!(kinds("else"), vec![KW_ELSE]);
    assert_eq!(kinds("function"), vec![KW_FUNCTION]);
    assert_eq!(kinds("stopping"), vec![KW_STOPPING]);
    assert_eq!(kinds("cycle"), vec![KW_CYCLE]);
    assert_eq!(kinds("shuffle"), vec![KW_SHUFFLE]);
    assert_eq!(kinds("once"), vec![KW_ONCE]);
    assert_eq!(kinds("DONE"), vec![KW_DONE]);
    assert_eq!(kinds("END"), vec![KW_END]);
    assert_eq!(kinds("TODO"), vec![KW_TODO]);
}

#[test]
fn keyword_prefix_is_ident() {
    assert_eq!(kinds("returning"), vec![IDENT]);
    assert_eq!(kinds("hasntx"), vec![IDENT]);
    assert_eq!(kinds("EXTERNAL2"), vec![IDENT]);
}

#[test]
fn keywords_are_case_sensitive() {
    assert_eq!(kinds("include"), vec![IDENT]);
    assert_eq!(kinds("var"), vec![IDENT]);
    assert_eq!(kinds("Temp"), vec![IDENT]);
    assert_eq!(kinds("TRUE"), vec![IDENT]);
    assert_eq!(kinds("False"), vec![IDENT]);
}
