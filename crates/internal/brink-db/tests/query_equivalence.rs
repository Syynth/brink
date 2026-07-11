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

use brink_db::ProjectDb;
use brink_ir::{FileId, HirFile, SymbolManifest};

fn db_with(files: &[(&str, &str)]) -> ProjectDb {
    let mut db = ProjectDb::new();
    for (path, src) in files {
        db.set_file(path, (*src).to_owned());
    }
    db
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
        let expected = brink_analyzer::signature(*def, &index, &hir_refs);
        let got = db.signature(*def);
        assert_eq!(got, expected, "signature mismatch for {def:?}");
        checked += 1;
    }
    assert!(checked >= 3, "expected several declarations, got {checked}");
}

#[test]
fn signature_is_none_for_locals() {
    // #517: `resolution_index` (which `signature_query` reads) drops locals
    // entirely, so `signature(def)` for a `Param`/`Temp` id is always `None`
    // — not yet a regression, since no consumer calls `signature` with a
    // local id today (see `resolve_query`'s own per-file locals lookup).
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
