//! §5's obligations: overlap dropping, fixpoint convergence, the round cap,
//! a cross-file round, and determinism of the edit order.
//!
//! The batch tests drive **real source through the real registry** — `E025`
//! auto-import on both surfaces (`.ink` and `.brink`), the diagnostics off the
//! same `ProjectDb` road the editor's squiggles come from, the edits applied
//! and the compilation re-analyzed. The [`plan`] unit tests use hand-built
//! candidates because the collision cases they pin (adjacent ranges, a
//! multi-edit fix, a cross-file fix) are properties of the planner, not of any
//! one fixer — but they call the same [`plan`] the batch road runs.

use brink_analyzer::Dialect;
use rowan::TextSize;

use super::*;
use crate::fix::{Applicability, FixMode};

// ── Fixtures ─────────────────────────────────────────────────────────

const QUEST: &str = "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n";
const MARKET: &str = "#@module(market)\n== barter ==\n#@public\nDeal.\n-> DONE\n";

fn session(files: &[(&str, &str)]) -> IdeSession {
    let mut session = IdeSession::new();
    session.set_language_dialect(Dialect::Brink);
    for (path, src) in files {
        session.update_source(path, (*src).to_owned());
    }
    for (path, src) in files {
        session.update_and_analyze(path, (*src).to_owned());
    }
    session
}

/// `town.ink` before the batch: it reaches into two other modules without
/// importing either. Both fixes insert an import at the same offset (just
/// below the `INCLUDE` block), so they collide.
///
/// The `INCLUDE`s are what let `brink compile` see the same two `E025`s the db
/// road reports here — the reachability check on #3418 compiles this exact
/// source and then the text the batch leaves behind.
const TOWN_BEFORE: &str = "#@module(town)\nINCLUDE quest.ink\nINCLUDE market.ink\n== square ==\nHi\n* [Fight] -> ambush\n* [Trade] -> barter\n";

/// The same file after `fix_all` — both imports written out, each on its own
/// line, in the order the two rounds applied them.
const TOWN_AFTER: &str = "#@module(town)\nIMPORT { ambush } FROM quest\nIMPORT { barter } FROM market\nINCLUDE quest.ink\nINCLUDE market.ink\n== square ==\nHi\n* [Fight] -> ambush\n* [Trade] -> barter\n";

/// Two `E025`s in **one** file, so their two fixes collide.
fn two_imports_one_file() -> IdeSession {
    session(&[
        ("quest.ink", QUEST),
        ("market.ink", MARKET),
        ("town.ink", TOWN_BEFORE),
    ])
}

/// One `E025` in each of two files, one per surface — the cross-file round.
fn one_import_each_file() -> IdeSession {
    session(&[
        ("quest.ink", QUEST),
        ("town.ink", "#@module(town)\n== square ==\nHi\n-> ambush\n"),
        ("market/barter.brink", "flow start() {\n  -> ambush\n}\n"),
    ])
}

/// `E025` promoted to `auto` — §6.1's "promote a Suggested fix to batch for
/// this project". Without it the default policy batches nothing here: all four
/// registered fixers declare `Suggested`.
fn promoted() -> FixPolicy {
    FixPolicy::new().with(DiagnosticCode::E025, FixMode::Auto)
}

fn e025_count(session: &IdeSession, paths: &[&str]) -> usize {
    paths
        .iter()
        .filter_map(|p| session.file_id(p))
        .filter_map(|f| session.db().diagnostics(f))
        .flatten()
        .filter(|d| d.code == DiagnosticCode::E025)
        .count()
}

fn edit_shape(round: &Round, session: &IdeSession) -> Vec<(String, u32, String)> {
    round
        .edits
        .iter()
        .map(|e| {
            (
                session.file_path(e.file).unwrap_or_default().to_owned(),
                u32::from(e.range.start()),
                e.new_text.clone(),
            )
        })
        .collect()
}

// ── The fixture actually carries the diagnostics ─────────────────────

