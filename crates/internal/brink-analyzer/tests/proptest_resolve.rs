#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::needless_pass_by_value
)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into production output can't ship quietly.
// This file's three `HashSet`s are all membership-only (`.contains()` /
// `.insert()`-as-duplicate-guard on a `prop_assert!`) and never iterated —
// order-free by construction — but `rowan::TextRange`/`DiagnosticCode`
// don't implement `Ord`, so routing them through the crate's `LookupSet`
// alias (`pub(crate)`, invisible to this external test-binary crate) or a
// `BTreeSet` both need trait changes out of this issue's scope. A file-level
// allow here (test-only code, not production output) is the narrower fix.
#![allow(
    clippy::disallowed_types,
    reason = "membership-only HashSets, audited order-free — see file doc"
)]

//! Property-based tests for name resolution.
//!
//! These tests generate arbitrary symbol manifests and verify structural
//! invariants of the resolution algorithm — things that must hold regardless
//! of what names or scopes are involved.

use std::collections::HashSet;

use brink_ir::{
    DeclaredSymbol, DiagnosticCode, FileId, RefKind, Scope, SymbolKind, SymbolManifest,
    UnresolvedRef,
};
use proptest::prelude::*;
use rowan::{TextRange, TextSize};

use brink_analyzer::analyze;
use brink_analyzer::test_support::{is_builtin_function, is_t1b_stdlib_name};
use brink_ir::HirFile;

// ─── Strategies ─────────────────────────────────────────────────────

fn range(offset: u32, len: u32) -> TextRange {
    TextRange::new(TextSize::new(offset), TextSize::new(offset + len))
}

/// Generate a valid ink-style identifier (lowercase ascii + underscore, 1-12 chars).
fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z_]{0,11}".prop_filter("not empty", |s| !s.is_empty())
}

fn arb_scope(knot_names: Vec<String>) -> impl Strategy<Value = Scope> {
    if knot_names.is_empty() {
        Just(Scope {
            knot: None,
            stitch: None,
        })
        .boxed()
    } else {
        (
            prop::sample::select(knot_names),
            prop::option::of(arb_ident()),
        )
            .prop_map(|(knot, stitch)| Scope {
                knot: Some(knot),
                stitch,
            })
            .boxed()
    }
}

fn arb_ref_kind() -> impl Strategy<Value = RefKind> {
    prop_oneof![
        Just(RefKind::Divert),
        Just(RefKind::Variable),
        Just(RefKind::Function),
        Just(RefKind::List),
        Just(RefKind::Struct),
        Just(RefKind::Type),
    ]
}

/// [`arb_ref_kind`] narrowed to the kinds that **always** diagnose on a
/// miss (issue #2249) — every kind except `RefKind::Type`, whose own doc
/// (`resolve::resolve_type_ref`) explains why "unresolved" is not
/// synonymous with "invalid" for a TM-2 annotation (`int`, `List`, … are
/// equally legal `Named` leaves that were never meant to resolve as a
/// struct at all). Used by properties like `missing_ref_always_diagnosed`
/// that assert the unconditional "a ref to something that isn't declared
/// always gets a diagnostic" contract — true for four of five kinds, not
/// this one.
fn arb_diagnosing_ref_kind() -> impl Strategy<Value = RefKind> {
    prop_oneof![
        Just(RefKind::Divert),
        Just(RefKind::Variable),
        Just(RefKind::Function),
        Just(RefKind::List),
        Just(RefKind::Struct),
    ]
}

/// Structural exhaustiveness guard (issue #1542, extending the same
/// exhaustiveness-guard pattern — #667/#883, most recently extended by
/// #1521 in `brink-runtime`'s `law_transcript_roundtrip.rs`): a match over
/// every current [`RefKind`] variant with no wildcard arm, so this fails to
/// compile the moment a new variant is added to the enum. Never called —
/// the forcing function is the compile error itself. `arb_ref_kind` above
/// used to hand-list only 4 of `RefKind`'s 5 variants — `Struct` (TM-4b) was
/// added to the enum without a matching `prop_oneof!` arm, so every
/// `proptest!` in this file that exercises `arb_ref_kind`/`arb_manifest` ran
/// 500+ generated cases per run and never once generated a struct-kind ref.
/// Whoever adds a `RefKind` variant must now also add an arm here — and
/// extend `arb_ref_kind` to generate it. `RefKind::Type` (issue #2249) is
/// the first documented-exclusion precedent for this enum, the
/// `OutputPart::Checkpoint` pattern `law_transcript_roundtrip.rs` (#1521)
/// established: it stays listed here (never a wildcard) so the guard still
/// trips if it is ever removed or split, but see `completeness`'s own doc
/// below for why it needs special handling in that one property, not a
/// blanket exclusion from generation.
#[expect(dead_code, reason = "compile-time-only exhaustiveness guard, see doc")]
fn assert_ref_kind_variants_exhaustive(kind: RefKind) {
    match kind {
        RefKind::Divert
        | RefKind::Variable
        | RefKind::Function
        | RefKind::List
        | RefKind::Struct
        | RefKind::Type => {}
    }
}

