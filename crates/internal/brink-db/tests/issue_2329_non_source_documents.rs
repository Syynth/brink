//! Non-source documents (`brink.toml`, `.md`, `.json`, `.ink.json`, and any
//! other non-`.ink`/`.brink` extension such as `.txt`) must never be parsed
//! as ink, join the project symbol index, or contribute diagnostics (issue
//! #2329 — the general follow-up to #2318/#2327, which only stopped these
//! files from voting on `is_all_native` nativity but left them lowering
//! through the ink frontend and joining every whole-project query surface).
//!
//! Each non-source document below is deliberately planted with content that
//! *would* pollute the symbol index or the diagnostic stream if it were ever
//! lowered as ink — a knot header (a real symbol) and an unresolved divert
//! target (a real semantic diagnostic) — so a regression here is loud rather
//! than silent.
//!
//! `is_source_file` must be an allowlist (`.ink`/`.brink`/no-extension), not
//! a blocklist of a few named extensions (a review finding on this issue's
//! first attempt: a blocklist of only `toml`/`md`/`json` left `.txt` still
//! reaching the ink frontend) — see
//! `a_non_blocklisted_extension_is_still_excluded_from_source` below. And
//! `project_is_all_native`'s nativity vote must keep reading a strict
//! `.ink`/`.brink` allowlist of its own (`has_recognized_source_extension`),
//! not `is_source_file` — see
//! `project_is_all_native_ignores_unrecognized_extensions` below.

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

/// A plain-text note — deliberately NOT one of the extensions a blocklist
/// approach would happen to name (`toml`/`md`/`json`). Planted the same way
/// as `README` to prove `is_source_file` is a `.ink`/`.brink`-plus-no-
/// extension allowlist, not a blocklist of a few known-bad extensions
/// (review finding on this issue: a blocklist leaves every unlisted
/// extension, like `.txt`, still lowering through the ink frontend).
const NOTES_TXT: &str =
    "Just some notes.\n\n== forged_knot_txt ==\nA planted knot name.\n-> nowhere_at_all\n";

/// A real, valid ink source file that must remain the ONLY contributor to
/// the project's symbol index and diagnostics.
const STORY: &str = "== start ==\nHello, world.\n-> DONE\n";

