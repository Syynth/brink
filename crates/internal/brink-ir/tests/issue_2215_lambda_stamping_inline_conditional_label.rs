//! Issue #2215: `stamp_lambdas_in_expr`'s `Fragment` arm and
//! `stamp_lambdas_in_content_part`'s `InlineConditional`/`InlineSequence`
//! arms used to call `lookup_label_id` with `file: None`, bypassing the
//! same-file-preferred label lookup issue #2197/#2213 added for the
//! primary weave walk (`stamp_block`/`stamp_stmt`).
//!
//! ## Confirming the premise was real (rule 12r)
//!
//! Filed as "not exercised by #2197's own repro... this issue is filed to
//! track the gap itself, not a confirmed live bug." Before writing this
//! test, the gap was confirmed live end to end via the real production
//! path (`brink_environment::compile`), using two `.brink` files that each
//! declare a `flow start()` with an identically-named labeled choice
//! nested inside a `{if …}` conditional embedded mid-line in prose (making
//! it a `ContentPart::InlineConditional`, per
//! `hir::lower_native::cond`'s module doc: native disambiguates
//! inline-vs-block placement *positionally*, so the exact same choice/label
//! grammar that works at block level also nests inside a content-embedded
//! conditional). That reproduced the *exact* #2197 symptom — `[E060]
//! internal codegen error: duplicate DefinitionId … assigned to two
//! different containers, at paths "start.0.0" and "start.0.0"` — sourced
//! from this issue's own `lookup_label_id(index, None, …)` call site, not
//! from the already-fixed `lookup_container_id`/`lookup_global`.
//!
//! That full end-to-end repro also trips a SEPARATE, broader bug this
//! issue does not own: `stamp_container_ids`'s per-knot loop qualifies a
//! knot's *interior* anonymous (unlabeled) containers by the knot's own
//! name alone (`stamp_block(&mut knot.body, *file_id, knot_path,
//! knot_path, ...)`), with no per-file qualifier — unlike root content's
//! `root_content_scope_path`, which prefixes `#file:{path}`. Two files
//! that legitimately declare a same-named knot (M-2d) collide on every
//! *unlabeled* descendant container at the same structural position, with
//! or without this issue's fix. That is reported separately (see this
//! issue's tracking comment) rather than fixed here — it is a different
//! call site with a different owning issue.
//!
//! This test instead isolates *only* the label-lookup fix, at the
//! `stamp_container_ids` unit level (mirroring
//! `issue_1727_lambda_hir_minted_id.rs`'s and
//! `native_use_import_scope.rs`'s own techniques), so the assertions below
//! are not perturbed by that separate anonymous-container bug: it only
//! ever fires for the *unlabeled* gather, never for the labeled choice
//! this test reads `container_id` from.
//!
//! ## Regression-test discipline (house rule 20a)
//!
//! Verified this test FAILS with the production fix reverted (the two
//! `stamp_stmt(s, None, …)` calls restored in `stamp_lambdas_in_expr`'s
//! `Fragment` arm and `stamp_lambdas_in_content_part`'s
//! `InlineConditional` arm, and `lookup_label_id`'s signature reverted to
//! `file: Option<FileId>`): `joint_id_b` came back equal to `isolated_id_a`
//! instead of `isolated_id_b` (file B's own labeled choice silently
//! inherited file A's `DefinitionId` — the unscoped `.find()` picking
//! whichever entry sorts first in `by_name`'s insertion order), so both
//! the "self-identity preserved" assertion for B and the "the two files'
//! ids differ" assertion failed. Restored after confirming red.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map for these fixtures — brink_ir::determinism::LookupMap \
              is pub(crate) and invisible to this external test-binary crate, same allow \
              issue_1727_lambda_hir_minted_id.rs's own file doc carries for the identical reason"
)]

use std::collections::HashMap;

mod analysis_fixture;

use brink_analyzer::{AnalysisOptions, Dialect, ModuleMap, ResolvedModule};
use brink_format::DefinitionId;
use brink_ir::hir::lower_native;
use brink_ir::{Choice, ContentPart, FileId, HirFile, Stmt, SymbolManifest};