#[test]
fn the_two_import_fixture_really_reports_two_e025s() {
    let session = two_imports_one_file();
    assert_eq!(
        e025_count(&session, &["town.ink"]),
        2,
        "fixture must carry two E025s for the overlap test to mean anything"
    );
}

// ── Overlap dropping ─────────────────────────────────────────────────

/// §5 step 2 on real source: two auto-imports into one file insert at the same
/// offset, so the round keeps the first and defers the second.
#[test]
fn a_round_drops_a_fix_whose_edit_touches_a_kept_one() {
    let session = two_imports_one_file();
    let round = apply_round(&FixCx::new(session.db()), &Select::all(), &promoted());

    assert_eq!(round.applied.len(), 1, "one of the two imports is kept");
    assert_eq!(
        round.skipped_overlap.len(),
        1,
        "the colliding one is deferred, not merged"
    );
    assert_eq!(round.edits.len(), 1, "one edit set, not two");
    assert_eq!(round.skipped_overlap[0].code, DiagnosticCode::E025);
}

// ── Fixpoint convergence ─────────────────────────────────────────────

/// The deferred fix is not lost: round two recomputes it against the source
/// round one produced (where the import block now exists, so the insertion
/// point has moved) and applies it. Round three finds nothing.
#[test]
fn fix_all_reaches_a_fixpoint_after_the_deferred_fix_lands() {
    let mut session = two_imports_one_file();
    let report = fix_all(
        &mut session,
        &Select::all(),
        &promoted(),
        DEFAULT_MAX_ROUNDS,
    );

    assert_eq!(
        report.rounds, 2,
        "the collision costs exactly one extra round"
    );
    assert_eq!(report.applied.len(), 2, "both imports land");
    assert_eq!(report.skipped_overlap, 1, "one deferral, reported");
    assert!(
        report.remaining.is_empty(),
        "converged, so nothing is left: {:?}",
        report.remaining
    );
    assert!(!report.cap_hit, "converged well inside the cap");
    assert_eq!(
        e025_count(&session, &["town.ink"]),
        0,
        "the compilation really has no E025 left"
    );
}

/// Re-running on a fixpoint applies nothing at all — the loop's stop
/// condition, observed from outside.
#[test]
fn fix_all_on_a_fixpoint_applies_nothing() {
    let mut session = two_imports_one_file();
    let _ = fix_all(
        &mut session,
        &Select::all(),
        &promoted(),
        DEFAULT_MAX_ROUNDS,
    );
    let again = fix_all(
        &mut session,
        &Select::all(),
        &promoted(),
        DEFAULT_MAX_ROUNDS,
    );

    assert_eq!(again.rounds, 0);
    assert!(again.applied.is_empty());
    assert!(again.remaining.is_empty());
    assert!(!again.cap_hit);
}

// ── Cap breach ───────────────────────────────────────────────────────

/// §5: the cap is reported, never silent. One round is not enough for the
/// two colliding imports, so the report says so and names what is left.
#[test]
fn a_round_cap_breach_is_reported_and_names_what_is_left() {
    let mut session = two_imports_one_file();
    let report = fix_all(&mut session, &Select::all(), &promoted(), 1);

    assert_eq!(report.rounds, 1);
    assert_eq!(report.applied.len(), 1);
    assert!(report.cap_hit, "the cap cut the fixpoint short");
    assert_eq!(
        report.remaining.len(),
        1,
        "and the report names the leftover"
    );
    assert_eq!(report.remaining[0].code, DiagnosticCode::E025);
    assert_eq!(
        e025_count(&session, &["town.ink"]),
        1,
        "the leftover is a real diagnostic still in the compilation"
    );
}

/// A cap of zero applies nothing and still reports the breach — the degenerate
/// end of the same path.
#[test]
fn a_zero_round_cap_applies_nothing_and_still_reports() {
    let mut session = two_imports_one_file();
    let report = fix_all(&mut session, &Select::all(), &promoted(), 0);

    assert_eq!(report.rounds, 0);
    assert!(report.applied.is_empty());
    assert!(report.cap_hit);
    assert_eq!(report.remaining.len(), 2);
}

