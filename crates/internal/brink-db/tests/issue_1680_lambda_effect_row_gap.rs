//! Issue #1680, characterization: a **lambda-lifted function has no row in
//! the `EffectRows` table at all**, so `effects-spec.md` §7's "a live fn
//! value is a token; its row is a table lookup" cannot resolve for the one
//! value form the native higher-order core is built on.
//!
//! This is a *gap* pin, not a contract — it asserts what the compiler does
//! today so the next attempt at #1680 finds the prerequisite already
//! measured instead of rediscovering it. It is the same discipline #1709
//! used for its `E052` fence, and it must **flip** when #1680 lands.
//!
//! ## Why the row is missing
//!
//! The two halves are minted a layer apart:
//!
//! - `populate_effect_rows` (`brink-db/src/queries/mod.rs`) walks
//!   `inferable_defs_query`, which is
//!   `brink_analyzer::inferable_defs_from_index` — `index.symbols` filtered
//!   to `SymbolKind::Knot | SymbolKind::Stitch`
//!   (`brink-analyzer/src/infer/mod.rs`). A lambda is an inline
//!   `hir::Expr::Lambda`, never an indexed knot/stitch symbol, so no
//!   iteration of that set can ever yield one.
//! - The lifted function's `DefinitionId` is minted in **LIR** lowering, by
//!   `IdAllocator::alloc_lambda_address` (`brink-ir/src/lir/lower/context.rs`),
//!   which runs after the analyzer has already produced every row.
//!
//! So the id that ends up in a live `VAL_FN_REF`/`VAL_CLOSURE` token is a
//! `DefinitionTag::Address` id that the row table was never given a chance
//! to key.
//!
//! ## What is *not* broken
//!
//! Soundness. `InferPass::infer_lambda` (`brink-analyzer/src/infer/body.rs`)
//! walks the lambda body's expressions inside the **enclosing** definition's
//! pass, so every atom the body performs is absorbed into the enclosing
//! def's row. That over-reports (spec §3's conservative-total direction —
//! over-report is always allowed) rather than under-reporting. The second
//! test below pins exactly that, so a future precision change cannot quietly
//! drop the absorption while making the first test pass.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::ProjectDb;
use brink_format::DefinitionId;

/// A native project whose only read of `counter` happens *inside* a lambda,
/// which is then lifted to its own top-level function by #1709.
const SOURCE: &str = "\
var counter = 7

fn bumped(n: int): int {
  let f = |x| x + counter;
  return f(n);
}

flow main() {
  Bumped: {bumped(3)}
  -> END
}
";

/// Compile `SOURCE` and hand back the linked story plus the ids of every
/// lambda-lifted container in it.
///
/// Lifted containers are found through `address_paths`: lifting names its
/// synthesized function `{enclosing scope path}.#lambda-{source start
/// offset}` (`brink-ir/src/lir/lower/lambda.rs`), and `#lambda-` is not a
/// spelling any author path can produce — `#` cannot start a native path
/// segment.
fn lifted_lambda_defs(story: &brink_format::StoryData) -> Vec<DefinitionId> {
    story
        .address_paths
        .iter()
        .filter(|ap| {
            story
                .name_table
                .get(ap.path.0 as usize)
                .is_some_and(|p| p.contains("#lambda-"))
        })
        .map(|ap| ap.target)
        .collect()
}

/// The gap itself: the lifted function is a real, addressable container in
/// the story, and the `EffectRows` table has nothing for it.
#[test]
fn a_lifted_lambda_ships_no_effect_row() {
    let mut db = ProjectDb::new();
    db.set_file("main.brink", SOURCE.to_owned());
    db.set_entry("main.brink");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    // Guard the guard: if lifting ever stops emitting an addressable
    // container the filter below would vacuously pass.
    let lifted = lifted_lambda_defs(story);
    assert!(
        !lifted.is_empty(),
        "the lambda must lift to an addressable container (#1709); \
         name_table = {:?}",
        story.name_table
    );

    assert!(
        !story.effect_rows.is_empty(),
        "the story's knots/stitches do ship rows, so an empty table would \
         make the next assertion meaningless"
    );

    let with_rows: Vec<DefinitionId> = story.effect_rows.iter().map(|r| r.def).collect();
    for def in &lifted {
        assert!(
            !with_rows.contains(def),
            "#1680: a lambda-lifted function is expected to carry no \
             `EffectRows` entry today — if this now fails, the row table \
             reaches lifted lambdas and this characterization must be \
             replaced by the real contract"
        );
    }
}

/// The soundness half: the lambda's read of `counter` is absorbed into the
/// **enclosing** definition's row, so nothing under-reports while the lifted
/// function's own row is missing (spec §3, conservative-total).
#[test]
fn the_enclosing_def_absorbs_the_lambda_bodys_atoms() {
    let mut db = ProjectDb::new();
    db.set_file("main.brink", SOURCE.to_owned());
    db.set_entry("main.brink");

    let index = db.symbol_index();
    let bumped = *index
        .by_name
        .get("bumped")
        .expect("`bumped` is indexed")
        .first()
        .expect("indexed name has at least one def");
    let counter = *index
        .by_name
        .get("counter")
        .expect("`counter` is indexed")
        .first()
        .expect("indexed name has at least one def");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    let row = story
        .effect_rows
        .iter()
        .find(|r| r.def == bumped)
        .expect("`bumped` ships a container row");

    assert!(
        row.direct.reads.contains(&counter),
        "the lambda body's read of `counter` must be absorbed into the \
         enclosing def's row — reads = {:?}",
        row.direct.reads
    );
}
