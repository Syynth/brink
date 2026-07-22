use super::*;
use crate::ast::support;
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

#[test]
fn call_expr_callee_resolves_the_path_not_a_path_expr() {
    // B0.6 finding: `expr::path_or_call` wraps a bare `PATH` node inside
    // `CALL_EXPR`, never a `PATH_EXPR` — the old `Option<PathExpr>` accessor
    // could never succeed.
    let parse = parse("const x = greet(1, 2)\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let const_decl = support::child::<ConstDecl>(file.syntax()).expect("const decl");
    let call = support::child::<CallExpr>(const_decl.syntax()).expect("call expr");
    let callee = call.callee().expect("callee path resolves");
    let segs: Vec<_> = callee.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["greet".to_string()]);
    assert!(call.arg_list().expect("arg list").is_open());
}

#[test]
fn var_decl_value_is_the_initializer_node() {
    let parse = parse("var hp = 10\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let var_decl = support::child::<VarDecl>(file.syntax()).expect("var decl");
    let value = var_decl.value().expect("initializer node");
    assert_eq!(value.kind(), crate::SyntaxKind::INTEGER_LIT);
}

#[test]
fn flags_decl_members() {
    let parse = parse("flags Mood = (calm), wary, hostile\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let flags = support::child::<FlagsDecl>(file.syntax()).expect("flags decl");
    assert_eq!(flags.name_token().expect("name").text(), "Mood");
    let members: Vec<_> = flags
        .member_list()
        .expect("member list")
        .members()
        .collect();
    assert_eq!(members.len(), 3);
    assert!(members[0].is_active());
    assert!(!members[1].is_active());
    assert_eq!(members[0].name_token().expect("name").text(), "calm");
}

#[test]
fn struct_decl_fields() {
    let parse = parse("struct Npc { name: string, hp: int }\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let sd = support::child::<StructDecl>(file.syntax()).expect("struct decl");
    assert_eq!(sd.name_token().expect("name").text(), "Npc");
    let fields: Vec<_> = sd.fields().collect();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name_token().expect("name").text(), "name");
    let ty_segs: Vec<_> = fields[0]
        .type_path()
        .expect("type path")
        .segments()
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(ty_segs, vec!["string".to_string()]);
}

#[test]
fn extern_decl_params() {
    let parse = parse("extern do_thing(a, ref b)\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let ed = support::child::<ExternDecl>(file.syntax()).expect("extern decl");
    assert_eq!(ed.name_token().expect("name").text(), "do_thing");
    let params: Vec<_> = ed.param_list().expect("param list").params().collect();
    assert_eq!(params.len(), 2);
    assert!(!params[0].is_ref());
    assert!(params[1].is_ref());
}

#[test]
fn use_tree_path_segments_and_alias() {
    let parse = parse("use story::market::{barter, haggle as h};\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let use_decl = support::child::<UseDecl>(file.syntax()).expect("use decl");
    let tree = use_decl.tree().expect("use tree");
    let segs: Vec<_> = tree.path_segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["story".to_string(), "market".to_string()]);
    let nested = tree.nested_list().expect("nested list");
    let items: Vec<_> = nested.trees().collect();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[1]
            .path_segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["haggle".to_string()]
    );
    assert_eq!(items[1].alias_token().expect("alias").text(), "h");
}

#[test]
fn module_decl_accessors() {
    let parse = parse("module npcs {\n  flow greet() {\n    Hi!\n  }\n}\n");
    let file = SourceFile::cast(parse.syntax()).expect("SOURCE_FILE root");
    let md = support::child::<ModuleDecl>(file.syntax()).expect("module decl");
    assert_eq!(md.name_token().expect("name").text(), "npcs");
    assert!(md.body().is_some());
}