/// What the *author* is left with: not a count, the actual file. Both imports
/// on their own lines under the module header, in the order the rounds applied
/// them — the text a `brink compile` of this project then accepts.
#[test]
fn the_author_is_left_with_both_imports_written_out() {
    let mut session = two_imports_one_file();
    let _ = fix_all(
        &mut session,
        &Select::all(),
        &promoted(),
        DEFAULT_MAX_ROUNDS,
    );
    let town = session.file_id("town.ink").expect("fixture file is loaded");
    assert_eq!(session.source(town).unwrap_or_default(), TOWN_AFTER);
}

// ── Cross-file in one round ──────────────────────────────────────────

/// §4: the scope is the compilation. One round's edit set spans two files —
/// and two surfaces, `.ink` and `.brink`, which render different import
/// syntax.
#[test]
fn one_round_fixes_both_surfaces_across_files() {
    let mut session = one_import_each_file();
    let round = apply_round(&FixCx::new(session.db()), &Select::all(), &promoted());

    assert_eq!(round.applied.len(), 2, "no collision: different files");
    assert!(round.skipped_overlap.is_empty());
    let files: Vec<String> = round
        .edits
        .iter()
        .map(|e| session.file_path(e.file).unwrap_or_default().to_owned())
        .collect();
    assert!(
        files.contains(&"town.ink".to_owned()) && files.contains(&"market/barter.brink".to_owned()),
        "the round's edits must span both files: {files:?}"
    );
    let native = round
        .edits
        .iter()
        .find(|e| session.file_path(e.file) == Some("market/barter.brink"));
    assert!(native.is_some(), "the native file must get an edit");
    assert!(
        native
            .expect("just asserted above")
            .new_text
            .contains("use quest::ambush"),
        "the native surface gets `use module::name;`, not ink's IMPORT: {:?}",
        native.map(|e| &e.new_text)
    );

    let report = fix_all(
        &mut session,
        &Select::all(),
        &promoted(),
        DEFAULT_MAX_ROUNDS,
    );
    assert_eq!(report.rounds, 1, "both land in a single round");
    assert_eq!(
        e025_count(&session, &["town.ink", "market/barter.brink"]),
        0
    );
}

// ── Determinism ──────────────────────────────────────────────────────

/// Same input, same edit order — twice off one session, and again off an
/// independently built one.
#[test]
fn the_edit_order_is_deterministic() {
    let session = one_import_each_file();
    let first = apply_round(&FixCx::new(session.db()), &Select::all(), &promoted());
    let second = apply_round(&FixCx::new(session.db()), &Select::all(), &promoted());
    assert_eq!(edit_shape(&first, &session), edit_shape(&second, &session));

    let rebuilt = one_import_each_file();
    let third = apply_round(&FixCx::new(rebuilt.db()), &Select::all(), &promoted());
    assert_eq!(edit_shape(&first, &session), edit_shape(&third, &rebuilt));
}

/// The edits a round hands back are sorted by `(file, start, end)`, whatever
/// order the diagnostics arrived in.
#[test]
fn a_rounds_edits_come_back_sorted_by_file_then_offset() {
    let session = one_import_each_file();
    let round = apply_round(&FixCx::new(session.db()), &Select::all(), &promoted());
    let keys: Vec<(FileId, TextSize)> = round
        .edits
        .iter()
        .map(|e| (e.file, e.range.start()))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
}

// ── Policy and selection gates ───────────────────────────────────────

/// The default policy batches nothing here: `E025` is `Suggested`, which
/// defaults to `ask`.
#[test]
fn the_default_policy_batches_no_suggested_fix() {
    let session = two_imports_one_file();
    let round = apply_round(&FixCx::new(session.db()), &Select::all(), &FixPolicy::new());
    assert!(round.applied.is_empty());
    assert!(round.edits.is_empty());
}

/// `off` withdraws the fixer even from an explicit selection of its code.
#[test]
fn an_off_code_is_not_batched() {
    let session = two_imports_one_file();
    let policy = FixPolicy::new().with(DiagnosticCode::E025, FixMode::Off);
    let select = Select::all().with_codes(vec![DiagnosticCode::E025]);
    let round = apply_round(&FixCx::new(session.db()), &select, &policy);
    assert!(round.applied.is_empty());
}

