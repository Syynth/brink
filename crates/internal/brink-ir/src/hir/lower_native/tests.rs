#![allow(clippy::panic)]

use super::*;
use crate::{BlockStmt, DiagnosticCode, ElseBranch, Name, Tail, Terminator};
use rowan::TextRange;

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
    // B0.7 (`docs/b0-sequencing.md` §B0.7): bodies are real prose-dialect
    // lowering now, not the B0.6-era empty stub — see
    // `hir::lower_native::body`'s own test module for full construct
    // coverage; this fixture only checks that *something* lowered.
    assert!(!knot.body.stmts.is_empty(), "body must no longer be a stub");
    assert_eq!(manifest.knots.len(), 1);
    assert_eq!(manifest.knots[0].name, "greet");
}

#[test]
fn fn_decl_sets_is_function() {
    // Plain `{ }` on a `fn` is code-ground by default (charter §4, RULED
    // 2026-07-23, #1309) — `return hp;` exercises that default, not the
    // `>{ }` prose override (which has its own coverage elsewhere).
    let (hir, manifest, diags) = lower_src("fn heal(hp) {\n  return hp;\n}\n");
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
fn leading_doc_comment_populates_knot_doc() {
    // B0.6b end-to-end: a `///`-documented top-level flow lowers with
    // `doc: Some(..)`, the `@param` tag parsed via the shared
    // `hir::doc_block::parse_lines`.
    let (hir, _manifest, diags) = lower_src(
        "/// Greets the player.\n/// @param name {string}\nflow greet(name) {\n  Hi!\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let knot = &hir.knots[0];
    let doc = knot.doc.as_ref().expect("doc attached");
    assert_eq!(doc.doc.as_deref(), Some("Greets the player."));
    assert_eq!(
        doc.params,
        vec![("name".to_string(), crate::TypeRef("string".to_string()))]
    );
}

#[test]
fn malformed_param_in_leading_doc_reports_e038() {
    let (hir, _manifest, diags) = lower_src("/// @param name\nflow greet(name) {\n}\n");
    assert!(hir.knots[0].doc.is_none(), "no valid tags -> no DocBlock");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E038),
        "expected E038 for the malformed @param: {diags:?}"
    );
}

