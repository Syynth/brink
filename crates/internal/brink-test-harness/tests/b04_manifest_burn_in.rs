//! B0.4 exit criterion: the DIFFERENTIAL BURN-IN
//! (docs/hir-admission-contract.md Q3(b), docs/b0-sequencing.md §B0.4,
//! issue #1173) — mandatory and run BEFORE the legacy hand-built manifest
//! path is deleted.
//!
//! Runs both the legacy hand-built `SymbolManifest` (still built by
//! `LowerSink`/`EffectSink` as the frontend lowers each file — unchanged by
//! B0.4's first two steps) and the new `brink_ir::symbols::project_manifest`
//! pipeline projection across the WHOLE oracle corpus (the same 390-case
//! `collect_oracle_cases` corpus `b03_admission_corpus.rs` and
//! `oracle_snapshots.rs` use — CLAUDE.md's "390 cases total"), and asserts
//! the projected manifest is **structurally identical** to the legacy one.
//! Only once this is green does the legacy path get deleted (a later B0.4
//! commit) — this test is kept in-tree afterward as the proof (per the
//! sequencing doc: "Keep the burn-in test in-tree").
//!
//! # What "structurally identical" means here (judgment call — flagged for
//! the coordinator)
//!
//! Every bucket **except** `unresolved`/`locals`/`labels` is compared
//! byte-for-byte, `Vec` order included: `knots`, `stitches`, `variables`,
//! `constants`, `lists`, `structs`, `externals`, `list_items`, and `docs`
//! (a `BTreeMap`, inherently order-normalized). Those buckets' order is
//! provably legacy-compatible — the frontend always declares a symbol at
//! the exact point it's encountered in a single top-down pass over the
//! declaration site, and `project_manifest`'s walker visits the *same*
//! already-lowered tree in the *same* top-down order (see
//! `brink_ir::symbols::project_manifest`'s module doc for the walk-order
//! proof), so this is a meaningful, maximally strict check.
//!
//! `unresolved`, `locals`, and `labels` are compared **order-insensitively**
//! (sorted by `(range.start(), range.end())`, which is a legal sort key —
//! `brink_analyzer::admission`'s E125 check already proves every
//! `UnresolvedRef.range` in a corpus-clean file is unique, and the same
//! holds for each local's/label's own declaration range). The legacy
//! interleaved lowering has at least two accidental orderings baked into
//! `lower_knot_body`'s call sequencing that are not part of the contract:
//! root content's refs land *last* in `manifest.unresolved` (after every
//! knot's, even though root content precedes every knot in the source and
//! in `HirFile.root_content`'s conceptual position), and a knot's
//! *stitches*' refs land *before* the knot's *own* body's refs (because
//! `lower_knot_body` lowers `body.stitches()` before the knot's own
//! `body.lower_block(..)`). Nothing downstream keys off vector position —
//! `brink_analyzer::resolve::lookup_local_in_scope` picks the
//! closest-*preceding* local by `range.start()`, not vector index, and
//! there is no equivalent "closest match wins" logic for `unresolved` at
//! all (each ref resolves independently by path text). Replicating those
//! two accidental orderings inside `project_manifest` was considered and
//! rejected: it would bake an implementation accident of the *old* pipeline
//! into the *new* one's design, exactly the kind of hand-kept-consistency
//! B0.4 exists to delete (D3). If this judgment call is wrong — if some
//! downstream consumer this audit missed *does* depend on manifest vector
//! order — the fix is to special-case that consumer's list on read (a
//! local `.sort_by_key` at its call site), not to reintroduce accidental
//! ordering into the projection.

use std::path::Path;

use brink_db::ProjectDb;
use brink_ir::symbols::project_manifest;
use brink_ir::{FileId, SymbolManifest};
use brink_test_harness::corpus::collect_oracle_cases;

fn tests_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Mirrors `b03_admission_corpus.rs`'s `ink_files_in`: every `.ink` file
/// directly inside a flat corpus case directory.
fn ink_files_in(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ink"))
        .collect();
    out.sort();
    out
}

