//! Issue #2111 (NS-T seam 1/6) — the serialized conventions projection
//! query and its invalidation. Exercised end-to-end through `ProjectDb`'s
//! salsa query layer, the same level `issue_1844_conventions_module_fence.rs`
//! tests the sibling `E169` confinement check at (and this suite reuses that
//! file's resolution fixtures deliberately, to prove the same pointer
//! resolves identically for both consumers).
//!
//! Per `ConventionsProjection`'s own doc (`brink-ir`), there is no
//! comptime-fault / last-good-value case to test here: the mechanism that
//! would have needed one (`fn conventions()` registration, issue #1840) is
//! dissolved (`docs/decision-log.md` 2026-08-03). What remains testable —
//! and is tested below — is the query SHAPE, its KEYING against the
//! `[project] conventions` pointer, and its INVALIDATION footprint —
//! **the resolved conventions module file plus its transitive `IMPORT`
//! closure** (finding 3's widened footprint; see
//! `editing_an_imported_struct_file_updates_the_projection` and the
//! two-hop/cyclic cases below), never the whole project.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_analyzer::AnalysisOptions;
use brink_db::ProjectDb;
use brink_ir::{ConventionAttachField, ConventionAttachSchema, ConventionMode, SchemaTypeShape};

const CLAIMING_HANDLER: &str = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\n\
    fn interior(place: content) {\n  return place;\n}\n";

// Issue #2264/`E186`: `block` and `attach = StructName` are now mutually
// exclusive on one handler — this fixture previously combined them (the
// exact silent-drop shape #2264 closed), which the new diagnostic now
// rejects outright. Kept as a pure `block` (Wrap-mode) handler with no
// `attach` clause; attach-mode + schema resolution is covered separately
// by `attach_resolves_across_an_imported_struct_file` and its neighbors
// below, none of which needed `block` for what they actually test.
const BLOCK_CLAIMING_HANDLER: &str = "@[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, block)]\n\
    fn cue(name: string, body: content) {\n  return name;\n}\n";

fn opts_with_conventions(pointer: &str) -> AnalysisOptions {
    AnalysisOptions {
        conventions: Some(pointer.to_owned()),
        ..AnalysisOptions::default()
    }
}

#[test]
fn unset_conventions_projects_to_empty() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(AnalysisOptions::default());
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

#[test]
fn a_preset_name_pointer_projects_to_empty_for_now() {
    // Mirrors `a_preset_name_pointer_never_fires` in the E169 confinement
    // suite: nothing resolves a bare preset name to its mounted source yet
    // (`brink_analyzer`'s own `BUILTIN_ELEMENT_PRESETS` doc — needs #1582's
    // pub marker and #2167's closure-scoped confinement, neither built).
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("screenplay"));
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

#[test]
fn an_unresolvable_conventions_pointer_projects_to_empty() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("typo.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

#[test]
fn the_configured_modules_own_handlers_are_projected_in_order() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{BLOCK_CLAIMING_HANDLER}{CLAIMING_HANDLER}"),
    );
    let projection = db.conventions_projection();
    let names: Vec<&str> = projection
        .entries
        .iter()
        .map(|e| e.name.text.as_str())
        .collect();
    // `cue` (order 5) before `interior` (order 10) — ascending order, not
    // declaration position (`cue` is declared second in the source above).
    assert_eq!(names, vec!["cue", "interior"], "{projection:?}");
    assert_eq!(projection.entries[0].mode, ConventionMode::Wrap);
    // Issue #2264/`E186`: `block` (Wrap mode, what `cue` declares here) and
    // `attach = StructName` are mutually exclusive on one handler now, so a
    // Wrap-mode entry's own `attach` is always `None` — schema resolution
    // for an `attach`-declaring handler is covered by
    // `attach_resolves_across_an_imported_struct_file` and its neighbors
    // below, none of which are Wrap mode.
    assert_eq!(projection.entries[0].attach, None);
    assert_eq!(projection.entries[1].mode, ConventionMode::Attach);
    assert_eq!(projection.entries[1].attach, None);
}