/// Strategy that generates a manifest with 1-5 knots, 0-3 variables,
/// 0-2 lists with items, and 0-8 unresolved refs that may or may not match.
/// A `DeclaredSymbol` with only name/range set — every other field at its
/// "nothing declared" default. Shared by every `arb_manifest` construction
/// site below to keep the strategy closure short.
fn decl_sym(name: String, range: TextRange) -> DeclaredSymbol {
    DeclaredSymbol {
        name,
        range,
        params: Vec::new(),
        detail: None,
        visibility: None,
        was: None,
    }
}

fn arb_manifest() -> impl Strategy<Value = SymbolManifest> {
    (
        prop::collection::vec(arb_ident(), 1..=5), // knot names
        prop::collection::vec(arb_ident(), 0..=3), // variable names
        prop::collection::vec(
            (arb_ident(), prop::collection::vec(arb_ident(), 1..=4)), // lists with items
            0..=2,
        ),
        prop::collection::vec(arb_ident(), 0..=2), // externals
        prop::collection::vec(arb_ident(), 0..=2), // struct names
    )
        .prop_flat_map(|(knots, vars, lists, externals, structs)| {
            let knot_names = knots.clone();
            let all_names: Vec<String> = knots
                .iter()
                .chain(vars.iter())
                .chain(externals.iter())
                .chain(structs.iter())
                .chain(lists.iter().map(|(name, _)| name))
                .chain(lists.iter().flat_map(|(_, items)| items.iter()))
                .cloned()
                .collect();

            // Mix of resolvable and unresolvable refs
            let ref_targets = all_names
                .into_iter()
                .chain(std::iter::once("definitely_missing".to_string()))
                .collect::<Vec<_>>();

            let refs_strategy = prop::collection::vec(
                (
                    prop::sample::select(ref_targets),
                    arb_ref_kind(),
                    arb_scope(knot_names.clone()),
                ),
                0..=8,
            );

            refs_strategy.prop_map(move |refs| {
                let mut manifest = SymbolManifest::default();
                let mut offset = 0u32;

                for name in &knots {
                    manifest
                        .knots
                        .push(decl_sym(name.clone(), range(offset, name.len() as u32)));
                    offset += name.len() as u32 + 1;
                }

                for name in &vars {
                    manifest
                        .variables
                        .push(decl_sym(name.clone(), range(offset, name.len() as u32)));
                    offset += name.len() as u32 + 1;
                }

                for (list_name, items) in &lists {
                    manifest.lists.push(decl_sym(
                        list_name.clone(),
                        range(offset, list_name.len() as u32),
                    ));
                    offset += list_name.len() as u32 + 1;

                    for item in items {
                        let qualified = format!("{list_name}.{item}");
                        manifest
                            .list_items
                            .push(decl_sym(qualified, range(offset, item.len() as u32)));
                        offset += item.len() as u32 + 1;
                    }
                }

                for name in &externals {
                    manifest
                        .externals
                        .push(decl_sym(name.clone(), range(offset, name.len() as u32)));
                    offset += name.len() as u32 + 1;
                }

                for name in &structs {
                    manifest
                        .structs
                        .push(decl_sym(name.clone(), range(offset, name.len() as u32)));
                    offset += name.len() as u32 + 1;
                }

                // Each unresolved ref gets a unique offset so ranges don't collide
                let mut ref_offset = 10_000u32;
                for (path, kind, scope) in &refs {
                    manifest.unresolved.push(UnresolvedRef {
                        path: path.clone(),
                        range: range(ref_offset, path.len() as u32),
                        kind: *kind,
                        scope: scope.clone(),
                        arg_count: None,
                        module_qualified: false,
                    });
                    ref_offset += path.len() as u32 + 100;
                }

                manifest
            })
        })
}

/// Strategy for two manifests (simulating cross-file analysis).
/// Offsets the second manifest's ranges to avoid collisions.
fn arb_two_file_manifests() -> impl Strategy<Value = Vec<(FileId, SymbolManifest)>> {
    (arb_manifest(), arb_manifest()).prop_map(|(m1, mut m2)| {
        // Shift all ranges in m2 to avoid collisions with m1
        let shift = TextSize::new(50_000);
        for sym in m2
            .knots
            .iter_mut()
            .chain(m2.stitches.iter_mut())
            .chain(m2.variables.iter_mut())
            .chain(m2.lists.iter_mut())
            .chain(m2.externals.iter_mut())
            .chain(m2.structs.iter_mut())
            .chain(m2.labels.iter_mut())
            .chain(m2.list_items.iter_mut())
        {
            sym.range = TextRange::new(sym.range.start() + shift, sym.range.end() + shift);
        }
        for uref in &mut m2.unresolved {
            uref.range = TextRange::new(uref.range.start() + shift, uref.range.end() + shift);
        }
        vec![(FileId(0), m1), (FileId(1), m2)]
    })
}

// ─── Empty HirFile for analyze() ────────────────────────────────────