/// Compare two manifests structurally: exact `Vec` order for the buckets
/// that provably match, sorted-by-range for the three that don't (see the
/// module doc). Returns a human-readable diff description, or `None` if
/// they match.
fn diff_manifests(legacy: &SymbolManifest, projected: &SymbolManifest) -> Option<String> {
    let mut problems = Vec::new();

    macro_rules! check_exact {
        ($field:ident) => {
            if legacy.$field != projected.$field {
                problems.push(format!(
                    "{}: legacy={:?} projected={:?}",
                    stringify!($field),
                    legacy.$field,
                    projected.$field
                ));
            }
        };
    }
    check_exact!(knots);
    check_exact!(stitches);
    check_exact!(variables);
    check_exact!(constants);
    check_exact!(lists);
    check_exact!(structs);
    check_exact!(externals);
    check_exact!(list_items);
    check_exact!(docs);

    let mut legacy_unresolved = legacy.unresolved.clone();
    let mut projected_unresolved = projected.unresolved.clone();
    legacy_unresolved.sort_by_key(|r| (r.range.start(), r.range.end()));
    projected_unresolved.sort_by_key(|r| (r.range.start(), r.range.end()));
    if legacy_unresolved != projected_unresolved {
        problems.push(format!(
            "unresolved (sorted): legacy={legacy_unresolved:?} projected={projected_unresolved:?}"
        ));
    }

    let mut legacy_locals = legacy.locals.clone();
    let mut projected_locals = projected.locals.clone();
    legacy_locals.sort_by_key(|l| (l.range.start(), l.range.end()));
    projected_locals.sort_by_key(|l| (l.range.start(), l.range.end()));
    if legacy_locals != projected_locals {
        problems.push(format!(
            "locals (sorted): legacy={legacy_locals:?} projected={projected_locals:?}"
        ));
    }

    let mut legacy_labels = legacy.labels.clone();
    let mut projected_labels = projected.labels.clone();
    legacy_labels.sort_by_key(|l| (l.range.start(), l.range.end()));
    projected_labels.sort_by_key(|l| (l.range.start(), l.range.end()));
    if legacy_labels != projected_labels {
        problems.push(format!(
            "labels (sorted): legacy={legacy_labels:?} projected={projected_labels:?}"
        ));
    }

    if problems.is_empty() {
        None
    } else {
        Some(problems.join("\n    "))
    }
}

#[test]
fn projected_manifest_matches_legacy_across_the_whole_oracle_corpus() {
    let root = tests_dir();
    let cases = collect_oracle_cases(&root);
    assert!(
        !cases.is_empty(),
        "expected to find oracle cases under {root:?}"
    );

    let mut db = ProjectDb::new();
    let mut failures: Vec<String> = Vec::new();
    let mut files_checked = 0usize;

    for case_dir in &cases {
        for ink_path in ink_files_in(case_dir) {
            let Ok(source) = std::fs::read_to_string(&ink_path) else {
                continue;
            };
            let rel = ink_path
                .strip_prefix(&root)
                .unwrap_or(&ink_path)
                .to_string_lossy()
                .into_owned();
            let file_id: FileId = db.set_file(&rel, source);
            files_checked += 1;

            let Some(hir) = db.hir(file_id) else {
                failures.push(format!("{}: no HIR produced", ink_path.display()));
                continue;
            };
            let Some(legacy_manifest) = db.manifest(file_id) else {
                failures.push(format!("{}: no manifest produced", ink_path.display()));
                continue;
            };
            let projected = project_manifest(hir);

            if let Some(diff) = diff_manifests(legacy_manifest, &projected) {
                failures.push(format!("{}:\n    {diff}", ink_path.display()));
            }
        }
    }

    assert!(
        !failures.is_empty() || files_checked > 0,
        "checked zero files — corpus discovery is broken"
    );
    assert!(
        failures.is_empty(),
        "{} of {} corpus files have a projected/legacy manifest mismatch:\n{}",
        failures.len(),
        files_checked,
        failures.join("\n")
    );

    let case_count = cases.len();
    eprintln!(
        "B0.4 manifest burn-in: {files_checked} files across {case_count} oracle cases, \
         projected manifest structurally identical to legacy in every case"
    );
}
