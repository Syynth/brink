//! T2-3 (#862, docs/effects-spec.md §11): the `EffectRows` **emission** path,
//! exercised end-to-end through `ProjectDb::story_data`.
//!
//! T2-1 (`t2_1_effect_rows.rs`) proved the advisory `effects(def)` query. This
//! proves the *reachability* of the real emission: compiling a story routes the
//! inferred rows into `StoryData::effect_rows` (one factored row per
//! knot/stitch — the host's resume-scheduling estimate, §12.1), with the call
//! kinds interned into the name table and the capability-parameter slot
//! populated `Any`. The rows are additive metadata — the runtime never reads
//! them — so this changes no episode; the point here is that the section is
//! actually populated on a real compile, not just unit-tested in isolation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::ProjectDb;
use brink_format::{CapabilityParam, DefinitionId};

fn def_named(db: &ProjectDb, name: &str) -> DefinitionId {
    let index = db.symbol_index();
    let ids = index.by_name.get(name).expect("def should be indexed");
    *ids.first().expect("indexed name has at least one def")
}

#[test]
fn story_data_emits_a_populated_effect_rows_table() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR gold = 0\nVAR hp = 10\nEXTERNAL play_sfx(x)\n\
         === function spend(cost) ===\n~ gold = gold - cost\n\
         ~ temp before = hp\n~ play_sfx(cost)\n~ return gold\n\
         === start ===\n~ spend(3)\n-> DONE\n"
            .to_owned(),
    );
    db.set_entry("main.ink");

    let spend = def_named(&db, "spend");
    let gold = def_named(&db, "gold");
    let hp = def_named(&db, "hp");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    // The section is populated (not the empty reserved section): every
    // inferable def ships a row, so `spend` and `start` both appear.
    assert!(
        !story.effect_rows.is_empty(),
        "effect rows are emitted for a story with knots/stitches"
    );

    // Rows are sorted by `def` (BTreeSet order) — a determinism guard.
    let defs: Vec<DefinitionId> = story.effect_rows.iter().map(|r| r.def).collect();
    let mut sorted = defs.clone();
    sorted.sort_by_key(|d| d.to_raw());
    assert_eq!(
        defs, sorted,
        "effect rows are emitted in sorted `def` order"
    );

    // `spend`'s row carries its read/write cells and its call atom.
    let spend_row = story
        .effect_rows
        .iter()
        .find(|r| r.def == spend)
        .expect("spend ships a container row");
    assert!(spend_row.direct.reads.contains(&gold), "reads gold");
    assert!(spend_row.direct.reads.contains(&hp), "reads hp");
    assert!(spend_row.direct.writes.contains(&gold), "writes gold");
    assert!(
        !spend_row.direct.opaque,
        "fully-visible body is not pessimal"
    );
    assert!(
        spend_row.dispatches.is_empty(),
        "v1 emits no per-dispatch entries"
    );

    // The call kind is a real interned name-table entry, and its
    // capability-parameter slot is populated `Any` with the handle slot `None`.
    let call = spend_row
        .direct
        .calls
        .first()
        .expect("spend calls play_sfx");
    assert_eq!(call.capability, CapabilityParam::Any, "v1 slot is Any");
    assert_eq!(
        call.handle_param, None,
        "reserved handle slot is None in v1"
    );
    let call_name = story
        .name_table
        .get(call.name.0 as usize)
        .expect("call atom name is a valid name-table index");
    assert_eq!(
        call_name, "play_sfx",
        "call atom resolves to the external kind"
    );

    // #882: the pre-modules all-public world ships every row as an entry.
    assert!(spend_row.is_entry, "public def ships as a host entry point");
}

/// #882 — the freeze semantics T2-3 was missing: a `#@private` def's row is
/// excluded from the **entry set** (`is_entry: false` — not a legitimate
/// host-lookup target, `docs/effects-spec.md` §10 / `docs/modules-spec.md` §4
/// rule 2) but the row itself is NOT dropped from the `DefinitionId -> row`
/// table — `#@private` hides the *name*, not the *cell* (modules-spec §4 rule
/// 1). Here a *public* function holds a call-token to a *private* one
/// (`secret_heal`), so the dispatch-narrowing machinery must still be able to
/// resolve `secret_heal`'s row by `DefinitionId` even though it is not a host
/// entry point. The public knot/function stay unaffected (`is_entry: true`).
#[test]
fn private_def_row_stays_in_table_but_not_the_entry_set() {
    use brink_analyzer::{AnalysisOptions, Dialect};

    let mut db = ProjectDb::new();
    // `#@private` is dialect-gated (M-2, docs/modules-spec.md §2/§4) — under
    // the default `StrictInk` it still computes visibility (superset-parse-
    // then-reject) but also raises an E051-class dialect diagnostic that
    // would block `story_data`'s LIR gate, so this test opts into `Brink`
    // like the other M-2 db-level tests (`brink_opts()` precedent).
    db.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    });
    db.set_file(
        "main.ink",
        "VAR hp = 10\n\
         === function secret_heal(amount) ===\n#@private\n~ hp = hp + amount\n~ return hp\n\
         === start ===\n~ secret_heal(5)\n-> DONE\n"
            .to_owned(),
    );
    db.set_entry("main.ink");

    let secret_heal = def_named(&db, "secret_heal");
    let start = def_named(&db, "start");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    assert!(
        story.private_defs.contains(&secret_heal),
        "sanity: secret_heal is recorded #@private"
    );

    // Not an entry: `secret_heal`'s row opts out of the entry set.
    let secret_row = story
        .effect_rows
        .iter()
        .find(|r| r.def == secret_heal)
        .expect("private def's row still resolvable via the DefinitionId table");
    assert!(
        !secret_row.is_entry,
        "#@private def's row is not a host entry point"
    );

    // Still in the table: the entry set never controls table membership.
    assert!(
        !story
            .effect_rows
            .iter()
            .filter(|r| r.is_entry)
            .any(|r| r.def == secret_heal),
        "the entry set (is_entry rows) excludes the private def"
    );

    // Public def unaffected.
    let start_row = story
        .effect_rows
        .iter()
        .find(|r| r.def == start)
        .expect("start ships a row");
    assert!(start_row.is_entry, "public def is unaffected");
}