const FILE_A: FileId = FileId(0);
const FILE_B: FileId = FileId(1);

/// Both files declare a `flow start()` with an identically-named
/// `(dup)`-labeled choice nested inside a `{if …}` conditional embedded
/// mid-line in prose — the exact `ContentPart::InlineConditional` shape
/// this issue's fix threads `file` through. `flag`/`flag2` and the target
/// flow names differ only so each file parses/lowers with zero unrelated
/// diagnostics; they are not otherwise load-bearing.
const SOURCE_A: &str = "\
var flag = true

flow start() {
  Some prose {if flag {
    {?
      * (dup) A's choice text -> a_end
    }
  }} more.
  -> a_end
}

flow a_end() {
  Done A.
  -> END
}
";

const SOURCE_B: &str = "\
var flag2 = true

flow start() {
  Some prose {if flag2 {
    {?
      * (dup) B's choice text -> b_end
    }
  }} more.
  -> b_end
}

flow b_end() {
  Done B.
  -> END
}
";

fn lower(file: FileId, source: &str) -> (HirFile, SymbolManifest) {
    let parsed = brink_syntax_native::parse(source);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let (hir, manifest, diags) = lower_native::lower(file, &parsed.tree());
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );
    (hir, manifest)
}

/// The module map a real native project derives from file paths (`docs/
/// modules-spec.md` §1: native modules are always declared) — one entry
/// per file passed to this call, matching what `analyze_with_modules`
/// would see whether run alone or alongside the other file.
fn module_map(files: &[FileId]) -> ModuleMap {
    files
        .iter()
        .map(|&f| {
            let name = if f == FILE_A { "story::a" } else { "story::b" };
            (
                f,
                ResolvedModule {
                    name: name.to_string(),
                    declared: true,
                    was: None,
                },
            )
        })
        .collect()
}

/// Analyze and stamp exactly the given files together, returning the
/// stamped `HirFile`s in the same order.
fn analyze_and_stamp(files: Vec<(FileId, HirFile, SymbolManifest)>) -> Vec<HirFile> {
    let file_ids: Vec<FileId> = files.iter().map(|(id, _, _)| *id).collect();
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = files
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = analysis_fixture::analyze_with_map(&inputs, &module_map(&file_ids), &opts, true);

    let file_paths: HashMap<FileId, String> = HashMap::new();
    let mut slice: Vec<(FileId, HirFile)> = files
        .into_iter()
        .map(|(id, hir, _manifest)| (id, hir))
        .collect();
    brink_ir::stamp_container_ids(&mut slice, &result.index, &file_paths);
    slice.into_iter().map(|(_, hir)| hir).collect()
}

/// Navigate to the `(dup)`-labeled choice nested inside `start`'s
/// content-embedded `{if …}` conditional — the fixture's shape is fixed,
/// so a direct match is clearer than a generic visitor for a test this
/// targeted. Panics with a precise message identifying which expected
/// node was missing, so a fixture/grammar drift is loud rather than
/// silently vacuous (house rule 12b).
fn dup_choice(hir: &HirFile) -> &Choice {
    let start = hir
        .knots
        .iter()
        .find(|k| k.name.text == "start")
        .expect("fixture must declare a `start` flow");
    let Stmt::Content(content) = &start.body.stmts[0] else {
        panic!(
            "expected `start`'s first statement to be prose Content, got {:?}",
            start.body.stmts[0]
        );
    };
    let cond = content
        .parts
        .iter()
        .find_map(|p| match p {
            ContentPart::InlineConditional(c) => Some(c),
            _ => None,
        })
        .expect(
            "expected the mid-line `{if …}` to lower to a ContentPart::InlineConditional — if \
             this fails, the fixture no longer exercises this issue's code path",
        );
    let branch = &cond.branches[0];
    let cs = branch
        .body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .expect("expected the conditional's branch body to contain a Stmt::ChoiceSet");
    cs.choices
        .first()
        .expect("expected exactly one choice in the nested choice point")
}

