#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::needless_pass_by_value
)]

//! Property-based tests for name resolution.
//!
//! These tests generate arbitrary symbol manifests and verify structural
//! invariants of the resolution algorithm — things that must hold regardless
//! of what names or scopes are involved.

use std::collections::HashSet;

use brink_ir::{
    DeclaredSymbol, DiagnosticCode, FileId, RefKind, Scope, SymbolManifest, UnresolvedRef,
};
use proptest::prelude::*;
use rowan::{TextRange, TextSize};

use brink_analyzer::analyze;
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
    ]
}

/// Strategy that generates a manifest with 1-5 knots, 0-3 variables,
/// 0-2 lists with items, and 0-8 unresolved refs that may or may not match.
fn arb_manifest() -> impl Strategy<Value = SymbolManifest> {
    (
        prop::collection::vec(arb_ident(), 1..=5), // knot names
        prop::collection::vec(arb_ident(), 0..=3), // variable names
        prop::collection::vec(
            (arb_ident(), prop::collection::vec(arb_ident(), 1..=4)), // lists with items
            0..=2,
        ),
        prop::collection::vec(arb_ident(), 0..=2), // externals
    )
        .prop_flat_map(|(knots, vars, lists, externals)| {
            let knot_names = knots.clone();
            let all_names: Vec<String> = knots
                .iter()
                .chain(vars.iter())
                .chain(externals.iter())
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
                    manifest.knots.push(DeclaredSymbol {
                        name: name.clone(),
                        range: range(offset, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                    });
                    offset += name.len() as u32 + 1;
                }

                for name in &vars {
                    manifest.variables.push(DeclaredSymbol {
                        name: name.clone(),
                        range: range(offset, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                    });
                    offset += name.len() as u32 + 1;
                }

                for (list_name, items) in &lists {
                    manifest.lists.push(DeclaredSymbol {
                        name: list_name.clone(),
                        range: range(offset, list_name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                    });
                    offset += list_name.len() as u32 + 1;

                    for item in items {
                        let qualified = format!("{list_name}.{item}");
                        manifest.list_items.push(DeclaredSymbol {
                            name: qualified,
                            range: range(offset, item.len() as u32),
                            params: Vec::new(),
                            detail: None,
                        });
                        offset += item.len() as u32 + 1;
                    }
                }

                for name in &externals {
                    manifest.externals.push(DeclaredSymbol {
                        name: name.clone(),
                        range: range(offset, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                    });
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
        externals: Vec::new(),
        includes: Vec::new(),
    }
}

// ─── Property tests ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Every unresolved ref either resolves to a valid ID or produces exactly
    /// one diagnostic. No ref is silently dropped.
    #[test]
    fn completeness(manifest in arb_manifest()) {
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

        // Diagnostics that are resolution errors (E024, E025, or E027)
        let unresolved_diag_ranges: HashSet<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code == DiagnosticCode::E024
                    || d.code == DiagnosticCode::E025
                    || d.code == DiagnosticCode::E027
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

    /// When two files declare a knot with the same name, duplicates are
    /// silently accepted (inklecate permits redefinition).
    #[test]
    fn duplicate_knots_across_files(name in arb_ident()) {
        let m1 = SymbolManifest {
            knots: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(0, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                    }],
            ..Default::default()
        };
        let m2 = SymbolManifest {
            knots: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(100, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
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
            0,
            "expected no duplicate diagnostics for knot `{}`, got {}",
            name, dup_diags.len(),
        );
    }

    /// When two files declare a global variable with the same name, duplicates
    /// are silently accepted (inklecate permits redefinition).
    #[test]
    fn duplicate_variables_across_files(name in arb_ident()) {
        let m1 = SymbolManifest {
            variables: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(0, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
                    }],
            ..Default::default()
        };
        let m2 = SymbolManifest {
            variables: vec![DeclaredSymbol {
                name: name.clone(),
                range: range(100, name.len() as u32),
                        params: Vec::new(),
                        detail: None,
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
            0,
            "expected no duplicate diagnostics for variable `{}`, got {}",
            name, dup_diags.len(),
        );
    }

    /// A ref that is NOT among the declared symbols always produces an
    /// unresolved diagnostic.
    #[test]
    fn missing_ref_always_diagnosed(
        knots in prop::collection::vec(arb_ident(), 1..=3),
        suffix in arb_ident(),
        kind in arb_ref_kind(),
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
                    });
            offset += name.len() as u32 + 1;
        }
        manifest.unresolved.push(UnresolvedRef {
            path: missing.clone(),
            range: range(5000, missing.len() as u32),
            kind,
            scope: Scope::default(),
            arg_count: None,
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
                || d.code == DiagnosticCode::E027)
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
    // Duplicates are silently accepted (inklecate permits redefinition)
    assert!(dups.is_empty());
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
    assert!(unresolved.is_empty(), "unresolved diverts: {unresolved:?}",);
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
    // Duplicates should not prevent compilation
    assert!(
        result.diagnostics.is_empty(),
        "duplicates should be silently accepted: {:?}",
        result.diagnostics,
    );
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