/// `Select::codes` restricts the diagnostics; a code with no diagnostics
/// selects nothing even when the policy would admit it.
#[test]
fn select_codes_restricts_the_selection() {
    let session = two_imports_one_file();
    let policy = promoted().with(DiagnosticCode::E081, FixMode::Auto);
    let select = Select::all().with_codes(vec![DiagnosticCode::E081]);
    let round = apply_round(&FixCx::new(session.db()), &select, &policy);
    assert!(round.applied.is_empty(), "no E081 in this compilation");
}

/// `Select::tiers` filters on the offered fix's tier, not the diagnostic.
#[test]
fn select_tiers_filters_on_the_offered_fix() {
    let session = two_imports_one_file();
    let safe_only = Select::all().with_tiers(vec![Applicability::Safe]);
    let round = apply_round(&FixCx::new(session.db()), &safe_only, &promoted());
    assert!(
        round.applied.is_empty(),
        "the E025 fix is Suggested, so a Safe-only selection skips it"
    );

    let suggested = Select::all().with_tiers(vec![Applicability::Suggested]);
    let round = apply_round(&FixCx::new(session.db()), &suggested, &promoted());
    assert_eq!(round.applied.len(), 1);
}

/// `Select::in_file` restricts which diagnostics are picked up — the
/// "fix all in this file" selection.
#[test]
fn select_in_file_picks_up_only_that_files_diagnostics() {
    let session = one_import_each_file();
    let native = session
        .file_id("market/barter.brink")
        .expect("fixture file is loaded");
    let select = Select::all().in_file(session.db(), native);
    let round = apply_round(&FixCx::new(session.db()), &select, &promoted());

    assert_eq!(round.applied.len(), 1);
    assert_eq!(round.applied[0].file, native);
}

/// `Select::at_offset` is the cursor-menu narrowing: an offset the diagnostic
/// does not cover selects nothing.
#[test]
fn select_at_offset_narrows_to_the_squiggle() {
    let session = one_import_each_file();
    let town = session.file_id("town.ink").expect("fixture file is loaded");
    let src = session.source(town).unwrap_or_default().to_owned();
    let on = u32::try_from(src.find("ambush").unwrap_or(0)).unwrap_or(0);

    let hit = Select::all().at_offset(town, TextSize::from(on));
    assert_eq!(
        apply_round(&FixCx::new(session.db()), &hit, &promoted())
            .applied
            .len(),
        1
    );

    let miss = Select::all().at_offset(town, TextSize::from(0));
    assert!(
        apply_round(&FixCx::new(session.db()), &miss, &promoted())
            .applied
            .is_empty(),
        "offset 0 is the `#@module` header, not the squiggle"
    );
}

// ── The planner, on hand-built candidates ────────────────────────────

fn candidate(code: DiagnosticCode, title: &str, edits: Vec<(u32, u32, u32)>) -> Candidate {
    let edits: Vec<FileEdit> = edits
        .into_iter()
        .map(|(file, start, end)| FileEdit {
            file: FileId(file),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            new_text: title.to_owned(),
        })
        .collect();
    let first = edits
        .first()
        .map_or(TextRange::empty(TextSize::from(0)), |e: &FileEdit| e.range);
    Candidate {
        site: FixSite {
            code,
            file: edits.first().map_or(FileId(0), |e| e.file),
            range: first,
        },
        fix: Fix {
            code,
            title: title.to_owned(),
            applicability: Applicability::Safe,
            edits,
            caret: None,
        },
    }
}

/// Earliest range wins, and *touching* is enough to collide: `[0,4)` and
/// `[4,8)` share no byte but meet.
#[test]
fn plan_drops_a_touching_range_and_keeps_the_earlier_one() {
    let round = plan(vec![
        candidate(DiagnosticCode::E014, "second", vec![(0, 4, 8)]),
        candidate(DiagnosticCode::E014, "first", vec![(0, 0, 4)]),
    ]);
    assert_eq!(round.edits.len(), 1);
    assert_eq!(round.edits[0].new_text, "first");
    assert_eq!(round.skipped_overlap.len(), 1);
}

