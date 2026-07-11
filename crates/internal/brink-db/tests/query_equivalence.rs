//! Equivalence gates for the salsa query pipeline (phase 0 slice B).
//!
//! The query-composed pipeline must be *output-identical* to the monolithic
//! analyzer path — including the range-stripped `resolution_index` cutoff
//! seam, which is only legal because resolution never reads non-local
//! declaration ranges. These tests pin that equivalence on fixtures chosen
//! to poke the risky corners: locals (params/temps), duplicate names across
//! files, cross-file duplicate scoped locals (the finding-4 collision case),
//! and unresolved references.

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
    // Finding 4: duplicate knots across files with same-named scoped locals
    // share a `DefinitionId`. Resolution behavior in this pathological case
    // (last-writer-wins, closest-preceding across files) must be preserved
    // bit-for-bit by the query path.
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

    // Full-index reference computation.
    let index = db.symbol_index();
    let inputs = db.analysis_inputs();
    let hir_refs: Vec<(FileId, &HirFile)> = inputs.iter().map(|(id, hir, _)| (*id, hir)).collect();

    let mut checked = 0;
    for def in index.symbols.keys() {
        let expected = brink_analyzer::signature(*def, &index, &hir_refs);
        let got = db.signature(*def);
        assert_eq!(got, expected, "signature mismatch for {def:?}");
        checked += 1;
    }
    assert!(checked >= 5, "expected several definitions, got {checked}");
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
fn story_data_matches_monolithic_lowering() {
    // Slice-C gate: the chunked query path (range-stripped `lir_index`
    // projection, per-file resolutions, name-table chaining, assembly) must
    // be byte-identical to the eager monolithic path (full-range index,
    // whole-project resolutions, one threaded name table). The fixture pokes
    // the risky corners: temps interned mid-body across two files (chain
    // state), lists + externals (decl interning order), labels + weaves
    // (stamped ids), a root-level temp (shared root scope), and label
    // visit-count reads (counting-flag union).
    let files = [
        (
            "main.ink",
            "INCLUDE lib.ink\nLIST moods = calm, (tense)\nVAR gold = 10\nEXTERNAL beep(x)\n~ temp opening = 1\nRoot line {opening}.\n-> town\n",
        ),
        (
            "lib.ink",
            "=== town ===\n~ temp mood_now = tense\nYou are in town.\n* (shop_opt) [Shop] -> shop\n* [Wait]\n  {&one|two}\n- (waited) Waited {waited}.\n-> shop\n\n= shop\n~ gold = gold - 1\nShop: {gold} gold. Visited {shop_opt} times.\n{ shuffle:\n- A.\n- B.\n}\n-> END\n",
        ),
    ];
    let db = db_with(&files);
    let mut db = db;
    let entry = db.set_entry("main.ink").expect("entry");
    let product = db.story_data().expect("entry set").clone();
    assert!(
        product.story.is_some(),
        "fixture must compile cleanly: {:?}",
        product.errors
    );

    // Monolithic reference: full-range index + whole-project resolutions
    // through the eager lower_to_program, then emit.
    let analysis = monolithic_analysis(&db);
    let inputs = db.analysis_inputs();
    let hir_by_id: std::collections::HashMap<FileId, &HirFile> =
        inputs.iter().map(|(id, hir, _)| (*id, hir)).collect();
    let topo = db.file_ids_topo(entry);
    let hir_refs: Vec<(FileId, &HirFile)> = topo
        .iter()
        .filter_map(|id| hir_by_id.get(id).map(|h| (*id, *h)))
        .collect();
    let paths: std::collections::HashMap<FileId, String> = topo
        .iter()
        .filter_map(|id| db.file_path(*id).map(|p| (*id, p.to_owned())))
        .collect();
    let (program, _warnings) =
        brink_ir::lir::lower_to_program(&hir_refs, &analysis.index, &analysis.resolutions, &paths);
    let expected = brink_codegen_inkb::emit(&program);

    let story = product.story.expect("checked above");
    let mut got_bytes = Vec::new();
    let mut expected_bytes = Vec::new();
    brink_format::write_inkb(&story, &mut got_bytes);
    brink_format::write_inkb(&expected, &mut expected_bytes);
    assert_eq!(
        got_bytes, expected_bytes,
        "chunked query pipeline diverged from monolithic lowering"
    );
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