#[test]
fn non_source_documents_never_join_the_symbol_index() {
    let mut db = ProjectDb::new();
    db.set_file("brink.toml", BRINK_TOML.to_owned());
    db.set_file("README.md", README.to_owned());
    db.set_file("story.ink.json", STORY_INK_JSON.to_owned());
    db.set_file("notes.txt", NOTES_TXT.to_owned());
    db.set_file("story.ink", STORY.to_owned());

    let index = db.symbol_index();

    assert!(
        index.by_name.contains_key("start"),
        "the real ink source file's own knot must still index: {:?}",
        index.by_name.keys().collect::<Vec<_>>()
    );
    for planted in ["forged_knot", "leaked_knot", "forged_knot_txt"] {
        assert!(
            !index.by_name.contains_key(planted),
            "`{planted}` was planted in a non-source document (brink.toml/\
             .md/.json/.ink.json/.txt are not compiler source) and must \
             never join the project symbol index: {:?}",
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
    db.set_file("notes.txt", NOTES_TXT.to_owned());
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

/// The literal scenario named on this issue's review: a blocklist of
/// `toml`/`md`/`json` leaves any other extension — `.txt` here — still
/// lowering through the ink frontend and joining the symbol index and
/// diagnostic stream. `is_source_file` must be an allowlist
/// (`.ink`/`.brink`/no-extension), not a blocklist of a few named
/// extensions, or this test fails exactly the way the issue describes.
#[test]
fn a_non_blocklisted_extension_is_still_excluded_from_source() {
    let mut db = ProjectDb::new();
    db.set_file("notes.txt", NOTES_TXT.to_owned());
    db.set_file("story.ink", STORY.to_owned());
    db.set_entry("story.ink");

    let index = db.symbol_index();
    assert!(
        !index.by_name.contains_key("forged_knot_txt"),
        "a `.txt` file must never join the project symbol index: {:?}",
        index.by_name.keys().collect::<Vec<_>>()
    );
    assert!(
        !db.has_errors(),
        "the planted unresolved divert lives only in `notes.txt` and must \
         never surface as a project error"
    );
}

/// Regression coverage for the review finding that a bare blocklist swap
/// silently broke `project_is_all_native`: a native project sharing its
/// session with a tracked file that has neither a `.brink` nor an `.ink`
/// extension (here, an extension-less file and a `.txt` file) must still
/// read as fully native — exactly the `brink.toml`-coexistence guarantee
/// issue #2318 established, now proven for extensions `is_source_file`
/// itself does not recognize as source.
#[test]
fn project_is_all_native_ignores_unrecognized_extensions() {
    const NATIVE_STORY: &str = "flow main() {\n  Hi. -> END\n}\n";

    let mut db = ProjectDb::new();
    db.set_file("main.brink", NATIVE_STORY.to_owned());
    db.set_file("notes.txt", NOTES_TXT.to_owned());
    db.set_file("NOTES", "no extension at all".to_owned());
    db.set_entry("main.brink");

    assert!(
        db.is_all_native(),
        "a `.txt` file or an extension-less file sharing the session with a \
         native `.brink` module must not disqualify `is_all_native` — \
         neither is a recognized `.ink`/`.brink` source file"
    );
}

/// A `.md` document planted with a directive (`#@module(...)`) that is
/// brink-only syntax (M-1, docs/modules-spec.md §3) — under the default
/// `StrictInk` dialect this trips `brink_analyzer::dialect_gate`'s `E051`
/// purely from this file's own HIR, with no other file or project-wide
/// resolution involved. Unlike `README`'s planted unresolved divert (a
/// *resolution*-time diagnostic, already excluded by
/// `analysis_diagnostics_query`'s own pre-existing per-file loop gate),
/// `E051` is exactly the class of diagnostic `per_file_diagnostics_query`
/// itself produces — the review finding's literal claim — so this is the
/// fixture that actually distinguishes "gated" from "not gated" for that
/// query (a regression test using `README` alone would stay green even with
/// `per_file_diagnostics_query`'s own gate deleted, since `dialect_gate`
/// never fires on `README`'s content).
const DIALECT_GATE_TRIGGER_MD: &str = "#@module(forged)\nHi\n";

/// A `.md` document planted with a genuine ink *parse* error (an
/// unterminated inline conditional) — this is the other diagnostic source
/// [`diagnostics_query`]/`ProjectDb::diagnostics`/`ProjectDb::file_diagnostics`
/// read directly off `lowered_query(file).diagnostics` (parser errors folded
/// in as `E037`), distinct from [`DIALECT_GATE_TRIGGER_MD`]'s
/// `per_file_diagnostics_query`-sourced `E051`. A regression test using only
/// one of the two fixtures would stay green even with the *other* query's
/// gate deleted.
const PARSE_ERROR_TRIGGER_MD: &str = "{\n";

/// Review finding on this issue: `per_file_diagnostics_query` and
/// `diagnostics_query` are direct per-file entry points
/// (`ProjectDb::per_file_diagnostics`/`ProjectDb::diagnostics`) reachable
/// without going through the whole-project aggregators'
/// `is_source_file` gate — as are the even-more-direct `hir`/`manifest`/
/// `file_diagnostics`/`admission_diagnostics` accessors, which read
/// `lowered_query` straight off the file id. Each must independently refuse
/// to surface a non-source document's bogus ink-lowered HIR/diagnostics.
#[test]
fn non_source_documents_are_excluded_from_every_direct_per_file_accessor() {
    let mut db = ProjectDb::new();
    db.set_file("README.md", DIALECT_GATE_TRIGGER_MD.to_owned());
    db.set_file("broken.md", PARSE_ERROR_TRIGGER_MD.to_owned());
    db.set_file("story.ink", STORY.to_owned());
    db.set_entry("story.ink");

    let readme_id = db.file_id("README.md").expect("README.md loaded");
    let broken_id = db.file_id("broken.md").expect("broken.md loaded");

    for id in [readme_id, broken_id] {
        assert!(
            db.hir(id).is_none(),
            "a non-source document must have no HIR"
        );
        assert!(
            db.manifest(id).is_none(),
            "a non-source document must have no symbol manifest"
        );
        assert!(
            db.file_diagnostics(id).is_none(),
            "a non-source document must have no file_diagnostics"
        );
        assert!(
            db.admission_diagnostics(id).is_none(),
            "a non-source document must have no admission_diagnostics"
        );
        assert!(
            db.per_file_diagnostics(id).is_none_or(|d| d.is_empty()),
            "a non-source document must contribute no per_file_diagnostics"
        );
        assert!(
            db.diagnostics(id).is_none_or(<[_]>::is_empty),
            "a non-source document must contribute no diagnostics"
        );
    }
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