/// Two pure insertions at one offset collide — the shape two auto-imports
/// produce.
#[test]
fn plan_drops_a_second_insertion_at_the_same_offset() {
    let round = plan(vec![
        candidate(DiagnosticCode::E025, "a", vec![(0, 10, 10)]),
        candidate(DiagnosticCode::E025, "b", vec![(0, 10, 10)]),
    ]);
    assert_eq!(round.edits.len(), 1);
    assert_eq!(
        round.edits[0].new_text, "a",
        "the code/title tiebreak is stable"
    );
    assert_eq!(round.skipped_overlap.len(), 1);
}

/// Same offsets in *different* files are not a collision.
#[test]
fn plan_does_not_collide_across_files() {
    let round = plan(vec![
        candidate(DiagnosticCode::E025, "a", vec![(0, 10, 10)]),
        candidate(DiagnosticCode::E025, "b", vec![(1, 10, 10)]),
    ]);
    assert_eq!(round.edits.len(), 2);
    assert!(round.skipped_overlap.is_empty());
}

/// A fix is atomic: a candidate whose *second* edit collides is deferred
/// whole, so no half-applied fix reaches the compilation.
#[test]
fn plan_defers_a_multi_edit_fix_whole_when_any_edit_collides() {
    let round = plan(vec![
        candidate(DiagnosticCode::E025, "kept", vec![(0, 0, 2)]),
        candidate(DiagnosticCode::E035, "spans", vec![(1, 0, 2), (0, 2, 4)]),
    ]);
    assert_eq!(round.applied.len(), 1);
    assert_eq!(round.skipped_overlap.len(), 1);
    assert_eq!(
        round.edits.len(),
        1,
        "the deferred fix contributes neither of its edits: {:?}",
        round.edits.iter().map(|e| &e.new_text).collect::<Vec<_>>()
    );
}

/// A cross-file fix whose edits collide with nothing keeps both of them.
#[test]
fn plan_keeps_every_edit_of_a_cross_file_fix() {
    let round = plan(vec![candidate(
        DiagnosticCode::E035,
        "spans",
        vec![(1, 0, 2), (0, 2, 4)],
    )]);
    assert_eq!(round.edits.len(), 2);
    assert_eq!(round.applied.len(), 1);
    // Sorted by (file, start) on the way out.
    assert_eq!(round.edits[0].file, FileId(0));
    assert_eq!(round.edits[1].file, FileId(1));
}

/// A fix with no edits changes nothing, so it is neither applied nor
/// deferred.
#[test]
fn plan_drops_an_edit_less_fix() {
    let round = plan(vec![candidate(DiagnosticCode::E014, "empty", vec![])]);
    assert!(round.edits.is_empty());
    assert!(round.applied.is_empty());
    assert!(round.skipped_overlap.is_empty());
}

/// The ordering is a total order over the candidate set, so shuffling the
/// input cannot change the outcome.
#[test]
fn plan_is_order_independent() {
    let build = || {
        vec![
            candidate(DiagnosticCode::E014, "c", vec![(1, 0, 1)]),
            candidate(DiagnosticCode::E025, "a", vec![(0, 8, 9)]),
            candidate(DiagnosticCode::E081, "b", vec![(0, 0, 4)]),
            candidate(DiagnosticCode::E063, "d", vec![(0, 3, 6)]),
        ]
    };
    let forward = plan(build());
    let mut reversed = build();
    reversed.reverse();
    let backward = plan(reversed);

    let shape = |r: &Round| -> Vec<(u32, u32, String)> {
        r.edits
            .iter()
            .map(|e| (e.file.0, u32::from(e.range.start()), e.new_text.clone()))
            .collect()
    };
    assert_eq!(shape(&forward), shape(&backward));
    assert_eq!(
        forward.skipped_overlap.len(),
        backward.skipped_overlap.len()
    );
}
