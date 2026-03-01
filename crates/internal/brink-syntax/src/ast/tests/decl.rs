use super::*;

// ── KnotHeader ───────────────────────────────────────────────────────

#[test]
fn knot_header_name() {
    let header = parse_first::<KnotHeader>("=== myKnot ===\n");
    assert_eq!(header.name().as_deref(), Some("myKnot"));
    assert!(!header.is_function());
}

#[test]
fn function_knot_header() {
    let header = parse_first::<KnotHeader>("== function greet(ref name) ==\n");
    assert!(header.is_function());
    assert_eq!(header.name().as_deref(), Some("greet"));
    let params = header.params().unwrap();
    let param = params.params().next().unwrap();
    assert!(param.is_ref());
    assert_eq!(param.name().as_deref(), Some("name"));
}

#[test]
fn knot_header_function_kw_token() {
    let header = parse_first::<KnotHeader>("== function greet() ==\n");
    assert!(header.function_kw().is_some());
}

#[test]
fn knot_header_no_params() {
    let header = parse_first::<KnotHeader>("=== myKnot ===\n");
    assert!(header.params().is_none());
}

// ── StitchHeader ─────────────────────────────────────────────────────

#[test]
fn stitch_header_name() {
    let tree = parse_tree("=== myKnot ===\n= myStitch\nContent\n");
    let stitch: StitchDef = first(tree.syntax());
    let header = stitch.header().unwrap();
    assert_eq!(header.name().as_deref(), Some("myStitch"));
}

// ── KnotDef ──────────────────────────────────────────────────────────

#[test]
fn knot_def_header_and_body() {
    let knot = parse_first::<KnotDef>("=== k ===\nContent\n");
    assert!(knot.header().is_some());
    assert!(knot.body().is_some());
}

// ── KnotBody ─────────────────────────────────────────────────────────

#[test]
fn knot_body_stitches() {
    let tree = parse_tree("=== k ===\n= s1\nContent\n= s2\nMore\n");
    let body: KnotBody = first(tree.syntax());
    assert_eq!(body.stitches().count(), 2);
}

#[test]
fn knot_body_content_lines() {
    let tree = parse_tree("=== k ===\nLine one\nLine two\n");
    let body: KnotBody = first(tree.syntax());
    assert_eq!(body.content_lines().count(), 2);
}

#[test]
fn knot_body_logic_lines() {
    let tree = parse_tree("=== k ===\n~ temp x = 1\n~ temp y = 2\n");
    let body: KnotBody = first(tree.syntax());
    assert_eq!(body.logic_lines().count(), 2);
}

#[test]
fn knot_body_choices() {
    let tree = parse_tree("=== k ===\n* Choice A\n* Choice B\n");
    let body: KnotBody = first(tree.syntax());
    assert_eq!(body.choices().count(), 2);
}

#[test]
fn knot_body_gathers() {
    let tree = parse_tree("=== k ===\n* Choice\n- Gather\n");
    let body: KnotBody = first(tree.syntax());
    assert_eq!(body.gathers().count(), 1);
}

// ── StitchBody ───────────────────────────────────────────────────────

#[test]
fn stitch_body_content_lines() {
    let tree = parse_tree("=== k ===\n= s\nLine one\nLine two\n");
    let body: StitchBody = first(tree.syntax());
    assert_eq!(body.content_lines().count(), 2);
}

#[test]
fn stitch_body_choices() {
    let tree = parse_tree("=== k ===\n= s\n* A\n* B\n");
    let body: StitchBody = first(tree.syntax());
    assert_eq!(body.choices().count(), 2);
}

#[test]
fn stitch_body_gathers() {
    let tree = parse_tree("=== k ===\n= s\n* A\n- Gather\n");
    let body: StitchBody = first(tree.syntax());
    assert_eq!(body.gathers().count(), 1);
}

// ── VarDecl ──────────────────────────────────────────────────────────