fn dup_choice_id(hir: &HirFile) -> DefinitionId {
    dup_choice(hir)
        .container_id
        .expect("stamp_container_ids must assign every choice a container_id")
}

#[test]
fn inline_conditional_labeled_choice_keeps_self_identity_when_a_coexisting_module_shares_the_label()
{
    // ── M-2d ground truth: both files' `start.dup` labels must actually
    // ── coexist in the merged index, or this test proves nothing.
    {
        let (hir_a, manifest_a) = lower(FILE_A, SOURCE_A);
        let (hir_b, manifest_b) = lower(FILE_B, SOURCE_B);
        let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> =
            vec![(FILE_A, &hir_a, &manifest_a), (FILE_B, &hir_b, &manifest_b)];
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        };
        let result = analysis_fixture::analyze_with_map(
            &inputs,
            &module_map(&[FILE_A, FILE_B]),
            &opts,
            true,
        );
        assert_eq!(
            result.index.by_name.get("start.dup").map(Vec::len),
            Some(2),
            "both files' `start.dup` labels must coexist in the index (M-2d) — if only one \
             survives, this test's premise (a real cross-module label collision) does not hold"
        );
    }

    // ── Isolated: each file stamped alone, with the SAME module identity
    // ── it would carry in the joint compile (native modules are always
    // ── declared, per docs/modules-spec.md §1) — so any id difference
    // ── between isolated and joint below is caused ONLY by the other
    // ── file's coexisting declaration, not by a module-identity change.
    let isolated_id_a = {
        let (hir, manifest) = lower(FILE_A, SOURCE_A);
        let [stamped] = analyze_and_stamp(vec![(FILE_A, hir, manifest)])
            .try_into()
            .unwrap_or_else(|_| panic!("expected exactly one stamped file"));
        dup_choice_id(&stamped)
    };
    let isolated_id_b = {
        let (hir, manifest) = lower(FILE_B, SOURCE_B);
        let [stamped] = analyze_and_stamp(vec![(FILE_B, hir, manifest)])
            .try_into()
            .unwrap_or_else(|_| panic!("expected exactly one stamped file"));
        dup_choice_id(&stamped)
    };

    // ── Joint: both files compiled together, exactly like the mounted
    // ── stdlib scenario #2197/#2213 fixed for knots/globals/the primary
    // ── weave walk's own labels.
    let (hir_a, manifest_a) = lower(FILE_A, SOURCE_A);
    let (hir_b, manifest_b) = lower(FILE_B, SOURCE_B);
    let stamped = analyze_and_stamp(vec![
        (FILE_A, hir_a, manifest_a),
        (FILE_B, hir_b, manifest_b),
    ]);
    let [stamped_a, stamped_b] = <[HirFile; 2]>::try_from(stamped)
        .unwrap_or_else(|_| panic!("expected exactly two stamped files"));
    let joint_id_a = dup_choice_id(&stamped_a);
    let joint_id_b = dup_choice_id(&stamped_b);

    assert_eq!(
        joint_id_a, isolated_id_a,
        "file A's own `(dup)` choice must keep the SAME DefinitionId it has in isolation — \
         presence of file B's coexisting `start.dup` label must not change which id file A's \
         own declaration resolves to (this is the #2215 gap: the InlineConditional lambda-\
         stamping traversal used to call lookup_label_id with file: None, an unscoped lookup \
         that can silently prefer the wrong file's entry)"
    );
    assert_eq!(
        joint_id_b, isolated_id_b,
        "file B's own `(dup)` choice must keep the SAME DefinitionId it has in isolation — same \
         self-identity requirement as file A's, in the other direction"
    );
    assert_ne!(
        joint_id_a, joint_id_b,
        "file A's and file B's `(dup)` choices are genuinely distinct containers and must never \
         collapse to the same DefinitionId — the exact #2197-class collision this issue tracks, \
         reached this time through the InlineConditional lambda-stamping traversal instead of \
         the primary weave walk"
    );
}
