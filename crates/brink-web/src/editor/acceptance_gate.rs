//! The editor acceptance gate (2026-08-06 decision-log ruling, "The editor
//! path gets its own CI-enforced acceptance gate").
//!
//! One canonical multi-file native project — the same content as
//! `packages/brink-studio/src/main.tsx`'s `NATIVE_FIXTURE` (`?fixture=native`)
//! — driven end-to-end through a real `EditorSession`, exactly as an
//! embedding host does: files loaded via `update_file`, project config
//! applied via `discover_project_config` (the #2324 `ProjectSession` seam),
//! stdlib mounted by `EditorSession::new` (#2231).
//!
//! ## Why this exists
//!
//! On 2026-08-05 the oracle ratchet held at 5,608 all day while the editor
//! path visibly regressed (the studio fixture went 1 → 3 → 7 diagnostics),
//! because the editor track had no measured invariant — the compiler had a
//! ratchet, the editor had nothing. Worse, the browser-based substitute was
//! itself broken: the playground never applied `brink.toml` at all (#2324),
//! so "verified in the studio" was measuring an instrument that never read
//! the input under test. This module is the replacement: Rust-level,
//! CI-enforced (it runs in the required `Test` lane via
//! `cargo test --workspace`), and asserting the *whole* contract rather
//! than the absence of specific known failure codes.
//!
//! ## Standing
//!
//! This gate has the same standing as `RATCHET_EPISODE_COUNT` in
//! `brink-test-harness`: if a change turns it red, the change broke the
//! editor contract — fix the change, never weaken the gate. Assertions here
//! may only be relaxed by a recorded ruling in `docs/decision-log.md`.
//! Extending the gate when new editor behavior is ruled (e.g. #2077's slug
//! captures, #2306's read-only library enforcement) is expected and
//! encouraged.
//!
//! ## What the fixture deliberately exercises
//!
//! - **Project-wide conventions claiming** (#2289): handlers live in their
//!   own `brink.toml`-named module and claim lines in *other* files.
//! - **Module-qualified diverts** (#2287): `-> barter::haggle` under a
//!   module-only `use`.
//! - **Stdlib name collisions** (#2318): the project's `Cue`/`cue`/`heading`
//!   coexist with the mounted `std/conventions/screenplay.brink`'s
//!   same-named declarations (peer roots, #2245).
//! - **Config discovery through the virtual mount** (#2324/#1414): the
//!   session learns everything from `brink.toml`, not from setters.
//! - **Slug- and tag-bearing headings claim** (#2077): `story.brink`'s
//!   heading spells an explicit `[market]` slug and a trailing `#act1`
//!   tag — before the 2026-08-06 ruling "Slug-bearing headings: strip
//!   structure, then match", either would have declined the claim and
//!   analyzed with an `E129` through the editor path, not just the CLI.

#![cfg(test)]

use super::EditorSession;

const BRINK_TOML: &str =
    "[project]\nentry = \"story.brink\"\nconventions = \"conventions.brink\"\n";

const CONVENTIONS: &str = "struct Cue {\n  \
   speaker: string,\n\
 }\n\n\
 @[convention(claims = \"^(?<name>[A-Z][A-Z '-]*)$\", attach = Cue, order = 10)]\n\
 fn cue(name: string): Cue {\n  \
   return Cue { speaker: name };\n\
 }\n\n\
 @[convention(claims = \"^(?<kind>INT|EXT)\\\\. (?<title>.+)$\", order = 20)]\n\
 fn heading(kind: string, title: string) {\n  \
   return \"-- {kind}. {title} --\";\n\
 }\n";

const STORY: &str = "use story::market::barter;\n\n\
 pub flow main() {\n  \
   INT. MARKET SQUARE - NIGHT [market] #act1\n  \
   The square is empty.\n\n  \
   VENDOR\n  \
   You shouldn't be here after dark.\n\n  \
   -> barter::haggle\n\
 }\n";

const BARTER: &str = "pub flow haggle() {\n  \
   KID\n  \
   How much for the lantern?\n  \
   -> DONE\n\
 }\n";

/// The canonical session: files loaded the way the editor's own file-loading
/// path loads them, config discovered from the entry the way `ProjectSession`
/// discovers it (#2324). Every test in this module starts here.
fn gate_session() -> EditorSession {
    let mut s = EditorSession::new();
    s.update_file("brink.toml", BRINK_TOML);
    s.update_file("conventions.brink", CONVENTIONS);
    s.update_file("story.brink", STORY);
    s.update_file("market/barter.brink", BARTER);
    s.discover_project_config("story.brink")
        .expect("brink.toml discovery from the entry file must succeed");
    s
}