#[test]
fn var_decl_name() {
    let tree = parse_tree("VAR x = 5\n");
    let decl = tree.var_decls().next().unwrap();
    assert_eq!(decl.name().as_deref(), Some("x"));
}

// ── ConstDecl ────────────────────────────────────────────────────────

#[test]
fn const_decl_name() {
    let tree = parse_tree("CONST pi = 3\n");
    let decl = tree.const_decls().next().unwrap();
    assert_eq!(decl.name().as_deref(), Some("pi"));
}

// ── ListDecl ─────────────────────────────────────────────────────────

#[test]
fn list_decl_name() {
    let tree = parse_tree("LIST items = sword, shield\n");
    let decl = tree.list_decls().next().unwrap();
    assert_eq!(decl.name().as_deref(), Some("items"));
}

#[test]
fn list_decl_definition() {
    let tree = parse_tree("LIST items = sword, shield\n");
    let decl = tree.list_decls().next().unwrap();
    let def = decl.definition().unwrap();
    assert_eq!(def.members().count(), 2);
}

#[test]
fn list_member_on_name() {
    let tree = parse_tree("LIST items = (sword), shield\n");
    let member_on: ListMemberOn = first(tree.syntax());
    assert_eq!(member_on.name().as_deref(), Some("sword"));
}

#[test]
fn list_member_off_name() {
    let tree = parse_tree("LIST items = sword, shield\n");
    let member_off: ListMemberOff = first(tree.syntax());
    assert_eq!(member_off.name().as_deref(), Some("sword"));
}

// ── TempDecl ─────────────────────────────────────────────────────────

#[test]
fn temp_decl_name() {
    let temp = parse_first::<TempDecl>("=== k ===\n~ temp x = 1\n");
    assert_eq!(temp.name().as_deref(), Some("x"));
}

#[test]
fn temp_decl_eq_token() {
    let temp = parse_first::<TempDecl>("=== k ===\n~ temp x = 1\n");
    assert!(temp.eq_token().is_some());
}

#[test]
fn temp_decl_value() {
    let temp = parse_first::<TempDecl>("=== k ===\n~ temp x = 42\n");
    let val = temp.value().unwrap();
    assert!(matches!(val, Expr::IntegerLit(_)));
}

#[test]
fn temp_decl_value_expr() {
    let temp = parse_first::<TempDecl>("=== k ===\n~ temp x = 1 + 2\n");
    let val = temp.value().unwrap();
    assert!(matches!(val, Expr::Infix(_)));
}

// ── VarDecl value ────────────────────────────────────────────────────

#[test]
fn var_decl_value_integer() {
    let tree = parse_tree("VAR x = 5\n");
    let decl = tree.var_decls().next().unwrap();
    let val = decl.value().unwrap();
    assert!(matches!(val, Expr::IntegerLit(_)));
}

#[test]
fn var_decl_value_string() {
    let tree = parse_tree("VAR x = \"hello\"\n");
    let decl = tree.var_decls().next().unwrap();
    let val = decl.value().unwrap();
    assert!(matches!(val, Expr::StringLit(_)));
}

// ── ConstDecl value ──────────────────────────────────────────────────

#[test]
fn const_decl_value_integer() {
    let tree = parse_tree("CONST pi = 3\n");
    let decl = tree.const_decls().next().unwrap();
    let val = decl.value().unwrap();
    assert!(matches!(val, Expr::IntegerLit(_)));
}

#[test]
fn const_decl_value_float() {
    let tree = parse_tree("CONST pi = 3.14\n");
    let decl = tree.const_decls().next().unwrap();
    let val = decl.value().unwrap();
    assert!(matches!(val, Expr::FloatLit(_)));
}

// ── ListMemberOn / ListMemberOff value ──────────────────────────────

#[test]
fn list_member_on_value() {
    let member = parse_first::<ListMemberOn>("LIST mood = (happy = 3)\n");
    assert_eq!(member.value(), Some(3));
}