fn empty_hir() -> HirFile {
    HirFile {
        root_content: brink_ir::Block::default(),
        knots: Vec::new(),
        variables: Vec::new(),
        constants: Vec::new(),
        lists: Vec::new(),
        structs: Vec::new(),
        externals: Vec::new(),
        includes: Vec::new(),
        module: None,
        imports: Vec::new(),
        visibility: Vec::new(),
        was_directives: Vec::new(),
        allow_scopes: Vec::new(),
        element_matches: Vec::new(),
        cue_names: Vec::new(),
        native: false,
        claim_handlers: Vec::new(),
    }
}

/// Issue #2856: names the second category `completeness` below excludes,
/// alongside `RefKind::Type` — an unresolved ref whose `path` names one of
/// the analyzer's compiler-reserved, resolution-optional identifiers
/// (`resolve::is_builtin_function`'s classic uppercase ink intrinsics —
/// `TURNS_SINCE`, `RANDOM`, … — or `resolve::is_t1b_stdlib_name`'s
/// lowercase T1b stdlib names — `len`, `push`, … — re-exported for this
/// purpose via `brink_analyzer::test_support`, not hand-duplicated here:
/// see `is_builtin_function`'s own doc for why a third hand-copy was
/// rejected). `resolve_variable`/`resolve_function` may skip resolving such
/// a reference with NO diagnostic — but, as of issue #2856 point 3's fix,
/// ONLY once every real lookup (locals, globals, list items, knots,
/// externals, …) has already failed to find a matching **declared**
/// symbol; a declared symbol of the same name always wins first and is
/// fully counted by `completeness` as resolved, never silently skipped
/// (`integration_var_shadows_uppercase_builtin_name` below pins this at the
/// analyzer layer; `crates/brink-compiler/tests/
/// issue_2856_builtin_shadow.rs` pins the same guarantee end-to-end through
/// codegen + the VM).
///
/// **Why the exclusion is safe, not merely incidental:** the silent skip
/// this predicate targets fires only when NOTHING in the whole project
/// declares a symbol under that name — which is the intended "defer
/// resolution to the VM-native builtin at LIR lowering, no false E025"
/// behavior the 2026-03-06 "Built-in function recognition belongs in the
/// analyzer" decision-log ruling establishes, not a drop of real reference
/// data. Issue #2836 previously found the opposite failure mode — an
/// author-declared `pop` list item losing to the stdlib verb — WAS a real
/// bug; that shape is excluded from this predicate's reach precisely
/// because the lookup-before-fallback ordering (fixed for `pop`/
/// `is_t1b_stdlib_name` by #2830/#2836, and for `is_builtin_function` by
/// #2856 itself) makes the declared symbol win resolution instead of ever
/// reaching here.
///
/// **Why this predicate is presently unreachable, not presently
/// vacuous:** `arb_manifest`'s generator only ever produces a ref `path`
/// that either exactly matches a declared symbol's name (which resolves
/// normally, per the guarantee above — this predicate is never even
/// consulted for it) or is the fixed `"definitely_missing"` sentinel
/// (which is not a reserved name, so this predicate returns `false` for
/// it too). No generated ref can currently be both "genuinely undeclared"
/// and "a reserved name" at once — `arb_ident`'s `[a-z][a-z_]{0,11}`
/// charset additionally makes `is_builtin_function`'s all-uppercase set
/// unreachable by construction, independent of the sentinel argument. If
/// the generator is ever extended to emit such a ref (a `path` that names
/// a reserved identifier while nothing in the manifest declares a matching
/// symbol), THIS predicate — not an accidental generator gap — is what
/// keeps `completeness` correctly excluding it, rather than reopening a
/// #2830-shaped intermittent flake on an unrelated PR.
fn is_generator_unreachable_reserved_name(path: &str) -> bool {
    // NS-A1 (`docs/stdlib-spec.md` §1.4): the bare `none` Option-absence
    // literal gets the identical silent-skip treatment in `resolve_variable`
    // (see that function's own doc) but lives outside `is_t1b_stdlib_name`
    // — it is a value-position literal, not a call name.
    path == "none" || is_builtin_function(path) || is_t1b_stdlib_name(path)
}

