//! `DefinitionId` agreement between an IDE session's analysis and the
//! project db's per-def queries, for **native `.brink` projects**
//! (issue #1526).
//!
//! A native file's module is its path (`market/barter.brink` →
//! `story::market::barter`) and is always *declared*, so it qualifies
//! `DefinitionId` identity — unlike the undeclared stem-modules the entire
//! ink corpus uses, where module-blind and module-aware hashing agree by
//! construction. `IdeSession`'s analysis therefore has to be fed the db's
//! resolved [`brink_analyzer::ModuleMap`]; without it every id it hands to
//! an IDE feature is a bare-name hash that misses in `db.effects` /
//! `db.signature` / `db.infer_body`, and hover silently drops the effect
//! row, the declared signature, and inferred types for every native symbol.
//!
//! These tests use a *multi-file* project in a subdirectory precisely
//! because that is where the two identity schemes diverge most visibly: the
//! qualifying module differs per file, so a single shared bare-name index
//! cannot stand in for it.

use brink_analyzer::Dialect;
use brink_ide::hover::hover;
use brink_ide::session::IdeSession;
use rowan::TextSize;

/// `market/barter.brink` — the definition side of the boundary.
const BARTER: &str = "\
var gold = 10

/// Trade at the market stall.
flow haggle() {
  You haggle over the price.
}
";

/// `main.brink` — the reference side: a divert whose target lives in the
/// other file's module.
const MAIN: &str = "\
use story::market::barter::haggle;

flow start() {
  The market is busy.
  -> haggle
}
";

/// A two-file native project, analyzed.
fn native_session() -> (IdeSession, brink_ir::FileId) {
    let mut session = IdeSession::new();
    session.set_language_dialect(Dialect::Brink);
    session.update_source("market/barter.brink", BARTER.to_owned());
    let main = session.update_and_analyze("main.brink", MAIN.to_owned());
    (session, main)
}

/// The root cause (#1526): the ids in the session's `AnalysisResult` must be
/// the ids the db's queries are keyed by. Asserted on the db's *own* query
/// surface (`db.effects`/`db.signature`/`db.inferred_signature` all return
/// `Some`) rather than on index equality alone, because a hit there is what
/// every `info.id`-based IDE feature actually depends on.
#[test]
fn native_analysis_ids_key_the_db_per_def_queries() {
    let (session, _main) = native_session();
    let analysis = session.analysis().expect("analysis");
    let db = session.db();

    let db_index = db.symbol_index();
    for (id, info) in &analysis.index.symbols {
        assert!(
            db_index.symbols.contains_key(id),
            "`{}` ({:?}) has id {id} in the session's analysis but no such id \
             in the db's symbol index — the two mint different identities",
            info.name,
            info.kind,
        );
    }

    // Both flows, across both files: identity is path-derived, so a per-file
    // divergence would show up on one and not the other.
    for name in ["haggle", "start"] {
        let ids = analysis.index.by_name.get(name).expect("flow is declared");
        assert_eq!(ids.len(), 1, "exactly one `{name}`");
        let id = ids[0];
        assert!(
            db.effects(id).is_some(),
            "`db.effects` missed for `{name}` ({id})"
        );
        assert!(
            db.signature(id).is_some(),
            "`db.signature` missed for `{name}` ({id})"
        );
        assert!(
            db.inferred_signature(id).is_some(),
            "`db.inferred_signature` missed for `{name}` ({id})"
        );
        assert!(
            db.infer_body(id).is_some(),
            "`db.infer_body` missed for `{name}` ({id})"
        );
    }
}

/// The user-visible path: hovering a cross-file divert target in a native
/// project. The effect row (`db.effects(info.id)`, hover.rs) is the visible
/// tell — it is present only when the hovered symbol's id, minted by the
/// session's analysis, keys the db's per-def query.
#[test]
fn native_cross_file_hover_shows_the_db_backed_effect_row() {
    let (session, main_id) = native_session();
    let analysis = session.analysis().expect("analysis");
    let offset = u32::try_from(MAIN.find("haggle\n}").expect("divert target")).expect("offset");
    let files = session.db().file_metadata();

    let content = hover(
        analysis,
        session.db(),
        main_id,
        MAIN,
        TextSize::from(offset),
        &files,
    )
    .expect("hover over the divert target")
    .content;

    assert!(content.contains("**knot** `haggle`"), "{content}");
    assert!(
        content.contains("*Defined in `market/barter.brink`*"),
        "the hover must cross the file boundary: {content}"
    );
    assert!(
        content.contains("**effects**"),
        "the db-backed effect row must survive the identity join: {content}"
    );
    assert!(content.contains("Trade at the market stall."), "{content}");
}
