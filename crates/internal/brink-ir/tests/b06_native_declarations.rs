//! B0.6 exit-criterion tests: native `.brink` declaration-layer lowering
//! (`docs/b0-sequencing.md` §B0.6, `docs/hir-admission-contract.md`, issue
//! #1175).
//!
//! Lives as an integration test (not `#[cfg(test)] mod tests` inside
//! `brink-ir`'s own `src/`) because it needs `brink-analyzer::
//! validate_admission` — a dev-dependency that depends back on `brink-ir`
//! itself. An in-`lib` unit test compiles as part of the `brink_ir` crate
//! under test, so pulling in a dev-dependency that also depends on
//! `brink-ir` produces two non-interchangeable `brink_ir` type instances
//! (one built `--cfg test`, one built plain for `brink-analyzer`) and the
//! compile fails with "there are multiple different versions of crate
//! `brink_ir`". An integration test under `tests/` is its own crate that
//! links the *already-built* `brink-ir` rlib — the same one
//! `brink-analyzer` links — so there is only ever one `brink_ir` in scope.
//! (Existing precedent: `provenance_seam.rs`, `lir_lowering.rs` both use
//! this same shape for the same reason.)
//!
//! The gate (per the builder brief): a native declaration-only fixture
//! lowers to HIR that (a) passes admission with zero diagnostics, (b)
//! projects a `SymbolManifest` structurally matching what the declarations
//! actually say.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::{FileId, SymbolKind};

/// Declaration heads only, every kind B0.6 owns touched once: `var`/
/// `const`/`flags`/`struct`/`extern`/`use`/`import`, a two-level `flow`
/// (knot + stitch, with a `ref` param), and a top-level `fn`.
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

fn lower_fixture(
    src: &str,
) -> (
    brink_ir::HirFile,
    brink_ir::SymbolManifest,
    Vec<brink_ir::Diagnostic>,
) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let tree = parse.tree();
    lower_native::lower(FileId(0), &tree)
}

#[test]
fn declaration_fixture_lowers_with_no_diagnostics() {
    let (_hir, _manifest, diags) = lower_fixture(DECLARATION_FIXTURE);
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );
}

/// The B0.6 gate, part (a): admission-clean.
#[test]
fn declaration_fixture_is_admission_clean() {
    let (hir, manifest, diags) = lower_fixture(DECLARATION_FIXTURE);
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );

    let file_len = rowan::TextSize::of(DECLARATION_FIXTURE);
    let admission_diags = brink_analyzer::validate_admission(FileId(0), &hir, &manifest, file_len);
    assert!(
        admission_diags.is_empty(),
        "native HIR must pass B0.3 admission with zero diagnostics: {admission_diags:?}"
    );
}

/// The B0.6 gate, part (b): the projected `SymbolManifest` structurally
/// matches the declarations — every declared symbol, param, and list item
/// the source actually names, with the right `SymbolKind` bucket and
/// qualification.
#[test]
fn declaration_fixture_projects_a_correct_manifest() {
    let (hir, manifest, diags) = lower_fixture(DECLARATION_FIXTURE);
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );

    // project_manifest is a pure projection — re-deriving it from the same
    // HIR must be byte-identical (PartialEq), proving the manifest returned
    // by `lower` really is just `project_manifest(&hir)` and not something
    // hand-built alongside it (the whole payoff of B0.4's Q3(b)).
    assert_eq!(manifest, brink_ir::project_manifest(&hir));

    assert_eq!(
        manifest
            .variables
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["hp"]
    );
    assert_eq!(
        manifest
            .constants
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["max_hp"]
    );
    assert_eq!(
        manifest
            .lists
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Mood"]
    );
    assert_eq!(
        manifest
            .list_items
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Mood.calm", "Mood.wary", "Mood.hostile"]
    );
    assert_eq!(
        manifest
            .structs
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Npc"]
    );
    assert_eq!(
        manifest
            .externals
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["do_thing"]
    );
    let ext = &manifest.externals[0];
    assert_eq!(ext.params.len(), 2);
    assert_eq!(ext.params[0].name, "a");
    assert_eq!(ext.params[1].name, "b");

    // Containers: `garden` a knot, `gate` a qualified `garden.gate` stitch,
    // `heal` a knot with the function sentinel.
    assert_eq!(
        manifest
            .knots
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["garden", "heal"]
    );
    let garden = manifest
        .knots
        .iter()
        .find(|s| s.name == "garden")
        .expect("garden");
    assert_eq!(garden.detail, None, "flow is not a function");
    let heal = manifest
        .knots
        .iter()
        .find(|s| s.name == "heal")
        .expect("heal");
    assert_eq!(heal.detail.as_deref(), Some("function"));

    assert_eq!(
        manifest
            .stitches
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["garden.gate"]
    );
    let gate = &manifest.stitches[0];
    assert_eq!(gate.params.len(), 1);
    assert_eq!(gate.params[0].name, "visits");
    assert!(gate.params[0].is_ref);

    // Params project as locals scoped to their declaring container.
    assert!(manifest.locals.iter().any(|l| l.name == "mood"
        && l.kind == SymbolKind::Param
        && l.scope.knot.as_deref() == Some("garden")));
    assert!(manifest.locals.iter().any(|l| l.name == "visits"
        && l.kind == SymbolKind::Param
        && l.scope.knot.as_deref() == Some("garden")
        && l.scope.stitch.as_deref() == Some("gate")));
    assert!(manifest.locals.iter().any(|l| l.name == "target"
        && l.kind == SymbolKind::Param
        && l.scope.knot.as_deref() == Some("heal")));
}

/// Depth-3 nesting (Q4(b)) and a nested `fn` are rejected loudly, never
/// silently flattened or dropped — and the parts of the file that *are*
/// well-formed still lower.
#[test]
fn deferred_constructs_are_loud_not_silent() {
    let src = "\
flow a() {
  flow b() {
    flow c() {
      Too deep.
    }
  }
  fn d() {
    x
  }
}

module npcs {
  flow greet() {
    Hi!
  }
}
";
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, manifest, diags) = lower_native::lower(FileId(0), &parse.tree());

    let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
    assert!(
        codes.contains(&brink_ir::DiagnosticCode::E130),
        "depth-3 flow nesting must be E130: {codes:?}"
    );
    assert!(
        codes.contains(&brink_ir::DiagnosticCode::E129),
        "nested fn + module block must be E129: {codes:?}"
    );

    // `a` still lowers with its well-formed stitch `b`; `c` is skipped, not
    // flattened into `a` or `b`.
    let a = hir
        .knots
        .iter()
        .find(|k| k.name.text == "a")
        .expect("knot a");
    assert_eq!(a.stitches.len(), 1);
    assert_eq!(a.stitches[0].name.text, "b");

    // The module block's contents are still flattened in (a separate,
    // documented judgment call from the fence above) — `greet` shows up as
    // an ordinary top-level knot.
    assert!(hir.knots.iter().any(|k| k.name.text == "greet"));

    // Every diagnostic that *was* pushed still leaves an admission-clean
    // HIR/manifest pair for the well-formed remainder — the deferred
    // constructs are cleanly excised, not half-lowered garbage.
    let file_len = rowan::TextSize::of(src);
    let admission_diags = brink_analyzer::validate_admission(FileId(0), &hir, &manifest, file_len);
    assert!(
        admission_diags.is_empty(),
        "the well-formed remainder must still be admission-clean: {admission_diags:?}"
    );
}
