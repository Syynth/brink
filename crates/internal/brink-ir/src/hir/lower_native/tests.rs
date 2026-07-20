use super::*;
use crate::DiagnosticCode;

fn lower_src(src: &str) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
    let parse = brink_syntax_native::parse(src);
    let tree = parse.tree();
    lower(FileId(0), &tree)
}

#[test]
fn top_level_flow_lowers_to_knot() {
    let (hir, manifest, diags) = lower_src("flow greet(name) {\n  Hi, {name}!\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots.len(), 1);
    let knot = &hir.knots[0];
    assert_eq!(knot.name.text, "greet");
    assert!(!knot.is_function);
    assert_eq!(knot.params.len(), 1);
    assert_eq!(knot.params[0].name.text, "name");
    assert!(knot.body.stmts.is_empty(), "body must be the empty stub");
    assert_eq!(manifest.knots.len(), 1);
    assert_eq!(manifest.knots[0].name, "greet");
}

#[test]
fn fn_decl_sets_is_function() {
    let (hir, manifest, diags) = lower_src("fn heal(hp) {\n  Heal.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots.len(), 1);
    assert!(hir.knots[0].is_function);
    assert_eq!(
        manifest.knots[0].detail.as_deref(),
        Some("function"),
        "is_function must agree with the manifest's function sentinel (E123)"
    );
}

#[test]
fn nested_flow_becomes_a_stitch() {
    let (hir, manifest, diags) =
        lower_src("flow garden() {\n  flow gate(ref hp) {\n    Creak.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots.len(), 1);
    assert_eq!(hir.knots[0].stitches.len(), 1);
    let stitch = &hir.knots[0].stitches[0];
    assert_eq!(stitch.name.text, "gate");
    assert_eq!(stitch.params.len(), 1);
    assert!(stitch.params[0].is_ref);
    assert_eq!(manifest.stitches.len(), 1);
    assert_eq!(
        manifest.stitches[0].name, "garden.gate",
        "knot.stitch qualification"
    );
}

#[test]
fn depth_three_nesting_is_rejected_loudly() {
    let (hir, _manifest, diags) =
        lower_src("flow a() {\n  flow b() {\n    flow c() {\n      Too deep.\n    }\n  }\n}\n");
    assert_eq!(hir.knots.len(), 1);
    assert_eq!(hir.knots[0].stitches.len(), 1, "b still lowers as a stitch");
    assert!(hir.knots[0].stitches[0].name.text.eq("b"));
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E130),
        "expected E130 for the depth-3 flow c(): {diags:?}"
    );
}

#[test]
fn nested_fn_is_not_yet_lowered() {
    let (hir, _manifest, diags) = lower_src("flow a() {\n  fn b() {\n    x\n  }\n}\n");
    assert_eq!(hir.knots.len(), 1);
    assert!(hir.knots[0].stitches.is_empty());
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::E129));
}

#[test]
fn var_const_flags_lower_and_hoist_globally() {
    let (hir, manifest, diags) = lower_src(
        "var hp = 10\nconst max_hp = 100\nflags Mood = (calm), wary, hostile\nflow a() {\n  var nested = 1\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.variables.len(),
        2,
        "top-level hp + nested-in-flow-body var"
    );
    assert_eq!(hir.constants.len(), 1);
    assert_eq!(hir.lists.len(), 1);
    assert_eq!(hir.lists[0].members.len(), 3);
    assert!(hir.lists[0].members[0].is_active);
    assert!(!hir.lists[0].members[1].is_active);
    assert_eq!(manifest.variables.len(), 2);
    assert_eq!(manifest.constants.len(), 1);
    assert_eq!(manifest.lists.len(), 1);
    assert_eq!(manifest.list_items.len(), 3);
}