#[test]
fn list_member_on_no_value() {
    let member = parse_first::<ListMemberOn>("LIST items = (sword), shield\n");
    assert_eq!(member.value(), None);
}

#[test]
fn list_member_off_value() {
    let member = parse_first::<ListMemberOff>("LIST mood = sad = 1, happy\n");
    assert_eq!(member.value(), Some(1));
}

#[test]
fn list_member_off_no_value() {
    let member = parse_first::<ListMemberOff>("LIST items = sword, shield\n");
    assert_eq!(member.value(), None);
}

// ── ExternalDecl ─────────────────────────────────────────────────────

#[test]
fn external_decl_name() {
    let tree = parse_tree("EXTERNAL greet(a, b)\n");
    let ext = tree.externals().next().unwrap();
    assert_eq!(ext.name().as_deref(), Some("greet"));
    let params = ext.param_list().unwrap();
    let names: Vec<_> = params.params().filter_map(|id| id.name()).collect();
    assert_eq!(names, vec!["a", "b"]);
}

// ── KnotParamDecl ────────────────────────────────────────────────────

#[test]
fn knot_param_decl_ref() {
    let param = parse_first::<KnotParamDecl>("== function f(ref x) ==\n");
    assert!(param.is_ref());
    assert_eq!(param.name().as_deref(), Some("x"));
}

#[test]
fn knot_param_decl_divert() {
    let param = parse_first::<KnotParamDecl>("== function f(-> x) ==\n");
    assert!(param.is_divert());
}

#[test]
fn knot_param_decl_plain() {
    let param = parse_first::<KnotParamDecl>("== function f(x) ==\n");
    assert!(!param.is_ref());
    assert!(!param.is_divert());
    assert_eq!(param.name().as_deref(), Some("x"));
}

// ── LogicLine ────────────────────────────────────────────────────────

#[test]
fn logic_line_return_stmt() {
    let ll = parse_first::<LogicLine>("== function f() ==\n~ return 1\n");
    assert!(ll.return_stmt().is_some());
}

#[test]
fn logic_line_temp_decl() {
    let ll = parse_first::<LogicLine>("=== k ===\n~ temp x = 1\n");
    assert!(ll.temp_decl().is_some());
}

#[test]
fn logic_line_assignment() {
    let ll = parse_first::<LogicLine>("=== k ===\n~ x = 1\n");
    assert!(ll.assignment().is_some());
}

// ── SourceFile iterators ─────────────────────────────────────────────

#[test]
fn source_file_includes() {
    let tree = parse_tree("INCLUDE helper.ink\n");
    assert_eq!(tree.includes().count(), 1);
    let inc = tree.includes().next().unwrap();
    let fp = inc.file_path().unwrap();
    assert_eq!(fp.text(), "helper.ink");
}

#[test]
fn source_file_multiple_knots() {
    let tree = parse_tree("=== a ===\nContent\n=== b ===\nMore\n");
    assert_eq!(tree.knots().count(), 2);
}

// ── ListExpr items ───────────────────────────────────────────────────

#[test]
fn list_expr_items_multi() {
    let le = parse_first::<ListExpr>("VAR x = (a, b, c)\n");
    let names: Vec<_> = le.items().map(|p| p.full_name()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn list_expr_items_single() {
    let le = parse_first::<ListExpr>("VAR x = (a)\n");
    let names: Vec<_> = le.items().map(|p| p.full_name()).collect();
    assert_eq!(names, vec!["a"]);
}

#[test]
fn list_expr_items_dotted() {
    let le = parse_first::<ListExpr>("VAR x = (a.b, c.d)\n");
    let names: Vec<_> = le.items().map(|p| p.full_name()).collect();
    assert_eq!(names, vec!["a.b", "c.d"]);
}

#[test]
fn list_expr_items_dotted_segments() {
    let le = parse_first::<ListExpr>("VAR x = (a.b)\n");
    let path = le.items().next().unwrap();
    assert_eq!(path.segments().count(), 2);
}
