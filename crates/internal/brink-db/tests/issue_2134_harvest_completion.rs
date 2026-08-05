//! `ProjectDb::harvest_completion_names` — the range-free completion
//! projection of the harvest index (issue #2134, `docs/prose-dialect-spec.md`
//! §5).
//!
//! These tests exercise the real db pipeline
//! (`ProjectDb::set_file`/`update_file` + `harvest_completion_names()`), not
//! just the pure `HarvestIndex::names()` projection (unit-tested in
//! `brink-analyzer/src/harvest.rs`). Two load-bearing properties:
//!
//! 1. **Cross-file completion**: a cue harvested only in one file appears in
//!    the project-wide completion projection regardless of which file is
//!    being edited — the actual deliverable issue #2134 asks for.
//! 2. **The Eq-cutoff property**: [`brink_analyzer::HarvestIndex`] can never
//!    `Eq`-cutoff (every site carries a real `TextRange`), but
//!    `harvest_completion_names` must — a pure range shift (no cue/span/attr
//!    name added or removed) must leave the *projection*'s value unchanged
//!    even though the raw index's value legitimately differs.

use brink_db::ProjectDb;

/// The load-bearing cross-file property (§5: "every @NAME cue in the
/// project completes everywhere"): a cue written only in `a.brink` appears
/// in the completion projection queried while the project also contains
/// `b.brink` — proving the projection is a project-wide merge, not scoped to
/// whichever file happens to be open.
#[test]
fn a_cue_declared_in_one_file_completes_via_the_project_wide_projection() {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.brink",
        "flow a() {\n  @VENDOR\n  Something for the road?\n}\n".to_owned(),
    );
    db.set_file(
        "b.brink",
        "flow b() {\n  Nothing to do with a vendor.\n}\n".to_owned(),
    );

    let names = db.harvest_completion_names();
    assert!(
        names.cues.contains("VENDOR"),
        "a cue declared only in a.brink must still appear in the project-wide \
         completion projection: {:?}",
        names.cues
    );
}

/// An unclaimed cue (no conventions handler claims it — the raw §5
/// "harvest by default" case) still completes through the projection, same
/// as it does through the raw index.
#[test]
fn an_unclaimed_cue_still_completes_through_the_projection() {
    let mut db = ProjectDb::new();
    db.set_file("a.brink", "flow a() {\n  @KID\n  Says who?\n}\n".to_owned());

    let names = db.harvest_completion_names();
    assert!(names.cues.contains("KID"), "{:?}", names.cues);
}

/// The Eq-cutoff property itself: inserting a blank line before a cue
/// shifts its `TextRange` (and thus the raw `harvest_index()`'s value) but
/// adds/removes no cue/span/attribute name anywhere — the completion
/// projection must be unchanged.
///
/// This is a genuine regression guard, not a vacuous one: reverting
/// `HarvestIndex::names()` to (say) cloning `HarvestSite` ranges through
/// instead of dropping them makes this assertion fail, because the shifted
/// range would then differ between `before` and `after`.
#[test]
fn harvest_completion_names_is_unchanged_by_a_pure_range_shift() {
    let mut db = ProjectDb::new();
    db.set_file("a.brink", "flow a() {\n  @KID\n  Says who?\n}\n".to_owned());

    let names_before = db.harvest_completion_names();
    let index_before = db.harvest_index();

    // Insert two blank lines before the cue: every subsequent byte offset
    // shifts, but no cue/span/attribute name is added or removed anywhere.
    db.update_file(
        "a.brink",
        "flow a() {\n\n\n  @KID\n  Says who?\n}\n".to_owned(),
    );

    let index_after = db.harvest_index();
    assert_ne!(
        index_before, index_after,
        "sanity: the raw harvest index must actually differ across this \
         edit (its sites carry real ranges), or this test proves nothing"
    );

    let names_after = db.harvest_completion_names();
    assert_eq!(
        names_before, names_after,
        "a pure range shift changed the range-free completion projection — \
         the Eq-cutoff property issue #2134 asks for is broken"
    );
}

/// The same Eq-cutoff property for markup span kinds/attributes, the other
/// half of the harvest index.
#[test]
fn harvest_completion_names_spans_are_unchanged_by_a_pure_range_shift() {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.brink",
        "flow a() {\n  <wave amount=\"3\">shimmer</wave>\n}\n".to_owned(),
    );

    let names_before = db.harvest_completion_names();

    db.update_file(
        "a.brink",
        "flow a() {\n\n\n  <wave amount=\"3\">shimmer</wave>\n}\n".to_owned(),
    );

    let names_after = db.harvest_completion_names();
    assert_eq!(names_before, names_after);
    assert!(
        names_after
            .spans
            .get("wave")
            .expect("wave")
            .attrs
            .contains("amount")
    );
}

/// A genuine change (a *new* cue name added anywhere in the project) must
/// still be visible through the projection — the Eq-cutoff guard must not
/// accidentally suppress real changes, only range-only ones.
#[test]
fn a_genuinely_new_cue_name_still_appears_after_an_edit() {
    let mut db = ProjectDb::new();
    db.set_file("a.brink", "flow a() {\n  @KID\n  Says who?\n}\n".to_owned());

    let names_before = db.harvest_completion_names();
    assert!(!names_before.cues.contains("STRANGER"));

    db.update_file(
        "a.brink",
        "flow a() {\n  @KID\n  Says who?\n  @STRANGER\n  Just passing through.\n}\n".to_owned(),
    );

    let names_after = db.harvest_completion_names();
    assert!(
        names_after.cues.contains("STRANGER"),
        "a real new cue must appear: {:?}",
        names_after.cues
    );
    assert_ne!(names_before, names_after);
}