/// Render every diagnostic from **both** analysis roads as
/// `road: path:offset [CODE] message` for failure output worth reading.
///
/// Two roads, deliberately (#1880's doc corrections record the split):
/// - `session.analysis()` — the off-db snapshot road
///   (`IdeSnapshot::analyze` → `analyze_with_modules`).
/// - `session.db().diagnostics(file)` — the db-direct road
///   (`per_file_diagnostics_query`), which is what the studio's Problems
///   panel actually renders.
///
/// Since issue #2335 BOTH roads run the E169 conventions confinement/
/// unconfigured checks (`analyze_with_modules` used to never read
/// `opts.conventions` at all) — see `gate_misplaced_convention_handler_
/// is_e169_on_both_roads` below for the divergence case that proves it.
///
/// The gate's own red-check proved reading only the first road is a trap:
/// with config discovery disabled, the off-db road stayed clean while the
/// db road carried the E169 misfires the studio showed live on 2026-08-05.
fn all_diagnostics(s: &EditorSession) -> Vec<String> {
    let analysis = s.session.analysis().expect("analysis available");
    let mut out: Vec<String> = analysis
        .diagnostics
        .iter()
        .map(|d| {
            format!(
                "analysis: {}:{} [{}] {}",
                s.session.file_path(d.file).unwrap_or("?"),
                u32::from(d.range.start()),
                d.code.as_str(),
                d.message
            )
        })
        .collect();

    // Every project file plus every mounted stdlib file, through the db road.
    let mut files: Vec<brink_ir::FileId> =
        ["conventions.brink", "story.brink", "market/barter.brink"]
            .iter()
            .map(|p| s.session.file_id(p).expect("fixture file loaded"))
            .collect();
    files.extend(s.mounted_std_ids.iter().copied());
    for id in files {
        let path = s.session.file_path(id).unwrap_or("?").to_owned();
        for d in s.session.db().diagnostics(id).into_iter().flatten() {
            out.push(format!(
                "db: {path}:{} [{}] {}",
                u32::from(d.range.start()),
                d.code.as_str(),
                d.message
            ));
        }
    }
    out
}

/// UTF-16 offset of `needle` in `haystack` — the offset convention every
/// `EditorSession` query takes. The fixture is pure ASCII, so byte offsets
/// and UTF-16 offsets coincide; the assert pins that assumption so a future
/// fixture edit with non-ASCII content fails loudly here instead of
/// producing off-by-N queries.
fn offset_of(haystack: &str, needle: &str) -> u32 {
    assert!(haystack.contains(needle), "fixture must contain {needle:?}");
    let byte = haystack.find(needle).expect("just asserted above");
    assert!(
        haystack.is_ascii(),
        "gate fixture must stay ASCII or offset_of must convert to UTF-16"
    );
    u32::try_from(byte).expect("fixture offsets fit u32")
}

/// The whole project — and the mounted stdlib alongside it — analyzes with
/// **zero** diagnostics of any severity. Not "no E169", not "no collision
/// codes": zero. If a future change legitimately introduces a diagnostic on
/// this fixture, that is a contract change and needs a ruling, not a looser
/// assertion.
#[test]
fn gate_the_canonical_project_analyzes_clean() {
    let s = gate_session();
    let diags = all_diagnostics(&s);
    assert!(
        diags.is_empty(),
        "the canonical native project must analyze clean through the editor \
         path (CLI already compiles it clean — divergence here is an \
         editor-path bug):\n{diags:#?}"
    );
}

// ── Divergence case (issue #2335): the off-db `analysis` road must agree ──
// with the db-direct road on a MISPLACED `@[convention]` handler, not just
// on the canonical (correctly-configured) fixture above. Same file names as
// `gate_session()` so `all_diagnostics`'s hardcoded db-road file list still
// applies — only `heading` moves out of `conventions.brink` and into
// `story.brink` itself, everything else byte-identical to the canonical
// fixture.

