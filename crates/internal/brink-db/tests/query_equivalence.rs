//! Equivalence gates for the salsa query pipeline (phase 0 slice B, locals
//! split in #517).
//!
//! The query-composed pipeline must be *output-identical* to the monolithic
//! analyzer path — including the decls-only, range-stripped
//! `resolution_index` cutoff seam, which is only legal because resolution
//! never reads locals or non-local declaration ranges from the merged index
//! (locals resolve from the declaring file's own `manifest.locals` instead —
//! both the monolithic and query-composed paths go through the same
//! `brink_analyzer::resolve_file`, so they cannot diverge from each other,
//! even though the *values* resolution now returns changed for the
//! finding-4 cross-file duplicate-scoped-locals case, see below). These
//! tests pin that equivalence on fixtures chosen to poke the risky corners:
//! locals (params/temps), duplicate names across files, cross-file duplicate
//! scoped locals, and unresolved references.
//!
//! FG-6 (#841) audited this family against the decision-log ruling to
//! retire "the composed-equals-monolithic equivalence family in favor of
//! cross-version byte-identity (`inkb_hashes`)": that ruling targets
//! `brink-compiler`'s now-removed *production* one-shot-driver-vs-`ProjectDb`
//! duplication (collapsed in #844/#841 — there was never a committed test
//! comparing those two, only ad hoc `inkb_hashes` verification during the
//! switch). The tests below compare something different and still real: a
//! salsa **query composition** (`db.analysis()`/`db.signature()`) against a
//! **direct, non-decomposed `brink_analyzer` call** on the same inputs, to
//! prove FG-1/FG-3's per-file/per-def query decomposition doesn't change
//! analyzer output. That seam has nothing to do with which pipeline
//! `brink-compiler` routes through and remains load-bearing after FG-6 —
//! kept, not retired. See `fg2_scc_dependency_edges.rs` for FG-2's analogous
//! per-SCC seam.

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_ir::{
    BaseType, Constraint, DiagnosticCode, FileId, HirFile, HostManifest, ManifestExternal,
    ManifestParam, SemanticTypeDef, SymbolManifest, TypeRef,
};

fn db_with(files: &[(&str, &str)]) -> ProjectDb {
    let mut db = ProjectDb::new();
    for (path, src) in files {
        db.set_file(path, (*src).to_owned());
    }
    db
}

/// `SymbolIndex::symbols` is a `HashMap`, so compare id *sets*, never
/// iteration order (determinism rule).
fn ids(index: &brink_ir::SymbolIndex) -> std::collections::BTreeSet<brink_format::DefinitionId> {
    index.symbols.keys().copied().collect()
}

/// Run the monolithic analyzer (full-range index throughout) over the db's
/// analysis inputs — the exact pre-salsa `Driver::analyze` path.
fn monolithic_analysis(db: &ProjectDb) -> brink_analyzer::AnalysisResult {
    let inputs = db.analysis_inputs();
    let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    brink_analyzer::analyze(&refs)
}

fn assert_analysis_matches(files: &[(&str, &str)]) {
    let db = db_with(files);
    let query = db.analysis();
    let monolithic = monolithic_analysis(&db);
    assert_eq!(
        *query, monolithic,
        "query-composed analysis diverged from the monolithic analyzer"
    );
}

#[test]
fn analysis_matches_on_multi_file_project() {
    assert_analysis_matches(&[
        ("main.ink", "INCLUDE lib.ink\nVAR gold = 10\n-> town\n"),
        (
            "lib.ink",
            "=== town ===\nThe town square.\n* [Shop] -> shop\n* [Leave] -> END\n\n= shop\nYou browse. {gold} gold left.\n-> town\n",
        ),
    ]);
}

#[test]
fn analysis_matches_with_locals_and_shadowing() {
    // Params + temps, including a temp shadowing a param name in a sibling
    // stitch — exercises `lookup_local_in_scope`'s closest-preceding pick,
    // the one consumer of index-side ranges.
    assert_analysis_matches(&[(
        "main.ink",
        "=== greet(name) ===\n~ temp name2 = name\nHello {name2}.\n-> feast(3)\n\n\
         === feast(count) ===\n~ temp count2 = count\n~ temp count3 = count2 + 1\nServed {count3}.\n-> END\n",
    )]);
}