// ─── Property tests ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Every unresolved ref either resolves to a valid ID or produces exactly
    /// one diagnostic. No ref is silently dropped.
    ///
    /// **Two categories are excluded from this count — both filtered out
    /// deliberately below, not merely absent from what the generator
    /// happens to emit:**
    ///
    /// 1. **`RefKind::Type` (issue #2249).** Every other `RefKind` names
    ///    something that must be declared to be valid — an unresolved one
    ///    is unconditionally an error. A TM-2 type annotation is not:
    ///    `int`, `float`, `List`, … are equally legal `Named` leaves that
    ///    were never meant to resolve as a struct at all
    ///    (`resolve::resolve_type_ref`'s own doc), so "no declared struct
    ///    named this" is the overwhelmingly common, entirely legal outcome
    ///    — diagnosing it would misfire on every scalar-typed annotation in
    ///    every corpus file. This property's "resolved xor diagnosed,
    ///    never neither" dichotomy genuinely does not hold for this one
    ///    kind; a third, legal "resolved to nothing, nothing wrong" state
    ///    is included in coverage (via `arb_ref_kind`/`resolved_ids_are_valid`)
    ///    but excluded from this specific count.
    /// 2. **A reserved compiler name with nothing declared under it
    ///    (issue #2856), per [`is_generator_unreachable_reserved_name`] —
    ///    see that predicate's own doc for the full "why excluded" and "why
    ///    presently unreachable, not vacuous" argument.** In short:
    ///    `resolve_variable`/`resolve_function` may skip such a reference
    ///    with no diagnostic, but only once a real declared symbol of the
    ///    same name has already failed to be found — and a declared
    ///    symbol always wins resolution first (issue #2856 point 3 made
    ///    this true for `is_builtin_function`'s classic uppercase set too,
    ///    matching `is_t1b_stdlib_name`'s pre-existing correct ordering),
    ///    so this predicate's exclusion is currently a no-op against what
    ///    `arb_manifest` can generate, not a live carve-out papering over a
    ///    real gap.
    #[test]
    fn completeness(manifest in arb_manifest()) {
        let manifest = SymbolManifest {
            unresolved: manifest
                .unresolved
                .into_iter()
                .filter(|r| {
                    r.kind != RefKind::Type && !is_generator_unreachable_reserved_name(&r.path)
                })
                .collect(),
            ..manifest
        };
        let total_refs = manifest.unresolved.len();
        let ref_ranges: Vec<_> = manifest.unresolved.iter().map(|r| r.range).collect();

        let hir = empty_hir();
        let files = vec![(FileId(0), &hir, &manifest)];
        let result = analyze(&files);

        let resolved_ranges: std::collections::HashSet<_> = result
            .resolutions
            .iter()
            .map(|r| r.range)
            .collect();
        let resolved_count = ref_ranges
            .iter()
            .filter(|r| resolved_ranges.contains(r))
            .count();

        // Diagnostics that are resolution errors — one per `RefKind` variant
        // (E024 divert, E025 variable/function, E027 list, E068 struct).
        // Function refs also land on E025 (`resolve_function`'s own
        // `unresolved_diag` call site) — no separate code of its own.
        let unresolved_diag_ranges: HashSet<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code == DiagnosticCode::E024
                    || d.code == DiagnosticCode::E025
                    || d.code == DiagnosticCode::E027
                    || d.code == DiagnosticCode::E068
            })
            .map(|d| d.range)
            .collect();

        let diagnosed_count = ref_ranges
            .iter()
            .filter(|r| unresolved_diag_ranges.contains(r))
            .count();

        // Every ref is either resolved or diagnosed (not both, not neither)
        prop_assert_eq!(
            resolved_count + diagnosed_count,
            total_refs,
            "resolved={}, diagnosed={}, total={}",
            resolved_count, diagnosed_count, total_refs,
        );
    }

    /// Every resolved `DefinitionId` exists in the symbol index.
    #[test]
    fn resolved_ids_are_valid(manifest in arb_manifest()) {
        let hir = empty_hir();
        let files = vec![(FileId(0), &hir, &manifest)];
        let result = analyze(&files);

        for resolved in &result.resolutions {
            prop_assert!(
                result.index.symbols.contains_key(&resolved.target),
                "resolved to {:?} which is not in the index",
                resolved.target,
            );
        }
    }

    /// The `by_name` reverse index is consistent with `symbols`:
    /// every ID in `by_name` exists in `symbols`, and every symbol in
    /// `symbols` appears in `by_name` under its name.
    #[test]
    fn by_name_consistent_with_symbols(manifest in arb_manifest()) {
        let hir = empty_hir();
        let files = vec![(FileId(0), &hir, &manifest)];
        let result = analyze(&files);

        // Forward: every ID in by_name exists in symbols
        for (name, ids) in &result.index.by_name {
            for id in ids {
                prop_assert!(
                    result.index.symbols.contains_key(id),
                    "by_name[{name}] contains {:?} which is not in symbols",
                    id,
                );
            }
        }

        // Reverse: every symbol is in by_name
        for (id, info) in &result.index.symbols {
            let ids = result.index.by_name.get(&info.name);
            prop_assert!(
                ids.is_some_and(|ids| ids.contains(id)),
                "symbol {:?} ({}) not found in by_name",
                id,
                info.name,
            );
        }
    }

    /// Resolution is deterministic within a process: running analyze twice
    /// on the same input produces the same resolution map.
    ///
    /// Note: when multiple list items share a bare name (e.g., `A.x` and
    /// `B.x`), the winner depends on `HashMap` iteration order which is
    /// randomized per-process. This test validates within-process consistency
    /// by running both calls in the same invocation.
    #[test]
    fn resolution_is_deterministic(manifest in arb_manifest()) {
        let m1 = manifest.clone();
        let m2 = manifest;
        let hir1 = empty_hir();
        let hir2 = empty_hir();

        let files1 = vec![(FileId(0), &hir1, &m1)];
        let result1 = analyze(&files1);

        let files2 = vec![(FileId(0), &hir2, &m2)];
        let result2 = analyze(&files2);

        prop_assert_eq!(
            result1.resolutions.len(),
            result2.resolutions.len(),
            "different number of resolutions",
        );

        for r1 in &result1.resolutions {
            let found = result2.resolutions.iter().find(|r2| r2.range == r1.range && r2.file == r1.file);
            prop_assert!(
                found.is_some_and(|r2| r2.target == r1.target),
                "resolution differs for range {:?} in file {:?}",
                r1.range, r1.file,
            );
        }
    }

    /// When two files declare a knot with the same name, a duplicate
    /// warning (E022) is emitted. First-wins semantics preserved.
    #[test]
    fn duplicate_knots_across_files(name in arb_ident()) {
        let m1 = SymbolManifest {
            knots: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(0, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                        visibility: None,
                        was: None,
                    }],
            ..Default::default()
        };
        let m2 = SymbolManifest {
            knots: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(100, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                        visibility: None,
                        was: None,
                    }],
            ..Default::default()
        };
        let hir1 = empty_hir();
        let hir2 = empty_hir();

        let files = vec![
            (FileId(0), &hir1, &m1),
            (FileId(1), &hir2, &m2),
        ];
        let result = analyze(&files);

        let dup_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E022)
            .collect();

        prop_assert_eq!(
            dup_diags.len(),
            1,
            "expected exactly one duplicate warning for knot `{}`, got {}",
            name, dup_diags.len(),
        );
    }

    /// When two files declare a global variable with the same name, a
    /// duplicate warning (E023) is emitted.
    #[test]
    fn duplicate_variables_across_files(name in arb_ident()) {
        let m1 = SymbolManifest {
            variables: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(0, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                        visibility: None,
                        was: None,
                    }],
            ..Default::default()
        };
        let m2 = SymbolManifest {
            variables: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(100, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                        visibility: None,
                        was: None,
                    }],
            ..Default::default()
        };
        let hir1 = empty_hir();
        let hir2 = empty_hir();

        let files = vec![
            (FileId(0), &hir1, &m1),
            (FileId(1), &hir2, &m2),
        ];
        let result = analyze(&files);

        let dup_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E023)
            .collect();

        prop_assert_eq!(
            dup_diags.len(),
            1,
            "expected exactly one duplicate warning for variable `{}`, got {}",
            name, dup_diags.len(),
        );
    }

    /// A ref that is NOT among the declared symbols always produces an
    /// unresolved diagnostic. `kind` is drawn from
    /// [`arb_diagnosing_ref_kind`], not [`arb_ref_kind`] — see that
    /// function's doc for why `RefKind::Type` is excluded.
    #[test]
    fn missing_ref_always_diagnosed(
        knots in prop::collection::vec(arb_ident(), 1..=3),
        suffix in arb_ident(),
        kind in arb_diagnosing_ref_kind(),
    ) {
        // Prefix guarantees it won't collide with any generated names
        let missing = format!("zzz_{suffix}");
        let mut manifest = SymbolManifest::default();
        let mut offset = 0u32;
        for name in &knots {
            manifest.knots.push(DeclaredSymbol {
                name: name.clone(),
                range: range(offset, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                        visibility: None,
                        was: None,
                    });
            offset += name.len() as u32 + 1;
        }
        manifest.unresolved.push(UnresolvedRef {
            path: missing.clone(),
            range: range(5000, missing.len() as u32),
            kind,
            scope: Scope::default(),
            arg_count: None,
            module_qualified: false,
        });

        let hir = empty_hir();
        let files = vec![(FileId(0), &hir, &manifest)];
        let result = analyze(&files);

        let target_range = range(5000, missing.len() as u32);
        let has_resolution = result.resolutions.iter().any(|r| r.range == target_range);
        prop_assert!(
            !has_resolution,
            "expected `{}` to NOT resolve, but it did", missing,
        );

        let has_diag = result.diagnostics.iter().any(|d| {
            (d.code == DiagnosticCode::E024
                || d.code == DiagnosticCode::E025
                || d.code == DiagnosticCode::E027
                || d.code == DiagnosticCode::E068)
                && d.range == range(5000, missing.len() as u32)
        });
        prop_assert!(
            has_diag,
            "expected unresolved diagnostic for `{}`", missing,
        );
    }

    /// No duplicate diagnostics are emitted for the same range+code.
    #[test]
    fn no_duplicate_diagnostics(manifests in arb_two_file_manifests()) {
        let hirs: Vec<_> = manifests.iter().map(|_| empty_hir()).collect();
        let inputs: Vec<_> = manifests
            .iter()
            .zip(hirs.iter())
            .map(|((id, m), h)| (*id, h, m))
            .collect();
        let result = analyze(&inputs);

        let mut seen = HashSet::new();
        for d in &result.diagnostics {
            let key = (d.range, d.code);
            prop_assert!(
                seen.insert(key),
                "duplicate diagnostic: {:?} at {:?}",
                d.code,
                d.range,
            );
        }
    }
}