#[test]
fn inner_doc_populates_knot_doc_when_no_leading_doc() {
    let (hir, _manifest, diags) =
        lower_src("flow greet() {\n//! Describes this flow from within.\nHi!\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let doc = hir.knots[0].doc.as_ref().expect("inner doc attached");
    assert_eq!(doc.doc.as_deref(), Some("Describes this flow from within."));
}

#[test]
fn leading_doc_wins_over_inner_doc_when_both_present() {
    let src = "/// Outer doc.\nflow greet() {\n//! Inner doc.\nHi!\n}\n";
    let (hir, _manifest, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let doc = hir.knots[0].doc.as_ref().expect("doc attached");
    assert_eq!(doc.doc.as_deref(), Some("Outer doc."));
}

#[test]
fn leading_doc_comment_populates_stitch_doc() {
    let src = "flow garden() {\n  /// The gate stitch.\n  flow gate() {\n    Creak.\n  }\n}\n";
    let (hir, _manifest, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let stitch = &hir.knots[0].stitches[0];
    let doc = stitch.doc.as_ref().expect("doc attached");
    assert_eq!(doc.doc.as_deref(), Some("The gate stitch."));
}

#[test]
fn leading_doc_comment_populates_var_const_flags_struct_extern_doc() {
    let src = "\
/// Player health.
var hp = 10
/// Max health.
const max_hp = 100
/// Mood states.
flags Mood = calm, wary
/// An NPC.
struct Npc {\n  hp: int\n}
/// Logs a message.
extern log_msg(msg)
";
    let (hir, _manifest, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.variables[0].doc.as_ref().and_then(|d| d.doc.clone()),
        Some("Player health.".to_string())
    );
    assert_eq!(
        hir.constants[0].doc.as_ref().and_then(|d| d.doc.clone()),
        Some("Max health.".to_string())
    );
    assert_eq!(
        hir.lists[0].doc.as_ref().and_then(|d| d.doc.clone()),
        Some("Mood states.".to_string())
    );
    assert_eq!(
        hir.structs[0].doc.as_ref().and_then(|d| d.doc.clone()),
        Some("An NPC.".to_string())
    );
    assert_eq!(
        hir.externals[0].doc.as_ref().and_then(|d| d.doc.clone()),
        Some("Logs a message.".to_string())
    );
}

#[test]
fn undocumented_declarations_have_no_doc_native() {
    let (hir, _manifest, diags) = lower_src("var hp = 10\nflow greet() {\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.variables[0].doc.is_none());
    assert!(hir.knots[0].doc.is_none());
}

// ── `pub` — the native visibility marker (issue #1582, RULED 2026-08-03) ──

#[test]
fn pub_flow_and_fn_lower_with_public_visibility() {
    let (hir, _manifest, diags) =
        lower_src("pub flow greet() {\n}\npub fn heal() {\n  return 1;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots.len(), 2);
    assert_eq!(hir.knots[0].visibility, Some(crate::VisibilityMark::Public));
    assert_eq!(hir.knots[1].visibility, Some(crate::VisibilityMark::Public));
}

#[test]
fn pub_nested_flow_stitch_lowers_with_public_visibility() {
    let (hir, _manifest, diags) =
        lower_src("flow garden() {\n  pub flow gate() {\n    Creak.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.knots[0].visibility, None,
        "outer flow was not marked pub"
    );
    assert_eq!(
        hir.knots[0].stitches[0].visibility,
        Some(crate::VisibilityMark::Public)
    );
}

#[test]
fn pub_var_const_flags_struct_extern_lower_with_public_visibility() {
    let src = "\
pub var hp = 10
pub const MAX = 100
pub flags Mood = calm, wary
pub struct Npc {\n  hp: int\n}
pub extern log_msg(msg)
";
    let (hir, _manifest, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.variables[0].visibility,
        Some(crate::VisibilityMark::Public)
    );
    assert_eq!(
        hir.constants[0].visibility,
        Some(crate::VisibilityMark::Public)
    );
    assert_eq!(hir.lists[0].visibility, Some(crate::VisibilityMark::Public));
    assert_eq!(
        hir.structs[0].visibility,
        Some(crate::VisibilityMark::Public)
    );
    assert_eq!(
        hir.externals[0].visibility,
        Some(crate::VisibilityMark::Public)
    );
}

#[test]
fn absent_pub_leaves_visibility_none_native() {
    // The ratified 2026-07-23 default is untouched by this issue: absent
    // `pub`, every one of the seven forms still lowers with
    // `visibility: None` (not `Some(Private)`) — `effective_visibility`
    // treats `None` under a declared module as `Private` regardless.
    let src = "\
flow greet() {\n}
fn heal() {\n  return 1;\n}
var hp = 10
const MAX = 100
flags Mood = calm, wary
struct Npc {\n  hp: int\n}
extern log_msg(msg)
";
    let (hir, _manifest, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots[0].visibility, None);
    assert_eq!(hir.knots[1].visibility, None);
    assert_eq!(hir.variables[0].visibility, None);
    assert_eq!(hir.constants[0].visibility, None);
    assert_eq!(hir.lists[0].visibility, None);
    assert_eq!(hir.structs[0].visibility, None);
    assert_eq!(hir.externals[0].visibility, None);
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

// ─── `use` / `import` → `Import` (issue #1581: `Import.module` must be a
// real, `::`-joined module name, with the leaf item kept out of it) ─────────

#[test]
fn use_decl_lowers_to_import() {
    let (hir, _manifest, diags) = lower_src("use story::market::{barter, haggle as h};\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    let imp = &hir.imports[0];
    assert_eq!(imp.module, "story::market");
    assert!(imp.bare);
    assert_eq!(imp.items.len(), 2);
    assert_eq!(imp.items[0].name, "barter");
    assert_eq!(imp.items[0].alias, None);
    assert_eq!(imp.items[1].name, "haggle");
    assert_eq!(imp.items[1].alias.as_deref(), Some("h"));
}

/// The plain `use path::item;` shape: the leaf is the imported *item*, and
/// the module is the `::`-joined prefix — exactly the module name
/// `brink_db::modules::native_module_path` mints for `market/barter.brink`.
/// Before #1581 this produced `module: "story.market.barter.haggle"`, which
/// no module could ever equal.
#[test]
fn use_decl_leaf_is_the_item_not_part_of_the_module() {
    let src = "use story::market::barter::haggle;\n";
    let (hir, _manifest, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    let imp = &hir.imports[0];
    assert_eq!(imp.module, "story::market::barter");
    assert!(imp.bare, "a named item is a name-precise (bare) import");
    assert_eq!(imp.items.len(), 1);
    assert_eq!(imp.items[0].name, "haggle");
    assert_eq!(imp.items[0].alias, None);
    assert_eq!(&src[imp.items[0].range], "haggle");
    assert_eq!(&src[imp.module_range], "story::market::barter");
}

/// `use path::item as alias;` — previously rejected outright as an
/// unrepresentable "module-level alias" (E129). With the leaf read as an
/// item it is ink's `IMPORT { item AS alias } FROM path`, which `Import`
/// represents exactly.
#[test]
fn aliased_use_decl_lowers_to_an_aliased_item() {
    let src = "use story::market::barter as b;\n";
    let (hir, _manifest, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    let imp = &hir.imports[0];
    assert_eq!(imp.module, "story::market");
    assert!(imp.bare);
    assert_eq!(imp.items.len(), 1);
    assert_eq!(imp.items[0].name, "barter");
    assert_eq!(imp.items[0].alias.as_deref(), Some("b"));
    assert_eq!(&src[imp.items[0].range], "barter as b");
}

/// A single segment has no prefix to be the module, so it can only name the
/// module itself — the qualified form, same as `import story;`.
#[test]
fn single_segment_use_decl_lowers_to_bare_false_import() {
    let (hir, _manifest, diags) = lower_src("use story;\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    assert_eq!(hir.imports[0].module, "story");
    assert!(!hir.imports[0].bare);
    assert!(hir.imports[0].items.is_empty());
}

/// …and aliasing *that* is a module-level alias, which ink's `Import` has no
/// field for — still loud, never silently dropped.
#[test]
fn single_segment_aliased_use_decl_is_flagged() {
    let (hir, _manifest, diags) = lower_src("use story as s;\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "a module-level alias must be flagged: {diags:?}"
    );
    assert!(hir.imports.is_empty());
}

#[test]
fn import_decl_lowers_to_qualified_import() {
    let (hir, _manifest, diags) = lower_src("import story::market\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.imports.len(), 1);
    assert_eq!(hir.imports[0].module, "story::market");
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
fn root_content_is_empty_without_a_main_flow() {
    let (hir, _manifest, _diags) = lower_src("flow a() {}\n");
    assert!(hir.root_content.stmts.is_empty());
    assert!(hir.includes.is_empty());
    assert!(hir.module.is_none());
}

// ─── `flow main()` entry convention (ruled 2026-07-21, docs/decision-log.md) ──

#[test]
fn top_level_main_flow_synthesizes_a_root_divert() {
    let (hir, _manifest, diags) = lower_src("flow main() {\n  Hi.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.root_content.stmts.len(), 1);
    let Stmt::Divert(d) = &hir.root_content.stmts[0] else {
        panic!(
            "expected root_content to be a single Divert, got {:?}",
            hir.root_content.stmts[0]
        );
    };
    assert!(
        d.ptr.is_none(),
        "synthesized entry divert has no source ptr"
    );
    let DivertPath::Path(path) = &d.target.path else {
        panic!("expected a named divert target, got {:?}", d.target.path);
    };
    assert_eq!(path.segments.len(), 1);
    assert_eq!(path.segments[0].text, "main");
    assert!(d.target.args.is_empty());
    assert!(
        matches!(
            hir.root_content.tail(),
            Tail::Diverge(Terminator::Divert(_))
        ),
        "the synthesized divert must also drive Block::tail: {:?}",
        hir.root_content.tail()
    );
}

#[test]
fn nested_main_flow_does_not_synthesize_an_entry() {
    // Only a *top-level* `main` counts — a stitch named `main` nested inside
    // another flow is not the story's entry point.
    let (hir, _manifest, diags) = lower_src("flow outer() {\n  flow main() {\n    Hi.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.root_content.stmts.is_empty());
}

#[test]
fn function_named_main_does_not_synthesize_an_entry() {
    // `fn main()` is a function, not a flow — not eligible for the entry
    // convention (a function is called for its value, not diverted into).
    let (hir, _manifest, diags) = lower_src("fn main() {\n  return;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.root_content.stmts.is_empty());
}

#[test]
fn parameterized_main_flow_does_not_synthesize_an_entry() {
    // A bare entry divert cannot supply arguments — synthesizing one for a
    // `main` that requires params would either drop them silently or invent
    // values from nowhere. Neither is acceptable, so no entry is
    // synthesized; `main` remains an ordinary, host-enterable flow.
    let (hir, _manifest, diags) = lower_src("flow main(who) {\n  Hi, {who}.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.root_content.stmts.is_empty());
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
  return;
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

// ─── B0.7: prose-dialect body lowering (docs/b0-sequencing.md §B0.7) ──────
//
// These are the HIR-differential gate for this slice (see the B0.7 issue's
// exit-criteria discussion): the native→episode path isn't wired yet, so
// correctness is proven at the HIR-shape level — each construct lowers to
// the same target `hir::types` shapes the old ink frontend produces for the
// equivalent construct.

use crate::{
    ChoiceSetContext, CondKind, ContentPart, DivertPath, Expr, ReturnKind, SequenceType, Stmt,
};

fn only_knot_body(hir: &HirFile) -> &crate::Block {
    &hir.knots[0].body
}

#[test]
fn content_glue_interpolation_and_tags_lower() {
    let (hir, _m, diags) = lower_src("flow a() {\n  Hi, {name}! <> #mood: happy\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    // Glue suppresses the EndOfLine (Content only), then the flow's
    // implicit-end grace (charter §15) appends a synthesized `-> DONE`.
    assert_eq!(
        body.stmts.len(),
        2,
        "glue suppresses the EndOfLine: {body:?}"
    );
    assert!(
        matches!(
            &body.stmts[1],
            Stmt::Divert(d) if d.target.path == DivertPath::Done
        ),
        "expected trailing implicit `-> DONE`, got {:?}",
        body.stmts[1]
    );
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    assert_eq!(c.tags.len(), 1);
    assert!(matches!(&c.parts[0], ContentPart::Text(t) if t == "Hi, "));
    assert!(matches!(
        &c.parts[1],
        ContentPart::Interpolation(Expr::Path(_))
    ));
    assert!(matches!(&c.parts[2], ContentPart::Text(t) if t == "! "));
    assert!(matches!(c.parts[3], ContentPart::Glue));
}

// ── `#@…` directive-shaped tags (issue #1835) ────────────────────────

#[test]
fn a_tag_starting_with_at_on_a_trailing_tag_line_emits_e172() {
    // A trailing tag on a content line — the `lower_content_run` TAG arm.
    let (hir, _m, diags) = lower_src("flow a() {\n  Hi. #@was(\"old_name\")\n}\n");
    assert_eq!(
        diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E172)
            .count(),
        1,
        "expected exactly one E172, got: {diags:?}"
    );
    // It still lowers as an ordinary runtime tag — no directive effect —
    // so the diagnostic is a warning, not a dropped statement.
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    assert_eq!(c.tags.len(), 1, "the tag still lowers as ordinary content");
    assert!(matches!(&c.tags[0].parts[0], ContentPart::Text(t) if t == "@was(\"old_name\")"));
}

// ── `\#` inside a tag body (issue #1738) ─────────────────────────────

#[test]
fn a_tag_with_an_escaped_hash_lowers_with_the_backslash_stripped() {
    // Issue #1738: before the parser fix (`content::tag`'s doc comment),
    // `\#` inside a tag split it into two sibling `TAG`s at the `#` —
    // this would lower to `c.tags.len() == 2`. After that fix, one `TAG`
    // survives with the escaped `#` preserved. Issue #2045 (this test,
    // updated): `ast::Tag::text()` now strips the *recognized* escape's
    // backslash too, parity with `markup::escape`'s stripping for ordinary
    // content — `lower_tag` no longer hand-rolls the leading-`HASH`-skip
    // itself, it delegates to `ast::Tag::text()`, which owns both that and
    // this stripping in one place.
    let (hir, _m, diags) = lower_src("flow a() {\n  Hi. #tag \\#more\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    assert_eq!(
        c.tags.len(),
        1,
        "an escaped `#` must not split this into two tags: {:?}",
        c.tags
    );
    assert!(
        matches!(&c.tags[0].parts[0], ContentPart::Text(t) if t == "tag #more"),
        "expected the literal `#` preserved with its escaping backslash \
         stripped (issue #2045), got {:?}",
        c.tags[0].parts
    );
}

#[test]
fn a_tag_with_an_escaped_open_brace_lowers_with_the_backslash_stripped() {
    // Issue #2045's own scope note: `\{` gets the identical treatment as
    // `\#`, not just the hash case.
    let (hir, _m, diags) = lower_src("flow a() {\n  Hi. #tag \\{gold\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    assert_eq!(c.tags.len(), 1);
    assert!(
        matches!(&c.tags[0].parts[0], ContentPart::Text(t) if t == "tag {gold"),
        "expected the literal `{{` preserved with its escaping backslash \
         stripped (issue #2045), got {:?}",
        c.tags[0].parts
    );
}

#[test]
fn a_standalone_at_prefixed_tag_line_emits_e172() {
    // A whole-line tag — the `TAG_LINE` arm of `lower_one_item`.
    let (hir, _m, diags) = lower_src("flow a() {\n  #@private\n}\n");
    assert_eq!(
        diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E172)
            .count(),
        1,
        "expected exactly one E172, got: {diags:?}"
    );
    assert_eq!(hir.knots.len(), 1);
}

#[test]
fn e172_names_the_native_annotation_equivalent_when_one_exists() {
    let (_hir, _m, diags) = lower_src("flow a() {\n  #@was(\"old\")\n}\n");
    let msg = &diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E172)
        .expect("E172 expected")
        .message;
    assert!(
        msg.contains("@[was(") && msg.contains("was"),
        "expected the native `@[was(…)]` spelling to be named, got: {msg}"
    );
}

#[test]
fn e172_says_no_native_meaning_when_no_annotation_equivalent_exists() {
    let (_hir, _m, diags) = lower_src("flow a() {\n  #@local\n}\n");
    let msg = &diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E172)
        .expect("E172 expected")
        .message;
    assert!(
        msg.contains("no directive channel") && msg.contains("no `local` equivalent"),
        "expected the no-native-meaning wording, got: {msg}"
    );
}

#[test]
fn e172_gives_allow_its_own_wording_rather_than_calling_it_an_ink_directive() {
    // Review of #1953, finding (a): ink's own directive recognizer
    // (`hir::lower::directive::apply_scope_directives`) does not know
    // `allow` — only `module`/`public`/`private`/`local`/`was`/`effects`
    // are recognized names, so `#@allow` is an *unknown* directive in ink
    // too. The message must not claim `#@allow` "is the ink-dialect
    // directive-tag spelling" (the `was`/`effects` wording) just because
    // native happens to have an `@[allow(…)]` annotation of its own.
    let (_hir, _m, diags) = lower_src("flow a() {\n  #@allow(E172)\n}\n");
    let msg = &diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E172)
        .expect("E172 expected")
        .message;
    assert!(
        msg.contains("no directive meaning in either dialect") && msg.contains("@[allow("),
        "expected `#@allow`'s own wording (no ink meaning, names the unrelated native \
         `@[allow(…)]` suppression channel), got: {msg}"
    );
    assert!(
        !msg.contains("is the ink-dialect directive-tag spelling"),
        "must not claim ink recognizes `allow` as a directive name: {msg}"
    );
}

#[test]
fn e172_does_not_assert_ink_membership_for_an_unrecognized_name() {
    // Review of #1953, finding (b): the fallback branch must not tell
    // every unmatched name it "is an ink-dialect compiler-directive
    // spelling" — a project may deliberately tag content with its own
    // `@`-led runtime convention (the issue's own caution, e.g.
    // `#@narrator`), and that author's tag genuinely isn't an ink
    // directive. The message may say the tag has directive *shape*, but
    // must not assert ink membership.
    let (_hir, _m, diags) = lower_src("flow a() {\n  #@narrator\n}\n");
    let msg = &diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E172)
        .expect("E172 expected")
        .message;
    assert!(
        msg.contains("has the shape of an ink-dialect compiler-directive tag"),
        "expected the shape-only wording for an unrecognized name, got: {msg}"
    );
    assert!(
        !msg.contains("is an ink-dialect compiler-directive spelling"),
        "must not assert ink recognizes `narrator` as a directive name: {msg}"
    );
}

#[test]
fn an_ordinary_tag_without_a_leading_at_does_not_emit_e172() {
    let (_hir, _m, diags) = lower_src("flow a() {\n  Hi. #mood: happy\n}\n");
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E172),
        "unexpected E172 for a plain tag: {diags:?}"
    );
}

#[test]
fn content_without_glue_gets_end_of_line() {
    let (hir, _m, diags) = lower_src("flow a() {\n  Plain line.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    // Content + EndOfLine, then the flow's implicit-end `-> DONE`
    // (charter §15).
    assert_eq!(body.stmts.len(), 3);
    assert!(matches!(body.stmts[0], Stmt::Content(_)));
    assert!(matches!(body.stmts[1], Stmt::EndOfLine));
    assert!(matches!(
        &body.stmts[2],
        Stmt::Divert(d) if d.target.path == DivertPath::Done
    ));
}

// ── Inline markup (#1716; docs/prose-dialect-spec.md §4) ─────────────

#[test]
fn a_span_lowers_to_content_part_span_with_name_attrs_and_children() {
    let (hir, _m, diags) =
        lower_src("flow a() {\n  He hands you <item id=\"lantern\">the lantern</item>.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    assert!(matches!(&c.parts[0], ContentPart::Text(t) if t == "He hands you "));
    let ContentPart::Span(span) = &c.parts[1] else {
        panic!("expected Span, got {:?}", c.parts[1]);
    };
    assert_eq!(span.name, "item");
    assert_eq!(span.attrs, vec![("id".to_string(), "lantern".to_string())]);
    assert_eq!(span.children.len(), 1);
    assert!(matches!(&span.children[0], ContentPart::Text(t) if t == "the lantern"));
    assert!(matches!(&c.parts[2], ContentPart::Text(t) if t == "."));
}

#[test]
fn a_self_closing_span_lowers_with_no_children_no_attrs() {
    // The point-marker shape (§8b.11).
    let (hir, _m, diags) = lower_src("flow a() {\n  Bell tolls. <pause/> Door slams.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    let span = c
        .parts
        .iter()
        .find_map(|p| match p {
            ContentPart::Span(s) => Some(s),
            _ => None,
        })
        .expect("expected a Span part");
    assert_eq!(span.name, "pause");
    assert!(span.attrs.is_empty());
    assert!(span.children.is_empty());
}

#[test]
fn nested_spans_lower_to_nested_span_parts() {
    let (hir, _m, diags) = lower_src("flow a() {\n  <b><i>hi</i></b>\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    let ContentPart::Span(outer) = &c.parts[0] else {
        panic!("expected Span, got {:?}", c.parts[0]);
    };
    assert_eq!(outer.name, "b");
    let ContentPart::Span(inner) = &outer.children[0] else {
        panic!("expected nested Span, got {:?}", outer.children[0]);
    };
    assert_eq!(inner.name, "i");
    assert!(matches!(&inner.children[0], ContentPart::Text(t) if t == "hi"));
}

#[test]
fn a_span_may_contain_interpolation() {
    let (hir, _m, diags) = lower_src("flow a(name) {\n  <b>hello {name}</b>\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    let ContentPart::Span(span) = &c.parts[0] else {
        panic!("expected Span, got {:?}", c.parts[0]);
    };
    assert!(matches!(&span.children[0], ContentPart::Text(t) if t == "hello "));
    assert!(matches!(
        &span.children[1],
        ContentPart::Interpolation(Expr::Path(_))
    ));
}

#[test]
fn all_four_escapes_lower_to_literal_text_merged_into_one_part() {
    let (hir, _m, diags) = lower_src("flow a() {\n  \\< \\{ \\# \\\\\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    // Merged into a single Text part (push_literal's whole point) — not
    // four separate Text parts around three escapes, which would defeat
    // `try_recognize`'s Phase-1 "exactly one Text part" Plain recognition
    // for a line with no actual dynamic content.
    assert_eq!(c.parts.len(), 1, "expected one merged Text part: {c:?}");
    let ContentPart::Text(t) = &c.parts[0] else {
        panic!("expected Text, got {:?}", c.parts[0]);
    };
    assert_eq!(t.as_str(), "< { # \\");
}

#[test]
fn a_leading_backslash_at_lowers_to_literal_text_not_a_cue() {
    // §8d.6's line-start escape set (issue #1744): `\@VENDOR` must lower
    // to a plain Content line, never claim `@NAME`'s CUE dispatch
    // (§8b.9). `push_escape` is generic over any `ESCAPE` node, so this
    // is the same merged-single-Text-part shape the four inline escapes
    // get.
    let (hir, _m, diags) = lower_src("flow a() {\n  \\@VENDOR waves.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content (not a Cue), got {:?}", body.stmts[0]);
    };
    assert_eq!(c.parts.len(), 1, "expected one merged Text part: {c:?}");
    let ContentPart::Text(t) = &c.parts[0] else {
        panic!("expected Text, got {:?}", c.parts[0]);
    };
    assert_eq!(t.as_str(), "@VENDOR waves.");
}

#[test]
fn a_backslash_before_anything_else_is_a_parse_error_not_a_hir_diagnostic() {
    // The escape set is validated by the parser (§8d.6); a bad escape is a
    // `ParseError`, never reaches `hir::lower_native` at all as a node —
    // `\n` (not in the four-item escape set) recovers as an `ERROR` node
    // the parser already wraps, which `lower_content_run`'s `N::ERROR` arm
    // silently skips (matching every other parse-error recovery site).
    let parse = brink_syntax_native::parse("flow a() {\n  \\n not an escape\n}\n");
    assert!(!parse.errors().is_empty());
}

#[test]
fn a_conditional_branch_may_contain_a_fully_closed_span() {
    let (hir, _m, diags) = lower_src("flow a() {\n  {if hp > 0: <i>yawn</i> else: Ready.}\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Conditional(cond) = &body.stmts[0] else {
        panic!("expected Conditional, got {:?}", body.stmts[0]);
    };
    let Stmt::Content(c) = &cond.branches[0].body.stmts[0] else {
        panic!("expected Content, got {:?}", cond.branches[0].body.stmts[0]);
    };
    assert!(matches!(&c.parts[0], ContentPart::Span(s) if s.name == "i"));
}

#[test]
fn if_else_conditional_lowers_to_if_else_branches() {
    // Braced-arm form, not the single-line colon form
    // (`{if cond: … else: …}` all on one line) — that shape has a real
    // grammar gap: `content_line`'s scan doesn't stop before a bare `else`
    // reached mid-line (nothing in `content_items_until` special-cases the
    // `KW_ELSE` keyword), so "Happy. else: Sad." is swallowed whole as one
    // TEXT run and `at_else_arm` never gets a chance to fire. The
    // multi-line colon form (`else:` alone at the start of its own line)
    // is unaffected — `at_else_arm` is checked before each `body_line`
    // call, so it fires correctly there. Flagged for the coordinator as a
    // brink-syntax-native finding, not fixed here (B0.7 lowers whatever
    // the parser hands it; this is a parser-level fix).
    let (hir, _m, diags) =
        lower_src("var mood = 1\nflow a() {\n  {if mood > 0 { Happy. } else { Sad. }}\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Conditional(cond) = &body.stmts[0] else {
        panic!("expected Conditional, got {body:?}");
    };
    // `InitialCondition`, not `IfElse` — see `cond::lower_conditional`'s doc
    // comment: this is what ink's own equivalent `{cond: body - else: body2}`
    // spelling actually compiles to (cross-frontend differential finding,
    // `tests/b07_native_body.rs`).
    assert_eq!(cond.kind, CondKind::InitialCondition);
    assert_eq!(cond.branches.len(), 2);
    assert!(cond.branches[0].condition.is_some());
    assert!(cond.branches[1].condition.is_none());
}

#[test]
fn match_conditional_lowers_to_switch_with_subject() {
    let (hir, _m, diags) =
        lower_src("var mood = 1\nflow a() {\n  {match mood { 1 => Happy. 2 => Sad. }}\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Conditional(cond) = &body.stmts[0] else {
        panic!("expected Conditional, got {body:?}");
    };
    assert!(matches!(cond.kind, CondKind::Switch(_)));
    assert_eq!(cond.branches.len(), 2);
}

#[test]
fn block_level_alternation_gets_leading_end_of_line_per_branch() {
    let (hir, _m, diags) = lower_src("flow a() {\n  {~ One. | Two. | Three.}\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Sequence(seq) = &body.stmts[0] else {
        panic!("expected Sequence, got {body:?}");
    };
    assert_eq!(seq.kind, SequenceType::SHUFFLE);
    assert_eq!(seq.branches.len(), 3);
    for branch in &seq.branches {
        assert!(
            matches!(branch.body.stmts[0], Stmt::EndOfLine),
            "block-level sequence branch must lead with EndOfLine: {branch:?}"
        );
    }
}

#[test]
fn inline_alternation_inside_content_does_not_get_leading_eol() {
    let (hir, _m, diags) = lower_src("flow a() {\n  You see {& a cat|a dog}.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Content, got {:?}", body.stmts[0]);
    };
    let inline = c
        .parts
        .iter()
        .find_map(|p| match p {
            ContentPart::InlineSequence(s) => Some(s),
            _ => None,
        })
        .expect("expected an InlineSequence part");
    assert_eq!(inline.branches.len(), 2);
    assert!(
        inline.branches[0]
            .body
            .stmts
            .iter()
            .all(|s| !matches!(s, Stmt::EndOfLine))
    );
}

#[test]
fn choice_point_lowers_to_choice_set_with_sticky_and_once() {
    let (hir, _m, diags) = lower_src("flow a() {\n  {?\n    * Once.\n    + Sticky.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::ChoiceSet(cs) = &body.stmts[0] else {
        panic!("expected ChoiceSet, got {body:?}");
    };
    assert_eq!(
        cs.context,
        ChoiceSetContext::Inline,
        "D4 posture: native-normal neutral"
    );
    assert_eq!(cs.depth, 0, "D4 posture: native-normal neutral");
    assert_eq!(cs.choices.len(), 2);
    assert!(!cs.choices[0].is_sticky);
    assert!(cs.choices[1].is_sticky);
}

#[test]
fn choice_guard_and_label_lower() {
    let (hir, _m, diags) =
        lower_src("var gold = 1\nflow a() {\n  {?\n    * {if gold > 0} (rich) Buy it.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::ChoiceSet(cs) = &body.stmts[0] else {
        panic!("expected ChoiceSet, got {body:?}");
    };
    let choice = &cs.choices[0];
    assert!(choice.condition.is_some());
    assert_eq!(choice.label.as_ref().unwrap().text, "rich");
}

#[test]
fn else_branch_lowers_to_fallback_choice() {
    let (hir, _m, diags) = lower_src("flow a() {\n  {?\n    * A.\n    else { B. }\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::ChoiceSet(cs) = &body.stmts[0] else {
        panic!("expected ChoiceSet, got {body:?}");
    };
    assert_eq!(cs.choices.len(), 2);
    assert!(!cs.choices[0].is_fallback);
    assert!(cs.choices[1].is_fallback);
}

#[test]
fn dissolved_gather_becomes_choice_set_continuation() {
    let (hir, _m, diags) =
        lower_src("flow a() {\n  Intro.\n  {?\n    * A.\n    * B.\n  }\n  Reconverged.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    // "Reconverged." must NOT be a sibling statement after the ChoiceSet —
    // it must live inside the choice set's continuation (the dissolved
    // gather).
    let cs = body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .expect("expected a ChoiceSet among the body's statements");
    assert!(
        !cs.continuation.stmts.is_empty(),
        "reconverged content must be absorbed into the continuation"
    );
    let Stmt::Content(c) = &cs.continuation.stmts[0] else {
        panic!(
            "expected Content in continuation, got {:?}",
            cs.continuation.stmts[0]
        );
    };
    assert!(matches!(&c.parts[0], ContentPart::Text(t) if t == "Reconverged."));
}

#[test]
fn labeled_gather_after_choices_attaches_label_to_continuation() {
    let (hir, _m, diags) =
        lower_src("flow a() {\n  {?\n    * A.\n  }\n  (again)\n  Loop point.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::ChoiceSet(cs) = &body.stmts[0] else {
        panic!("expected ChoiceSet, got {body:?}");
    };
    assert_eq!(
        cs.continuation.label.as_ref().map(|n| n.text.as_str()),
        Some("again"),
        "gather label must attach directly to continuation.label, not a nested LabeledBlock"
    );
}

#[test]
fn standalone_labeled_content_line_becomes_labeled_block() {
    let (hir, _m, diags) = lower_src("flow a() {\n  Intro.\n  (mid) Middle.\n  End.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let labeled = body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::LabeledBlock(b) => Some(b),
            _ => None,
        })
        .expect("expected a LabeledBlock absorbing the labeled line and everything after it");
    assert_eq!(labeled.label.as_ref().unwrap().text, "mid");
    // "End." must be inside the labeled block (absorbed), not a sibling.
    assert!(labeled.stmts.len() >= 2, "labeled block: {labeled:?}");
}

#[test]
fn divert_and_tunnel_lower() {
    let (hir, _m, diags) =
        lower_src("flow b() {\n  Bye.\n}\nflow a() {\n  -> b\n}\nflow c() {\n  -> b ->\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a_body = &hir.knots[1].body;
    assert!(matches!(a_body.stmts[0], Stmt::Divert(_)));
    let c_body = &hir.knots[2].body;
    assert!(matches!(c_body.stmts[0], Stmt::TunnelCall(_)));
}

#[test]
fn divert_target_call_args_are_wired_through_not_dropped() {
    // Issue #2136: brink-syntax-native's parser captures `-> knot(args)`
    // call args as an `ARG_LIST` under `DIVERT_TARGET` (bug #1196), but this
    // HIR pass used to discard them entirely and emit E129 ("parses but has
    // no HIR lowering yet") instead of wiring them into `DivertTarget::args`
    // — pre-#2136, this exact fixture asserted `d.target.args.is_empty()`
    // alongside an E129 diagnostic. Now the arg lowers cleanly into
    // `DivertTarget::args`, mirroring the ink-dialect path
    // (`hir::lower::divert::lower_divert_target_with_args`), and E129 must
    // not fire for this construct at all.
    let (hir, _m, diags) = lower_src("flow b(x) {\n  Bye.\n}\nflow a() {\n  -> b(1)\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a_body = &hir.knots[1].body;
    let Stmt::Divert(d) = &a_body.stmts[0] else {
        panic!("expected Divert, got {:?}", a_body.stmts[0]);
    };
    assert_eq!(
        d.target.args.len(),
        1,
        "the call arg must survive lowering into DivertTarget::args: {:?}",
        d.target.args
    );
    assert!(
        matches!(&d.target.args[0], Expr::Int(1)),
        "expected the literal `1` argument to lower to Expr::Int(1), got: {:?}",
        d.target.args[0]
    );
}

#[test]
fn tunnel_call_target_args_are_wired_through_not_dropped() {
    // The same `lower_divert_target` helper backs `TUNNEL_CALL` targets
    // (`-> b(1) ->`), not just plain `DIVERT_STMT` — this pins that the
    // fix applies uniformly rather than only to the bare-divert call site.
    let (hir, _m, diags) = lower_src("flow b(x) {\n  Bye.\n}\nflow a() {\n  -> b(1) ->\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a_body = &hir.knots[1].body;
    let Stmt::TunnelCall(t) = &a_body.stmts[0] else {
        panic!("expected TunnelCall, got {:?}", a_body.stmts[0]);
    };
    assert_eq!(t.targets.len(), 1);
    assert_eq!(
        t.targets[0].args.len(),
        1,
        "the tunnel-call target's arg must survive lowering: {:?}",
        t.targets[0].args
    );
}

#[test]
fn return_redirect_target_call_args_are_wired_through_not_dropped() {
    // Review finding on this PR: `lower_divert_target` also backs
    // `RETURN_REDIRECT` (`return -> b(1)`, charter §11's tunnel-return
    // respelling) through `lower_return_redirect` — a third call site,
    // distinct from the plain-divert and tunnel-call ones pinned above.
    // `parser/divert.rs::return_stmt` routes `return -> …` through the same
    // `divert_target` production those two go through, so this construct
    // parsed clean and hard-failed with E129 before #2136 and would
    // otherwise now silently drop the arg into `onwards_args` — unlike the
    // two siblings, it lands in `Return::onwards_args`, not
    // `DivertTarget::args` (`lower_return_redirect`'s `Stmt::Return { kind:
    // TunnelRedirect, onwards_args, .. }`), a different field consumed by a
    // different LIR site (`lir::lower::stmts::lower_return`, not
    // `lower_divert_target`).
    let (hir, _m, diags) = lower_src("flow b(x) {\n  Bye.\n}\nflow a() {\n  return -> b(1)\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = &hir.knots[1].body.stmts[0] else {
        panic!("expected Return, got {:?}", hir.knots[1].body.stmts[0]);
    };
    assert_eq!(r.kind, ReturnKind::TunnelRedirect);
    assert!(matches!(r.value, Some(Expr::DivertTarget(_))));
    assert_eq!(
        r.onwards_args.len(),
        1,
        "the call arg must survive lowering into Return::onwards_args: {:?}",
        r.onwards_args
    );
    assert!(
        matches!(r.onwards_args[0], Expr::Int(1)),
        "expected the literal `1` argument to lower to Expr::Int(1), got: {:?}",
        r.onwards_args[0]
    );
}

#[test]
fn inline_divert_mid_content_line_splits_into_two_statements() {
    let (hir, _m, diags) = lower_src("flow b() {\n  Bye.\n}\nflow a() {\n  The wager. -> b\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[1].body;
    assert!(matches!(body.stmts[0], Stmt::Content(_)));
    assert!(matches!(body.stmts[1], Stmt::Divert(_)));
}

// ── Issue #1991: `~ stmt` — the content-ground line escape into code ─
// ── (charter §8.2, RULED 2026-07-23) ──────────────────────────────────
//
// Before this landed, `LOGIC_LINE` had no lowering at all — because the
// parser never produced the node in the first place, `~ n = 5` reached
// this pass folded into an ordinary `Stmt::Content` (the literal text
// `~ n = 5`), never as a statement. These pin the fixed lowering
// specifically; `tests/tier1-native/logic-line-escape/` pins the same
// fix end-to-end through a real compile+run.

// ── Issue #1972: `~ let name = expr` — the same content-ground escape, ─
// ── extended to a temp declaration ────────────────────────────────────
//
// #1991 wired `LOGIC_LINE`'s `ASSIGN_STMT`/`EXPR_STMT` children; `LET_STMT`
// had no `KW_LET` dispatch in the parser at all, so `~ let n = 5` reached
// `expr_stmt_line`'s `expr::expression`, which diagnoses `let` as an
// unrecognized atom rather than parsing a declaration. These pin the fixed
// lowering; `tests/tier1-native/logic-line-escape/` extends the same
// end-to-end fixture #1991 added.

#[test]
fn logic_line_temp_decl_lowers_to_stmt_temp_decl() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ let n = 5\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::TempDecl(t) = &body.stmts[0] else {
        panic!("expected Stmt::TempDecl, got {:?}", body.stmts[0]);
    };
    assert_eq!(t.name.text, "n");
    assert!(matches!(t.value, Some(Expr::Int(5))));
}

#[test]
fn logic_line_temp_decl_without_initializer_lowers_with_no_value() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ let n: int\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::TempDecl(t) = &body.stmts[0] else {
        panic!("expected Stmt::TempDecl, got {:?}", body.stmts[0]);
    };
    assert!(t.value.is_none());
    assert!(
        t.annotation.is_some(),
        "expected the `: int` annotation to lower"
    );
}

#[test]
fn logic_line_temp_decl_from_an_emitting_call_lowers_to_end_of_line() {
    // Mirrors `logic_line_assignment_from_an_emitting_call_lowers_to_end_of_line`
    // for `TempDecl`: the ink-dialect frontend's own `LogicLineOutput::
    // has_call` rule applies `td.value.as_ref().is_some_and(expr_contains_call)`
    // identically across `TempDecl`/`Assignment`/`ExprStmt` — this pins the
    // native lowering's parity for the one variant `#1991` didn't cover.
    let (hir, _m, diags) =
        lower_src("fn shout() >{\n  Hi\n  return 7\n}\nflow a() {\n  ~ let n = shout()\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[1].body;
    let Stmt::TempDecl(t) = &body.stmts[0] else {
        panic!("expected Stmt::TempDecl, got {:?}", body.stmts[0]);
    };
    assert!(matches!(t.value, Some(Expr::Call(..))));
    assert!(
        matches!(body.stmts[1], Stmt::EndOfLine),
        "a temp decl whose value contains a call must still get the trailing \
         EndOfLine the ink-dialect frontend applies to the same construct: {:?}",
        body.stmts
    );
}

#[test]
fn logic_line_assignment_lowers_to_stmt_assignment() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ n = 5\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Assignment(a) = &body.stmts[0] else {
        panic!("expected Stmt::Assignment, got {:?}", body.stmts[0]);
    };
    assert_eq!(a.op, crate::AssignOp::Set);
    assert!(matches!(&a.target, Expr::Path(p) if p.segments.last().unwrap().text == "n"));
    assert!(matches!(a.value, Expr::Int(5)));
}

#[test]
fn logic_line_compound_assignment_lowers_op() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ n += 3\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Assignment(a) = &body.stmts[0] else {
        panic!("expected Stmt::Assignment, got {:?}", body.stmts[0]);
    };
    assert_eq!(a.op, crate::AssignOp::Add);
}

#[test]
fn logic_line_bare_call_lowers_to_expr_stmt_with_end_of_line() {
    // Mirrors the ink-dialect's own `LogicLineOutput::has_call` rule
    // (`hir::lower::content::logic_line`): a call-only logic line needs a
    // trailing `Stmt::EndOfLine` to match inklecate's behavior — the same
    // semantic construct ("ink's logic line, kept") on the same runtime.
    let (hir, _m, diags) = lower_src("fn bump() {\n  return 1;\n}\nflow a() {\n  ~ bump()\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[1].body;
    assert!(matches!(body.stmts[0], Stmt::ExprStmt(Expr::Call(..))));
    assert!(matches!(body.stmts[1], Stmt::EndOfLine));
}

#[test]
fn logic_line_assignment_from_an_emitting_call_lowers_to_end_of_line() {
    // Finding F1 (PR #2002 review): `lower_logic_line`'s ASSIGN_STMT arm
    // originally never appended `Stmt::EndOfLine`, so a call's own emitted
    // content lost its trailing line break when the logic line assigned
    // the call's result instead of discarding it — `~ shout()` printed
    // "Hi\n" but `~ n = shout()` printed "Hi" with no break, even though
    // both reach the exact same `>{ }` function body. Mirrors
    // `logic_line_bare_call_lowers_to_expr_stmt_with_end_of_line`, but for
    // `Stmt::Assignment` — the ink-dialect frontend this dialect claims
    // parity with applies the same `expr_contains_call` rule to both
    // `Assignment` and `ExprStmt` (`hir/lower/content/logic_line.rs`).
    let (hir, _m, diags) =
        lower_src("fn shout() >{\n  Hi\n  return 7\n}\nflow a() {\n  ~ n = shout()\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[1].body;
    let Stmt::Assignment(a) = &body.stmts[0] else {
        panic!("expected Stmt::Assignment, got {:?}", body.stmts[0]);
    };
    assert!(matches!(a.value, Expr::Call(..)));
    assert!(
        matches!(body.stmts[1], Stmt::EndOfLine),
        "an assignment whose value contains a call must still get the trailing \
         EndOfLine the ink-dialect frontend applies to the same construct: {:?}",
        body.stmts
    );
}

#[test]
fn logic_line_precedes_ordinary_content_unaffected() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ n = 5\n  Value is {n}.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert!(matches!(body.stmts[0], Stmt::Assignment(_)));
    assert!(
        body.stmts[1..]
            .iter()
            .any(|s| matches!(s, Stmt::Content(_))),
        "the content line after the logic line must still lower normally: {:?}",
        body.stmts
    );
}

// ── Issue #1972 (second slice): `~ until cond` / `~{ … }` — the same ──
// ── content-ground escape, extended to native's `Await`/`LogicBlock` ──
//
// `until` is native's sole flow-suspension spelling (decision-log
// 2026-07-23 item 4, retiring `await`); `~{ … }` is the multi-statement
// logic-block escape. Both lower via the exact same functions the
// code-ground `until <cond>;` statement and the whole-body `~{ }` override
// already use (`lower_native::control_flow::lower_until_stmt`/
// `lower_stmt_block`) — only the wrapper (`Stmt::Await`/`Stmt::LogicBlock`
// vs. `BlockStmt::Await`/a `StmtBlock` item-position call) differs.

#[test]
fn logic_line_until_lowers_to_stmt_await() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ until n > 0\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Await(a) = &body.stmts[0] else {
        panic!("expected Stmt::Await, got {:?}", body.stmts[0]);
    };
    assert!(matches!(a.condition, Some(Expr::Infix(_))));
}

#[test]
fn logic_line_until_precedes_ordinary_content_unaffected() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ until n > 0\n  Value is {n}.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert!(matches!(body.stmts[0], Stmt::Await(_)));
    assert!(
        body.stmts[1..]
            .iter()
            .any(|s| matches!(s, Stmt::Content(_))),
        "the content line after the logic line must still lower normally: {:?}",
        body.stmts
    );
}

#[test]
fn logic_line_block_lowers_to_stmt_logic_block_with_standalone_scope() {
    let (hir, _m, diags) = lower_src("flow a() {\n  ~{\n    let m = 1;\n    n = m;\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::LogicBlock(lb) = &body.stmts[0] else {
        panic!("expected Stmt::LogicBlock, got {:?}", body.stmts[0]);
    };
    assert_eq!(lb.scope, crate::LogicBlockScope::Standalone);
    assert_eq!(lb.stmts.len(), 2);
    assert!(matches!(lb.stmts[0], crate::BlockStmt::TempDecl(_)));
    assert!(matches!(lb.stmts[1], crate::BlockStmt::Assignment(_)));
}

#[test]
fn logic_line_block_precedes_ordinary_content_unaffected() {
    // The escape's own T1b lexical scope (`~{ }`) is Standalone here — a
    // single content-ground `~{ }` island is never split the way a
    // *whole* code-ground body is around a nested `> text` line (issue
    // #1992/#2028's `LogicBlockScope::Opens`/`Continues`, only produced by
    // `lower_stmt_block_as_body`, a different call site than this one) —
    // so a `let` declared inside it does not need to stay visible to the
    // ordinary content line after it; this only pins that the surrounding
    // stream still lowers normally.
    let (hir, _m, diags) = lower_src("flow a() {\n  ~{\n    n = 1;\n  }\n  Value is {n}.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert!(matches!(body.stmts[0], Stmt::LogicBlock(_)));
    assert!(
        body.stmts[1..]
            .iter()
            .any(|s| matches!(s, Stmt::Content(_))),
        "the content line after the logic block must still lower normally: {:?}",
        body.stmts
    );
}

#[test]
fn logic_line_block_containing_a_call_lowers_to_end_of_line() {
    // Review finding (w111): mirrors
    // `logic_line_bare_call_lowers_to_expr_stmt_with_end_of_line`/
    // `logic_line_assignment_from_an_emitting_call_lowers_to_end_of_line`
    // for the `~{ … }` multi-statement block — a call anywhere inside the
    // block still needs the trailing `Stmt::EndOfLine` a single-statement
    // `~ expr`/`~ x = expr` escape already gets, or its output glues into
    // whatever content line follows (`tests/tier1-native/logic-line-escape/`
    // pins the same fix end-to-end through a real compile+run).
    let (hir, _m, diags) = lower_src(
        "fn shout() >{\n  Hi\n  return\n}\nflow a() {\n  ~{\n    let m = 1;\n    shout();\n  }\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[1].body;
    assert!(matches!(body.stmts[0], Stmt::LogicBlock(_)));
    assert!(
        matches!(body.stmts[1], Stmt::EndOfLine),
        "a `~{{ }}` block containing a call must still get the trailing \
         EndOfLine its single-statement siblings apply to the same \
         construct: {:?}",
        body.stmts
    );
}

#[test]
fn logic_line_block_with_no_call_gets_no_end_of_line() {
    // The converse of the above: a block with no call anywhere (the
    // pre-existing `logic_line_block_lowers_to_stmt_logic_block_with_standalone_scope`
    // shape) must not gain a spurious trailing EndOfLine — only a call's
    // own pending output needs the flush.
    let (hir, _m, diags) = lower_src("flow a() {\n  ~{\n    let m = 1;\n    n = m;\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert!(matches!(body.stmts[0], Stmt::LogicBlock(_)));
    // The body falls off the end, so `apply_implicit_done` appends a
    // synthesized `-> DONE` divert right after the block (unrelated to this
    // fix) — what this test pins is that the statement immediately after
    // the block is NOT a spurious `Stmt::EndOfLine`.
    assert!(
        !matches!(body.stmts.get(1), Some(Stmt::EndOfLine)),
        "a call-free block must not gain a trailing EndOfLine: {:?}",
        body.stmts
    );
}

#[test]
fn logic_line_with_no_recognized_child_is_a_loud_e129_not_a_silent_drop() {
    // Defense in depth: `lower_logic_line` itself must never silently
    // return an empty `Vec<Stmt>` for a `LOGIC_LINE` with neither an
    // `ASSIGN_STMT` nor an `EXPR_STMT` child — CLAUDE.md's "flag silent
    // data drops". The parser's own recovery loop (`stmt::logic_line`)
    // means a fully-unrecognized shape (e.g. `~ if`) still produces an
    // `EXPR_STMT` wrapper (empty, since `expression` failed at the first
    // token) — that missing-expr shape is already covered by `E015`
    // below; this pins the HIR-level fence for the node-shape case
    // directly, independent of what the parser happens to produce today.
    let (hir, _m, diags) = lower_src("flow a() {\n  ~ if\n}\n");
    assert!(
        !diags.is_empty(),
        "an unrecognized/malformed logic line must raise a diagnostic, never silently drop"
    );
    let body = only_knot_body(&hir);
    assert!(
        !body.stmts.iter().any(|s| matches!(s, Stmt::Content(_))),
        "a malformed logic line must never lower to visible story content: {:?}",
        body.stmts
    );
}

// ── Issue #1992: `> text` — the code-ground line escape into prose ────
// ── (charter §8.2, RULED 2026-07-23) ───────────────────────────────────
//
// The mirror image of #1991 (above) at the opposite ground: `> text` emits
// a prose line inside an otherwise code-ground `fn`/`flow` body. Before
// this landed the parser had no `GT` dispatch in `stmt::statement()` at
// all, so `> hi` inside a code-ground body was a parse error (`expected an
// expression, found GT`), not a lowering gap — these tests pin the fixed
// grammar's HIR lowering; `tests/tier1-native/prose-line-escape/` pins the
// same fix end-to-end through a real compile+run.

#[test]
fn prose_line_only_body_lowers_to_content_and_end_of_line_with_no_logic_block() {
    let (hir, _m, diags) = lower_src("fn radio() {\n  > hi\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert!(
        matches!(body.stmts[0], Stmt::Content(_)),
        "a code-ground body with only a prose line must never wrap it in a \
         LogicBlock (content is out of that closed set by design): {:?}",
        body.stmts
    );
    assert!(matches!(body.stmts[1], Stmt::EndOfLine));
    assert_eq!(body.stmts.len(), 2);
}

#[test]
fn prose_line_carries_interpolation_like_any_content_line() {
    // The issue's own repro: `> [{chan}] {text}`.
    let (hir, _m, diags) = lower_src("fn radio(chan, text) {\n  > [{chan}] {text}\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::Content(c) = &body.stmts[0] else {
        panic!("expected Stmt::Content, got {:?}", body.stmts[0]);
    };
    let interpolations = c
        .parts
        .iter()
        .filter(|p| matches!(p, crate::ContentPart::Interpolation(_)))
        .count();
    assert_eq!(interpolations, 2, "one per `{{…}}` interpolation: {c:?}");
}

#[test]
fn prose_line_with_no_prose_still_lowers_to_a_single_logic_block_unchanged() {
    // Strict generalization, not a behavior change: a code-ground body with
    // no `> text` line in it (every body before this issue) must still
    // lower to exactly one `LogicBlock`, byte-for-byte the prior shape.
    let (hir, _m, diags) = lower_src("fn bump() {\n  n += 1;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert_eq!(body.stmts.len(), 1);
    let Stmt::LogicBlock(lb) = &body.stmts[0] else {
        panic!("expected Stmt::LogicBlock, got {:?}", body.stmts[0]);
    };
    assert_eq!(lb.stmts.len(), 1);
    assert!(matches!(lb.stmts[0], BlockStmt::Assignment(_)));
}

#[test]
fn prose_line_with_no_prose_anchors_provenance_on_the_whole_stmt_block() {
    // Review finding F4: `flush_code_ground_run` anchors a `LogicBlock`'s
    // `ptr` on the run's first item (`run_start`), which is the right
    // choice once a `> text` split has actually happened, but the
    // no-split case must still anchor on the whole `STMT_BLOCK` node —
    // the pre-#1992 shape — since `lb.ptr` is read for diagnostic ranges
    // (`brink-analyzer/src/validate.rs`, `coalesce.rs`) and the module doc
    // claims "byte-for-byte the prior shape" for exactly this case.
    let src = "fn bump() {\n  n += 1;\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    let Stmt::LogicBlock(lb) = &body.stmts[0] else {
        panic!("expected Stmt::LogicBlock, got {:?}", body.stmts[0]);
    };
    let parse = brink_syntax_native::parse(src);
    let expected = parse
        .tree()
        .syntax()
        .descendants()
        .find(|n| n.kind() == N::STMT_BLOCK)
        .expect("STMT_BLOCK in tree")
        .text_range();
    assert_eq!(
        lb.ptr.range, expected,
        "the single-run LogicBlock's ptr must span the whole `STMT_BLOCK`, \
         not just its first statement"
    );
}

#[test]
fn prose_line_interleaves_with_logic_block_runs() {
    // Runs of ordinary statements on either side of a `> text` line each
    // become their own `LogicBlock`, with the content emission sitting
    // between them as an ordinary sibling `Stmt` — not nested inside
    // either logic segment (content is out of `BlockStmt`'s closed set by
    // design, `docs/t1b-surface-spec.md` §2's seam rule).
    let (hir, _m, diags) = lower_src("fn radio() {\n  n = 1;\n  > hi\n  n = 2;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert_eq!(
        body.stmts.len(),
        4,
        "expected [LogicBlock, Content, EndOfLine, LogicBlock]: {:?}",
        body.stmts
    );
    assert!(matches!(body.stmts[0], Stmt::LogicBlock(_)));
    assert!(matches!(body.stmts[1], Stmt::Content(_)));
    assert!(matches!(body.stmts[2], Stmt::EndOfLine));
    assert!(matches!(body.stmts[3], Stmt::LogicBlock(_)));
    let Stmt::LogicBlock(first) = &body.stmts[0] else {
        unreachable!()
    };
    assert!(matches!(first.stmts[0], BlockStmt::Assignment(_)));
    let Stmt::LogicBlock(second) = &body.stmts[3] else {
        unreachable!()
    };
    assert!(matches!(second.stmts[0], BlockStmt::Assignment(_)));
}

#[test]
fn call_containing_run_with_no_split_gets_a_trailing_end_of_line() {
    // Issue #2056: `flush_code_ground_run` built `Stmt::LogicBlock` directly
    // — the whole-body `~{ }` override / `fn`-default path, never going
    // through `lower_logic_line` — so it never inherited #2055's
    // `needs_eol` fix. A call-containing code-ground body with no `> text`
    // split (the plain `fn`-default shape) produced no trailing
    // `Stmt::EndOfLine` at all, gluing the call's emitted output into
    // whatever content followed at the call site (verified end to end via
    // `tests/tier1-native/whole-body-code-ground-call/`).
    let src = "fn shout() >{\n  Hi\n  return\n}\nfn wrapper() {\n  shout();\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let wrapper = hir
        .knots
        .iter()
        .find(|k| k.name.text == "wrapper")
        .expect("wrapper knot");
    assert_eq!(
        wrapper.body.stmts.len(),
        2,
        "expected [LogicBlock, EndOfLine]: {:?}",
        wrapper.body.stmts
    );
    assert!(matches!(wrapper.body.stmts[0], Stmt::LogicBlock(_)));
    assert!(matches!(wrapper.body.stmts[1], Stmt::EndOfLine));
}

#[test]
fn call_containing_run_with_no_split_still_anchors_provenance_on_the_whole_stmt_block() {
    // The trailing `Stmt::EndOfLine` #2056 adds means `stmts` is no longer
    // necessarily a one-element slice for the no-split case — so
    // `lower_stmt_block_as_body`'s F4 re-anchoring (see
    // `prose_line_with_no_prose_anchors_provenance_on_the_whole_stmt_block`
    // above) must still find and re-anchor the single `LogicBlock` by
    // counting `LogicBlock`s, not by matching the whole slice's length.
    let src = "fn shout() >{\n  Hi\n  return\n}\nfn wrapper() {\n  shout();\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let wrapper = hir
        .knots
        .iter()
        .find(|k| k.name.text == "wrapper")
        .expect("wrapper knot");
    let Stmt::LogicBlock(lb) = &wrapper.body.stmts[0] else {
        panic!("expected Stmt::LogicBlock, got {:?}", wrapper.body.stmts[0]);
    };
    let parse = brink_syntax_native::parse(src);
    let expected = parse
        .tree()
        .syntax()
        .descendants()
        .filter(|n| n.kind() == N::STMT_BLOCK)
        .last()
        .expect("wrapper's STMT_BLOCK in tree")
        .text_range();
    assert_eq!(
        lb.ptr.range, expected,
        "the single-run LogicBlock's ptr must still span the whole `STMT_BLOCK`, \
         even though it now has a trailing Stmt::EndOfLine sibling"
    );
}

#[test]
fn one_run_split_body_still_anchors_provenance_on_run_start_not_the_whole_stmt_block() {
    // Review finding F1: the F4 re-anchor above must fire ONLY for a
    // genuinely unsplit body, not merely whenever `stmts` happens to
    // contain exactly one `Stmt::LogicBlock`. A `> text` prose line can
    // split a code-ground body into one run (`Content`/`EndOfLine`) plus a
    // trailing `LogicBlock` with no call in it, e.g. `fn radio() { > hi\n
    // n = 1; }` lowers to `[Content, EndOfLine, LogicBlock]` — exactly one
    // `LogicBlock`, but the item stream WAS split, so `lb.ptr` must stay on
    // `flush_code_ground_run`'s `run_start` anchor (the `n = 1;`
    // `ASSIGN_STMT`), never widen to the whole `STMT_BLOCK` (which would
    // wrongly include the `> hi` prose line in the LogicBlock's
    // diagnostic range).
    let src = "fn radio() {\n  > hi\n  n = 1;\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert_eq!(
        body.stmts.len(),
        3,
        "expected [Content, EndOfLine, LogicBlock]: {:?}",
        body.stmts
    );
    assert!(matches!(body.stmts[0], Stmt::Content(_)));
    assert!(matches!(body.stmts[1], Stmt::EndOfLine));
    let Stmt::LogicBlock(lb) = &body.stmts[2] else {
        panic!("expected Stmt::LogicBlock, got {:?}", body.stmts[2]);
    };
    let parse = brink_syntax_native::parse(src);
    let expected = parse
        .tree()
        .syntax()
        .descendants()
        .find(|n| n.kind() == N::ASSIGN_STMT)
        .expect("ASSIGN_STMT in tree")
        .text_range();
    assert_eq!(
        lb.ptr.range, expected,
        "a split body's trailing single-statement LogicBlock must keep \
         run_start anchoring (the ASSIGN_STMT alone), not widen to the \
         whole STMT_BLOCK"
    );
}

#[test]
fn prose_line_interleaves_with_a_call_containing_logic_block_run() {
    // The split-run sibling of the two tests above: a `> text` line splits
    // a code-ground body into more than one `LogicBlock`, and only the run
    // that actually contains a call gets the trailing `Stmt::EndOfLine` —
    // the boundary belongs right after the call that produced it, not only
    // at the very end of the body (`flush_code_ground_run`'s doc).
    let src =
        "fn shout() >{\n  Hi\n  return\n}\nfn wrapper() {\n  shout();\n  > mid\n  n = 2;\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let wrapper = hir
        .knots
        .iter()
        .find(|k| k.name.text == "wrapper")
        .expect("wrapper knot");
    assert_eq!(
        wrapper.body.stmts.len(),
        5,
        "expected [LogicBlock(call), EndOfLine, Content, EndOfLine, LogicBlock(no call)]: {:?}",
        wrapper.body.stmts
    );
    assert!(matches!(wrapper.body.stmts[0], Stmt::LogicBlock(_)));
    assert!(matches!(wrapper.body.stmts[1], Stmt::EndOfLine));
    assert!(matches!(wrapper.body.stmts[2], Stmt::Content(_)));
    assert!(matches!(wrapper.body.stmts[3], Stmt::EndOfLine));
    assert!(matches!(wrapper.body.stmts[4], Stmt::LogicBlock(_)));
}

#[test]
fn prose_line_nested_in_an_if_body_is_a_loud_e129_not_silent() {
    // This slice only gives `> text` a real content-emission home at a
    // `flow`/`fn`'s own top-level code-ground body (`body::
    // lower_stmt_block_as_body`'s doc); the escape still *parses* at any
    // nesting depth (`stmt::statement()`'s shared dispatch), so a
    // `PROSE_LINE` nested inside an `if` body reaches `control_flow::
    // lower_block_item` directly and must fall to its default `E129` arm —
    // loud, never a silent drop, and never folded into a `LogicBlock`
    // either (that would require inventing the very `BlockStmt::Content`
    // variant the seam rule forbids).
    let (hir, _m, diags) = lower_src("fn radio() {\n  if true {\n    > hi\n  }\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "expected E129 for a prose line nested inside an if body, got: {diags:?}"
    );
    let body = only_knot_body(&hir);
    assert!(
        !body_contains_content(body),
        "a prose line with no content-emission home in this context must never \
         still surface as content: {body:?}"
    );
}

#[test]
fn a_g1_label_on_a_code_ground_prose_line_is_a_loud_e129_not_silently_dropped() {
    // Review finding F3: `lower_code_ground_items` called
    // `lower_content_line_body`, which its own doc says skips the
    // `CONTENT_LINE`'s `LABEL` child deliberately — that's correct for
    // `lower_items`'s weave-ground absorption algorithm (a label there
    // decides how much of the item stream to swallow, consumed by the
    // *caller* before this helper ever sees the line), but this
    // split-run loop has no absorption target at all, so a `(name)` label
    // on a `> text` line here was silently vanishing with no diagnostic —
    // and a later `-> again` divert to that name would then fail to
    // resolve, with no way to trace why.
    let (hir, _m, diags) = lower_src("fn radio() {\n  n = 1;\n  > (again) hi\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "expected E129 for a G-1 label on a code-ground prose line, got: {diags:?}"
    );
    // The label's loss doesn't also swallow the line's own content — the
    // prose line still lowers to ordinary `Stmt::Content`/`Stmt::EndOfLine`
    // siblings, same as an unlabeled one.
    let body = only_knot_body(&hir);
    assert!(
        body_contains_content(body),
        "the prose line's own content must still lower even though its label \
         is rejected: {body:?}"
    );
}

/// Whether any `Stmt::Content` appears anywhere in `block`, including nested
/// inside a `Stmt::LogicBlock`'s own `BlockStmt::If`/`While`/`For` bodies —
/// used by [`prose_line_nested_in_an_if_body_is_a_loud_e129_not_silent`] to
/// assert the *absence* of content at any depth, not just the top level.
fn body_contains_content(block: &crate::Block) -> bool {
    block.stmts.iter().any(stmt_contains_content)
}

fn stmt_contains_content(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Content(_) => true,
        Stmt::LogicBlock(lb) => lb.stmts.iter().any(block_stmt_contains_content),
        _ => false,
    }
}

fn block_stmt_contains_content(stmt: &BlockStmt) -> bool {
    match stmt {
        BlockStmt::If(i) => {
            i.body.iter().any(block_stmt_contains_content)
                || i.else_branch.as_ref().is_some_and(|e| match e {
                    ElseBranch::ElseIf(inner) => {
                        block_stmt_contains_content(&BlockStmt::If((**inner).clone()))
                    }
                    ElseBranch::Else(stmts) => stmts.iter().any(block_stmt_contains_content),
                })
        }
        BlockStmt::While(w) => w.body.iter().any(block_stmt_contains_content),
        BlockStmt::For(f) => f.body.iter().any(block_stmt_contains_content),
        _ => false,
    }
}

#[test]
fn end_and_done_targets_lower_to_sentinel_paths() {
    let (hir, _m, diags) = lower_src("flow a() {\n  -> END\n}\nflow b() {\n  -> DONE\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Divert(d) = &hir.knots[0].body.stmts[0] else {
        panic!("expected Divert");
    };
    assert_eq!(d.target.path, DivertPath::End);
    let Stmt::Divert(d) = &hir.knots[1].body.stmts[0] else {
        panic!("expected Divert");
    };
    assert_eq!(d.target.path, DivertPath::Done);
}

#[test]
fn splice_before_any_choice_becomes_preamble_thread_start() {
    let (hir, _m, diags) = lower_src(
        "flow opts() {\n  {?\n    * X.\n  }\n}\nflow a() {\n  {?\n    <- opts()\n    * Y.\n  }\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[1].body;
    assert!(
        matches!(body.stmts[0], Stmt::ThreadStart(_)),
        "splice before any choice line must be a sibling preceding the ChoiceSet: {body:?}"
    );
    assert!(matches!(body.stmts[1], Stmt::ChoiceSet(_)));
}

#[test]
fn splice_after_a_choice_attaches_to_that_choices_body() {
    let (hir, _m, diags) = lower_src(
        "flow opts() {\n  {?\n    * X.\n  }\n}\nflow a() {\n  {?\n    * Y.\n    <- opts()\n  }\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::ChoiceSet(cs) = &hir.knots[1].body.stmts[0] else {
        panic!("expected ChoiceSet");
    };
    assert!(
        cs.choices[0]
            .body
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::ThreadStart(_))),
        "splice after a choice line must land in that choice's body: {:?}",
        cs.choices[0].body
    );
}

#[test]
fn explicit_return_stamps_explicit_kind() {
    // `>{ }` forces the prose-ground override so this exercises the
    // content-ground `RETURN_STMT` (no `;`) `fixup_return_kind` corrects —
    // `fn`'s new code-ground default has its own always-`Explicit` return
    // with no fixup needed (`parser/stmt.rs::return_stmt`'s doc), covered
    // by `fn_decl_sets_is_function` and the code-ground differential tests.
    let (hir, _m, diags) = lower_src("fn f() >{\n  return\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = &hir.knots[0].body.stmts[0] else {
        panic!("expected Return");
    };
    assert_eq!(r.kind, ReturnKind::Explicit);
    assert!(r.value.is_none());
}

#[test]
fn content_ground_return_with_value_lowers_the_expression() {
    // `>{ }` forces the prose-ground override on an `fn`, exercising the
    // content-ground `RETURN_STMT` value grammar (issue #1973) rather than
    // the code-ground `return expr;` form `lower_return_stmt` already
    // covers. `is_function: true` keeps this legal per E032 (checked in
    // `brink-analyzer`, not here) — the corpus motivation
    // (I003-tunnel-to-death's `~ return hp > 0`) is exactly a function
    // knot's value-carrying return.
    let (hir, _m, diags) = lower_src("fn f() >{\n  return hp > 0\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = &hir.knots[0].body.stmts[0] else {
        panic!("expected Return, got {:?}", hir.knots[0].body.stmts[0]);
    };
    assert_eq!(r.kind, ReturnKind::Explicit);
    assert!(
        matches!(r.value, Some(Expr::Infix(_))),
        "expected an infix comparison value, got {:?}",
        r.value
    );
}

#[test]
fn content_ground_return_with_value_stays_explicit_in_a_non_function() {
    // A value-carrying `return <expr>` inside an ordinary `flow` (not
    // `fn`) parses and lowers cleanly at THIS layer — `fixup_return_kind`
    // only demotes a *bare* (`value.is_none()`) return to
    // `TunnelRedirect`, so a valued one stays `Explicit` regardless of
    // `is_function`. Whether that's semantically legal is
    // `brink-analyzer`'s E032 call (`validate.rs`'s
    // `value_carrying_return_in_non_function_still_emits_e032`), a
    // different crate/pass — this test only pins the lowering shape.
    let (hir, _m, diags) = lower_src("flow f() {\n  Hello.\n  return 5\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = hir.knots[0].body.stmts.last().expect("a Return statement") else {
        panic!("expected Return, got {:?}", hir.knots[0].body.stmts);
    };
    assert_eq!(r.kind, ReturnKind::Explicit);
    assert!(matches!(r.value, Some(Expr::Int(5))));
}

#[test]
fn return_redirect_to_named_path_stamps_tunnel_redirect() {
    let (hir, _m, diags) = lower_src("flow b() {\n  Bye.\n}\nflow a() {\n  return -> b\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = &hir.knots[1].body.stmts[0] else {
        panic!("expected Return, got {:?}", hir.knots[1].body.stmts[0]);
    };
    assert_eq!(r.kind, ReturnKind::TunnelRedirect);
    assert!(matches!(r.value, Some(Expr::DivertTarget(_))));
}

#[test]
fn bare_return_inside_a_non_function_flow_is_a_tunnel_redirect() {
    // `return` inside an ordinary `flow` (reached via tunnel call, ink's
    // bare `->->`) must NOT be `Explicit` — E032 (return outside function)
    // would otherwise fire for perfectly valid tunnel-return code
    // (`tests/tier1-brink-respell/basic-tunnel`).
    let (hir, _m, diags) = lower_src("flow f() {\n  Hello\n  return\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = hir.knots[0].body.stmts.last().expect("a Return statement") else {
        panic!("expected Return, got {:?}", hir.knots[0].body.stmts);
    };
    assert_eq!(r.kind, ReturnKind::TunnelRedirect);
    assert!(
        matches!(
            hir.knots[0].body.tail(),
            Tail::Diverge(Terminator::Return(_))
        ),
        "tail must be recomputed after the fixup: {:?}",
        hir.knots[0].body.tail()
    );
}

#[test]
fn bare_return_inside_a_function_stays_explicit() {
    // `>{ }` — see `explicit_return_stamps_explicit_kind`'s comment: this
    // pins the content-ground fixup's function-vs-flow branch, distinct
    // from the code-ground default's own always-`Explicit` return.
    let (hir, _m, diags) = lower_src("fn f() >{\n  return\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = &hir.knots[0].body.stmts[0] else {
        panic!("expected Return");
    };
    assert_eq!(r.kind, ReturnKind::Explicit);
}

#[test]
fn bare_return_inside_a_choice_body_of_a_non_function_flow_is_a_tunnel_redirect() {
    let (hir, _m, diags) =
        lower_src("flow f() {\n  {?\n    * A. {\n        return\n      }\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::ChoiceSet(cs) = &hir.knots[0].body.stmts[0] else {
        panic!("expected ChoiceSet, got {:?}", hir.knots[0].body.stmts[0]);
    };
    let Stmt::Return(r) = cs.choices[0].body.stmts.last().expect("a Return statement") else {
        panic!(
            "expected Return in choice body, got {:?}",
            cs.choices[0].body.stmts
        );
    };
    assert_eq!(r.kind, ReturnKind::TunnelRedirect);
}

// ─── `fixup_return_kind` recursion into `Stmt::LogicBlock` (#1334) ──
//
// `~{ }` code-ground bodies wrap their statements in a single
// `Stmt::LogicBlock(LogicBlock { stmts: Vec<BlockStmt>, .. })` — a
// structurally different shape from the weave-level `Stmt` variants
// `fixup_return_kind` already walked (`ChoiceSet`/`Conditional`/
// `Sequence`/`LabeledBlock`). Before this fix the `LogicBlock` arm was a
// no-op, so a bare `return;` anywhere inside a `~{ }` body — even directly,
// not just nested inside `if`/`while` — kept `lower_return_stmt`'s
// unconditional `ReturnKind::Explicit` stamp in a non-function flow, which
// is the same E032 false-positive `bare_return_inside_a_non_function_flow_
// is_a_tunnel_redirect` (content-ground/weave level) already guards
// against — just unreached on the code-ground side.
//
// Each test below pins its code-ground result against the equivalent
// content-ground (brink-dialect prose `>{ }`) shape one of the existing
// weave-level fixup tests already established, so "correct ReturnKind"
// means "agrees with the sibling dialect that was already covered", not a
// freshly invented expectation.

#[test]
fn logic_block_bare_return_in_non_function_flow_is_a_tunnel_redirect() {
    // The direct case: `return;` as an immediate `BlockStmt` of the
    // `LogicBlock` itself (no `if`/`while` nesting) — this alone was
    // mis-stamped before the fix, since the old `LogicBlock(_) => {}` arm
    // skipped the block's contents entirely.
    let (hir, _m, diags) = lower_src("flow f() ~{\n  return;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::LogicBlock(lb) = &hir.knots[0].body.stmts[0] else {
        panic!("expected LogicBlock, got {:?}", hir.knots[0].body.stmts[0]);
    };
    let BlockStmt::Return(r) = &lb.stmts[0] else {
        panic!("expected Return, got {:?}", lb.stmts[0]);
    };
    assert_eq!(
        r.kind,
        ReturnKind::TunnelRedirect,
        "must agree with the content-ground equivalent \
         (bare_return_inside_a_non_function_flow_is_a_tunnel_redirect)"
    );
}

#[test]
fn logic_block_bare_return_inside_if_body_is_a_tunnel_redirect() {
    let (hir, _m, diags) = lower_src("flow f() ~{\n  if true {\n    return;\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::LogicBlock(lb) = &hir.knots[0].body.stmts[0] else {
        panic!("expected LogicBlock, got {:?}", hir.knots[0].body.stmts[0]);
    };
    let BlockStmt::If(if_stmt) = &lb.stmts[0] else {
        panic!("expected If, got {:?}", lb.stmts[0]);
    };
    let BlockStmt::Return(r) = &if_stmt.body[0] else {
        panic!("expected Return in if body, got {:?}", if_stmt.body[0]);
    };
    assert_eq!(r.kind, ReturnKind::TunnelRedirect);
}

#[test]
fn logic_block_bare_return_inside_else_body_is_a_tunnel_redirect() {
    let (hir, _m, diags) =
        lower_src("flow f() ~{\n  if false {\n    return;\n  } else {\n    return;\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::LogicBlock(lb) = &hir.knots[0].body.stmts[0] else {
        panic!("expected LogicBlock, got {:?}", hir.knots[0].body.stmts[0]);
    };
    let BlockStmt::If(if_stmt) = &lb.stmts[0] else {
        panic!("expected If, got {:?}", lb.stmts[0]);
    };
    let Some(ElseBranch::Else(else_stmts)) = &if_stmt.else_branch else {
        panic!("expected an else branch, got {:?}", if_stmt.else_branch);
    };
    let BlockStmt::Return(r) = &else_stmts[0] else {
        panic!("expected Return in else body, got {:?}", else_stmts[0]);
    };
    assert_eq!(r.kind, ReturnKind::TunnelRedirect);
}

#[test]
fn logic_block_bare_return_inside_while_body_is_a_tunnel_redirect() {
    let (hir, _m, diags) = lower_src("flow f() ~{\n  while true {\n    return;\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::LogicBlock(lb) = &hir.knots[0].body.stmts[0] else {
        panic!("expected LogicBlock, got {:?}", hir.knots[0].body.stmts[0]);
    };
    let BlockStmt::While(while_stmt) = &lb.stmts[0] else {
        panic!("expected While, got {:?}", lb.stmts[0]);
    };
    let BlockStmt::Return(r) = &while_stmt.body[0] else {
        panic!(
            "expected Return in while body, got {:?}",
            while_stmt.body[0]
        );
    };
    assert_eq!(r.kind, ReturnKind::TunnelRedirect);
}

#[test]
fn logic_block_bare_return_nested_in_if_stays_explicit_inside_a_function() {
    // Mirrors `bare_return_inside_a_function_stays_explicit` (content-ground
    // `>{ }`): the `is_function` guard must still hold at this nesting
    // depth, not just at the `LogicBlock`'s own top level.
    let (hir, _m, diags) = lower_src("fn f() {\n  if true {\n    return;\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::LogicBlock(lb) = &hir.knots[0].body.stmts[0] else {
        panic!("expected LogicBlock, got {:?}", hir.knots[0].body.stmts[0]);
    };
    let BlockStmt::If(if_stmt) = &lb.stmts[0] else {
        panic!("expected If, got {:?}", lb.stmts[0]);
    };
    let BlockStmt::Return(r) = &if_stmt.body[0] else {
        panic!("expected Return in if body, got {:?}", if_stmt.body[0]);
    };
    assert_eq!(r.kind, ReturnKind::Explicit);
}

#[test]
fn return_redirect_to_done_lowers_as_plain_divert() {
    let (hir, _m, diags) = lower_src("flow a() {\n  return -> DONE\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Divert(d) = &hir.knots[0].body.stmts[0] else {
        panic!(
            "expected a plain Divert (Expr::DivertTarget cannot represent DONE), got {:?}",
            hir.knots[0].body.stmts[0]
        );
    };
    assert_eq!(d.target.path, DivertPath::Done);
}

#[test]
fn misplaced_body_annotation_is_diagnosed_not_dropped() {
    let (hir, _m, diags) = lower_src("flow a() {\n  @[effects(pure)]\n}\n");
    // The misplaced annotation itself produces no statement — the only
    // statement is the flow's synthesized implicit `-> DONE` (charter §15).
    let body = &hir.knots[0].body;
    assert_eq!(body.stmts.len(), 1, "only the implicit `-> DONE`: {body:?}");
    assert!(matches!(
        &body.stmts[0],
        Stmt::Divert(d) if d.target.path == DivertPath::Done
    ));
    // A recognized name (`effects`) with nothing following it inside a body
    // is not the placement `annotation::is_consumed_position` accepts (a
    // `flow`/`fn` head immediately after) — E112, not the blanket E129 —
    // but it is still diagnosed, never silently dropped.
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E112),
        "a misplaced body-position annotation must be diagnosed, not silently dropped: {diags:?}"
    );
}

#[test]
fn effects_annotation_on_a_nested_fn_is_diagnosed_not_silently_dropped() {
    // A nested `fn` never lowers to anything (no HIR container below `Knot`
    // carries `is_function`, `container.rs`'s E129 fence) — its attached
    // `@[effects(…)]` must not be waved through as "consumed" only to be
    // read by nothing. `attached_declaration` sees an `FN_DECL` immediately
    // after, so this pins the depth check, not just the declaration kind.
    let (_hir, _m, diags) =
        lower_src("flow a() {\n  @[effects(pure)]\n  fn b() {\n    x\n  }\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "the nested fn itself is still the E129 fence: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E112),
        "the annotation attached to it must be separately diagnosed, not silently dropped: {diags:?}"
    );
}

#[test]
fn effects_annotation_on_a_depth_three_flow_is_diagnosed_not_silently_dropped() {
    // A `flow` nested three levels deep never lowers (the E130 depth fence)
    // — its attached `@[effects(…)]` must not be waved through as
    // "consumed" only to be read by nothing.
    let (_hir, _m, diags) = lower_src(
        "flow a() {\n  flow b() {\n    @[effects(pure)]\n    flow c() {\n      Too deep.\n    }\n  }\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E130),
        "the depth-3 flow itself is still the E130 fence: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E112),
        "the annotation attached to it must be separately diagnosed, not silently dropped: {diags:?}"
    );
}

// ─── `@[element]` / `@[style]` declaration surface (issue #1719) ─────

#[test]
fn element_annotation_lowers_pattern_and_captures() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+): (?<text>.+)$\")]\nflow radio(chan, text) {\n  Hi, {chan} and {text}!\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let knot = &hir.knots[0];
    let element = knot
        .element_annotation
        .as_ref()
        .expect("@[element] must lower to an ElementAnnotation");
    assert!(element.pattern.contains("(?<chan>"));
    assert_eq!(
        element.captures,
        vec!["chan".to_string(), "text".to_string()]
    );
    assert!(element.alias.is_none());
}

#[test]
fn element_annotation_alias_lowers() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+)$\", name = \"walkie\")]\nflow radio(chan) {\n  Hi, {chan}!\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let element = hir.knots[0].element_annotation.as_ref().expect("present");
    assert_eq!(element.alias.as_deref(), Some("walkie"));
}

#[test]
fn element_annotation_missing_args_clause_diagnoses_e159() {
    let (hir, _m, diags) = lower_src("@[element()]\nflow radio(chan) {\n  Hi, {chan}!\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E159),
        "an @[element] with no args= clause must raise E159: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn element_annotation_bad_regex_diagnoses_e159() {
    let (hir, _m, diags) =
        lower_src("@[element(args = \"(unclosed\")]\nflow radio(chan) {\n  Hi, {chan}!\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E159),
        "a pattern that doesn't compile as regex must raise E159: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn element_annotation_capture_without_matching_param_diagnoses_e160() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+)$\")]\nflow radio(other) {\n  Hi, {other}!\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E160),
        "a capture with no matching param must raise E160: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

// ─── `@[element(…, block)]` declaration surface (issue #1839,
// `docs/decision-log.md` 2026-07-31 "Conventions are annotated handlers")
// ───────────────────────────────────────────────────────────────────
//
// This is the declaration-surface slice only, matching the precedent
// #1719 already set for `element`/`style`: parse and validate the `block`
// clause's structural contract (a qualifying trailing `content`-typed
// param). The `!name`/natural-notation dispatch rewrite that would
// actually match a line, find the block's terminator, capture the
// following run as a `Value::FragmentRef`, and call the handler is issue
// #1838's scope and is not implemented here — so there is no dispatch
// pipeline yet to prove `content`'s interior lines keep producing their
// own translatable line entries; that test belongs with #1838's landing.

#[test]
fn element_annotation_block_flag_lowers_with_trailing_content_param() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^@(?<name>[A-Z]+)$\", block)]\nflow cue(name, body: content) {\n  Hi, {name}!\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let element = hir.knots[0]
        .element_annotation
        .as_ref()
        .expect("@[element(…, block)] must still lower to an ElementAnnotation");
    assert!(element.block, "the `block` flag must be recorded");
    assert_eq!(element.captures, vec!["name".to_string()]);
}

#[test]
fn element_annotation_block_without_content_param_diagnoses_e166() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^@(?<name>[A-Z]+)$\", block)]\nflow cue(name) {\n  Hi, {name}!\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E166),
        "a `block` element with no content-typed param must raise E166: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn element_annotation_block_content_param_not_last_diagnoses_e166() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^@(?<name>[A-Z]+)$\", block)]\nflow cue(body: content, name) {\n  Hi, {name}!\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E166),
        "the content-typed param must be trailing — E166 when it isn't last: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn element_annotation_block_content_param_matching_a_capture_diagnoses_e166() {
    // `body` is both the pattern's own named capture and the trailing
    // content-typed param — E160's "does the capture have a matching
    // param" check is satisfied by name alone, so this must be caught by
    // the block contract instead: a capture and the block receiver cannot
    // be the same parameter.
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^@(?<body>[A-Z]+)$\", block)]\nflow cue(body: content) {\n  Hi, {body}!\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E166),
        "a content param that is also a named capture must raise E166: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn element_annotation_duplicate_block_flag_diagnoses_e159() {
    // A single bare `block` in this exact argument position (trailing,
    // after `args`) must be accepted on its own — proven in the same test
    // as the duplicate, not just elsewhere in this file — so the E159
    // below is pinned to the *second* `block` specifically, not to `block`
    // being unrecognized in general (a bare, non-`key = "value"` arg was
    // already E159 before this PR's `ELEMENT_BLOCK` handling existed, so
    // an assertion against only the doubled form passes for the wrong
    // reason and does not guard the new duplicate-flag branch at all).
    let (single_hir, _m, single_diags) = lower_src(
        "@[element(args = \"^@(?<name>[A-Z]+)$\", block)]\nflow cue(name, body: content) {\n  Hi, {name}!\n}\n",
    );
    assert!(
        single_diags.is_empty(),
        "a lone `block` in this position must not raise a diagnostic: {single_diags:?}"
    );
    let single_element = single_hir.knots[0]
        .element_annotation
        .as_ref()
        .expect("a lone `block` must still lower to an ElementAnnotation");
    assert!(
        single_element.block,
        "the lone `block` flag must be recorded"
    );

    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^@(?<name>[A-Z]+)$\", block, block)]\nflow cue(name, body: content) {\n  Hi, {name}!\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E159),
        "a repeated bare `block` clause must raise E159: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn element_annotation_block_with_assigned_value_diagnoses_e159() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^@(?<name>[A-Z]+)$\", block = \"true\")]\nflow cue(name, body: content) {\n  Hi, {name}!\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E159),
        "`block` is a bare flag, not a `key = \"value\"` clause: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn element_annotation_without_block_flag_defaults_false() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+)$\")]\nflow radio(chan) {\n  Hi, {chan}!\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let element = hir.knots[0].element_annotation.as_ref().expect("present");
    assert!(!element.block, "no `block` clause must leave block false");
}

#[test]
fn style_annotation_lowers_with_paired_element() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+): (?<text>.+)$\")]\n@[style(chan = \"channel\", line = \"dim\")]\nflow radio(chan, text) {\n  Hi, {chan} and {text}!\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let style = hir.knots[0]
        .style_annotation
        .as_ref()
        .expect("@[style] must lower to a StyleAnnotation");
    assert_eq!(style.entries.len(), 2);
    assert_eq!(style.entries[0].key, "chan");
    assert_eq!(
        style.entries[0].value,
        crate::StyleToken::Custom("channel".to_string())
    );
    assert_eq!(style.entries[1].key, "line");
    assert_eq!(style.entries[1].value, crate::StyleToken::Dim);
}

#[test]
fn style_annotation_recognizes_built_in_vocabulary() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+)$\")]\n@[style(chan = \"uppercase\", line = \"conceal\", dispatch = \"#a1b2c3\")]\nflow radio(chan) {\n  Hi, {chan}!\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let style = hir.knots[0].style_annotation.as_ref().expect("present");
    assert_eq!(style.entries[0].value, crate::StyleToken::Uppercase);
    assert_eq!(style.entries[1].value, crate::StyleToken::Conceal);
    assert_eq!(
        style.entries[2].value,
        crate::StyleToken::Color("#a1b2c3".to_string())
    );
}

#[test]
fn style_annotation_without_paired_element_diagnoses_e163() {
    let (hir, _m, diags) =
        lower_src("@[style(line = \"dim\")]\nflow radio(chan) {\n  Hi, {chan}!\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E163),
        "@[style] with no paired @[element] must raise E163: {diags:?}"
    );
    assert!(hir.knots[0].style_annotation.is_none());
}

#[test]
fn style_annotation_unknown_key_diagnoses_e162() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+)$\")]\n@[style(nope = \"dim\")]\nflow radio(chan) {\n  Hi, {chan}!\n}\n",
    );
    // Exact vector, not `.any(…)` — `parse_style` must not also report the
    // empty-args `E161` once every clause is rejected (the `!ok` check runs
    // before the emptiness check, mirroring `parse_allow`).
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E162],
        "a style key that is neither line/dispatch nor a capture must raise exactly E162: {diags:?}"
    );
    assert!(hir.knots[0].style_annotation.is_none());
}