#[test]
fn analysis_matches_with_cross_file_duplicate_scoped_locals() {
    // Finding 4 (fixed by #517): duplicate knots across files with
    // same-named scoped locals share a `DefinitionId` in the *merged* index,
    // but resolution no longer reads locals from the merged index — each
    // file's own reference resolves against its own `manifest.locals`, so
    // `a.ink`'s `t` and `b.ink`'s `t` each resolve within their own file
    // regardless of the shared id. Both the monolithic and query-composed
    // paths go through the same `resolve_file`, so they cannot diverge from
    // each other even though the resolved values differ from pre-#517.
    assert_analysis_matches(&[
        (
            "a.ink",
            "=== dup(x) ===\n~ temp t = x\nA side: {t}.\n-> END\n",
        ),
        (
            "b.ink",
            "=== dup(x) ===\n~ temp t = x + 1\nB side: {t}.\n-> END\n",
        ),
    ]);
}

/// Composed == monolithic with the whole external-check family firing
/// (issue #750 / FG-3 completion): a registered host manifest with a
/// closed-domain semantic type, an arity mismatch (`E039`), an unknown
/// semantic type (`E040`), a call-site type mismatch (`E041`), a
/// closed-domain violation (`E042`), inline-doc'd knots (callable
/// enrichment), and doc'd VAR/CONST (value metas) — content, *order*, and
/// `symbol_meta` must all be identical between `db.analysis()` (now
/// assembled from `inline_docs`/`external_meta`/`call_site_metas` + per-file
/// value-meta/call-site queries) and the monolithic
/// `analyze_with_options` path.
#[test]
fn analysis_matches_with_host_manifest_and_external_checks() {
    let files: &[(&str, &str)] = &[
        (
            "main.ink",
            "INCLUDE lib.ink\n\
             /// The player's purse.\n\
             VAR gold = 10\n\
             /// Cap on everything.\n\
             CONST MAX = 99\n\
             /// @param who {actor_id}\n\
             EXTERNAL add_state(who)\n\
             EXTERNAL play_sound(sound)\n\
             EXTERNAL misdeclared(a, b)\n\
             === start ===\n\
             ~ play_sound(\"boom\")\n\
             ~ play_sound(\"quack\")\n\
             ~ play_sound(3)\n\
             -> helper(2)\n",
        ),
        (
            "lib.ink",
            "/// Helps out.\n\
             /// @param times {int}\n\
             === helper(times) ===\n\
             ~ play_sound(\"click\")\n\
             ~ misdeclared(1, 2)\n\
             -> END\n",
        ),
    ];
    let opts = AnalysisOptions {
        host_manifest: Some(HostManifest {
            markup: Vec::new(),
            externals: vec![
                ManifestExternal {
                    name: "play_sound".to_owned(),
                    params: vec![ManifestParam {
                        name: "sound".to_owned(),
                        ty: TypeRef("sound_id".to_owned()),
                    }],
                    returns: TypeRef::default(),
                    kind: brink_ir::ExternalKind::default(),
                    doc: Some("Play a sound effect.".to_owned()),
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
                ManifestExternal {
                    name: "misdeclared".to_owned(),
                    // One param in the manifest vs two in the ink decl: E039.
                    params: vec![ManifestParam {
                        name: "only".to_owned(),
                        ty: TypeRef("int".to_owned()),
                    }],
                    returns: TypeRef::default(),
                    kind: brink_ir::ExternalKind::default(),
                    doc: None,
                    widgets: Vec::new(),
                    path: Vec::new(),
                },
            ],
            // `actor_id` deliberately absent: the inline `@param who
            // {actor_id}` tag is an unknown semantic type once a manifest is
            // registered — E040.
            types: vec![SemanticTypeDef {
                name: "sound_id".to_owned(),
                base: BaseType::String,
                constraint: Some(Constraint::Enum {
                    values: vec!["click".to_owned(), "boom".to_owned()],
                }),
                values: None,
                widget: None,
            }],
        }),
        ..AnalysisOptions::default()
    };

    let mut db = db_with(files);
    db.set_analysis_options(opts.clone());
    let query = db.analysis();

    let inputs = db.analysis_inputs();
    let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let monolithic = brink_analyzer::analyze_with_options(&refs, &opts);

    // Non-vacuity: every code in the family actually fired.
    for code in [
        DiagnosticCode::E039,
        DiagnosticCode::E040,
        DiagnosticCode::E041,
        DiagnosticCode::E042,
    ] {
        assert!(
            query.diagnostics.iter().any(|d| d.code == code),
            "fixture must fire {code:?}: {:?}",
            query.diagnostics
        );
    }
    assert!(
        !query.symbol_meta.is_empty(),
        "fixture must produce symbol metas"
    );

    assert_eq!(
        *query, monolithic,
        "query-composed analysis (decomposed external-check family, issue \
         #750) diverged from the monolithic analyzer"
    );
}

/// Issue #1736: the two call-graph constructions this repo maintains —
/// the salsa-incremental path (`call_graph_query` → `call_edges_query`/
/// `direct_calls`, what `db.analysis()` and every per-def effects query
/// route through) and the monolithic `effects_project` path (which
/// explicitly folds `EffectAtoms::creates_fn_values` into its own local
/// call graph alongside `direct_calls`, docs/effects-spec.md §6.1a) — had
/// no test comparing their *outputs* on the same input. Today
/// `creates_fn_values` is a strict subset of `direct_calls` by
/// construction (`infer_fn_literal` already routes every `#fn` target
/// through `record_call_edge`), pinned by
/// `every_fn_value_creation_target_is_also_a_call_graph_edge` in
/// `brink-analyzer`'s `infer::mod` tests — so the salsa path's
/// `creates_fn_values`-blind graph and the monolithic path's explicit
/// union are edge-for-edge identical today, and this test's real job is
/// to keep proving that at the diagnostics layer both real production
/// consumers actually reach, not just at the atom layer that guard test
/// already covers.
///
/// The fixture must actually exercise `creates_fn_values` non-trivially
/// (rule 19q): a body with no `#fn` literal at all would leave
/// `creates_fn_values` empty on every def, and the union in
/// `effects_project`'s graph builder would be a no-op by triviality
/// (`x ∪ {} = x`), not by the interesting subset property this issue is
/// about — such a fixture would still pass with `creates_fn_values`
/// deleted outright. `user` below creates fn values for `bar` and `baz`
/// without ever calling either by name (only through the local `f`), and
/// the two targets touch two *different* globals so a single-edge
/// coincidence can't make this pass by accident either (mirrors
/// `t2_2_effects_assertions.rs`'s
/// `two_known_fn_origins_collapse_to_the_joined_row_instead_of_the_opaque_floor`
/// fixture, reused here for cross-path parity rather than single-path
/// correctness). The `@[effects(…)]` bound is declared exactly wide
/// enough to cover the joined row `solve_scc_effects` computes today.
///
/// What this actually guards: the two pipelines' *outputs* — including
/// the `@[effects(…)]` exceedance diagnostic — stay byte-identical on a
/// fixture that exercises `creates_fn_values` non-trivially, not just on
/// a trivial one. It is **not** a test of the two call-graph
/// constructions' edge sets directly, and while
/// `creates_fn_values` remains a strict subset of `direct_calls` by
/// construction (see `every_fn_value_creation_target_is_also_a_call_graph_edge`
/// in `brink-analyzer`'s `infer::mod` tests), the two constructions
/// cannot actually disagree here — `resolve_pending_value_calls`'s
/// call-through-a-local narrowing re-records `bar`/`baz` as
/// `direct_calls` at the `f()` call site regardless, so this fixture
/// cannot exercise a `creates_fn_values`-outside-`direct_calls` shape.
/// A future divergence in the two constructions' edge sets would need a
/// dedicated edge-set assertion (see the `call_graph_covers_effect_atoms`
/// unit test in `brink-db/src/queries/mod.rs`) to be caught at all.
#[test]
fn analysis_matches_with_fn_value_creation_and_effects_assertion() {
    let files: &[(&str, &str)] = &[(
        "main.ink",
        "VAR total = 0\nVAR extra = 0\n\
         === function bar(): int ===\n~ total = total + 1\n~ return total\n\
         === function baz(): int ===\n~ extra = extra + 100\n~ return extra\n\
         === function user(cond: int): int ===\n\
         @[effects(reads(total), reads(extra), writes(total), writes(extra))]\n\
         ~ temp f = #fn(bar)\n{cond:\n  ~ f = #fn(baz)\n}\n~ return f()\n",
    )];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };

    let mut db = db_with(files);
    db.set_analysis_options(opts.clone());
    let query = db.analysis();

    let inputs = db.analysis_inputs();
    let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let monolithic = brink_analyzer::analyze_with_options(&refs, &opts);

    assert_eq!(
        *query, monolithic,
        "query-composed analysis (salsa `call_graph_query`, blind to \
         `creates_fn_values`) diverged from the monolithic \
         `effects_project` path (which folds `creates_fn_values` into its \
         own call graph) on a fn-value-creation fixture — issue #1736"
    );
    assert_eq!(
        query.diagnostics,
        Vec::<brink_ir::Diagnostic>::new(),
        "the declared bound covers both #fn targets' joined row on both \
         paths; either path firing here would itself be the finding: {:?}",
        query.diagnostics
    );
}