#[test]
fn struct_and_extern_lower_at_top_level() {
    let (hir, manifest, diags) =
        lower_src("struct Npc {\n  name: string,\n  hp: int\n}\nextern do_thing(a, ref b)\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.structs.len(), 1);
    assert_eq!(hir.structs[0].fields.len(), 2);
    assert_eq!(hir.externals.len(), 1);
    assert_eq!(hir.externals[0].param_count, 2);
    assert_eq!(hir.externals[0].params.len(), 2);
    assert!(
        !hir.externals[0].params[1].is_ref,
        "EXTERNAL params always report is_ref=false, matching ink's convention"
    );
    assert_eq!(manifest.structs.len(), 1);
    assert_eq!(manifest.externals.len(), 1);
}

#[test]
fn struct_nested_in_a_flow_body_is_not_silently_dropped() {
    let (hir, _manifest, diags) = lower_src("flow a() {\n  struct Npc {\n    hp: int\n  }\n}\n");
    assert!(hir.structs.is_empty(), "not lowered — out of position");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "must diagnose, not silently drop: {diags:?}"
    );
}

#[test]
fn use_decl_lowers_to_import() {
    let (hir, _manifest, diags) = lower_src("use story::market::{barter, haggle as h};\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    let imp = &hir.imports[0];
    assert_eq!(imp.module, "story.market");
    assert!(imp.bare);
    assert_eq!(imp.items.len(), 2);
    assert_eq!(imp.items[0].name, "barter");
    assert_eq!(imp.items[0].alias, None);
    assert_eq!(imp.items[1].name, "haggle");
    assert_eq!(imp.items[1].alias.as_deref(), Some("h"));
}

#[test]
fn qualified_use_decl_lowers_to_bare_false_import() {
    let (hir, _manifest, diags) = lower_src("use story::market;\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    assert!(!hir.imports[0].bare);
    assert!(hir.imports[0].items.is_empty());
}

#[test]
fn import_decl_lowers_to_qualified_import() {
    let (hir, _manifest, diags) = lower_src("import story::market\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    assert_eq!(hir.imports[0].module, "story.market");
    assert!(!hir.imports[0].bare);
}

#[test]
fn module_block_is_flagged_and_flattened() {
    let (hir, manifest, diags) = lower_src("module npcs {\n  flow greet() {\n    Hi!\n  }\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "module nesting must be flagged: {diags:?}"
    );
    assert_eq!(
        hir.knots.len(),
        1,
        "contents still flattened into the file scope"
    );
    assert_eq!(hir.knots[0].name.text, "greet");
    assert_eq!(manifest.knots.len(), 1);
}

#[test]
fn root_content_is_always_empty() {
    let (hir, _manifest, _diags) = lower_src("flow a() {}\n");
    assert!(hir.root_content.stmts.is_empty());
    assert!(hir.includes.is_empty());
    assert!(hir.module.is_none());
}

#[test]
fn stray_top_level_content_is_diagnosed_not_dropped() {
    let (_hir, _manifest, diags) = lower_src("Just some loose prose.\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "stray content must not vanish silently: {diags:?}"
    );
}

/// The declaration-only gate fixture. A copy of the same source lives in
/// `tests/b06_native_declarations.rs`'s admission-clean check — kept
/// duplicated rather than shared because an in-`lib` unit test cannot link
/// `brink-analyzer` (see that integration test file's module doc for why),
/// so the two checks (diagnostic-free lowering; zero-diagnostic admission)
/// necessarily live in different compilation units.
const DECLARATION_FIXTURE: &str = "\
var hp = 10
const max_hp = 100
flags Mood = (calm), wary, hostile
struct Npc {
  name: string,
  hp: int
}
extern do_thing(a, ref b)
use story::market::{barter, haggle as h};
import story::npcs

flow garden(mood) {
  flow gate(ref visits) {
    Creak.
  }
}

fn heal(target, amount) {
  Heal.
}
";

#[test]
fn well_formed_declaration_fixture_lowers_with_no_diagnostics() {
    let (_hir, _manifest, diags) = lower_src(DECLARATION_FIXTURE);
    assert!(
        diags.is_empty(),
        "declaration-only fixture must be diagnostic-clean: {diags:?}"
    );
}