#[test]
fn style_annotation_empty_args_diagnoses_e161() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+)$\")]\n@[style()]\nflow radio(chan) {\n  Hi, {chan}!\n}\n",
    );
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E161],
        "an empty @[style()] argument list must raise exactly E161: {diags:?}"
    );
    assert!(hir.knots[0].style_annotation.is_none());
}

/// Regression for the ordering bug where `parse_style` checked
/// `entries.is_empty()` before `!ok`: when every clause in a non-empty
/// argument list is rejected, the diagnostic must be the clause's own code
/// (`E162` here), never also the empty-args `E161`.
#[test]
fn style_annotation_all_clauses_rejected_is_not_also_e161() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>\\\\w+)$\")]\n@[style(chan = bare)]\nflow radio(chan) {\n  Hi, {chan}!\n}\n",
    );
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E161],
        "a malformed clause must raise exactly one E161, not a spurious second one: {diags:?}"
    );
    assert!(hir.knots[0].style_annotation.is_none());
}

#[test]
fn element_and_style_annotation_on_a_nested_fn_is_diagnosed_not_silently_dropped() {
    // Same E129/E112 shape `effects_annotation_on_a_nested_fn_is_diagnosed_
    // not_silently_dropped` already covers for `@[effects]` — a nested `fn`
    // never lowers (E129 fence), so its attached `@[element]`/`@[style]`
    // must not be waved through as "consumed" only to be read by nothing.
    let (_hir, _m, diags) = lower_src(
        "flow a() {\n  @[element(args = \"^(?<chan>\\\\w+)$\")]\n  fn b(chan) {\n    x\n  }\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "the nested fn itself is still the E129 fence: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E112),
        "the annotation attached to it must be separately diagnosed, not silently dropped: {diags:?}"
    );
}