// ─── Integration tests: full pipeline (parse → lower → analyze) ────

/// Parse ink source, lower it, and run analysis. Returns the analysis result.
fn analyze_ink(source: &str) -> brink_analyzer::AnalysisResult {
    let parsed = brink_syntax::parse(source);
    let (hir, manifest, _lowering_diags) = brink_ir::lower(FileId(0), &parsed.tree());
    let files = vec![(FileId(0), &hir, &manifest)];
    analyze(&files)
}

/// Two-file analysis from ink sources.
fn analyze_ink_multi(sources: &[&str]) -> brink_analyzer::AnalysisResult {
    let files: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(i, source)| {
            let parsed = brink_syntax::parse(source);
            let file_id = FileId(i as u32);
            let (hir, manifest, _) = brink_ir::lower(file_id, &parsed.tree());
            (file_id, hir, manifest)
        })
        .collect();
    let refs: Vec<_> = files.iter().map(|(id, h, m)| (*id, h, m)).collect();
    analyze(&refs)
}

#[test]
fn integration_knot_divert_resolves() {
    let result = analyze_ink(
        "\
Hello!
-> greet

== greet ==
Welcome.
-> END
",
    );
    // The divert `-> greet` should resolve; no E024 diagnostics
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
    assert!(!result.resolutions.is_empty());
}

