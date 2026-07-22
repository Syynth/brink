#![allow(clippy::panic)]

use super::*;
use crate::{DiagnosticCode, Tail, Terminator};

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
    assert_eq!(
        body.stmts.len(),
        1,
        "glue suppresses the EndOfLine: {body:?}"
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

#[test]
fn content_without_glue_gets_end_of_line() {
    let (hir, _m, diags) = lower_src("flow a() {\n  Plain line.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = only_knot_body(&hir);
    assert_eq!(body.stmts.len(), 2);
    assert!(matches!(body.stmts[0], Stmt::Content(_)));
    assert!(matches!(body.stmts[1], Stmt::EndOfLine));
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
            matches!(branch.stmts[0], Stmt::EndOfLine),
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
fn inline_divert_mid_content_line_splits_into_two_statements() {
    let (hir, _m, diags) = lower_src("flow b() {\n  Bye.\n}\nflow a() {\n  The wager. -> b\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[1].body;
    assert!(matches!(body.stmts[0], Stmt::Content(_)));
    assert!(matches!(body.stmts[1], Stmt::Divert(_)));
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
    let (hir, _m, diags) = lower_src("fn f() {\n  return\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::Return(r) = &hir.knots[0].body.stmts[0] else {
        panic!("expected Return");
    };
    assert_eq!(r.kind, ReturnKind::Explicit);
    assert!(r.value.is_none());
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
fn unrecognized_body_construct_is_diagnosed_not_dropped() {
    let (hir, _m, diags) = lower_src("flow a() {\n  @[effects(pure)]\n}\n");
    assert!(hir.knots[0].body.stmts.is_empty());
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E129),
        "an unwired body-position construct must be diagnosed, not silently dropped: {diags:?}"
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
    let (hir, _m, diags) = lower_src("fn f() {\n  return\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let body = &hir.knots[0].body;
    assert!(
        matches!(body.tail(), Tail::Diverge(Terminator::Return(_))),
        "expected Diverge(Return) tail, got {:?}",
        body.tail()
    );
}

#[test]
fn plain_content_block_has_unit_tail() {
    let (hir, _m, diags) = lower_src("flow greet(name) {\n  Hi, {name}!\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(*hir.knots[0].body.tail(), Tail::Unit);
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