// ─── Natural-notation element dispatch (issue #1838) ────────────────
//
// The `claims = "…"` half of `@[element(…)]`: a pattern claiming a prose
// line that carries no `!name` sigil, rewritten to exactly one call with
// the pattern's named captures bound to the handler's params by name.

/// The single `Stmt::Content` a claimed line lowers to, or `None`.
fn only_claimed_call(block: &crate::Block) -> Option<(&str, Vec<String>)> {
    block.stmts.iter().find_map(|s| match s {
        Stmt::Content(c) => match c.parts.as_slice() {
            [ContentPart::Interpolation(Expr::Call(path, args))] => Some((
                path.segments[0].text.as_str(),
                args.iter()
                    .map(|a| match a {
                        Expr::String(se) => match se.parts.as_slice() {
                            [crate::StringPart::Literal(t)] => t.clone(),
                            other => panic!("expected a literal argument, got {other:?}"),
                        },
                        other => panic!("expected a string argument, got {other:?}"),
                    })
                    .collect(),
            )),
            _ => None,
        },
        _ => None,
    })
}

#[test]
fn a_claimed_content_line_lowers_to_exactly_one_call() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 10)]\nfn arrival(who) {\n  return who;\n}\n\nflow main() {\n  VENDOR enters\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, args) =
        only_claimed_call(&main.body).expect("the claimed line must lower to one call");
    assert_eq!(callee, "arrival");
    assert_eq!(args, vec!["VENDOR".to_string()]);
}

