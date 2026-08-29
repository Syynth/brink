//! #3275 (stage 3a) — the both-roads id-equality gate. Container ids are
//! stamped on pristine HIR and inherited/derived through the lift; the
//! db road (`normalized_stamped_query`) and the off-db road
//! (`lir::lower::build_prelude`) each run stamp-then-normalize
//! independently, and every id is an address in the emitted `.inkb` — so
//! byte-identical output IS the proof the two roads agree on every
//! container id, for exactly the shapes 3a changed: a mixed line whose
//! clones share a stateful alternative, a cloned stateless conditional,
//! a synthesized else, and a plain-`once` exhausted branch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

fn fixture() -> &'static str {
    // One scope exercising every 3a id shape:
    //  - mixed line: conditional + stateful alt (clones SHARE the alt)
    //  - two conditionals on one line (stateless clone → derived ids)
    //  - no-else conditional with suffix (synthesized else branch)
    //  - plain `once` with prefix (synthesized exhausted branch)
    //  - claimed two-alternative line (variant model, stamped wrappers)
    "VAR n = 0\n\
     -> loop\n\
     === loop ===\n\
     ~ n = n + 1\n\
     Line: {n > 1: late|early} {&p|q}\n\
     Pair: {n > 1: big|small} {n > 2: hot|cold} end\n\
     Tail {n > 1: extra} done\n\
     Once {!first|second} here\n\
     Roll {&a|b} and {x|y}.\n\
     { n < 3: -> loop }\n\
     -> END\n"
}

#[test]
fn db_and_offdb_roads_emit_identical_bytes() {
    // Off-db road: brink_compiler::compile → build_prelude.
    let offdb = brink_compiler::compile("main.ink", |p| {
        assert_eq!(p, "main.ink");
        Ok(fixture().to_string())
    })
    .expect("off-db road compiles");
    let mut offdb_bytes = Vec::new();
    brink_format::write_inkb(&offdb.data, &mut offdb_bytes);

    // Db road: ProjectDb → normalized_stamped_query → link.
    let mut db = brink_db::ProjectDb::new();
    db.set_file("main.ink", fixture().to_owned());
    db.set_entry("main.ink");
    let story: &Arc<brink_format::StoryData> = db
        .story_data()
        .and_then(|c| c.story.as_ref())
        .expect("db road compiles");
    let mut db_bytes = Vec::new();
    brink_format::write_inkb(story, &mut db_bytes);

    assert_eq!(
        db_bytes, offdb_bytes,
        "#3275 stage 3a: the db road and the off-db road disagree on the \
         compiled bytes of the id-shape fixture — the stamp/normalize \
         lockstep between `normalized_stamped_query` and `build_prelude` \
         has drifted (container ids are addresses in the emitted binary, \
         so any id disagreement lands here)"
    );
}