/// The non-vacuity companion to
/// [`analysis_matches_with_fn_value_creation_and_effects_assertion`]: the
/// same two fn-value-creation sites, but with a bound that names only
/// `bar`'s cell, deliberately leaving `baz`'s write uncovered. Both paths
/// must not just *agree*, they must agree on a firing exceedance — proving
/// the effects-assertion checker actually ran (and actually saw both
/// creation edges) on both paths rather than the equality above passing
/// because both silently skipped the check. If the two call-graph
/// constructions ever disagreed on `user`'s edge set, this is the shape
/// where it would surface as a *different* diagnostic set, not just a
/// missing one: the path that lost the `baz` edge would report `user`'s
/// row as `writes(total)` only (no exceedance beyond `total`, or none at
/// all), while the path that kept it would still name `extra`.
#[test]
fn analysis_matches_with_fn_value_creation_and_effects_exceedance() {
    let files: &[(&str, &str)] = &[(
        "main.ink",
        "VAR total = 0\nVAR extra = 0\n\
         === function bar(): int ===\n~ total = total + 1\n~ return total\n\
         === function baz(): int ===\n~ extra = extra + 100\n~ return extra\n\
         === function user(cond: int): int ===\n\
         @[effects(reads(total), writes(total))]\n\
         ~ temp f = #fn(bar)\n{cond:\n  ~ f = #fn(baz)\n}\n~ return f()\n",
    )];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };

    let mut db = db_with(files);
    db.set_analysis_options(opts.clone());
    let query = db.analysis();

    let inputs = db.analysis_inputs();
    let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let monolithic = brink_analyzer::analyze_with_options(&refs, &opts);

    assert_eq!(
        query.diagnostics.len(),
        1,
        "the narrowed bound must exceed on both paths — a silently \
         skipped check on either path is exactly the divergence this test \
         guards against: {:?}",
        query.diagnostics
    );
    assert_eq!(
        query.diagnostics[0].code,
        DiagnosticCode::E103,
        "{:?}",
        query.diagnostics
    );
    assert!(
        query.diagnostics[0].message.contains("extra"),
        "the exceedance must name the uncovered `baz` origin's cell, \
         proving the `baz` creation edge actually reached the row: {:?}",
        query.diagnostics
    );

    assert_eq!(
        *query, monolithic,
        "query-composed analysis diverged from the monolithic \
         `effects_project` path on the exceedance shape of the same \
         fn-value-creation fixture — issue #1736"
    );
}