#[test]
fn a_claimed_scene_heading_lowers_to_a_call_and_keeps_its_body() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 20)]\nfn interior(place) {\n  return place;\n}\n\nflow main() {\n  INT. MARKET SQUARE\n  The stalls are shuttered.\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, args) = only_claimed_call(&main.body).expect("the heading must lower to one call");
    assert_eq!(callee, "interior");
    assert_eq!(args, vec!["MARKET SQUARE".to_string()]);
    // The header-scoped body still lowers in place — the heading claims
    // only the heading line, never the run beneath it (block capture is
    // its own slice).
    let rendered = format!("{:?}", main.body.stmts);
    assert!(
        rendered.contains("The stalls are shuttered."),
        "the scene body's own lines must survive: {rendered}"
    );
}

#[test]
fn an_unclaimed_scene_heading_is_still_loudly_unlowered() {
    // The pre-#1838 baseline the dispatch must not disturb: with no
    // claiming handler in the file, a heading is still `E129`, never
    // silently read as ordinary prose.
    let (_hir, _m, diags) = lower_src("flow main() {\n  INT. MARKET SQUARE\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "an unclaimed heading must stay loud: {diags:?}"
    );
}

// ─── `CUE`/`PARENTHETICAL` claim candidates (issue #1720) ───────────
//
// `candidate()`'s widening to the two remaining literal-line grammar
// shapes named in `docs/prose-dialect-spec.md` §3.5b — a genuine `@NAME`
// cue and a chain-gated `(delivery)` parenthetical, not a look-alike
// plain `CONTENT_LINE` (the pre-#1720 fixtures in this file and
// `tests/tier1-native/annotations-element-block/story.brink` all claim
// bare `VENDOR`, never `@VENDOR` — the actual grammar node was never
// reachable before this).

#[test]
fn a_claimed_cue_lowers_to_a_call() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<name>[A-Z][A-Z ]*)$\", order = 30)]\nfn cue(name) {\n  return name;\n}\n\nflow main() {\n  @VENDOR\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, args) =
        only_claimed_call(&main.body).expect("the cue line must lower to one call");
    assert_eq!(callee, "cue");
    assert_eq!(args, vec!["VENDOR".to_string()]);
    assert_eq!(hir.element_matches.len(), 1);
    assert_eq!(hir.element_matches[0].kind, crate::ElementKind::Cue);
}

#[test]
fn an_unclaimed_cue_is_still_loudly_unlowered() {
    let (_hir, _m, diags) = lower_src("flow main() {\n  @VENDOR\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "an unclaimed cue must stay loud: {diags:?}"
    );
}

#[test]
fn a_cue_with_a_tag_extension_is_not_a_claim_candidate() {
    // §8d.4: cue extensions ride the tag channel, e.g. `@VENDOR #(v.o.)`.
    // The tag is structure the pattern is never shown — mirroring the
    // slug/tag-carrying `SCENE_HEADING` case above — so this still falls
    // to the loud `E129` default even with a matching handler declared.
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<name>[A-Z][A-Z ]*)$\", order = 40)]\nfn cue(name) {\n  return name;\n}\n\nflow main() {\n  @VENDOR #(v.o.)\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "a tag-carrying cue must not be silently claimed against a partial line: {diags:?}"
    );
}

#[test]
fn a_claimed_parenthetical_lowers_to_a_call() {
    // A parenthetical is chain-gated by the parser (`at_parenthetical`) —
    // it only parses as `PARENTHETICAL` directly after a live cue.
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<name>[A-Z][A-Z ]*)$\", order = 50)]\nfn cue(name) {\n  return name;\n}\n@[convention(claims = \"^(?<delivery>.+)$\", order = 60)]\nfn parenthetical(delivery) {\n  return delivery;\n}\n\nflow main() {\n  @VENDOR\n  (hushed)\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.element_matches.len(),
        2,
        "both the cue and the parenthetical must be claimed: {:?}",
        hir.element_matches
    );
    assert_eq!(
        hir.element_matches[1].kind,
        crate::ElementKind::Parenthetical
    );
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let rendered = format!("{:?}", main.body.stmts);
    assert!(
        rendered.contains("hushed"),
        "the parenthetical's captured delivery text must reach the call: {rendered}"
    );
}

#[test]
fn a_cue_block_capture_stops_at_a_following_parenthetical_and_the_parenthetical_claims_separately()
{
    // Exercises `block`'s general terminator rule in isolation using a
    // cue/parenthetical-shaped example (§8, "the complement-pass page"):
    // `@VENDOR` / `(hushed)` / dialogue. ⚠ This is no longer the shipped
    // screenplay preset's own shape — issue #2166 migrated the real
    // `std/conventions/screenplay.brink` `cue`/`parenthetical` off `block`
    // entirely, onto attach mode (`attach = StructName`, issue #2178), so
    // neither handler captures anything today. This test still pins the
    // general `block` terminator mechanism `capture_block` implements: the
    // ruled terminator ends a run at "any element-level line", and a
    // `PARENTHETICAL` is explicitly one of those (`element.rs`'s own
    // doc) — so a `block`-declared cue captures ZERO lines here (the very
    // next item is the parenthetical, not a plain `CONTENT_LINE`), and the
    // parenthetical is then claimed on its own next iteration, in turn
    // `block`-capturing the dialogue that follows IT. This is the intended
    // reading of the ruled terminator, not a bug: attachment across a cue
    // AND a parenthetical is two independent claims, not one.
    //
    // The parenthetical's own pattern is deliberately narrow
    // (`^[a-z][a-z' -]*$`, lowercase-only) rather than the obvious `.+`:
    // `try_claim` matches purely on the extracted text against
    // `handler.pattern`, with **no kind restriction at all** —
    // `candidate()` only decides whether a node offers literal text to
    // match, never which handler is allowed to claim which grammar shape.
    // A permissive parenthetical pattern would therefore also claim the
    // captured dialogue line a second time when `capture_block` re-lowers
    // it through the ordinary `body::lower_items` path (proven: an
    // earlier draft of this test used `.+` and got 3 `element_matches`,
    // not 2, because "You shouldn't be here." satisfied it too). This is
    // a real, load-bearing constraint on how the built-in preset's own
    // patterns must be written, not a mechanism bug — see the PR
    // description's own finding.
    let src = "@[convention(claims = \"^(?<name>[A-Z][A-Z ]*)$\", order = 70, block)]\nfn cue(name: string, body: content) {\n  return name;\n}\n@[convention(claims = \"^(?<delivery>[a-z][a-z' -]*)$\", order = 80, block)]\nfn parenthetical(delivery: string, body: content) {\n  return delivery;\n}\n\nflow main() {\n  @VENDOR\n  (hushed)\n  You shouldn't be here.\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.element_matches.len(), 2);
    let cue_match = &hir.element_matches[0];
    assert_eq!(cue_match.handler.text, "cue");
    assert_eq!(
        cue_match.content, None,
        "the cue's own block capture must see zero lines: the very next item is the \
         parenthetical, not a plain CONTENT_LINE"
    );
    let paren_match = &hir.element_matches[1];
    assert_eq!(paren_match.handler.text, "parenthetical");
    let content_range = paren_match
        .content
        .expect("the parenthetical's own block capture must see the dialogue line");
    assert_eq!(
        &src[usize::from(content_range.start())..usize::from(content_range.end())],
        "You shouldn't be here."
    );
}

#[test]
fn a_claim_records_handler_and_capture_spans() {
    let src = "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 90)]\nfn arrival(who) {\n  return who;\n}\n\nflow main() {\n  VENDOR enters\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.element_matches.len(), 1);
    let m = &hir.element_matches[0];
    assert_eq!(m.kind, crate::ElementKind::ContentLine);
    assert_eq!(m.disposition, crate::ElementDisposition::Call);
    assert_eq!(m.handler.text, "arrival");
    // Every recorded coordinate must point at the real bytes it claims to
    // — the no-invisible-expansion guard is worthless if the spans drift.
    assert_eq!(
        &src[usize::from(m.handler.range.start())..usize::from(m.handler.range.end())],
        "arrival"
    );
    assert_eq!(m.captures.len(), 1);
    let c = &m.captures[0];
    assert_eq!(c.name, "who");
    assert_eq!(c.text, "VENDOR");
    assert_eq!(
        &src[usize::from(c.range.start())..usize::from(c.range.end())],
        "VENDOR"
    );
    assert!(
        src[usize::from(m.annotation.start())..usize::from(m.annotation.end())]
            .starts_with("@[convention(claims"),
        "the annotation range must land on the claiming declaration"
    );
}

/// Pull the `Expr::Fragment` a block-capturing claim's call passes as its
/// last argument, panicking with a descriptive message if the shape isn't
/// what a `block`-declared handler's call is supposed to produce.
fn claimed_fragment_stmts(block: &crate::Block) -> &[Stmt] {
    let Some(Stmt::Content(c)) = block.stmts.first() else {
        panic!("expected the claimed line's Content statement first: {block:?}");
    };
    let [ContentPart::Interpolation(Expr::Call(_, args))] = c.parts.as_slice() else {
        panic!("expected a single-call interpolation: {:?}", c.parts);
    };
    let Some(Expr::Fragment(stmts)) = args.last() else {
        panic!("expected the last call argument to be a Fragment: {args:?}");
    };
    stmts
}