const CONVENTIONS_MISSING_HEADING: &str = "struct Cue {\n  \
   speaker: string,\n\
 }\n\n\
 @[convention(claims = \"^(?<name>[A-Z][A-Z '-]*)$\", attach = Cue, order = 10)]\n\
 fn cue(name: string): Cue {\n  \
   return Cue { speaker: name };\n\
 }\n";

const STORY_WITH_MISPLACED_HEADING: &str = "use story::market::barter;\n\n\
 @[convention(claims = \"^(?<kind>INT|EXT)\\\\. (?<title>.+)$\", order = 20)]\n\
 fn heading(kind: string, title: string) {\n  \
   return \"-- {kind}. {title} --\";\n\
 }\n\n\
 pub flow main() {\n  \
   INT. MARKET SQUARE - NIGHT [market] #act1\n  \
   The square is empty.\n\n  \
   VENDOR\n  \
   You shouldn't be here after dark.\n\n  \
   -> barter::haggle\n\
 }\n";

fn misplaced_handler_session() -> EditorSession {
    let mut s = EditorSession::new();
    s.update_file("brink.toml", BRINK_TOML);
    s.update_file("conventions.brink", CONVENTIONS_MISSING_HEADING);
    s.update_file("story.brink", STORY_WITH_MISPLACED_HEADING);
    s.update_file("market/barter.brink", BARTER);
    s.discover_project_config("story.brink")
        .expect("brink.toml discovery from the entry file must succeed");
    s
}

/// `heading` claims prose exactly like the canonical fixture, but is
/// declared directly in `story.brink` instead of the configured
/// `conventions.brink` — `E169` must fire on BOTH analysis roads. Before
/// issue #2335, the off-db `analysis` road silently passed this fixture
/// with zero diagnostics (`analyze_with_modules` never read
/// `opts.conventions`) while only the db-direct road caught it — exactly
/// the kind of two-roads divergence this gate exists to catch.
#[test]
fn gate_misplaced_convention_handler_is_e169_on_both_roads() {
    let s = misplaced_handler_session();
    let diags = all_diagnostics(&s);
    let e169: Vec<&String> = diags.iter().filter(|d| d.contains("[E169]")).collect();

    assert!(
        e169.iter().any(|d| d.starts_with("analysis:")),
        "a claim handler declared outside its configured conventions module \
         must be E169 on the off-db `analysis` road (issue #2335) — got {diags:#?}"
    );
    assert!(
        e169.iter().any(|d| d.starts_with("db:")),
        "a claim handler declared outside its configured conventions module \
         must still be E169 on the db-direct road — got {diags:#?}"
    );
}

/// Cross-file claiming (#2289) is live through the editor: handlers declared
/// in `conventions.brink` claim lines in `story.brink` AND
/// `market/barter.brink`. This is the assertion that was impossible to make
/// honestly before #1880 + #2324 — the projection was silently empty on this
/// path.
#[test]
fn gate_cross_file_claiming_is_live() {
    let mut s = gate_session();

    assert!(s.set_active_file("story.brink"));
    for (needle, expect_winner) in [("INT. MARKET SQUARE", "heading"), ("VENDOR", "cue")] {
        let v: serde_json::Value = serde_json::from_str(&s.explain_match(offset_of(STORY, needle)))
            .expect("explain_match returns valid JSON");
        assert_eq!(
            v["matched"],
            serde_json::json!(true),
            "{needle:?} must be claimed cross-file — got {v}"
        );
        assert_eq!(
            v["winner"]["handler"]["name"],
            serde_json::json!(expect_winner),
            "{needle:?} must be won by `{expect_winner}` — got {v}"
        );
    }

    // The second prose file: same handlers, different file — the part a
    // single-file fixture is structurally incapable of testing (#2289's
    // own hint records why).
    assert!(s.set_active_file("market/barter.brink"));
    let v: serde_json::Value = serde_json::from_str(&s.explain_match(offset_of(BARTER, "KID")))
        .expect("explain_match returns valid JSON");
    assert_eq!(
        v["matched"],
        serde_json::json!(true),
        "KID in the second file must be claimed by the same project-wide \
         conventions — got {v}"
    );
    assert_eq!(
        v["winner"]["handler"]["name"],
        serde_json::json!("cue"),
        "got {v}"
    );
}