#[test]
fn analysis_matches_with_unresolved_and_duplicates() {
    assert_analysis_matches(&[
        ("main.ink", "VAR hp = 3\nVAR hp = 4\n-> nowhere\n"),
        ("extra.ink", "=== spare ===\n-> also_nowhere\n"),
    ]);
}

#[test]
fn signature_matches_direct_analyzer_call() {
    let db = db_with(&[(
        "main.ink",
        "VAR gold = 10\nCONST MAX = 99\n=== quest(hero, ref log) ===\nOnward.\n-> END\n",
    )]);

    // Full-index reference computation, restricted to declarations —
    // `signature_query` reads the decls-only `resolution_index` (#517), so
    // it has no local (`Param`/`Temp`) entries to compare against; those are
    // covered by `signature_is_none_for_locals` below.
    let index = db.symbol_index();
    let inputs = db.analysis_inputs();
    let hir_refs: Vec<(FileId, &HirFile)> = inputs.iter().map(|(id, hir, _)| (*id, hir)).collect();

    let mut checked = 0;
    for def in index.symbols.keys() {
        if matches!(
            index.symbols.get(def).map(|info| info.kind),
            Some(brink_ir::SymbolKind::Param | brink_ir::SymbolKind::Temp)
        ) {
            continue;
        }
        let expected = brink_analyzer::signature(*def, &index, &hir_refs, None);
        let got = db.signature(*def);
        assert_eq!(got, expected, "signature mismatch for {def:?}");
        checked += 1;
    }
    assert!(checked >= 3, "expected several declarations, got {checked}");
}