/// Issue #2352: `!name` sigil dispatch (`@[element(args = "…")]`) handlers
/// get a projection row too, through the real `ProjectDb` salsa query — not
/// just a hand-built `ConventionsProjection::from_decls` unit test. Before
/// this issue's fix, `conventions_projection_query` read only
/// `hir.claim_handlers`; a project with nothing BUT a `!name` handler
/// projected to a totally empty `ConventionsProjection` (RED: `dispatch`
/// didn't exist on the type at all). GREEN: the row is here, reachable
/// through the exact query the editor's `explain_match`/`classify_line`
/// consumers read.
#[test]
fn a_bang_dispatch_handler_in_the_configured_module_is_projected() {
    const DISPATCH_HANDLER: &str = "@[element(args = \"^(?<chan>[A-Z0-9-]+): (?<text>.+)$\")]\n\
        fn radio(chan, text) {\n  return text;\n}\n";
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{DISPATCH_HANDLER}flow main() {{\n  !radio TAC-2: All units report in.\n}}\n"),
    );
    let projection = db.conventions_projection();
    assert!(
        projection.entries.is_empty(),
        "no @[convention] handler was declared: {projection:?}"
    );
    assert_eq!(
        projection.dispatch.len(),
        1,
        "the one @[element] handler must get a row: {projection:?}"
    );
    let row = &projection.dispatch[0];
    assert_eq!(row.name.text, "radio");
    assert_eq!(
        row.dispatch_name.as_deref(),
        Some("radio"),
        "no `name = \"…\"` alias was declared, so the dispatch key is the fn's own name"
    );
    assert_eq!(row.pattern, "^(?<chan>[A-Z0-9-]+): (?<text>.+)$");
    assert_eq!(row.mode, ConventionMode::Attach);
    assert_eq!(
        row.attach, None,
        "`@[element]` has no `attach` clause — must never be Unresolved either"
    );
}

/// Issue #2352 review, "known limitation" finding: `!name` dispatch is
/// file-local at the LANGUAGE level (`hir::lower_native::element`'s own
/// module doc, "Deliberately not here"; `docs/prose-dialect-spec.md` §9;
/// `docs/diagnostics/E169.md`) — a `!name` handler declared in an ORDINARY
/// story file (not the configured conventions module) is a perfectly legal,
/// live handler there. But `conventions_projection_query` only ever reads
/// `dispatch_handlers` off the ONE configured conventions-module file, so
/// this handler contributes NO row to the projection at all — pinning that
/// gap rather than letting it silently regress or silently get "fixed" by
/// an incomplete patch that only widens `entries`' own cross-file
/// machinery (which doesn't apply here; dispatch has none).
#[test]
fn a_bang_dispatch_handler_declared_outside_the_conventions_module_projects_no_row() {
    const DISPATCH_HANDLER: &str = "@[element(args = \"^ready$\")]\n\
        fn tally() {\n  return \"ready\";\n}\n";
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    // The configured conventions module exists, but declares nothing.
    db.set_file("conventions.brink", String::new());
    // The `!name` handler — and the line that dispatches to it — live in an
    // ordinary story file instead. This is legal: dispatch carries no
    // confinement rule.
    db.set_file(
        "story.brink",
        format!("{DISPATCH_HANDLER}flow main() {{\n  !tally ready\n}}\n"),
    );
    let projection = db.conventions_projection();
    assert!(
        projection.dispatch.is_empty(),
        "known limitation (issue #2352, open ruling): the projection only \
         reads the conventions module's own file, so a handler declared \
         elsewhere — however legal — contributes no row: {projection:?}"
    );
}

