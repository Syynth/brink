//! Call-vs-read resolution parity when a **local shadows a same-named
//! global at a call site** (issue #2083's review follow-up).
//!
//! `resolve_variable`'s bare-read lookup (`lookup_variable`) checks locals
//! *first* (its step 1), so a bare `{twice}` read with both a global
//! `const`/`var twice` and a local `twice` in scope resolves the LOCAL.
//! Before this fix, `resolve_function`'s call-site arm ran the global
//! `[Variable, Constant]` lookup *before* the locals arm — so the identical
//! name at a *call* site (`{twice(21)}`) resolved the GLOBAL instead: one
//! name calling one symbol and reading another. The runtime happened to be
//! saved by `lir::lower::expr::lower_call`'s own temp-slot-first check, but
//! `infer_call` type-checked the call against the wrong signature and IDE
//! navigation followed the wrong symbol.
//!
//! These tests pin the resolution *target* (the call-site `ResolvedRef`
//! must carry a `DefinitionTag::LocalVar` id, never `GlobalVar`) and the
//! type-visible consequence (`infer_call` must type the call against the
//! LOCAL's signature) for both global species — `const` and `var`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use brink_analyzer::{InferenceResult, Ty};
use brink_format::{DefinitionId, DefinitionTag};
use brink_ir::{FileId, ResolutionMap, SymbolIndex, SymbolKind};

/// Lower + index + resolve + infer one native (`.brink`) source.
fn analyze_native(src: &str) -> (ResolutionMap, InferenceResult, SymbolIndex) {
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
    let (index, _diags) = brink_analyzer::symbol_index(&[(FileId(0), &manifest)]);
    let (resolutions, resolve_diags) = brink_analyzer::resolve(
        FileId(0),
        &manifest,
        &index,
        &brink_analyzer::ImportScope::default(),
    );
    assert!(
        resolve_diags.is_empty(),
        "fixture must resolve cleanly: {resolve_diags:?}"
    );
    let inline_docs = BTreeMap::new();
    let result = brink_analyzer::infer_project(
        &[(FileId(0), &hir)],
        &index,
        &resolutions,
        None,
        &inline_docs,
    );
    (resolutions.to_vec(), result, (*index).clone())
}

/// The `ResolvedRef` target at the callee path of `needle` in `src` —
/// located by byte offset so the assertion is pinned to the one call site
/// under test, not any other reference to the same name.
fn call_site_target(src: &str, needle: &str, callee_len: u32, map: &ResolutionMap) -> DefinitionId {
    let offset = u32::try_from(src.find(needle).expect("needle present in fixture")).unwrap();
    let range = rowan::TextRange::new(offset.into(), (offset + callee_len).into());
    map.iter()
        .find(|r| r.range == range)
        .unwrap_or_else(|| panic!("no ResolvedRef at {range:?} (callee of `{needle}`) in {map:?}"))
        .target
}

/// The `DefinitionId` of the fn named `name`.
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

/// Fixture shape shared by both species: a fn-valued GLOBAL named `twice`
/// (int → int) and a same-named LOCAL `let twice = shout` (string → string)
/// in scope at the call site. The bare read and the call site must agree on
/// the local; the signatures deliberately differ so a wrong resolution is
/// also *type*-visible (`r` would infer `int`, not `string`).
fn fixture(global_decl: &str) -> String {
    format!(
        "\
fn double(n: int): int {{
  return n * 2;
}}

fn shout(s: string): string {{
  return s;
}}

{global_decl}

fn caller(): string {{
  let twice = shout;
  let r = twice(\"hi\");
  return r;
}}
"
    )
}

fn assert_call_resolves_the_local(global_decl: &str, species: &str) {
    let src = fixture(global_decl);
    let (map, result, index) = analyze_native(&src);

    // Resolution-level pin: the call site's target is the LOCAL (a
    // `LocalVar`-tagged id), not the same-named global.
    let target = call_site_target(&src, "twice(\"hi\")", 5, &map);
    assert_eq!(
        target.tag(),
        DefinitionTag::LocalVar,
        "with both a global `{species} twice` and a local `let twice` in \
         scope, the CALL site must resolve the local (as a bare read \
         already does — lookup_variable step 1); got a \
         {:?}-tagged target",
        target.tag()
    );

    // Type-level pin: `infer_call` typed the call against the LOCAL's
    // signature (string → string), not the global's (int → int).
    let caller = def_named(&index, "caller");
    assert_eq!(
        local_ty(&result, caller, "r"),
        Ty::String,
        "`r = twice(\"hi\")` must infer through the local `shout` \
         (string → string), not the global `{species} twice = double` \
         (int → int)"
    );
}

/// A global `const` and a same-named local: the call site resolves the
/// local. Red before the locals-first reorder in
/// `resolve::resolve_function` (the global const claimed the call).
#[test]
fn call_site_local_shadows_a_same_named_fn_valued_const_global() {
    assert_call_resolves_the_local("const twice = double", "const");
}

/// The `var` sibling — the same inversion existed for `Variable`-kind
/// globals before #2083's fix (the pre-existing half the locals-first
/// reorder deliberately also corrects).
#[test]
fn call_site_local_shadows_a_same_named_fn_valued_var_global() {
    assert_call_resolves_the_local("var twice = double", "var");
}

/// The read/call agreement itself: in the same fixture, a bare read of
/// `twice` inside `caller` and the call site must resolve to the SAME
/// definition — the divergence (a name reading one symbol and calling
/// another) is the bug, independent of which symbol "wins".
#[test]
fn bare_read_and_call_site_agree_on_the_local() {
    let src = "\
fn double(n: int): int {
  return n * 2;
}

fn shout(s: string): string {
  return s;
}

const twice = double

fn caller(): string {
  let twice = shout;
  let peek = twice;
  let r = twice(\"hi\");
  return r;
}
";
    let (map, _result, _index) = analyze_native(src);

    let read_target = call_site_target(src, "twice;\n  let r", 5, &map);
    let call_target = call_site_target(src, "twice(\"hi\")", 5, &map);
    assert_eq!(
        read_target, call_target,
        "a bare read and a call of the same name in the same scope must \
         resolve to the same definition"
    );
    assert_eq!(read_target.tag(), DefinitionTag::LocalVar);
}