#[test]
fn a_block_handler_captures_the_following_run_terminated_by_a_blank_line() {
    // Issue #1839's ruled terminator: "a blank line, or any element-level
    // line". This fixture exercises the blank-line half — two captured
    // lines, then a blank line, then a third line that must stay OUTSIDE
    // the capture (and outside `main.body`'s own top-level statements
    // entirely, since it is absorbed into the `Fragment` argument).
    let src = "@[convention(claims = \"^(?<name>[A-Z]+)$\", order = 100, block)]\nfn cue(name: string, body: content) {\n  return name;\n}\n\nflow main() {\n  VENDOR\n  Line one.\n  Line two.\n\n  After the blank line.\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.element_matches.len(), 1);
    let m = &hir.element_matches[0];
    assert_eq!(m.handler.text, "cue");
    let content_range = m
        .content
        .expect("a block match must record the captured block's own range");
    assert_eq!(
        &src[usize::from(content_range.start())..usize::from(content_range.end())],
        "Line one.\n  Line two.",
        "the recorded content range must cover exactly the two captured lines, no more"
    );

    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let stmts = claimed_fragment_stmts(&main.body);
    // Two captured content lines, each `Stmt::Content` + `Stmt::EndOfLine`.
    assert_eq!(
        stmts.len(),
        4,
        "expected two captured lines' worth of statements: {stmts:?}"
    );
    let rendered = format!("{stmts:?}");
    assert!(
        rendered.contains("Line one.") && rendered.contains("Line two."),
        "both captured lines must be present in the fragment: {rendered}"
    );

    // `main.body`'s own top-level statements are exactly the claimed call
    // (Content + EndOfLine) and the post-blank-line line (Content +
    // EndOfLine) — four statements, not more. The two captured lines live
    // ONLY inside the Fragment nested in the first Content's own call
    // (`stmts`, checked above) — checking `main.body.stmts`' own *length*
    // (rather than searching its `Debug` text, which would trivially find
    // "Line one." nested inside that same first statement's Fragment
    // regardless of whether it also, wrongly, appeared a second time as a
    // sibling) is what actually proves nothing was lowered twice.
    assert_eq!(
        main.body.stmts.len(),
        5,
        "main's own body must contain only the claimed call, the \
         post-blank-line line, and the flow's own implicit end-of-body \
         divert — not the captured lines a second time: {:?}",
        main.body.stmts
    );
    let main_rendered = format!("{:?}", main.body.stmts);
    assert!(
        main_rendered.contains("After the blank line."),
        "the line after the blank line must survive as ordinary content: {main_rendered}"
    );
}

#[test]
fn a_block_handler_captures_the_following_run_terminated_by_an_element_level_line() {
    // The other half of the ruled terminator: a non-`CONTENT_LINE` item
    // (here, a divert) ends the run immediately, even with NO blank line
    // separating it from the last captured line.
    let src = "@[convention(claims = \"^(?<name>[A-Z]+)$\", order = 110, block)]\nfn cue(name: string, body: content) {\n  return name;\n}\n\nflow main() {\n  VENDOR\n  Line one.\n  Line two.\n  -> END\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.element_matches.len(), 1);
    let m = &hir.element_matches[0];
    let content_range = m
        .content
        .expect("a block match must record the captured block's own range");
    assert_eq!(
        &src[usize::from(content_range.start())..usize::from(content_range.end())],
        "Line one.\n  Line two.",
        "the divert must not be absorbed into the captured range"
    );

    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let stmts = claimed_fragment_stmts(&main.body);
    assert_eq!(
        stmts.len(),
        4,
        "expected exactly the two captured lines: {stmts:?}"
    );

    // The divert must still lower as a real, ordinary top-level statement
    // in `main`'s own body, not be swallowed by the capture.
    assert!(
        main.body.stmts.iter().any(|s| matches!(s, Stmt::Divert(_))),
        "the terminating divert must still lower normally: {:?}",
        main.body.stmts
    );
}

#[test]
fn a_captured_line_ending_in_a_divert_does_not_join_the_block() {
    // Reviewer finding on #1839's PR: the terminator search above only
    // recognized a *separate* non-`CONTENT_LINE` sibling as "element-level"
    // — but the native parser fuses a trailing `->`/`->->`/`{?}` onto the
    // SAME `CONTENT_LINE` node as preceding prose
    // (`brink-syntax-native`'s `divert_inside_multiline_choice_body_after_
    // prose_is_a_divert_node` test proves the fused shape). Absorbing such
    // a line into the capture would leave a real `Divert` inside the
    // `Fragment`'s `BeginFragment`/`EndFragment` bracket with no way for
    // `EndFragment` to ever run — the divert transfers control away first
    // — silently corrupting the runtime's fragment-depth tracking
    // (`crates/brink-runtime/src/output/fragment.rs`). The line must stay
    // OUTSIDE the capture and lower normally instead.
    let src = "@[convention(claims = \"^(?<name>[A-Z]+)$\", order = 120, block)]\nfn cue(name: string, body: content) {\n  return name;\n}\n\nflow main() {\n  VENDOR\n  Line one.\n  Get out. -> END\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let m = &hir.element_matches[0];
    let content_range = m
        .content
        .expect("a block match must record the captured block's own range");
    assert_eq!(
        &src[usize::from(content_range.start())..usize::from(content_range.end())],
        "Line one.",
        "the divert-carrying line must not be folded into the captured range"
    );

    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let stmts = claimed_fragment_stmts(&main.body);
    assert_eq!(
        stmts.len(),
        2,
        "expected exactly the one captured line's worth of statements, not the \
         divert-carrying one too: {stmts:?}"
    );
    assert!(
        !format!("{stmts:?}").contains("Divert"),
        "the divert must never appear inside the fragment: {stmts:?}"
    );

    // The divert-carrying line must still lower normally, as an ordinary
    // top-level statement, with its own real `Stmt::Divert`.
    assert!(
        main.body.stmts.iter().any(|s| matches!(s, Stmt::Divert(_))),
        "the divert-carrying line must still lower as a normal top-level \
         statement, not be swallowed by the capture: {:?}",
        main.body.stmts
    );
    let main_rendered = format!("{:?}", main.body.stmts);
    assert!(
        main_rendered.contains("Get out."),
        "the divert line's own prose must survive: {main_rendered}"
    );
}

#[test]
fn a_captured_line_carrying_a_label_does_not_join_the_block() {
    // Same reviewer finding, the other fused shape: a labeled content line
    // (`(name) text`) is still `CONTENT_LINE` kind, but folding it into the
    // capture would let `lower_items`'s own label-absorption mechanism
    // swallow the REST of the captured run into a `Stmt::LabeledBlock`
    // nested inside the `Fragment` — which LIR then rejects with a
    // misleading `E059` about inline-content position rather than anything
    // about block capture (`lir::lower::stmts`'s
    // `reject_unsupported_inline_construct`). The labeled line must stay
    // OUTSIDE the capture and lower normally as its own top-level
    // `Stmt::LabeledBlock`.
    let src = "@[convention(claims = \"^(?<name>[A-Z]+)$\", order = 130, block)]\nfn cue(name: string, body: content) {\n  return name;\n}\n\nflow main() {\n  VENDOR\n  Line one.\n  (later) You wait.\n  -> END\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let m = &hir.element_matches[0];
    let content_range = m
        .content
        .expect("a block match must record the captured block's own range");
    assert_eq!(
        &src[usize::from(content_range.start())..usize::from(content_range.end())],
        "Line one.",
        "the labeled line must not be folded into the captured range"
    );

    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let stmts = claimed_fragment_stmts(&main.body);
    assert_eq!(
        stmts.len(),
        2,
        "expected exactly the one captured line's worth of statements, not the \
         labeled line too: {stmts:?}"
    );

    assert!(
        main.body
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::LabeledBlock(_))),
        "the labeled line must still lower as a normal top-level \
         LabeledBlock, not be swallowed by the capture: {:?}",
        main.body.stmts
    );
}

#[test]
fn a_claiming_handler_does_not_claim_lines_in_its_own_body() {
    // The staging rule §3.5 states for the conventions module ("it cannot
    // use the conventions it defines"): without this the handler's own
    // prose would rewrite into a call on itself.
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 140)]\nfn arrival(who) >{\n  VENDOR enters\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        hir.element_matches.is_empty(),
        "a handler must not claim its own body: {:?}",
        hir.element_matches
    );
}

#[test]
fn a_claiming_pattern_declaring_both_args_and_claims_diagnoses_e159() {
    let (hir, _m, diags) =
        lower_src("@[element(args = \"^a$\", claims = \"^b$\")]\nfn one() {\n  return 1;\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E159),
        "two spellings of the same slot must raise E159: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

#[test]
fn a_claiming_handler_param_with_no_capture_diagnoses_e167() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 150)]\nfn arrival(who, mood) {\n  return who;\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E167),
        "a param no capture binds must raise E167: {diags:?}"
    );
    assert!(hir.knots[0].element_annotation.is_none());
}

// ─── Typed captured params on a claiming handler (issue #1849) ─────
//
// `try_claim` (`hir::lower_native::element`) binds every capture as a
// plain `Expr::String` literal regardless of the receiving parameter's
// declared type — numeric capture coercion does not exist. These tests
// prove the resulting mismatch is now a targeted, declaration-pointed
// diagnostic (`E171`) rather than silence (the pre-#1849 state). Direct-call
// argument type-checking (`E063` for this shape) does not exist yet — that
// is what open issue #1864 asks to build.

#[test]
fn a_claiming_handler_numeric_typed_param_diagnoses_e171() {
    let src = "@[convention(claims = \"^Take (?<n>\\\\d+)$\", order = 160)]\nfn take(n: int) {\n  return n;\n}\n";
    let (hir, _m, diags) = lower_src(src);
    let e171 = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E171)
        .unwrap_or_else(|| panic!("a numeric captured param must raise E171: {diags:?}"));
    // The span must land on `n`'s own type annotation (`int`), not the
    // whole `@[convention(…)]` line and not a claimed prose line — the exact
    // complaint issue #1849 filed against the pre-existing `E063` path.
    assert_eq!(
        &src[usize::from(e171.range.start())..usize::from(e171.range.end())],
        "int",
        "E171 must point at the mismatched param's own type annotation"
    );
    // A handler that fails this check is never registered as a claiming
    // handler at all — same posture as E160/E166/E167 above.
    assert!(hir.knots[0].convention_annotation.is_none());
}

#[test]
fn a_claiming_handler_string_typed_param_does_not_diagnose_e171() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^Take (?<n>\\\\d+)$\", order = 170)]\nfn take(n: string) {\n  return n;\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E171),
        "a string-typed captured param must not raise E171: {diags:?}"
    );
    assert!(hir.knots[0].convention_annotation.is_some());
}

#[test]
fn a_claiming_handler_untyped_param_does_not_diagnose_e171() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^Take (?<n>\\\\d+)$\", order = 180)]\nfn take(n) {\n  return n;\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E171),
        "an untyped captured param must not raise E171: {diags:?}"
    );
    assert!(hir.knots[0].convention_annotation.is_some());
}

#[test]
fn a_claiming_handler_content_typed_param_does_not_diagnose_e171() {
    // `content` is deliberately exempt (see `is_satisfiable_by_a_string_
    // capture`'s own doc): it is the spec-ruled capture annotation form
    // (§3.5b, issue #1846/#1839) — the spec's own ruled `radio`/`interior`
    // examples and the `tier1-native/annotations-element` golden fixture
    // both declare a captured `content` param and compile clean today.
    // Flagging it here would break that already-shipped, ruled pattern
    // for no compiler-observable reason.
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 190)]\nfn interior(place: content) {\n  return \"-- inside {place} --\";\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E171),
        "a content-typed captured param must not raise E171: {diags:?}"
    );
    assert!(hir.knots[0].convention_annotation.is_some());
}

#[test]
fn a_non_claiming_handler_may_have_params_beyond_its_captures() {
    // The asymmetry E167 exists for: a `!name` handler stays callable by
    // hand, so an uncaptured param is not an error there.
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<who>[A-Z]+) enters$\")]\nfn arrival(who, mood) {\n  return who;\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].element_annotation.is_some());
    assert!(hir.knots[0].convention_annotation.is_none());
}

// The `!name` sigil dispatch half of `@[element(…)]` (issue #2004): a
// `BANG_DISPATCH` line dispatches by name to an `args = "…"`-annotated
// handler, the pattern parsing only the remainder after the sigil.

#[test]
fn a_bang_dispatch_line_lowers_to_exactly_one_call() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>[A-Z0-9-]+): (?<text>.+)$\")]\nfn radio(chan, text) {\n  return text;\n}\n\nflow main() {\n  !radio TAC-2: All units report in.\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, args) =
        only_claimed_call(&main.body).expect("the dispatched line must lower to one call");
    assert_eq!(callee, "radio");
    assert_eq!(
        args,
        vec!["TAC-2".to_string(), "All units report in.".to_string()]
    );
}

#[test]
fn a_bang_dispatch_records_handler_and_capture_spans() {
    let src = "@[element(args = \"^(?<chan>[A-Z0-9-]+): (?<text>.+)$\")]\nfn radio(chan, text) {\n  return text;\n}\n\nflow main() {\n  !radio TAC-2: All units report in.\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.element_matches.len(), 1);
    let m = &hir.element_matches[0];
    assert_eq!(m.kind, crate::ElementKind::BangDispatch);
    assert_eq!(m.disposition, crate::ElementDisposition::Call);
    assert_eq!(m.handler.text, "radio");
    assert_eq!(m.captures.len(), 2);
    assert_eq!(m.captures[0].name, "chan");
    assert_eq!(m.captures[0].text, "TAC-2");
    assert_eq!(
        &src[usize::from(m.captures[0].range.start())..usize::from(m.captures[0].range.end())],
        "TAC-2"
    );
    assert_eq!(m.captures[1].name, "text");
    assert_eq!(m.captures[1].text, "All units report in.");
}

#[test]
fn a_bang_dispatch_honors_a_name_alias() {
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^ready$\", name = \"walkie\")]\nfn tally() {\n  return \"ready\";\n}\n\nflow main() {\n  !walkie ready\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, _args) =
        only_claimed_call(&main.body).expect("the aliased dispatch must lower to one call");
    assert_eq!(callee, "tally");
}

#[test]
fn a_bang_dispatch_naming_an_undeclared_handler_is_loudly_unlowered() {
    // No handler at all is declared under this name — the line still
    // parses (the parser cannot know what the lowering pass will find),
    // and this compiler cannot honor it yet, so it must stay loud (E129)
    // rather than silently falling back to plain prose.
    let (_hir, _m, diags) = lower_src("flow main() {\n  !radio TAC-2: hello.\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "an undeclared dispatch name must stay loud: {diags:?}"
    );
}

#[test]
fn a_bang_dispatch_whose_remainder_does_not_match_is_loudly_unlowered() {
    let (_hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<chan>[A-Z0-9-]+): (?<text>.+)$\")]\nfn radio(chan, text) {\n  return text;\n}\n\nflow main() {\n  !radio this does not match the pattern\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "an unmatched remainder must stay loud: {diags:?}"
    );
}

#[test]
fn two_bang_dispatch_handlers_with_the_same_name_first_declared_wins() {
    // Interim rule (`Elements::dispatch`'s own doc), mirroring `claims`'s
    // own interim first-declared-wins pending #1840's registration order.
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^ready$\")]\nfn tally_first() {\n  return \"first\";\n}\n\n@[element(args = \"^ready$\", name = \"tally_first\")]\nfn tally_second() {\n  return \"second\";\n}\n\nflow main() {\n  !tally_first ready\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, _args) = only_claimed_call(&main.body).expect("must dispatch to the first");
    assert_eq!(callee, "tally_first");
}

#[test]
fn a_bang_dispatch_handler_with_an_uncaptured_param_does_not_dispatch() {
    // `annotation::parse_element`'s own doc: a `!name` handler is exempt
    // from `E167`'s declaration-time check and stays callable by hand with
    // ordinary arguments — but the sigil-dispatched rewrite still has no
    // other source of arguments, so a line dispatching to a handler with a
    // param no capture covers must decline (E129), not emit a call with a
    // missing argument.
    let (_hir, _m, diags) = lower_src(
        "@[element(args = \"^(?<who>[A-Z]+) enters$\")]\nfn arrival(who, mood) {\n  return who;\n}\n\nflow main() {\n  !arrival VENDOR enters\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "a param with no capture must decline the dispatch: {diags:?}"
    );
}

#[test]
fn an_escaped_bang_at_line_start_stays_plain_text_and_never_dispatches() {
    // Composition with §8d.6's line-start escape (`\!`, issue #1744/#1978):
    // an author writing a literal leading `!` must not have it silently
    // reinterpreted as a dispatch attempt.
    let (hir, _m, diags) = lower_src(
        "@[element(args = \"^radio.*$\")]\nfn radio() {\n  return \"ping\";\n}\n\nflow main() {\n  \\!radio still just prose.\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        hir.element_matches.is_empty(),
        "an escaped `\\!` must never dispatch: {:?}",
        hir.element_matches
    );
}

// ─── `order` (issue #2164, `docs/decision-log.md` 2026-08-03) ───────────

#[test]
fn a_convention_with_no_order_diagnoses_e178() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\")]\nfn arrival(who) {\n  return who;\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E178),
        "a @[convention] with no order clause must raise E178: {diags:?}"
    );
    // No default — a handler that fails this check is never registered as
    // a claiming handler at all, same posture as E159/E160/E166/E167.
    assert!(hir.knots[0].convention_annotation.is_none());
}

#[test]
fn a_convention_with_an_order_does_not_diagnose_e178() {
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 10)]\nfn arrival(who) {\n  return who;\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E178),
        "a @[convention] with an order clause must not raise E178: {diags:?}"
    );
    let convention = hir.knots[0]
        .convention_annotation
        .as_ref()
        .expect("present");
    assert_eq!(convention.order, 10);
}

#[test]
fn two_conventions_sharing_an_order_diagnose_e179_on_both() {
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^A$\", order = 10)]\nfn a() {\n  return \"a\";\n}\n\n@[convention(claims = \"^B$\", order = 10)]\nfn b() {\n  return \"b\";\n}\n",
    );
    let e179s: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E179)
        .collect();
    assert_eq!(
        e179s.len(),
        2,
        "a duplicate order must be reported against BOTH declarations, not just one: {diags:?}"
    );
    // Anchored on each declaration's own `@[convention(…)]` annotation
    // line, matching E168/E170's own anchor posture — never the `fn` body.
    let src = "@[convention(claims = \"^A$\", order = 10)]\nfn a() {\n  return \"a\";\n}\n\n@[convention(claims = \"^B$\", order = 10)]\nfn b() {\n  return \"b\";\n}\n";
    let ranges: Vec<&str> = e179s
        .iter()
        .map(|d| &src[usize::from(d.range.start())..usize::from(d.range.end())])
        .collect();
    assert!(
        ranges.iter().any(|r| r.contains("claims = \"^A$\"")),
        "{ranges:?}"
    );
    assert!(
        ranges.iter().any(|r| r.contains("claims = \"^B$\"")),
        "{ranges:?}"
    );
    // The ruling (`docs/decision-log.md` 2026-08-03) asks for the message
    // to name both conflicting declarations, the way a duplicate-definition
    // error does — not a generic sentence naming neither. Each message must
    // name the OTHER handler in the pair (its own name plus the one that
    // conflicts with it), and the shared `order` value.
    for d in &e179s {
        assert!(
            d.message.contains("`a`") && d.message.contains("`b`"),
            "each E179 message must name BOTH conflicting handlers: {d:?}"
        );
        assert!(
            d.message.contains("order = 10"),
            "each E179 message must name the shared `order` value: {d:?}"
        );
    }
}

#[test]
fn three_conventions_sharing_an_order_diagnose_e179_on_all_three() {
    // Review finding on #2176: an all-pairs walk over a group of size k
    // emits k*(k-1) diagnostics (six, for three handlers sharing one
    // `order`), each declaration repeated k-1 times. Grouping by `order`
    // must emit exactly one diagnostic per participating declaration.
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^A$\", order = 10)]\nfn a() {\n  return \"a\";\n}\n\n@[convention(claims = \"^B$\", order = 10)]\nfn b() {\n  return \"b\";\n}\n\n@[convention(claims = \"^C$\", order = 10)]\nfn c() {\n  return \"c\";\n}\n",
    );
    let e179s: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E179)
        .collect();
    assert_eq!(
        e179s.len(),
        3,
        "three handlers sharing one order must produce exactly one diagnostic \
         PER declaration (three), not one per pair (six): {diags:?}"
    );
    // Each message must name both OTHER handlers in the group.
    for d in &e179s {
        assert!(
            ["a", "b", "c"]
                .iter()
                .filter(|name| d.message.contains(&format!("`{name}`")))
                .count()
                >= 2,
            "each E179 message must name at least the two OTHER conflicting \
             handlers in a three-way group: {d:?}"
        );
    }
}

#[test]
fn distinct_orders_never_diagnose_e179() {
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^A$\", order = 10)]\nfn a() {\n  return \"a\";\n}\n\n@[convention(claims = \"^B$\", order = 20)]\nfn b() {\n  return \"b\";\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E179),
        "distinct orders must never raise E179: {diags:?}"
    );
}