/// A project's `@[convention]` and `@[element]` handlers coexist in the same
/// projection, in their own separate lists — issue #2352 adds `dispatch`
/// alongside `entries`, never merges the two (see
/// `ConventionsProjection::dispatch`'s own doc for why: a dispatch handler
/// has no `order` comparable to a claim handler's authored precedence).
///
/// The dispatch handler here is declared under an aliased name (`fn tally`
/// with `name = "walkie"`): `row.name.text` stays the declaration's own
/// function name (the jump-to-declaration anchor), but the only spelling an
/// author can write after `!` is the alias, `row.dispatch_name` — a review
/// finding on #2352 caught an earlier version of this test enshrining the
/// alias's loss by asserting only `name.text`.
#[test]
fn claim_and_dispatch_handlers_coexist_in_their_own_lists() {
    const DISPATCH_HANDLER: &str = "@[element(args = \"^ready$\", name = \"walkie\")]\n\
        fn tally() {\n  return \"ready\";\n}\n";
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}{DISPATCH_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n  !walkie ready\n}}\n"),
    );
    let projection = db.conventions_projection();
    assert_eq!(
        projection.entries.len(),
        1,
        "the one @[convention] handler: {projection:?}"
    );
    assert_eq!(projection.entries[0].name.text, "interior");
    assert_eq!(
        projection.dispatch.len(),
        1,
        "the one @[element] handler: {projection:?}"
    );
    assert_eq!(projection.dispatch[0].name.text, "tally");
    assert_eq!(
        projection.dispatch[0].dispatch_name.as_deref(),
        Some("walkie"),
        "the row must be findable under the author-writable `!walkie` spelling, \
         not just the fn's own name: {projection:?}"
    );
}

/// Issue #2111 finding 1 + finding 3: `attach = StructName` may legally name
/// a struct declared in an IMPORTED module, not only the conventions
/// module's own file — the schema still resolves, via the transitive
/// `IMPORT` closure.
#[test]
fn attach_resolves_across_an_imported_struct_file() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "schema.brink",
        "struct Cue {\n  speaker: string,\n  voiceover: bool,\n}\n".to_owned(),
    );
    db.set_file(
        "conventions.brink",
        "use story::schema::Cue;\n\
         @[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, attach = Cue)]\n\
         fn cue(name: string): Cue {\n  return Cue { speaker: name, voiceover: false };\n}\n"
            .to_owned(),
    );

    let projection = db.conventions_projection();
    assert_eq!(
        projection.entries[0].attach,
        Some(ConventionAttachSchema::Resolved {
            name: "Cue".to_string(),
            fields: vec![
                ConventionAttachField {
                    name: "speaker".to_string(),
                    ty: SchemaTypeShape::Named("string".to_string()),
                },
                ConventionAttachField {
                    name: "voiceover".to_string(),
                    ty: SchemaTypeShape::Named("bool".to_string()),
                },
            ],
        }),
        "{projection:?}"
    );
}

/// Issue #2111 finding 1 (review follow-up on PR #2931): `attach =
/// StructName` must also resolve when the struct is declared in the
/// conventions module's OWN file, not only through an `IMPORT`. Every other
/// `Resolved` case in this suite reaches its struct via `use story::…`,
/// which only proves the import-closure path; this proves the same-file
/// path — the plain entry-file walk inside `conventions_projection_query` —
/// independently.
#[test]
fn attach_resolves_against_a_struct_declared_in_the_conventions_module_itself() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "struct Cue {\n  speaker: string,\n}\n\
         @[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, attach = Cue)]\n\
         fn cue(name: string): Cue {\n  return Cue { speaker: name };\n}\n"
            .to_owned(),
    );

    let projection = db.conventions_projection();
    assert_eq!(
        projection.entries[0].attach,
        Some(ConventionAttachSchema::Resolved {
            name: "Cue".to_string(),
            fields: vec![ConventionAttachField {
                name: "speaker".to_string(),
                ty: SchemaTypeShape::Named("string".to_string()),
            }],
        }),
        "{projection:?} — Cue is declared in conventions.brink itself, not imported"
    );
}