#[test]
fn signature_is_none_for_locals() {
    // #517: `resolution_index` (which `signature_query` reads) drops locals
    // entirely, so `signature(def)` for a `Param`/`Temp` id is always
    // `None` — by design, not a regression: `signature`/`db.signature`
    // stays the decls-only query (per #531, deliberately not merged with
    // the full index), and a caller that already knows the local's own
    // declaring file gets it instead through `db.local_signature` (issue
    // #530's per-file locals path — see `local_signature.rs`'s tests).
    let db = db_with(&[(
        "main.ink",
        "=== quest(hero) ===\n~ temp step = 1\nOnward.\n-> END\n",
    )]);

    let index = db.symbol_index();
    let local_defs: Vec<_> = index
        .symbols
        .iter()
        .filter(|(_, info)| {
            matches!(
                info.kind,
                brink_ir::SymbolKind::Param | brink_ir::SymbolKind::Temp
            )
        })
        .map(|(id, _)| *id)
        .collect();
    assert!(
        local_defs.len() >= 2,
        "expected a param and a temp def, got {local_defs:?}"
    );
    for def in local_defs {
        assert_eq!(
            db.signature(def),
            None,
            "expected no signature for local {def:?}"
        );
    }
}

#[test]
fn story_data_incremental_equals_fresh() {
    let v1 = "VAR mood = 1\n-> start\n=== start ===\nFirst line.\n~ mood = mood + 1\n-> END\n";
    let v2 = "VAR mood = 1\n-> start\n=== start ===\nFirst line, revised.\n~ temp extra = 2\n~ mood = mood + extra\n-> END\n";

    // Incremental: load v1, pull story_data, then edit to v2 and re-pull.
    let mut db = ProjectDb::new();
    db.set_file("main.ink", v1.to_owned());
    db.set_entry("main.ink").expect("entry");
    let first = db.story_data().expect("entry set").clone();
    assert!(first.story.is_some(), "v1 compiles: {:?}", first.errors);

    db.update_file("main.ink", v2.to_owned());
    let incremental = db.story_data().expect("entry set").clone();

    // Fresh: a brand-new db loaded straight at v2.
    let mut fresh = ProjectDb::new();
    fresh.set_file("main.ink", v2.to_owned());
    fresh.set_entry("main.ink").expect("entry");
    let scratch = fresh.story_data().expect("entry set");

    assert_eq!(incremental, *scratch, "incremental != from-scratch");
    let story = incremental.story.expect("v2 compiles");
    let fresh_story = scratch.story.as_ref().expect("v2 compiles");
    let mut a = Vec::new();
    let mut b = Vec::new();
    brink_format::write_inkb(&story, &mut a);
    brink_format::write_inkb(fresh_story, &mut b);
    assert_eq!(a, b, "serialized StoryData differs");
}

