//! Project grouping for native `.brink` files (issue #1562).
//!
//! `ProjectDb::compute_projects` is what every editor surface scopes itself
//! to — the LSP's background pass analyzes one project at a time, and
//! navigation (go-to-def, references, rename, hover) only ever sees the
//! files of the project the cursor's file belongs to. Grouping is by
//! `INCLUDE` reachability, which is the right rule for ink and a
//! catastrophic one for native: `.brink` has no `INCLUDE` (the module system
//! replaced it), so every native file used to come back as its own
//! single-file project and every cross-file editor feature broke.
//!
//! The native rule mirrors the one `compilation_closure_files` already
//! applies to codegen (decision-log "Native multi-file linking", 2026-07-23):
//! the discovered module set *is* the compilation unit, so every `.brink`
//! file in the db belongs to one project.

use brink_db::ProjectDb;

/// `market/barter.brink` — the definition side of a two-file native project.
const BARTER: &str = "\
var gold = 10

/// Trade at the market stall.
flow haggle() {
  You haggle over the price.
}
";

/// `main.brink` — the reference side. No `INCLUDE` anywhere: native reaches
/// across files through `use`, which is not a graph `compute_projects` ever
/// consulted.
const MAIN: &str = "\
use story::market::barter::haggle;

flow start() {
  The market is busy.
  -> haggle
}
";

/// The bug (#1562): two native files, zero `INCLUDE` edges, and the grouping
/// used to hand back two single-file projects.
#[test]
fn native_files_group_into_one_project_despite_having_no_includes() {
    let mut db = ProjectDb::new();
    db.set_file("main.brink", MAIN.to_owned());
    db.set_file("market/barter.brink", BARTER.to_owned());

    let projects = db.compute_projects();

    assert_eq!(
        projects.len(),
        1,
        "the native module tree is one compilation unit, so it is one \
         project — got {projects:?}",
    );
    let (root, members) = &projects[0];
    let member_paths: Vec<&str> = members
        .iter()
        .map(|&id| db.file_path(id).unwrap_or_default())
        .collect();
    assert_eq!(member_paths, ["main.brink", "market/barter.brink"]);
    assert_eq!(db.file_path(*root), Some("main.brink"));
}

/// The root of the native project is chosen by *path*, not by insertion
/// order: the LSP mints `FileId`s in `didOpen`/scan order, which varies from
/// session to session, and the root is user-visible (it keys the published
/// analysis and names the project in multi-project diagnostics).
#[test]
fn native_project_root_does_not_depend_on_insertion_order() {
    let mut reversed = ProjectDb::new();
    reversed.set_file("market/barter.brink", BARTER.to_owned());
    reversed.set_file("main.brink", MAIN.to_owned());

    let projects = reversed.compute_projects();

    assert_eq!(projects.len(), 1, "{projects:?}");
    let (root, members) = &projects[0];
    assert_eq!(
        reversed.file_path(*root),
        Some("main.brink"),
        "the lexicographically first native path is the root regardless of \
         which file was registered first",
    );
    assert_eq!(members.len(), 2, "{projects:?}");
}

/// Ink keeps its `INCLUDE` grouping untouched, and the two frontends do not
/// contaminate each other when they share one db (the LSP's real shape: a
/// workspace scan admits both extensions).
#[test]
fn ink_include_grouping_survives_alongside_a_native_tree() {
    let mut db = ProjectDb::new();
    db.set_file("story.ink", "INCLUDE chapter.ink\n-> start\n".to_owned());
    db.set_file("chapter.ink", "== start ==\nHello.\n-> DONE\n".to_owned());
    db.set_file("main.brink", MAIN.to_owned());
    db.set_file("market/barter.brink", BARTER.to_owned());
    db.rebuild_include_graph();

    let projects = db.compute_projects();
    let rendered: Vec<(Option<&str>, Vec<&str>)> = projects
        .iter()
        .map(|(root, members)| {
            (
                db.file_path(*root),
                members
                    .iter()
                    .map(|&id| db.file_path(id).unwrap_or_default())
                    .collect(),
            )
        })
        .collect();

    // Members come back in `FileId` order, so both lists are the insertion
    // order above.
    assert_eq!(
        rendered,
        vec![
            (Some("story.ink"), vec!["story.ink", "chapter.ink"]),
            (
                Some("main.brink"),
                vec!["main.brink", "market/barter.brink"],
            ),
        ],
        "the ink project stays its INCLUDE closure and the native files are \
         one project of their own",
    );
}