/// Issue #2111 finding 3's review follow-up: the closure walk must be
/// genuinely TRANSITIVE, not just one hop deep. `conventions.brink` imports
/// `middle.brink`, which in turn imports `schema.brink` — `conventions.brink`
/// never names `schema.brink` directly, so this can only pass if
/// `import_closure_query` keeps walking past the first hop.
///
/// Per rule 20a, this is confirmed to actually exercise multi-hop traversal:
/// capping the walk at depth 1 (processing only the entry file's own
/// `hir.imports`, never a discovered file's) makes this test fail, because
/// `schema.brink` — and therefore `Cue`'s fields — never enters the closure.
#[test]
fn attach_resolves_transitively_through_a_two_hop_import_chain() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "schema.brink",
        "struct Cue {\n  speaker: string,\n  voiceover: bool,\n}\n".to_owned(),
    );
    db.set_file("middle.brink", "use story::schema::Cue;\n".to_owned());
    db.set_file(
        "conventions.brink",
        "use story::middle::Cue;\n\
         @[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, attach = Cue)]\n\
         fn cue(name: string): Cue {\n  return Cue { speaker: name, voiceover: false };\n}\n"
            .to_owned(),
    );

    let projection = db.conventions_projection();
    assert_eq!(
        projection.entries[0].attach,
        Some(ConventionAttachSchema::Resolved {
            name: "Cue".to_string(),
            fields: vec![
                ConventionAttachField {
                    name: "speaker".to_string(),
                    ty: SchemaTypeShape::Named("string".to_string()),
                },
                ConventionAttachField {
                    name: "voiceover".to_string(),
                    ty: SchemaTypeShape::Named("bool".to_string()),
                },
            ],
        }),
        "{projection:?} — schema.brink is two hops from conventions.brink \
         (via middle.brink); the closure walk must follow both hops"
    );
}

/// A cyclic import graph (`conventions.brink` <-> `schema.brink`, each
/// naming the other) must not hang `import_closure_query`'s traversal — the
/// `seen` set guards against revisiting a file, so the walk terminates and
/// still resolves `attach` correctly.
#[test]
fn a_cyclic_import_graph_terminates_and_still_resolves_attach() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "schema.brink",
        "use story::conventions::cue;\n\
         struct Cue {\n  speaker: string,\n}\n"
            .to_owned(),
    );
    db.set_file(
        "conventions.brink",
        "use story::schema::Cue;\n\
         @[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, attach = Cue)]\n\
         fn cue(name: string): Cue {\n  return Cue { speaker: name };\n}\n"
            .to_owned(),
    );

    // The assertion itself is secondary to this test simply COMPLETING —
    // a broken closure walk that chased the cycle without a `seen` guard
    // would hang here instead of returning.
    let projection = db.conventions_projection();
    assert_eq!(
        projection.entries[0].attach,
        Some(ConventionAttachSchema::Resolved {
            name: "Cue".to_string(),
            fields: vec![ConventionAttachField {
                name: "speaker".to_string(),
                ty: SchemaTypeShape::Named("string".to_string()),
            }],
        }),
        "{projection:?}"
    );
}

/// Issue #2111 finding 1: an `attach` name that resolves to no struct
/// anywhere in the conventions module's own file or its import closure is
/// flagged `Unresolved`, never silently dropped to `None`.
#[test]
fn attach_naming_a_nonexistent_struct_is_unresolved_not_dropped() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "@[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, attach = Ghost)]\n\
         fn cue(name: string): Ghost {\n  return Ghost { speaker: name };\n}\n"
            .to_owned(),
    );

    let projection = db.conventions_projection();
    assert_eq!(
        projection.entries[0].attach,
        Some(ConventionAttachSchema::Unresolved("Ghost".to_string()))
    );
}

/// Issue #2111 finding 3's invalidation half: editing a struct file that IS
/// in the conventions module's import closure must re-evaluate the
/// projection (a real content change to `attach`'s resolved fields), unlike
/// the "unrelated file" case above.
#[test]
fn editing_an_imported_struct_file_updates_the_projection() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "schema.brink",
        "struct Cue {\n  speaker: string,\n}\n".to_owned(),
    );
    db.set_file(
        "conventions.brink",
        "use story::schema::Cue;\n\
         @[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, attach = Cue)]\n\
         fn cue(name: string): Cue {\n  return Cue { speaker: name };\n}\n"
            .to_owned(),
    );

    let before = db.conventions_projection();
    assert_eq!(
        before.entries[0].attach,
        Some(ConventionAttachSchema::Resolved {
            name: "Cue".to_string(),
            fields: vec![ConventionAttachField {
                name: "speaker".to_string(),
                ty: SchemaTypeShape::Named("string".to_string()),
            }],
        })
    );

    // Add a field to the IMPORTED struct — not the conventions module
    // itself.
    db.update_file(
        "schema.brink",
        "struct Cue {\n  speaker: string,\n  voiceover: bool,\n}\n".to_owned(),
    );
    let after = db.conventions_projection();

    assert!(
        !Arc::ptr_eq(&before, &after),
        "editing an imported struct file (in the closure) must re-evaluate the projection"
    );
    assert_eq!(
        after.entries[0].attach,
        Some(ConventionAttachSchema::Resolved {
            name: "Cue".to_string(),
            fields: vec![
                ConventionAttachField {
                    name: "speaker".to_string(),
                    ty: SchemaTypeShape::Named("string".to_string()),
                },
                ConventionAttachField {
                    name: "voiceover".to_string(),
                    ty: SchemaTypeShape::Named("bool".to_string()),
                },
            ],
        }),
        "{after:?}"
    );
}

