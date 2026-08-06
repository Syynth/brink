//! Non-source documents (`brink.toml`, `.md`, `.json`, `.ink.json`) must
//! never be parsed as ink, join the project symbol index, or contribute
//! diagnostics (issue #2329 — the general follow-up to #2318/#2327, which
//! only stopped these files from voting on `is_all_native` nativity but left
//! them lowering through the ink frontend and joining every whole-project
//! query surface).
//!
//! Each non-source document below is deliberately planted with content that
//! *would* pollute the symbol index or the diagnostic stream if it were ever
//! lowered as ink — a knot header (a real symbol) and an unresolved divert
//! target (a real semantic diagnostic) — so a regression here is loud rather
//! than silent.

use brink_db::ProjectDb;

/// `brink.toml` shape is irrelevant to this test (the db never parses TOML);
/// what matters is that it is NOT `.ink`/`.brink`, so per `is_source_file`
/// it must never reach the ink frontend.
const BRINK_TOML: &str = "[project]\nentry = \"story.ink\"\n";

/// A `.md` document planted with a real ink knot header (`== forged_knot
/// ==`) and an unresolved divert. If this ever lowered through the ink
/// frontend, `forged_knot` would mint a real symbol-index entry and the
/// unresolved `-> nowhere_at_all` would raise a semantic diagnostic.
const README: &str = "# Notes\n\n== forged_knot ==\nA planted knot name.\n-> nowhere_at_all\n";

/// A `.ink.json` document (inklecate output, kept only for oracle
/// regeneration — never a compiler input) planted the same way as `README`.
const STORY_INK_JSON: &str =
    "{\n  \"inkVersion\": 21\n}\n== leaked_knot ==\nSuspicious content.\n-> also_nowhere\n";

/// A real, valid ink source file that must remain the ONLY contributor to
/// the project's symbol index and diagnostics.
const STORY: &str = "== start ==\nHello, world.\n-> DONE\n";

#[test]
fn non_source_documents_never_join_the_symbol_index() {
    let mut db = ProjectDb::new();
    db.set_file("brink.toml", BRINK_TOML.to_owned());
    db.set_file("README.md", README.to_owned());
    db.set_file("story.ink.json", STORY_INK_JSON.to_owned());
    db.set_file("story.ink", STORY.to_owned());

    let index = db.symbol_index();

    assert!(
        index.by_name.contains_key("start"),
        "the real ink source file's own knot must still index: {:?}",
        index.by_name.keys().collect::<Vec<_>>()
    );
    for planted in ["forged_knot", "leaked_knot"] {
        assert!(
            !index.by_name.contains_key(planted),
            "`{planted}` was planted in a non-source document (brink.toml/\
             .md/.json/.ink.json are not compiler source) and must never \
             join the project symbol index: {:?}",
            index.by_name.keys().collect::<Vec<_>>()
        );
    }

    let story_id = db.file_id("story.ink").expect("story.ink loaded");
    for info in index.symbols.values() {
        assert_eq!(
            info.file, story_id,
            "every indexed symbol must be attributed to the real source \
             file, never to a non-source document: {info:?}"
        );
    }
}

#[test]
fn non_source_documents_never_contribute_diagnostics() {
    let mut db = ProjectDb::new();
    db.set_file("brink.toml", BRINK_TOML.to_owned());
    db.set_file("README.md", README.to_owned());
    db.set_file("story.ink.json", STORY_INK_JSON.to_owned());
    db.set_file("story.ink", STORY.to_owned());
    db.set_entry("story.ink");

    assert!(
        !db.has_errors(),
        "the planted unresolved diverts live only in non-source documents \
         and must never surface as project errors — has_errors() must stay \
         false for a project whose only real source file (story.ink) is \
         clean"
    );
}

/// Files stay in the session even though they never join parsing/analysis —
/// classification, not deletion (issue #2329's design constraint): config
/// discovery still needs to read `brink.toml`, and the editor still opens
/// `.md` files as plain documents.
#[test]
fn non_source_documents_remain_readable_in_the_session() {
    let mut db = ProjectDb::new();
    db.set_file("brink.toml", BRINK_TOML.to_owned());
    db.set_file("README.md", README.to_owned());

    assert!(db.file_id("brink.toml").is_some());
    assert!(db.file_id("README.md").is_some());
}