#[test]
fn order_determines_precedence_not_declaration_position() {
    // The core of issue #2164: a LATER-declared handler with a LOWER
    // `order` must win the claim over an EARLIER-declared handler with a
    // HIGHER `order` — proving precedence now comes from `order`, not
    // textual position (the retired issue #1848 interim rule).
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+)$\", order = 20)]\nfn late_high_order(who) {\n  return \"late\";\n}\n\n@[convention(claims = \"^(?<who>VENDOR)$\", order = 10)]\nfn early_low_order(who) {\n  return \"early\";\n}\n\nflow main() {\n  VENDOR\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, _args) =
        only_claimed_call(&main.body).expect("the claimed line must lower to one call");
    assert_eq!(
        callee, "early_low_order",
        "the lower-`order` handler must win the claim regardless of its later declaration position"
    );
}

// ─── `attach = StructName` (issue #2178, split from #2164's 2026-08-03
//     design-backport comment) ─────────────────────────────────────────

#[test]
fn attach_matching_the_declared_return_type_does_not_diagnose_e180() {
    let (hir, _m, diags) = lower_src(
        "struct Cue {\n  speaker: string\n}\n\n@[convention(claims = \"^(?<who>[A-Z]+)$\", attach = Cue, order = 10)]\nfn cue(who): Cue {\n  return Cue { speaker: who };\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E180),
        "attach matching the declared return type must not raise E180: {diags:?}"
    );
    let convention = hir.knots[0]
        .convention_annotation
        .as_ref()
        .expect("present");
    assert_eq!(
        convention.attach.as_ref().map(|a| a.text.as_str()),
        Some("Cue")
    );
}

#[test]
fn attach_with_no_return_type_at_all_diagnoses_e180() {
    let (hir, _m, diags) = lower_src(
        "struct Cue {\n  speaker: string\n}\n\n@[convention(claims = \"^(?<who>[A-Z]+)$\", attach = Cue, order = 10)]\nfn cue(who) {\n  return who;\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E180),
        "attach with no declared return type at all must raise E180: {diags:?}"
    );
    // No partial annotation — same "never a partial one" posture E159/E178
    // already take.
    assert!(hir.knots[0].convention_annotation.is_none());
}

#[test]
fn attach_naming_a_different_type_than_the_return_type_diagnoses_e180() {
    let (_hir, _m, diags) = lower_src(
        "struct Cue {\n  speaker: string\n}\n\n@[convention(claims = \"^(?<who>[A-Z]+)$\", attach = Cue, order = 10)]\nfn cue(who): string {\n  return who;\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E180),
        "attach naming a struct the return type disagrees with must raise E180: {diags:?}"
    );
}

#[test]
fn no_attach_clause_at_all_never_diagnoses_e180() {
    // `attach` is optional (unlike `order`) — a claiming handler that only
    // ever emits text still needs no declared schema.
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+)$\", order = 10)]\nfn cue(who) {\n  return who;\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E180),
        "no attach clause must never raise E180: {diags:?}"
    );
    assert!(
        hir.knots[0]
            .convention_annotation
            .as_ref()
            .expect("present")
            .attach
            .is_none()
    );
}

#[test]
fn two_byte_identical_claim_patterns_diagnose_e168_on_the_later_one() {
    // Issue #1848: a duplicate claiming pattern is provably unreachable
    // (identical patterns match identical inputs), so the later-declared
    // handler is flagged, not the earlier — the earlier one is the one
    // that actually wins under the interim first-match-wins order.
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 200)]\nfn arrival(who) {\n  return who;\n}\n\n@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 210)]\nfn arrival_again(who) {\n  return who;\n}\n",
    );
    let e168s: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E168)
        .collect();
    assert_eq!(
        e168s.len(),
        1,
        "exactly one duplicate diagnostic, on the later declaration: {diags:?}"
    );
    let second_annotation_start = u32::try_from(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 220)]\nfn arrival(who) {\n  return who;\n}\n\n"
            .len(),
    )
    .expect("fixture length fits in u32");
    let e168_start: u32 = e168s[0].range.start().into();
    assert!(
        e168_start >= second_annotation_start,
        "E168 must point at the later (shadowed) declaration's annotation, not the earlier one: {:?}",
        e168s[0].range
    );
}

#[test]
fn a_byte_identical_twin_that_claims_the_earlier_handlers_own_body_is_not_e168() {
    // Review finding on #1860: `diagnose_duplicate_patterns` must not run
    // from `collect`, before any body is lowered — `try_claim` excludes a
    // handler from claiming lines inside its own declaration (the staging
    // rule), and that exclusion does not extend to a later byte-identical
    // twin. `b` is the *only* handler that can claim the `SIGNAL` line
    // inside `a`'s own body, since `a` is barred from claiming there. `b`
    // is genuinely live for that line, so E168 must not fire on it.
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^SIGNAL$\", order = 230)]\nfn a() >{\n  SIGNAL\n}\n\n@[convention(claims = \"^SIGNAL$\", order = 240)]\nfn b() >{\n  ok\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E168),
        "b actually claimed a line (inside a's own body) — E168 is a false positive here: {diags:?}"
    );
    assert_eq!(
        hir.element_matches.len(),
        1,
        "exactly one line (SIGNAL, inside a's body) is claimable, and only b can claim it: {:?}",
        hir.element_matches
    );
    assert_eq!(
        hir.element_matches[0].handler.text, "b",
        "b must be the handler that actually claimed the line: {:?}",
        hir.element_matches
    );
}

#[test]
fn three_byte_identical_claim_patterns_each_report_one_e168() {
    // Review finding on #1860: the original nested-loop check had no
    // `break`, so a later handler with two earlier identical twins was
    // reported once per twin — a byte-identical duplicate diagnostic.
    // With three identical patterns, exactly two later handlers (`c`,
    // `b`) are dead, and each must be reported exactly once, not once per
    // earlier twin it has.
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 250)]\nfn a(who) {\n  return who;\n}\n\n@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 260)]\nfn b(who) {\n  return who;\n}\n\n@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 270)]\nfn c(who) {\n  return who;\n}\n",
    );
    let e168_ranges: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E168)
        .map(|d| d.range)
        .collect();
    assert_eq!(
        e168_ranges.len(),
        2,
        "exactly one E168 per dead later handler (b, c) — not one per earlier twin: {diags:?}"
    );
    assert_ne!(
        e168_ranges[0], e168_ranges[1],
        "b and c are two distinct dead declarations and must not be reported at the same range twice: {e168_ranges:?}"
    );
}

#[test]
fn allow_e168_above_the_later_declaration_suppresses_it() {
    // Review finding on #1860: E168 was emitted at the annotation's own
    // range, which `AllowScope::range` explicitly excludes (`crate::
    // suppressions`'s module doc: "The annotation line itself is
    // therefore outside every scope it creates"), so `@[allow(E168)]`
    // above the later declaration could never suppress it. Round-trips
    // through `crate::suppressions::apply_suppressions`, the real
    // consumer path every other suppressible code is checked against.
    let src = "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 280)]\nfn arrival(who) {\n  return who;\n}\n\n@[allow(E168)]\n@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 290)]\nfn arrival_again(who) {\n  return who;\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E168),
        "the fixture must still produce E168 before suppression: {diags:?}"
    );
    let suppressions = crate::suppressions::Suppressions {
        allow_scopes: hir.allow_scopes.clone(),
        ..Default::default()
    };
    let remaining = crate::suppressions::apply_suppressions(FileId(0), src, diags, &suppressions);
    assert!(
        !remaining.iter().any(|d| d.code == DiagnosticCode::E168),
        "@[allow(E168)] above the later declaration must suppress its E168: {remaining:?}"
    );
}

#[test]
fn non_identical_overlapping_claim_patterns_that_never_win_are_e170() {
    // Two *different* patterns that can both match the same line
    // (issue #1859, follow-up to #1848). `arrival_general` matches any
    // `[A-Z]+ enters` line, and `arrival_vendor` matches only `VENDOR enters`.
    // Both patterns can accept the same input ("VENDOR enters"), so they
    // overlap. `arrival_vendor` is declared later and never actually wins a
    // claim in this file (the earlier `arrival_general` always wins first),
    // so it is flagged with E170.
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 300)]\nfn arrival_general(who) {\n  return who;\n}\n\n@[convention(claims = \"^(?<who>VENDOR) enters$\", order = 310)]\nfn arrival_vendor(who) {\n  return who;\n}\n",
    );
    let e170s: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E170)
        .collect();
    assert_eq!(
        e170s.len(),
        1,
        "exactly one E170 for the later, unreachable handler: {diags:?}"
    );
}

#[test]
fn distinct_overlapping_claim_patterns_pin_first_match_wins() {
    // Restores the dispatch-order pin the original
    // `distinct_overlapping_claim_patterns_are_not_yet_diagnosed_but_pin_first_match_wins`
    // test made (dropped when E170 was implemented): a non-identical
    // overlap under the interim first-match-wins order is still resolved
    // by declaring earlier — `arrival_general` is the handler that
    // actually claims "VENDOR enters", not `arrival_vendor`, even though
    // `arrival_vendor` is now also diagnosed E170 for never winning.
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 320)]\nfn arrival_general(who) {\n  return who;\n}\n\n@[convention(claims = \"^(?<who>VENDOR) enters$\", order = 330)]\nfn arrival_vendor(who) {\n  return who;\n}\n\nflow main() {\n  VENDOR enters\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E170),
        "arrival_vendor never wins a claim in this file, so E170 must still fire: {diags:?}"
    );
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, _args) =
        only_claimed_call(&main.body).expect("the claimed line must lower to one call");
    assert_eq!(
        callee, "arrival_general",
        "first-declared handler wins under the interim dispatch order"
    );
}

#[test]
fn allow_e170_above_the_later_declaration_suppresses_it() {
    // Parallel to `allow_e168_above_the_later_declaration_suppresses_it`:
    // E170 is deliberately emitted at `later.name.range` so
    // `@[allow(E170)]` can suppress it. Round-trips through
    // `crate::suppressions::apply_suppressions`, the real consumer path
    // every other suppressible code is checked against.
    let src = "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 340)]\nfn arrival_general(who) {\n  return who;\n}\n\n@[allow(E170)]\n@[convention(claims = \"^(?<who>VENDOR) enters$\", order = 350)]\nfn arrival_vendor(who) {\n  return who;\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E170),
        "the fixture must still produce E170 before suppression: {diags:?}"
    );
    let suppressions = crate::suppressions::Suppressions {
        allow_scopes: hir.allow_scopes.clone(),
        ..Default::default()
    };
    let remaining = crate::suppressions::apply_suppressions(FileId(0), src, diags, &suppressions);
    assert!(
        !remaining.iter().any(|d| d.code == DiagnosticCode::E170),
        "@[allow(E170)] above the later declaration must suppress its E170: {remaining:?}"
    );
}

#[test]
fn overlapping_patterns_where_later_handler_actually_wins_are_not_e170() {
    // Review finding on #1885: this fixture previously had no flow at all,
    // so `interior_daytime` never won a single claim and the test was
    // green only because the (then-broken) heuristic never proved overlap
    // either — it did not exercise the `fired` early-return at all.
    // `interior_daytime`'s pattern ("…- DAY$") is a strict subset of
    // `interior_full`'s ("…(?<p>.+)$"), so under first-match-wins dispatch
    // `interior_full` always wins first *except* inside its own
    // declaration, where the staging rule bars it from claiming — the one
    // place `interior_daytime` can actually win a claim of its own. Put
    // exactly such a line inside `interior_full`'s own body so
    // `interior_daytime` is genuinely live and must not be flagged.
    let (hir, _m, diags) = lower_src(
        "@[convention(claims = \"^INT\\\\. (?<p>.+)$\", order = 360)]\nfn interior_full(p) >{\n  INT. KITCHEN - DAY\n}\n\n@[convention(claims = \"^INT\\\\. (?<p>.+) - DAY$\", order = 370)]\nfn interior_daytime(p) >{\n  ok\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E170),
        "interior_daytime actually claimed a line (inside interior_full's own body) — E170 is a false positive here: {diags:?}"
    );
    assert_eq!(
        hir.element_matches.len(),
        1,
        "exactly one line (inside interior_full's body) is claimable, and only interior_daytime can claim it: {:?}",
        hir.element_matches
    );
    assert_eq!(
        hir.element_matches[0].handler.text, "interior_daytime",
        "interior_daytime must be the handler that actually claimed the line: {:?}",
        hir.element_matches
    );
}

#[test]
fn non_overlapping_patterns_are_not_e170() {
    // Two patterns that don't overlap should not be flagged.
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^INT\\\\. (?<p>.+)$\", order = 380)]\nfn interior(p) {\n  return p;\n}\n\n@[convention(claims = \"^EXT\\\\. (?<p>.+)$\", order = 390)]\nfn exterior(p) {\n  return p;\n}\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E170),
        "no overlap between INT.* and EXT.*: {diags:?}"
    );
}

#[test]
fn a_claim_on_a_flow_is_misplaced_e112() {
    // Only a top-level `fn` is callable as an expression, so only a
    // top-level `fn` may claim — and a claim that could never fire must be
    // loud, not inert.
    let (_hir, _m, diags) = lower_src(
        "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 400)]\nflow arrival(who) {\n  Hi, {who}!\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E112),
        "a claim on a flow must be diagnosed misplaced: {diags:?}"
    );
}

#[test]
fn a_claim_on_a_nested_fn_is_misplaced_e112() {
    let (_hir, _m, diags) = lower_src(
        "flow outer() {\n  @[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 410)]\n  fn arrival(who) {\n    return who;\n  }\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E112),
        "a claim on a nested fn must be diagnosed misplaced: {diags:?}"
    );
}

#[test]
fn a_claim_on_a_fn_inside_a_module_is_misplaced_e112() {
    // Issue #1847: a `fn` nested in a `module { … }` block has
    // `container_nesting_depth == 0` (a `MODULE_DECL` ancestor is not a
    // `flow`/`fn`), so it reads as "top-level" by depth alone — but
    // `element::collect` only scans the file's direct children, so a
    // claim admitted there would validate and then never be registered
    // as a handler: a silent drop. It must be diagnosed misplaced
    // instead, same as a claim on a flow or a nested fn.
    let (hir, _m, diags) = lower_src(
        "module npcs {\n  @[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 420)]\n  fn arrival(who) {\n    return who;\n  }\n}\n",
    );
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E112),
        "a claim on a fn nested in a module must be diagnosed misplaced: {diags:?}"
    );
    // And, honestly, it never claims anything either — no handler was
    // ever registered to claim with.
    assert!(
        hir.element_matches.is_empty(),
        "an unregistered claim must not claim: {:?}",
        hir.element_matches
    );
}

#[test]
fn a_line_carrying_interpolation_is_never_claimed() {
    // A claiming pattern matches literal source text; a line with dynamic
    // parts has no fixed text to match and no honest capture spans.
    let (hir, _m, diags) = lower_src(
        "var who = \"VENDOR\"\n\n@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 430)]\nfn arrival(who) {\n  return who;\n}\n\nflow main() {\n  {who} enters\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        hir.element_matches.is_empty(),
        "an interpolated line must not be claimed: {:?}",
        hir.element_matches
    );
}

#[test]
fn element_matches_are_recorded_in_source_order_across_a_choice_point() {
    // DOCS/CONSISTENCY review finding on this PR: `body::lower_items`
    // lowers a `CHOICE_POINT`'s continuation (source-*later*, via
    // `lower_continuation`) before it lowers the choice's own bodies
    // (source-*earlier*, via `lower_choice_point`) — so a claimed line
    // after a choice point is reached, and pushed onto `Elements::matches`,
    // before a claimed line inside the choice body that precedes it in
    // source. `HirFile::element_matches`'s own doc promises source order;
    // `hir::lower_native::lower` must restore it by sorting on `line`
    // before storing the field.
    let src = "@[convention(claims = \"^SIGNAL (?<sound>.+)$\", order = 440)]\nfn effect(sound) {\n  return sound;\n}\n\nflow main() {\n  {?\n    * Option. {\n      SIGNAL EARLY\n    }\n  }\n  SIGNAL LATE\n}\n";
    let (hir, _m, diags) = lower_src(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.element_matches.len(), 2, "{:?}", hir.element_matches);

    let early_pos = src.find("SIGNAL EARLY").expect("EARLY in fixture");
    let late_pos = src.find("SIGNAL LATE").expect("LATE in fixture");
    assert!(
        early_pos < late_pos,
        "fixture sanity: EARLY must precede LATE in source"
    );

    assert_eq!(
        usize::from(hir.element_matches[0].line.start()),
        early_pos,
        "the choice-body claim (source-earlier) must sort first: {:?}",
        hir.element_matches
    );
    assert_eq!(
        usize::from(hir.element_matches[1].line.start()),
        late_pos,
        "the continuation claim (source-later) must sort second: {:?}",
        hir.element_matches
    );
}

// ─── Conventions registry injection point (issue #1863) ────────────
//
// `element::collect`/`lower` accept an externally supplied, already
// ordered registry — claiming handlers declared in some OTHER file — and
// merge it into this file's own dispatch. #1840's comptime evaluator does
// not exist yet, so every fixture below hand-constructs the
// `ExternalConventions` the issue asks for: proof the seam works before
// the evaluator that will eventually feed it.

fn external_arrival_handler() -> ExternalConventions {
    // A hand-built stand-in for what a real project-layer join
    // (`brink_analyzer::conventions_registry::join_conventions_registry`)
    // would hand `lower_with_conventions` once #1840 exists — the exact
    // same `arrival(who)` handler `a_claimed_content_line_lowers_to_exactly_one_call`
    // declares LOCALLY above, this time arriving from outside.
    ExternalConventions::new(vec![ExternalClaimHandler {
        name: Name {
            text: "arrival".to_string(),
            range: TextRange::new(0.into(), 7.into()),
        },
        params: vec!["who".to_string()],
        pattern: "^(?<who>[A-Z]+) enters$".to_string(),
        annotation: TextRange::new(0.into(), 10.into()),
        block: false,
    }])
}

#[test]
fn lower_with_no_external_registry_is_byte_identical_to_lower() {
    // `external: None` must be `lower`'s own implementation, not a
    // parallel path — the whole point of `lower` being a thin wrapper.
    let src = "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 450)]\nfn arrival(who) {\n  return who;\n}\n\nflow main() {\n  VENDOR enters\n}\n";
    let parse = brink_syntax_native::parse(src);
    let tree = parse.tree();
    assert_eq!(
        lower(FileId(0), &tree),
        lower_with_conventions(FileId(0), &tree, None)
    );
}