/// The module-qualified divert (#2287) resolves across files through the
/// editor's navigation surface: goto-definition on `haggle` in
/// `-> barter::haggle` lands in `market/barter.brink`.
#[test]
fn gate_module_qualified_divert_navigates_across_files() {
    let mut s = gate_session();
    assert!(s.set_active_file("story.brink"));

    let offset = offset_of(STORY, "haggle"); // first occurrence: the divert
    let json = s.goto_definition(offset);
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(
        v["file"],
        serde_json::json!("market/barter.brink"),
        "goto-definition on the module-qualified divert target must land in \
         the defining module — got {json}"
    );
}

/// The project-wide symbol index reaches completions: a cross-file symbol
/// (`haggle`) and a same-file symbol (`main`) both appear. Flags like
/// `out_of_scope` are deliberately NOT asserted — import-affordance policy
/// is its own surface; the gate pins only that the index sees the whole
/// project.
#[test]
fn gate_project_wide_symbols_reach_completions() {
    let mut s = gate_session();
    assert!(s.set_active_file("story.brink"));

    let items: Vec<serde_json::Value> =
        serde_json::from_str(&s.completions(offset_of(STORY, "-> barter")))
            .expect("completions returns valid JSON");
    let names: Vec<&str> = items.iter().filter_map(|i| i["name"].as_str()).collect();
    for expected in ["main", "haggle"] {
        assert!(
            names.contains(&expected),
            "completions must see the project-wide index — `{expected}` \
             missing from {names:?}"
        );
    }
}

/// Folding on the canonical native project (#2291) decodes onto the
/// fixture's own real line numbers rather than merely being asserted
/// non-empty (#2280's own verification standard) — `STORY`'s
/// `pub flow main() { ... }` spans every line of the file's body, so its
/// structural fold's `end_line` pins the exact last content line.
///
/// This does **not** exercise the `is_native`-gated native-CST path this PR
/// added: structural folds come from `brink_ide::folding::folding_ranges`,
/// which reads only `hir`/`source`/`projection` (already dialect-correct
/// via `IdeSession::hir`/`projection`'s own `is_native` dispatch through
/// `lowered_query`) and never touches `syntax_root`/`syntax_root_native` —
/// the machinery/narrative fold-run pass this PR gated is off by default
/// (`fold_runs_enabled`) and this gate never opts in. That routing's own
/// reachability coverage lives in
/// `EditorSession`'s `native_folding_ranges_reach_the_native_cst_path` unit
/// test (`crates/brink-web/src/editor/mod.rs`), which enables
/// `set_fold_runs_enabled` and documents the same honest caveat: PR #2448
/// added a nested-structure test attempt, but confirmed that test's output
/// does not discriminate the routing (byte-identical either way). The genuine
/// routing coverage remains the two `brink-ide` block-comment tests, which
/// prove the wasm-facing entry point reaches the native path without
/// erroring (per #2291's reachability requirement).
#[test]
fn gate_folding_decodes_onto_real_native_line_numbers() {
    let mut s = gate_session();
    assert!(s.set_active_file("story.brink"));

    let ranges: serde_json::Value =
        serde_json::from_str(&s.folding_ranges()).expect("folding_ranges returns valid JSON");
    let array = ranges.as_array().expect("array");
    assert!(!array.is_empty(), "expected at least one fold: {ranges}");

    let last_line = u32::try_from(STORY.lines().count()).expect("fixture line count fits u32") - 1; // 0-based, the closing `}`
    assert!(
        array
            .iter()
            .any(|r| r["kind"] == "structural" && r["end_line"] == last_line),
        "the `pub flow main()` body must fold to its real closing brace \
         (line {last_line}) — a fold ending elsewhere means folding read \
         the wrong CST: {ranges}"
    );
}

/// The compile road (what the studio's debounced-compile lint and the Play
/// button actually run) produces a real artifact with zero warnings.
#[test]
fn gate_the_compile_road_produces_a_clean_artifact() {
    let mut s = gate_session();
    let result = s.compile_project("story.brink");
    let v: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
    assert_eq!(
        v["ok"],
        serde_json::json!(true),
        "compile must succeed: {result}"
    );
    assert!(v["error"].is_null(), "no compile error expected: {result}");
    let warnings = v["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.is_empty(),
        "the canonical project must compile with zero warnings on the \
         editor's compile road: {warnings:#?}"
    );
    assert!(
        v["story_bytes"].as_array().is_some_and(|b| !b.is_empty())
            || v["story_bytes"].as_str().is_some_and(|b| !b.is_empty()),
        "a real artifact must be produced: {result}"
    );
}
