#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Issue #858: `declared_shapes`/`ShapeInfo` promoted from `pub(crate)` to a
//! crate-public API so out-of-crate tooling (e.g. `brink-ide` struct-field
//! ref-path completion, T1e-3's deferred "path continuations after a
//! `.`/`[`" item) can query declared `STRUCT` shapes without re-deriving
//! the table `brink-analyzer` already builds for its own construction-
//! literal checks.
//!
//! This test lives in `tests/` (not `src/`) specifically so it exercises
//! the API the way an external consumer crate would: through
//! `brink_analyzer::{declared_shapes, ShapeInfo}` only, with no
//! crate-internal access.

use brink_analyzer::{ImportScope, ShapeInfo, declared_shapes};
use brink_ir::{FileId, HirFile, SymbolManifest};

/// Parse + lower one source file.
fn lower(file: FileId, source: &str) -> (HirFile, SymbolManifest) {
    let parsed = brink_syntax::parse(source);
    let (hir, manifest, diags) = brink_ir::lower(file, &parsed.tree());
    assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
    (hir, manifest)
}

const SRC: &str = "\
STRUCT Npc = #{name: string, hp: int}
VAR player = Npc#{name: \"Hero\", hp: 10}
-> DONE
";

#[test]
fn declared_shapes_is_reachable_from_outside_the_crate() {
    let file = FileId(0);
    let (hir, manifest) = lower(file, SRC);

    let (index, index_diags) = brink_analyzer::symbol_index(&[(file, &manifest)]);
    assert!(index_diags.is_empty(), "index diagnostics: {index_diags:?}");

    let files: &[(FileId, &HirFile)] = &[(file, &hir)];
    let shapes = declared_shapes(files, &index);

    // Issue #2241: `declared_shapes` is referrer-scoped, not a flat bare-name
    // table — a caller resolves a shape against a specific referring file's
    // `ImportScope` (`resolve`), which disambiguates a same-named shape
    // declared in more than one coexisting module (the stdlib mount,
    // #2080). A single-file project like this one always resolves to its
    // own declaration.
    let scope = ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
    let npc: &ShapeInfo = shapes
        .resolve("Npc", &scope, &index)
        .expect("Npc shape declared");
    assert!(npc.has_field("name"));
    assert!(npc.has_field("hp"));
    assert!(!npc.has_field("mana"), "undeclared field must report false");
    assert!(
        npc.field_ty("hp").is_some(),
        "declared field has a resolvable type slot"
    );
    assert!(
        npc.field_ty("mana").is_none(),
        "undeclared field has no type"
    );
}

#[test]
fn declared_shapes_is_empty_for_a_project_with_no_structs() {
    let file = FileId(0);
    let (hir, manifest) = lower(file, "Hello, world.\n-> DONE\n");
    let (index, index_diags) = brink_analyzer::symbol_index(&[(file, &manifest)]);
    assert!(index_diags.is_empty(), "index diagnostics: {index_diags:?}");
    let files: &[(FileId, &HirFile)] = &[(file, &hir)];
    let shapes = declared_shapes(files, &index);
    assert!(shapes.is_empty());
}