#[test]
fn an_injected_registry_claims_a_line_in_a_file_that_declares_no_local_handler() {
    // The gap issue #1863 names directly: today a file with zero local
    // `claims = "…"` handlers never dispatches anything, however the
    // project's `[project] elements` module is configured. An injected
    // registry is the seam that changes that.
    let (hir, _m, diags) = lower_with_conventions(
        FileId(0),
        &brink_syntax_native::parse("flow main() {\n  VENDOR enters\n}\n").tree(),
        Some(&external_arrival_handler()),
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let (callee, args) = only_claimed_call(&main.body)
        .expect("the injected handler must claim the line exactly like a local one would");
    assert_eq!(callee, "arrival");
    assert_eq!(args, vec!["VENDOR".to_string()]);
}

#[test]
fn an_injected_handler_never_populates_this_files_own_claim_handlers() {
    // `HirFile::claim_handlers` means "declared in this file" (issue
    // #1844's confinement ground truth) — an injected handler was
    // declared somewhere else, so it must never appear here. If it did,
    // the confinement check would falsely accuse the *injecting* file of
    // hosting a claiming handler it never wrote.
    let (hir, _m, _diags) = lower_with_conventions(
        FileId(0),
        &brink_syntax_native::parse("flow main() {\n  VENDOR enters\n}\n").tree(),
        Some(&external_arrival_handler()),
    );
    assert!(
        hir.claim_handlers.is_empty(),
        "an injected handler must not be recorded as locally declared: {:?}",
        hir.claim_handlers
    );
}

#[test]
fn a_local_handler_wins_over_an_injected_handler_of_the_same_name() {
    // The conventions module's own file is injected with (a subset of)
    // its own declarations once a real project-layer join exists — this
    // is the case that dedup exists for: the local declaration, with its
    // real self-suppression range, must be the one `try_claim` finds
    // first, never the injected duplicate.
    let (hir, _m, diags) = lower_with_conventions(
        FileId(0),
        &brink_syntax_native::parse(
            "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 460)]\nfn arrival(who) {\n  return who;\n}\n\nflow main() {\n  VENDOR enters\n}\n",
        )
        .tree(),
        Some(&external_arrival_handler()),
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.element_matches.len(),
        1,
        "no double-claim from the duplicate injected entry: {:?}",
        hir.element_matches
    );
}

#[test]
fn an_injected_duplicate_with_a_stale_pattern_is_dropped_not_merely_shadowed() {
    // `a_local_handler_wins_over_an_injected_handler_of_the_same_name`
    // above cannot actually distinguish "the same-name injected entry was
    // dropped at `collect` time" from "it was kept but never reached,
    // because `try_claim`'s `handlers.chain(external).find(..)` tries the
    // local one first and local+injected share one byte-identical
    // pattern" — with identical patterns those two behave identically.
    // This fixture gives the injected duplicate a DIFFERENT pattern (a
    // stand-in for a registry gone stale relative to the file's current
    // live declaration) so the two are observationally distinguishable:
    // if `collect` actually drops the duplicate, nothing in this file
    // ever tries the stale pattern at all, so a line matching *only* the
    // stale pattern must go unclaimed.
    let stale_duplicate = ExternalConventions::new(vec![ExternalClaimHandler {
        name: Name {
            text: "arrival".to_string(),
            range: TextRange::new(0.into(), 7.into()),
        },
        params: vec!["who".to_string()],
        // Deliberately NOT the local declaration's pattern — the stand-in
        // for staleness.
        pattern: "^(?<who>[A-Z]+) arrives$".to_string(),
        annotation: TextRange::new(0.into(), 10.into()),
        block: false,
    }]);
    let (hir, _m, diags) = lower_with_conventions(
        FileId(0),
        &brink_syntax_native::parse(
            "@[convention(claims = \"^(?<who>[A-Z]+) enters$\", order = 470)]\nfn arrival(who) {\n  return who;\n}\n\nflow main() {\n  VENDOR arrives\n}\n",
        )
        .tree(),
        Some(&stale_duplicate),
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        hir.element_matches.is_empty(),
        "a same-name injected duplicate must be dropped at collect time, not merely \
         out-dispatched by declaration order — a line matching only the stale \
         injected pattern must stay unclaimed: {:?}",
        hir.element_matches
    );
}

#[test]
fn an_injected_handlers_foreign_annotation_range_never_suppresses_a_claim() {
    // Correctness guard: `ExternalClaimHandler` carries no `decl` at all
    // (only `annotation`, from the DECLARING file's own text) — `collect`
    // sets an injected handler's `decl` to `None` rather than ever
    // reusing `annotation` as a stand-in. If it did, a claimed line in
    // THIS file whose byte offsets happen to fall inside that foreign
    // range would be wrongly self-suppressed — pure numeric coincidence
    // between two files' independent offset spaces, not a real "own
    // body". The fixture constructs exactly that coincidence: the
    // injected handler's (foreign) `annotation` is the same range this
    // file's own claimed `SIGNAL` line occupies inside `flow main()`.
    let src = "flow main() {\n  SIGNAL\n}\n";
    let claimed_start = u32::try_from(src.find("SIGNAL").expect("fixture contains SIGNAL"))
        .expect("fixture length fits in u32");
    let (hir, _m, diags) = lower_with_conventions(
        FileId(0),
        &brink_syntax_native::parse(src).tree(),
        Some(&ExternalConventions::new(vec![ExternalClaimHandler {
            name: Name {
                text: "effect".to_string(),
                range: TextRange::default(),
            },
            params: Vec::new(),
            pattern: "^SIGNAL$".to_string(),
            annotation: TextRange::new(claimed_start.into(), (claimed_start + 6).into()),
            block: false,
        }])),
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(
        hir.element_matches.len(),
        1,
        "a foreign annotation range must never suppress a claim in this file: {:?}",
        hir.element_matches
    );
}

#[test]
fn an_injected_block_declared_handler_still_captures_the_following_run() {
    // Issue #2068: the conventions-module handler this fixture stands in
    // for is `@[convention(claims = "^(?<name>[A-Z]+)$", order = 10, block)]
    // fn cue(name: string, body: content) { ... }` — declared `block` in
    // ANOTHER file,
    // matched here purely via cross-file injection (`external`, no local
    // declaration of `cue` at all). Before #2068 `ExternalClaimHandler`
    // had no `block` field, so `element::collect`'s external branch always
    // set `block: false` for an injected handler regardless of how it was
    // really declared. Traced through `try_claim`: with `block: false`,
    // `bound_len` covers every declared param including `body`, so the
    // capture loop looks for a named group called `body` in a pattern that
    // only ever named `name` — `caps.name("body")` returns `None` and the
    // whole claim is DECLINED, not rewritten with a missing argument. The
    // line stayed plain, unclaimed prose and `hir.element_matches` was
    // empty — that is what the reverted-fix run below actually asserts.
    let src = "flow main() {\n  VENDOR\n  Line one.\n  Line two.\n\n  After the blank line.\n}\n";
    let injected = ExternalConventions::new(vec![ExternalClaimHandler {
        name: Name {
            text: "cue".to_string(),
            range: TextRange::new(0.into(), 3.into()),
        },
        params: vec!["name".to_string(), "body".to_string()],
        pattern: "^(?<name>[A-Z]+)$".to_string(),
        annotation: TextRange::new(0.into(), 10.into()),
        // The declaring file's own `block` clause, carried across the
        // injection join — this is the exact flag issue #2068 fixes the
        // propagation of.
        block: true,
    }]);
    let (hir, _m, diags) = lower_with_conventions(
        FileId(0),
        &brink_syntax_native::parse(src).tree(),
        Some(&injected),
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.element_matches.len(), 1);
    let m = &hir.element_matches[0];
    assert_eq!(m.handler.text, "cue");
    let content_range = m
        .content
        .expect("an injected block-declared handler must still record a captured block range");
    assert_eq!(
        &src[usize::from(content_range.start())..usize::from(content_range.end())],
        "Line one.\n  Line two.",
        "the injected handler's captured range must cover exactly the two follow-on lines"
    );

    let main = hir
        .knots
        .iter()
        .find(|k| k.name.text == "main")
        .expect("main");
    let stmts = claimed_fragment_stmts(&main.body);
    assert_eq!(
        stmts.len(),
        4,
        "the injected handler's call must carry a Fragment with both captured lines' worth \
         of statements, not an empty or missing capture: {stmts:?}"
    );
    let rendered = format!("{stmts:?}");
    assert!(
        rendered.contains("Line one.") && rendered.contains("Line two."),
        "both captured lines must be present in the injected handler's Fragment: {rendered}"
    );
}

// ─── Block::tail (S1, docs/block-effect-model.md §10 row j) ────────
//
// Expand-phase groundwork only: `tail` is populated from `stmts`' final
// statement but consumed by nothing yet — `stmts` stays authoritative.
// These tests pin the native frontend's half of that population.

#[test]
fn block_ending_in_divert_has_diverge_tail() {
    let (hir, _m, diags) = lower_src("flow b() {\n  Bye.\n}\nflow a() {\n  -> b\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let a_body = &hir.knots[1].body;
    assert!(
        matches!(a_body.tail(), Tail::Diverge(Terminator::Divert(_))),
        "expected Diverge(Divert) tail, got {:?}",
        a_body.tail()
    );
}

#[test]
fn block_ending_in_explicit_return_has_diverge_tail() {
    // `>{ }` — see `explicit_return_stamps_explicit_kind`'s comment: a
    // code-ground `fn` body's own tail is `Unit` regardless of its last
    // statement (the whole `STMT_BLOCK` lowers as one `Stmt::LogicBlock`,
    // and `tail_from_stmts` only inspects the *top-level* `Stmt` list — see
    // `body::lower_stmt_block_as_body`'s doc), so this pins the
    // content-ground body's tail computation instead.
    let (hir, _m, diags) = lower_src("fn f() >{\n  return\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[0].body;
    assert!(
        matches!(body.tail(), Tail::Diverge(Terminator::Return(_))),
        "expected Diverge(Return) tail, got {:?}",
        body.tail()
    );
}

#[test]
fn plain_content_flow_body_gets_implicit_done_tail() {
    // A `flow` body that falls off the end (no author-written terminator)
    // inherits ink's root-content implicit-end grace (charter §15): the
    // container finalization appends a synthesized `-> DONE`, so the
    // finalized body's tail is `Diverge(Divert)`, not `Unit`. (The
    // block-construction step still produces `Unit`; the DONE is stamped
    // once, at the flow level, by `container.rs`.)
    let (hir, _m, diags) = lower_src("flow greet(name) {\n  Hi, {name}!\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[0].body;
    assert!(
        matches!(body.tail(), Tail::Diverge(Terminator::Divert(d)) if d.target.path == DivertPath::Done),
        "expected implicit `-> DONE` Diverge tail, got {:?}",
        body.tail()
    );
}

#[test]
fn splice_appended_after_a_choice_body_recomputes_tail() {
    // The choice's own body ends in `-> b` (a genuine terminator, folded
    // into the body preamble by `lower_choice`) before the trailing splice
    // is spliced onto it in place by `lower_choice_point` — this must flip
    // the choice body's tail from `Diverge` back to `Unit` since the splice
    // (`ThreadStart`, never a terminator) becomes the new final statement.
    let (hir, _m, diags) = lower_src(
        "flow opts() {\n  X.\n}\nflow a() {\n  {?\n    * Y. -> a\n    <- opts()\n  }\n}\n",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::ChoiceSet(cs) = &hir.knots[1].body.stmts[0] else {
        panic!("expected ChoiceSet, got {:?}", hir.knots[1].body.stmts[0]);
    };
    let choice = &cs.choices[0];
    assert!(
        matches!(choice.body.stmts.last(), Some(Stmt::ThreadStart(_))),
        "expected the splice to be spliced onto the choice body, got {:?}",
        choice.body.stmts
    );
    assert_eq!(
        *choice.body.tail(),
        Tail::Unit,
        "a trailing splice (non-terminator) must flip tail back to Unit, got {:?}",
        choice.body.tail()
    );
}

// ─── file-level `@[was("old::path")]` module rename (issue #1286) ────────────

#[test]
fn file_level_was_lowers_to_module_rename_record() {
    let (hir, _m, diags) = lower_src("@[was(\"story::old::barter\")]\nflow hero() {\n  hi\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let module = hir
        .module
        .as_ref()
        .expect("a `@[was]` file must carry a ModuleDecl");
    // Name stays empty — native module identity is path-derived and stamped at
    // the project layer (`module_map_query`), never authored in-file.
    assert_eq!(
        module.name, "",
        "native module name is not authored in-file"
    );
    assert_eq!(
        module.was.as_ref().map(|(old, _)| old.as_str()),
        Some("story::old::barter"),
        "the quoted old module path must reach `module.was`"
    );
    // The `@[was]` line must NOT be re-diagnosed as an unlowered construct.
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E129),
        "a recognized `@[was]` must not raise E129: {diags:?}"
    );
}

#[test]
fn file_level_was_unquoted_path_lowers_to_module_rename_record() {
    // Issue #1355: the unquoted `::`-path arg shape (issue #1349's grammar)
    // must reach `hir.module.was` exactly like the quoted string form does —
    // no E132, same rename record.
    let (hir, _m, diags) = lower_src("@[was(story::old::barter)]\nflow hero() {\n  hi\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let module = hir
        .module
        .as_ref()
        .expect("an unquoted `@[was]` file must carry a ModuleDecl");
    assert_eq!(
        module.was.as_ref().map(|(old, _)| old.as_str()),
        Some("story::old::barter"),
        "the unquoted old module path must reach `module.was`"
    );
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E129),
        "a recognized unquoted `@[was]` must not raise E129: {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E132),
        "the unquoted path form must not diagnose E132: {diags:?}"
    );
}

#[test]
fn no_was_annotation_leaves_module_none() {
    let (hir, _m, diags) = lower_src("flow hero() {\n  hi\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        hir.module.is_none(),
        "a file with no `@[was]` carries no ModuleDecl"
    );
}

#[test]
fn malformed_was_without_string_arg_diagnoses_e132() {
    // `@[was]` with no quoted old path: a malformed migration directive. It is
    // surfaced (E132), not silently dropped, and produces no rename record.
    let (hir, _m, diags) = lower_src("@[was]\nflow hero() {\n  hi\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E132),
        "a `@[was]` with no string argument must raise E132: {diags:?}"
    );
    assert!(
        hir.module.is_none(),
        "a malformed `@[was]` produces no rename record"
    );
    // Still recognized as a `was` line — not the generic unlowered-construct
    // E129.
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E129),
        "a malformed `@[was]` must not also raise E129: {diags:?}"
    );
}

#[test]
fn first_was_wins_when_several_are_present() {
    let (hir, _m, _diags) =
        lower_src("@[was(\"story::first\")]\n@[was(\"story::second\")]\nflow hero() {\n  hi\n}\n");
    assert_eq!(
        hir.module
            .as_ref()
            .and_then(|m| m.was.as_ref())
            .map(|(old, _)| old.as_str()),
        Some("story::first"),
        "first `@[was]` wins"
    );
}

// ── NG-A/NG-B/NG-C: `: type` annotations reach HIR ───────────────────
//
// Issues #1487/#1488/#1489. Every annotation position lowers to the SAME
// `hir::TypeExpr` the ink dialect's TM-2 grammar produces, so downstream
// consumers (`brink-analyzer::strict`'s annotation firewall above all)
// need no native-specific branch.

/// The nominal name of a `TypeExpr::Named`, for compact assertions.
fn named(ty: Option<&crate::TypeExpr>) -> Option<&str> {
    match ty? {
        crate::TypeExpr::Named { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

#[test]
fn annotated_params_lower_to_type_exprs() {
    let (hir, _m, diags) = lower_src("fn probability(g: Guest, ref n: int) {\n  return 1;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let params = &hir.knots[0].params;
    assert_eq!(params.len(), 2);
    assert_eq!(named(params[0].annotation.as_ref()), Some("Guest"));
    assert!(!params[0].is_ref);
    assert_eq!(named(params[1].annotation.as_ref()), Some("int"));
    assert!(params[1].is_ref, "`ref` survives alongside the annotation");
}

#[test]
fn unannotated_param_still_lowers_with_none() {
    let (hir, _m, diags) = lower_src("fn heal(hp) {\n  return hp;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].params[0].annotation.is_none());
}

#[test]
fn generic_param_annotation_lowers_with_its_arguments() {
    let (hir, _m, diags) = lower_src("fn tally(m: Map<string, int>) {\n  return 1;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Some(crate::TypeExpr::Generic { name, args, .. }) =
        hir.knots[0].params[0].annotation.as_ref()
    else {
        unreachable!("expected a generic annotation: {:?}", hir.knots[0].params);
    };
    assert_eq!(name, "Map");
    let arg_names: Vec<Option<&str>> = args.iter().map(|a| named(Some(a))).collect();
    assert_eq!(arg_names, vec![Some("string"), Some("int")]);
}

#[test]
fn stitch_params_take_annotations_too() {
    let (hir, _m, diags) =
        lower_src("flow garden() {\n  flow gate(hp: int) {\n    Creak.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let stitch = &hir.knots[0].stitches[0];
    assert_eq!(named(stitch.params[0].annotation.as_ref()), Some("int"));
}

#[test]
fn fn_return_type_lowers_to_knot_return_type() {
    let (hir, _m, diags) = lower_src("fn probability(g: Guest): float {\n  return 1;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(named(hir.knots[0].return_type.as_ref()), Some("float"));
}

#[test]
fn plain_flow_has_no_return_type() {
    let (hir, _m, diags) = lower_src("flow greet() {\n  Hi.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].return_type.is_none());
}

#[test]
fn a_return_typed_flow_does_not_get_the_implicit_done() {
    // The ruled coroutine-vs-state toggle (`docs/decision-log.md`
    // 2026-07-22 implicit-end ruling, item 3): "no return type ⇒ ends
    // implicitly as DONE; has one ⇒ must return". A plain flow's
    // fall-through still picks up the synthesized `-> DONE`; a
    // value-returning one must not, or an author's missing return would be
    // silently rewritten into a quiet ending.
    let (plain, _m, diags) = lower_src("flow quest() {\n  Onward.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        matches!(plain.knots[0].body.stmts.last(), Some(Stmt::Divert(d))
            if d.target.path == crate::DivertPath::Done),
        "a plain flow still ends implicitly: {:?}",
        plain.knots[0].body.stmts
    );

    let (typed, _m, diags) = lower_src("flow quest(): int {\n  Onward.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        !matches!(typed.knots[0].body.stmts.last(), Some(Stmt::Divert(d))
            if d.target.path == crate::DivertPath::Done),
        "a value-returning flow must not be given an implicit DONE: {:?}",
        typed.knots[0].body.stmts
    );
}

#[test]
fn a_return_typed_stitch_lowers_to_stitch_return_type() {
    // #1509: `hir::Stitch` now carries the same `return_type` field
    // `Knot` does, so a nested flow's `: type` clause is honored one level
    // down instead of being E129-fenced away.
    let (hir, _m, diags) = lower_src("flow garden() {\n  flow gate(): int {\n    Creak.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let stitch = &hir.knots[0].stitches[0];
    assert_eq!(named(stitch.return_type.as_ref()), Some("int"));
}

#[test]
fn plain_stitch_has_no_return_type() {
    let (hir, _m, diags) = lower_src("flow garden() {\n  flow gate() {\n    Creak.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.knots[0].stitches[0].return_type.is_none());
}

#[test]
fn a_return_typed_stitch_does_not_get_the_implicit_done() {
    // Same coroutine-vs-state toggle as a top-level flow/fn (see
    // `a_return_typed_flow_does_not_get_the_implicit_done`), now honored
    // one level down (#1509).
    let (plain, _m, diags) = lower_src("flow garden() {\n  flow gate() {\n    Creak.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        matches!(plain.knots[0].stitches[0].body.stmts.last(), Some(Stmt::Divert(d))
            if d.target.path == crate::DivertPath::Done),
        "a plain stitch still ends implicitly: {:?}",
        plain.knots[0].stitches[0].body.stmts
    );

    let (typed, _m, diags) =
        lower_src("flow garden() {\n  flow gate(): int {\n    Creak.\n  }\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(
        !matches!(typed.knots[0].stitches[0].body.stmts.last(), Some(Stmt::Divert(d))
            if d.target.path == crate::DivertPath::Done),
        "a value-returning stitch must not be given an implicit DONE: {:?}",
        typed.knots[0].stitches[0].body.stmts
    );
}

#[test]
fn annotated_var_and_const_lower_their_annotations() {
    let (hir, _m, diags) = lower_src("var hp: int = 10\nconst MAX: int = 100\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(named(hir.variables[0].annotation.as_ref()), Some("int"));
    assert_eq!(named(hir.constants[0].annotation.as_ref()), Some("int"));
    // The annotation is not mistaken for the initializer.
    assert!(matches!(hir.variables[0].value, Expr::Int(_)));
    assert!(matches!(hir.constants[0].value, Expr::Int(_)));
}

#[test]
fn unannotated_var_lowers_with_none() {
    let (hir, _m, diags) = lower_src("var hp = 10\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(hir.variables[0].annotation.is_none());
}

#[test]
fn annotated_let_lowers_to_temp_decl_annotation() {
    let (hir, _m, diags) =
        lower_src("fn heal(hp: int): int {\n  let boost: int = 2;\n  return hp + boost;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let temp = first_temp_decl(&hir.knots[0].body);
    assert_eq!(temp.name.text, "boost");
    assert_eq!(named(temp.annotation.as_ref()), Some("int"));
}

#[test]
fn unannotated_let_lowers_with_none() {
    let (hir, _m, diags) = lower_src("fn heal(hp) {\n  let boost = 2;\n  return hp + boost;\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert!(first_temp_decl(&hir.knots[0].body).annotation.is_none());
}

/// The first `let` binding in a code-ground `fn` body — the `STMT_BLOCK`
/// lowers to a single `Stmt::LogicBlock` whose `BlockStmt`s are the
/// statements.
fn first_temp_decl(body: &crate::Block) -> &crate::TempDecl {
    let Some(Stmt::LogicBlock(lb)) = body.stmts.first() else {
        unreachable!("expected a code-ground body: {:?}", body.stmts);
    };
    let Some(BlockStmt::TempDecl(temp)) = lb.stmts.first() else {
        unreachable!("expected a temp decl: {:?}", lb.stmts);
    };
    temp
}
