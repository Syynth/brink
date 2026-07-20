use super::*;
use crate::parser::parse;

#[test]
fn source_file_casts() {
    let parse = parse("flow greet() {\n  Hello!\n}\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    assert_eq!(file.flows().count(), 1);
}

#[test]
fn flow_decl_accessors() {
    let parse = parse("flow greet(name, ref hp) {\n  Hi, {name}!\n}\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let flow = file.flows().next().expect("one flow");
    assert_eq!(flow.name_token().expect("name").text(), "greet");
    let params: Vec<_> = flow.param_list().expect("param list").params().collect();
    assert_eq!(params.len(), 2);
    assert!(!params[0].is_ref());
    assert!(params[1].is_ref());
    assert!(flow.body().is_some());
}

#[test]
fn stitches_are_nested_flows() {
    let parse = parse("flow garden() {\n  flow gate() {\n    Creak.\n  }\n}\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let garden = file.flows().next().expect("garden");
    assert_eq!(garden.stitches().count(), 1);
}

#[test]
fn cast_rejects_wrong_kind() {
    let parse = parse("flow greet() {}\n");
    // The root is SOURCE_FILE, not FLOW_DECL — casting the root as a
    // FlowDecl must fail rather than panicking.
    assert!(FlowDecl::cast(parse.syntax()).is_none());
}
