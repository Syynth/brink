//! Native bare-name fn values carry a real type **and a real effect row**
//! (issue #1876 — the typing half of #1862's RULED 2026-08-01 lowering,
//! `docs/t1c-spec.md` §2a).
//!
//! #1862 made a statically-named function in expression position lower to a
//! fn value on the `.brink` surface (`lir::Expr::MakeFnValue`), but left
//! inference behind: `infer::body::InferPass::ty_of_def` had no per-file
//! frontend flag, so the same reference still typed `Ty::Unknown`. Since
//! #1680 a fn value's type is *where its effect row lives*
//! (`Ty::Fn(params, ret, FnRow)`), so an `Unknown`-typed reference carried
//! no row at all — the §6.1b/§7 story (row variables at call sites, token
//! lookup, narrowing) had nothing to work with for exactly the values
//! #1862 had just made writable.
//!
//! These tests therefore assert the **row**, not merely that the type is a
//! `Ty::Fn`: a reference to `double` must carry `FnRow::of_target(double)`,
//! the same row the ink `#fn(double)` spelling mints at its creation site
//! (`infer_fn_literal`) — one spelling per surface, one type.
//!
//! The ink half is pinned alongside: in ink the identical bare name is a
//! knot's **visit count**, so it must keep typing `Unknown` there. That
//! pairing is the whole point of threading a per-file flag rather than
//! changing the rule globally.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use brink_analyzer::{FnRow, InferenceResult, Ty};
use brink_format::DefinitionId;
use brink_ir::{FileId, SymbolIndex, SymbolKind, SymbolManifest};

/// Lower + index + resolve + infer one native (`.brink`) source.
fn infer_native(src: &str) -> (InferenceResult, SymbolIndex) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, manifest, diags) = brink_ir::hir::lower_native::lower(FileId(0), &parse.tree());
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );
    assert!(hir.native, "lower_native must stamp HirFile::native");
    infer(&hir, &manifest)
}

/// The ink counterpart of [`infer_native`], for the surface-split assertion.
fn infer_ink(src: &str) -> (InferenceResult, SymbolIndex) {
    let parsed = brink_syntax::parse(src);
    let (hir, manifest, _diags) = brink_ir::lower(FileId(0), &parsed.tree());
    assert!(
        !hir.native,
        "the ink frontend must not stamp HirFile::native"
    );
    infer(&hir, &manifest)
}

fn infer(hir: &brink_ir::HirFile, manifest: &SymbolManifest) -> (InferenceResult, SymbolIndex) {
    let (index, _diags) = brink_analyzer::symbol_index(&[(FileId(0), manifest)]);
    let (resolutions, _diags) = brink_analyzer::resolve(
        FileId(0),
        manifest,
        &index,
        &brink_analyzer::ImportScope::default(),
    );
    let inline_docs = BTreeMap::new();
    let result = brink_analyzer::infer_project(
        &[(FileId(0), hir)],
        &index,
        &resolutions,
        None,
        &inline_docs,
    );
    (result, (*index).clone())
}

/// The `DefinitionId` of a knot/stitch by (unqualified) name.
fn def_named(index: &SymbolIndex, name: &str) -> DefinitionId {
    *index
        .symbols
        .iter()
        .find(|(_, info)| {
            info.name == name && matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch)
        })
        .unwrap_or_else(|| panic!("no knot/stitch named `{name}` in the index"))
        .0
}

/// The type inference gave local `local` inside def `owner`.
fn local_ty(result: &InferenceResult, owner: DefinitionId, local: &str) -> Ty {
    result
        .bodies
        .get(&owner)
        .unwrap_or_else(|| panic!("no inferred body for {owner:?}"))
        .locals
        .get(local)
        .unwrap_or_else(|| panic!("no local `{local}` in the inferred body"))
        .clone()
}

const NATIVE_SRC: &str = "\
fn double(x: int): int {
  return x * 2;
}

fn hold(): int {
  let f = double;
  return f(3);
}
";

/// The core of #1876: the reference's inferred type is a `Ty::Fn` whose
/// **row names the target**, not the unknown top element — the row is the
/// deliverable, `Ty::Fn` alone is not.
#[test]
fn a_native_bare_name_reference_carries_its_targets_effect_row() {
    let (result, index) = infer_native(NATIVE_SRC);
    let double = def_named(&index, "double");
    let hold = def_named(&index, "hold");

    let ty = local_ty(&result, hold, "f");
    let Ty::Fn(params, ret, row) = ty else {
        panic!("a bare-name fn value must type as `Ty::Fn`, got {ty:?}");
    };
    assert_eq!(
        row,
        FnRow::of_target(double),
        "the row must name the creation target — an unknown row is exactly \
         the #1876 gap (row present but empty of evidence)"
    );
    assert!(
        !row.is_unknown(),
        "the top element carries no evidence for §6.1b/§7 narrowing"
    );
    // The whole signature is the value's parameter row: the native surface
    // has no partial-application spelling, so a bare name binds zero args.
    assert_eq!(params, vec![Ty::Int]);
    assert_eq!(*ret, Ty::Int);
}

/// The ink surface is untouched: the identical bare name is a knot's visit
/// count there, so it must keep typing `Unknown`. This is what makes the
/// per-file frontend flag necessary rather than a global rule change.
#[test]
fn the_same_bare_name_stays_a_visit_count_in_ink() {
    let (result, index) = infer_ink(
        "\
=== function double(x) ===
~ return x * 2

=== function hold() ===
~ temp f = double
~ return f
",
    );
    let hold = def_named(&index, "hold");
    assert_eq!(
        local_ty(&result, hold, "f"),
        Ty::Unknown,
        "in ink a bare knot name is a visit count, never a fn value"
    );
}

/// The effects half (`docs/effects-spec.md` §5/§6.1a): the reference is an
/// ordinary **creation site**, so it must be harvested as one — a call-graph
/// edge plus the Fork A creation atom (issue #1726), keeping
/// `EffectAtoms::creates_fn_values ⊆ EffectAtoms::direct_calls`. Without
/// this, a fn value #1862 made writable would be invisible to the effect
/// fixpoint that owns its row.
#[test]
fn a_native_bare_name_reference_is_harvested_as_a_creation_site() {
    let parse = brink_syntax_native::parse(NATIVE_SRC);
    let (hir, manifest, _diags) = brink_ir::hir::lower_native::lower(FileId(0), &parse.tree());
    let (index, _diags) = brink_analyzer::symbol_index(&[(FileId(0), &manifest)]);
    let (resolutions, _diags) = brink_analyzer::resolve(
        FileId(0),
        &manifest,
        &index,
        &brink_analyzer::ImportScope::default(),
    );
    let inferable = brink_analyzer::inferable_defs(&[(FileId(0), &hir)], &index);
    let double = def_named(&index, "double");
    let hold = def_named(&index, "hold");

    let atoms = brink_analyzer::def_effect_atoms(
        hold,
        &[(FileId(0), &hir)],
        &index,
        &resolutions,
        &inferable,
        None,
    );
    assert!(
        atoms.creates_fn_values.contains(&double),
        "the bare name is a creation site for `double`: {:?}",
        atoms.creates_fn_values
    );
    assert!(
        atoms.creates_fn_values.is_subset(&atoms.direct_calls),
        "creates_fn_values must stay a subset of direct_calls: {atoms:?}"
    );
    assert!(
        !atoms.opaque,
        "the call through `f` traces to a known creation target, so the body \
         must not fall to the pessimal floor: {atoms:?}"
    );
}