/// The other half of finding 3's invalidation contract: a file that is
/// neither the conventions module nor anywhere in its import closure must
/// never widen the footprint, even when the project DOES have an import
/// closure to speak of (distinguishing "closure-aware" from "everything is
/// now in the closure by accident").
#[test]
fn editing_a_file_outside_the_import_closure_never_reexecutes_the_projection() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "schema.brink",
        "struct Cue {\n  speaker: string,\n}\n".to_owned(),
    );
    db.set_file(
        "conventions.brink",
        "use story::schema::Cue;\n\
         @[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, attach = Cue)]\n\
         fn cue(name: string): Cue {\n  return Cue { speaker: name };\n}\n"
            .to_owned(),
    );
    db.set_file(
        "scenes/heading.brink",
        "flow main() {\n  A plain narrative line.\n}\n".to_owned(),
    );

    let before = db.conventions_projection();
    db.update_file(
        "scenes/heading.brink",
        "flow main() {\n  A DIFFERENT plain narrative line.\n}\n".to_owned(),
    );
    let after = db.conventions_projection();

    assert!(
        Arc::ptr_eq(&before, &after),
        "editing a file outside the import closure re-executed the conventions projection"
    );
}

/// A claiming handler declared OUTSIDE the configured conventions module
/// must never appear in the projection — the module is the SOLE source of
/// active claiming handlers (`docs/decision-log.md` 2026-08-03 "subtraction"
/// ruling), mirroring `E169` confinement's own "only the configured file's
/// declarations count" posture.
#[test]
fn a_claiming_handler_outside_the_configured_module_is_never_projected() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

/// The invalidation contract (`docs/decision-log.md` 2026-08-01 "Match
/// overlap… the projection is cached on its closure"): editing a file that
/// is NOT the conventions module must never re-execute the projection
/// query's closure. `Arc::ptr_eq`, not value equality, is the assertion —
/// see `fg3_dependency_edges.rs`'s own doc for why pointer identity is what
/// proves a memo was fully validated rather than recomputed.
#[test]
fn editing_an_unrelated_file_never_reexecutes_the_projection_query() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow other() {{\n  hi\n}}\n"),
    );
    db.set_file(
        "scenes/heading.brink",
        "flow main() {\n  A plain narrative line.\n}\n".to_owned(),
    );

    let before = db.conventions_projection();
    db.update_file(
        "scenes/heading.brink",
        "flow main() {\n  A DIFFERENT plain narrative line.\n}\n".to_owned(),
    );
    let after = db.conventions_projection();

    assert!(
        Arc::ptr_eq(&before, &after),
        "editing an unrelated file re-executed the conventions projection query (issue #2111)"
    );
}

/// The other half of the same contract: editing the conventions module
/// ITSELF must re-evaluate the projection — a real content change, not just
/// a re-execution with the same output.
#[test]
fn editing_the_conventions_module_itself_updates_the_projection() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow other() {{\n  hi\n}}\n"),
    );
    assert_eq!(db.conventions_projection().entries.len(), 1);

    db.update_file(
        "conventions.brink",
        format!("{BLOCK_CLAIMING_HANDLER}{CLAIMING_HANDLER}flow other() {{\n  hi\n}}\n"),
    );
    assert_eq!(db.conventions_projection().entries.len(), 2);
}