#[test]
fn integration_qualified_stitch_divert() {
    let result = analyze_ink(
        "\
-> kitchen.look

== kitchen ==
= look
Looking around the kitchen.
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_stitch_local_resolution() {
    let result = analyze_ink(
        "\
== kitchen ==
-> look
= look
Kitchen look.
-> END

== bedroom ==
-> look
= look
Bedroom look.
-> END
",
    );
    // Both `-> look` should resolve to their respective local stitches
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_variable_reference() {
    let result = analyze_ink(
        "\
VAR player_name = \"Alice\"
Hello, {player_name}!
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_list_bare_item_reference() {
    let result = analyze_ink(
        "\
LIST Colors = red, green, blue
~ temp x = red
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

/// Issue #2830: a bare list-item name that collides with a stdlib list verb
/// (`pop`), referenced from inside a knot whose name also collides with the
/// declaring list's name (`a`), must still resolve to the list item rather
/// than being silently swallowed by the `is_t1b_stdlib_name` fallback in
/// `resolve_function` — the exact shape the `completeness` proptest's pinned
/// regression seed (`proptest_resolve.proptest-regressions`) shrunk down to.
/// `#fn(target)` is a real-syntax `RefKind::Function` site with
/// `arg_count: None` (`brink_ir::symbols::project::Projector::walk_expr`'s
/// `Expr::FnLiteral` arm), matching the counterexample's shape exactly.
///
/// `#fn(…)` is a brink extension (docs/t1b-surface-spec.md §1), so this
/// analyzes with `Dialect::Brink` rather than `analyze_ink`'s default
/// `StrictInk` — under `StrictInk` the fixture would also carry a
/// `dialect_gate` `E051` ("brink extension") at the `#fn` site, and this
/// test's `E025`-only diagnostics filter would silently hide that unrelated
/// diagnostic rather than prove the fixture is clean. `brink_syntax::parse`
/// (the ink-compat parser) still does the parsing — it accepts the full
/// superset grammar unconditionally and only the dialect gate is
/// StrictInk-vs-Brink sensitive — so the source stays ink surface by
/// design, matching every other fixture in this file.
#[test]
fn integration_bare_list_item_collides_with_stdlib_verb_name() {
    let parsed = brink_syntax::parse(
        "\
LIST a = pop, other

== a ==
~ temp f = #fn(pop)
-> END
",
    );
    let (hir, manifest, _lowering_diags) = brink_ir::lower(FileId(0), &parsed.tree());
    let files = vec![(FileId(0), &hir, &manifest)];
    let opts = brink_analyzer::AnalysisOptions {
        dialect: brink_analyzer::Dialect::Brink,
        // Pin `Gradual` explicitly rather than taking `Dialect::Brink`'s
        // own default (`Strict`, per `resolve_type_policy`) — this
        // fixture's `~ temp f = #fn(pop)` never reads `f`, and TM-3 strict
        // inference's Unknown-escape check (`E065`) would fire on that,
        // which is noise unrelated to what this test is about.
        types: Some(brink_analyzer::TypePolicy::Gradual),
        ..brink_analyzer::AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_options(&files, &opts);

    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
    // The completeness guarantee this issue is about: no diagnostic here
    // would mean the ref could have been silently dropped instead of
    // resolved — a *silent drop* also produces no `E025`, so the assertion
    // above alone would pass even while dropping the ref. Assert the
    // reference was actually resolved, and to the specific `a.pop` list
    // item — not just "resolved to something".
    assert_eq!(
        result.resolutions.len(),
        1,
        "expected the `#fn(pop)` target to resolve to the `a.pop` list item; \
         resolutions: {:?}",
        result.resolutions
    );
    let target = result.resolutions[0].target;
    assert!(
        result.index.symbols.contains_key(&target),
        "resolution target {target:?} missing from symbol index"
    );
    let info = result
        .index
        .symbols
        .get(&target)
        .expect("just asserted above");
    assert_eq!(
        info.kind,
        SymbolKind::ListItem,
        "expected `#fn(pop)` to resolve to a ListItem, got {:?} ({:?})",
        info.kind,
        info.name,
    );
    assert_eq!(
        info.name, "a.pop",
        "expected `#fn(pop)` to resolve to the `a.pop` list item specifically",
    );
    // PR #2836 review finding 3 (SPEC DRIFT): resolving to a ListItem here
    // does not create a new silent-shadowing rule — `fn_values::check`'s
    // `E079` ("target does not resolve to a statically-named function
    // definition") still refuses a `#fn` target that resolves to anything
    // other than a function (docs/t1c-spec.md §2), so the list item wins
    // *resolution* but is still diagnosed as an invalid `#fn` target. This
    // is the resolved-and-diagnosed outcome the `completeness` invariant
    // wants, not a bare "no diagnostic" pass.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E079),
        "expected `#fn(pop)` (resolved to a ListItem) to be refused as an \
         invalid function-value target by E079; diagnostics: {:?}",
        result.diagnostics
    );
}

/// Issue #2856 point 3: a `VAR` declared with the same name as a classic
/// uppercase ink built-in (`is_builtin_function`) must shadow the builtin at
/// every *reference* site, exactly as the `E035` diagnostic fired at its
/// declaration (`manifest.rs`) already promises — "an author-defined
/// function with the same name shadows the builtin, with a warning
/// diagnostic," worded identically for `is_builtin_function` and
/// `is_t1b_stdlib_name`. Before this fix, `resolve_variable`/
/// `resolve_function` checked `is_builtin_function` *before* any lookup
/// (unlike `is_t1b_stdlib_name`, checked only as a post-lookup fallback), so
/// the declared `VAR RANDOM` was never consulted at the reference site: the
/// ref was silently skipped (no resolution, no diagnostic — a real
/// `completeness`-violating silent drop, reproduced end-to-end via
/// `brink-cli compile` + `play`: `{RANDOM}` rendered as empty text instead
/// of `42`, with a clean exit 0 and no diagnostic at default log level).
#[test]
fn integration_var_shadows_uppercase_builtin_name() {
    let result = analyze_ink(
        "\
VAR RANDOM = 42

The value is {RANDOM}.
-> DONE
",
    );

    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");

    // Two `RANDOM` refs exist in this source: the `VAR RANDOM = 42`
    // initializer's own name isn't a reference, but the interpolation site
    // is — assert it actually resolved to the declared `VAR`, not merely
    // "produced no E025" (a silent drop also produces no E025).
    let random_resolutions: Vec<_> = result
        .resolutions
        .iter()
        .filter(|r| {
            result
                .index
                .symbols
                .get(&r.target)
                .is_some_and(|info| info.name == "RANDOM")
        })
        .collect();
    assert_eq!(
        random_resolutions.len(),
        1,
        "expected the `{{RANDOM}}` interpolation to resolve to the declared \
         `VAR RANDOM`; resolutions: {:?}",
        result.resolutions
    );
    let target = random_resolutions[0].target;
    let info = result
        .index
        .symbols
        .get(&target)
        .expect("just asserted above");
    assert_eq!(
        info.kind,
        SymbolKind::Variable,
        "expected `{{RANDOM}}` to resolve to the declared `VAR`, got {:?}",
        info.kind,
    );

    // The declaration-site `E035` warning ("name shadows a built-in
    // function") still fires — shadowing is legal but discouraged, not
    // silent.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E035),
        "expected E035 at the `VAR RANDOM` declaration; diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn integration_function_call_to_external() {
    let result = analyze_ink(
        "\
EXTERNAL print_debug(x)
~ print_debug(42)
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_function_call_to_knot() {
    let result = analyze_ink(
        "\
~ greet()

== function greet ==
Hello!
~ return
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_unresolved_divert_diagnostic() {
    let result = analyze_ink("-> nonexistent_knot\n");
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert_eq!(unresolved.len(), 1);
}

#[test]
fn integration_unresolved_variable_diagnostic() {
    let result = analyze_ink("The answer is {unknown_var}.\n");
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert_eq!(unresolved.len(), 1);
}

#[test]
fn integration_end_done_no_unresolved() {
    let result = analyze_ink(
        "\
-> END

== other ==
-> DONE
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024 || d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_duplicate_knot_across_files() {
    let result = analyze_ink_multi(&[
        "== shared_knot ==\nFirst.\n-> END\n",
        "== shared_knot ==\nSecond.\n-> END\n",
    ]);
    let dups: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E022)
        .collect();
    // Duplicates emit a warning (not error) — inklecate permits redefinition.
    assert_eq!(dups.len(), 1);
}

#[test]
fn integration_label_divert_resolves() {
    let result = analyze_ink(
        "\
== meeting ==
Hello.
* (greet) Hi! -> greet
- (farewell) Bye.
-> END
",
    );
    // `-> greet` should resolve to the choice label
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(unresolved.is_empty(), "unresolved diverts: {unresolved:?}");
}

#[test]
fn integration_visit_count_as_variable() {
    // In ink, knot names can be used as variables (visit counts)
    let result = analyze_ink(
        "\
== greet ==
{greet > 1: You've been here before.}
Hello!
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_cross_file_divert() {
    let result = analyze_ink_multi(&[
        "-> helper_knot\n",
        "== helper_knot ==\nHello from helper.\n-> END\n",
    ]);
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_cross_file_variable() {
    let result = analyze_ink_multi(&["VAR score = 0\n", "The score is {score}.\n"]);
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_list_in_expression_context() {
    let result = analyze_ink(
        "\
LIST Mood = happy, sad, angry
VAR current_mood = happy
{current_mood == sad: You look sad.}
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_scope_tracks_through_stitches() {
    // Within a stitch, bare names should resolve to sibling stitches first
    let result = analyze_ink(
        "\
== chapter ==
= intro
Welcome.
-> middle

= middle
Middle part.
-> ending

= ending
The end.
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_multiple_lists_bare_item_unique() {
    let result = analyze_ink(
        "\
LIST Fruit = apple, banana
LIST Color = red, green
~ temp x = apple
~ temp y = red
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

#[test]
fn integration_ambiguous_bare_list_item() {
    let result = analyze_ink(
        "\
LIST Fruit = red, green
LIST Color = red, blue
~ temp x = red
",
    );
    let ambiguous: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E027)
        .collect();
    assert_eq!(
        ambiguous.len(),
        1,
        "expected ambiguity diagnostic for `red`"
    );
}

#[test]
fn integration_ambiguous_resolved_by_qualification() {
    let result = analyze_ink(
        "\
LIST Fruit = red, green
LIST Color = red, blue
~ temp x = Color.red
",
    );
    let ambiguous: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E027)
        .collect();
    assert!(
        ambiguous.is_empty(),
        "qualified reference should not be ambiguous",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
}

// ── Tests for corpus fix patterns ───────────────────────────────────

#[test]
fn integration_turns_builtin() {
    let result = analyze_ink(
        "\
=== function came_from(-> x) ===
~ return TURNS_SINCE(x) == 0

=== test ===
- (begin)
~ temp t = TURNS()
{t > 0: hello}
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(
        unresolved.is_empty(),
        "TURNS() should be a builtin: {unresolved:?}",
    );
}

#[test]
fn integration_duplicate_knot_no_error() {
    let result = analyze_ink_multi(&[
        "VAR x = 0\n== shared ==\n~ x = 1\n-> END\n",
        "== shared ==\n{x} -> END\n",
    ]);
    // Duplicates emit warnings but should not prevent compilation.
    // Expect 1 E022 (duplicate knot), no errors.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code.severity() == brink_ir::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "duplicate should be a warning, not an error: {errors:?}",
    );
    let dups: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E022)
        .collect();
    assert_eq!(dups.len(), 1);
}

#[test]
fn integration_qualified_label_visit_count() {
    // `adventure.encounter` where encounter is a label inside a stitch
    let result = analyze_ink(
        "\
=== adventure ===
= prints
* (encounter) Option A
  Hello
- -> END

=== other ===
{adventure.encounter: Already met!}
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(
        unresolved.is_empty(),
        "adventure.encounter should resolve as knot.stitch.label visit count: {unresolved:?}",
    );
}

#[test]
fn integration_choice_label_in_branchless_conditional() {
    let result = analyze_ink(
        "\
=== play_game ===
{ true:
  + (burny) [Burn]
    Hello
}
- -> burny
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(
        unresolved.is_empty(),
        "choice label inside branchless conditional should be declared: {unresolved:?}",
    );
}

#[test]
fn integration_cross_scope_label_divert() {
    // `-> begin` from inside a knot, where `begin` is a top-level gather label
    let result = analyze_ink(
        "\
- (begin)
-> example

=== example ===
~ temp t = TURNS_SINCE(-> begin)
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(
        unresolved.is_empty(),
        "top-level label should be visible from inside a knot: {unresolved:?}",
    );
}

#[test]
fn integration_temp_as_function_name() {
    let result = analyze_ink(
        "\
=== test ===
~ temp myFunc = -> helper
~ myFunc()
-> END

=== helper ===
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E025)
        .collect();
    assert!(
        unresolved.is_empty(),
        "temp used as function name should resolve: {unresolved:?}",
    );
}

#[test]
fn integration_qualified_stitch_divert_from_knot_scope() {
    // `-> a_package.forest` inside `adventure` knot should resolve
    // as `adventure.a_package.forest` (stitch.label)
    let result = analyze_ink(
        "\
=== adventure ===
= a_package
* (forest) Go to forest
  Trees!
- -> END

=== other ===
-> adventure.a_package.forest
-> END
",
    );
    let unresolved: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert!(
        unresolved.is_empty(),
        "qualified stitch.label divert should resolve: {unresolved:?}",
    );
}