#[test]
fn diagnostics_query_covers_lowering_and_analysis() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", "-> missing_knot\n".to_owned());
    let id = db.file_id("main.ink").expect("id");
    let diags = db.diagnostics(id).expect("diags");
    assert!(
        !diags.is_empty(),
        "unresolved divert should surface in diagnostics(FileId)"
    );
}

/// **Native `.brink` identity is module-qualified** (issue #1526), so the
/// module-*blind* `brink_analyzer::analyze_with_options` cannot stand in for
/// the db here the way it does for the undeclared-stem-module ink corpus
/// above: a native file's module is its path (`market/barter.brink` →
/// `story::market::barter`) and is always declared, so it folds into every
/// `DefinitionId` the db mints.
///
/// `analyze_with_modules`, fed `ProjectDb::module_map()`, is the entry point
/// that agrees — which is what lets an out-of-db analysis pass (the LSP's,
/// `IdeSession`'s) hand ids to `db.effects`/`db.signature`/`db.infer_body`.
/// The module-blind path is asserted to *disagree* in the same test, so this
/// is not vacuous: it would pass with `analyze_with_modules` aliased back to
/// `analyze_with_options` only if native identity stopped being qualified at
/// all.
#[test]
fn native_module_aware_analysis_matches_db_identity() {
    let files: &[(&str, &str)] = &[
        ("main.brink", "flow start() {\n  The market is busy.\n}\n"),
        (
            "market/barter.brink",
            "flow haggle() {\n  You haggle over the price.\n}\n",
        ),
    ];
    let mut db = db_with(files);
    db.set_analysis_options(AnalysisOptions {
        dialect: brink_analyzer::Dialect::Brink,
        ..AnalysisOptions::default()
    });
    let opts = db.analysis_options().clone();

    let inputs = db.analysis_inputs();
    let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();

    let db_ids = ids(&db.symbol_index());
    assert_eq!(db_ids.len(), 2, "two flows, two ids: {db_ids:?}");

    let module_aware = brink_analyzer::analyze_with_modules(&refs, db.module_map(), &opts, false);
    assert_eq!(
        ids(&module_aware.index),
        db_ids,
        "module-aware analysis must mint the db's `DefinitionId`s for native files"
    );

    let module_blind = brink_analyzer::analyze_with_options(&refs, &opts);
    assert_ne!(
        ids(&module_blind.index),
        db_ids,
        "non-vacuity: the module-blind path must NOT match — if it does, \
         native identity stopped being path-qualified"
    );
}

/// **A declared `#@module` on an ink file is module-qualified identity too**
/// (issue #1526 changeset correction): it isn't only native `.brink` files
/// that diverge between the module-blind convenience path and the db — an
/// ink file's *undeclared* stem-module never qualifies identity (see the
/// multi-file fixtures above, which stay equivalent), but a *declared*
/// `#@module(...)` does, exactly like a native file's path-derived module.
#[test]
fn ink_declared_module_aware_analysis_matches_db_identity() {
    let files: &[(&str, &str)] = &[
        (
            "quest.ink",
            "#@module(quest)\n=== start ===\nThe quest begins.\n-> END\n",
        ),
        (
            "town/market.ink",
            "#@module(town_market)\n=== haggle ===\nYou haggle over the price.\n-> END\n",
        ),
    ];
    let db = db_with(files);
    let opts = db.analysis_options().clone();

    let inputs = db.analysis_inputs();
    let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();

    let db_ids = ids(&db.symbol_index());
    assert_eq!(db_ids.len(), 2, "two knots, two ids: {db_ids:?}");

    let module_aware = brink_analyzer::analyze_with_modules(&refs, db.module_map(), &opts, false);
    assert_eq!(
        ids(&module_aware.index),
        db_ids,
        "module-aware analysis must mint the db's `DefinitionId`s for a \
         declared `#@module` ink file"
    );

    let module_blind = brink_analyzer::analyze_with_options(&refs, &opts);
    assert_ne!(
        ids(&module_blind.index),
        db_ids,
        "non-vacuity: the module-blind path must NOT match — if it does, \
         declared `#@module` identity stopped being qualified"
    );
}
