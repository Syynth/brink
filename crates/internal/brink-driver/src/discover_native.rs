//! Native (`.brink`) discovery: the "tree is the compilation universe" walk
//! (charter §13.2, decision-log 2026-07-22 "Native source-loading seam").
//!
//! Unlike ink's [`discover`](crate::discover::discover), there is no
//! `INCLUDE` graph to follow — a native project's whole file set *is* its
//! compilation universe, so discovery is just "enumerate, then load every
//! key," not a BFS from an entry point.

use std::path::Path;

use brink_db::{ProjectDb, SourceTree};
use tracing::info;

use crate::discover::DiscoverError;

/// Discover a native project via `tree`: [`SourceTree::list`] under `root`
/// (sorted, root-relative keys — the seam's own determinism contract), then
/// [`SourceTree::read`] + [`ProjectDb::set_file`] for each key **in that
/// order** — `FileId`s mint in sorted-key order, which is the whole point:
/// the same tree contents, discovered through any [`SourceTree`] impl in any
/// insertion order, always mint the same `FileId`s (and, since
/// `native_module_path` is a pure function of the path, the same module
/// identity).
///
/// Every key is checked for a `..` segment before any file is loaded, and
/// discovery is rejected wholesale (no partial load) if one is found — see
/// [`DiscoverError::InvalidKey`].
pub fn discover_native(
    db: &mut ProjectDb,
    tree: &dyn SourceTree,
    root: &Path,
) -> Result<(), DiscoverError> {
    let keys = tree.list(root)?;
    if let Some(bad) = keys.iter().find(|key| is_dotdot_polluted(key)) {
        return Err(DiscoverError::InvalidKey(bad.clone()));
    }

    let count = keys.len();
    for key in keys {
        let source = tree.read(&key)?;
        db.set_file(&key, source);
    }

    info!(files = count, "native discovery complete");
    Ok(())
}

/// A key is `..`-polluted if any `/`-separated segment is exactly `..` —
/// the format every [`SourceTree`] key is contractually forward-slash-joined
/// in (see `brink_db::source_tree`'s module docs).
fn is_dotdot_polluted(key: &str) -> bool {
    key.split('/').any(|segment| segment == "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use brink_db::InMemory;

    fn tree(files: &[(&str, &str)]) -> InMemory {
        InMemory::new(
            files
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    /// A 3-file tree loads all 3 files, keyed root-relative, discoverable
    /// afterward by exactly the keys `list()` returned.
    #[test]
    fn loads_all_files_root_relative_keyed() {
        let mut db = ProjectDb::new();
        let t = tree(&[
            ("z.brink", "flow z() {}"),
            ("a.brink", "flow a() {}"),
            ("nested/b.brink", "flow b() {}"),
        ]);

        discover_native(&mut db, &t, Path::new(".")).expect("discovery succeeds");

        let mut paths: Vec<_> = db.file_ids().filter_map(|id| db.file_path(id)).collect();
        paths.sort_unstable();
        assert_eq!(paths, vec!["a.brink", "nested/b.brink", "z.brink"]);
    }

    /// `FileId`s mint in sorted-key order, not discovery/insertion order —
    /// the determinism guarantee `SourceTree::list`'s sortedness contract
    /// exists to provide.
    #[test]
    fn file_ids_mint_in_sorted_key_order() {
        let mut db = ProjectDb::new();
        // Insertion order (as given to `InMemory::new`/`BTreeMap`) does not
        // matter — `tree()` builds a `BTreeMap`, which is already sorted;
        // what's under test is that `discover_native` walks the *returned*
        // (sorted) key list in order, minting FileIds as it goes.
        let t = tree(&[
            ("z.brink", "flow z() {}"),
            ("a.brink", "flow a() {}"),
            ("m.brink", "flow m() {}"),
        ]);

        discover_native(&mut db, &t, Path::new(".")).expect("discovery succeeds");

        let a = db.file_id("a.brink").expect("a.brink discovered");
        let m = db.file_id("m.brink").expect("m.brink discovered");
        let z = db.file_id("z.brink").expect("z.brink discovered");
        assert!(
            a.0 < m.0 && m.0 < z.0,
            "FileIds must mint in sorted-key order: a={}, m={}, z={}",
            a.0,
            m.0,
            z.0
        );
    }

    /// Determinism (hermetic via `InMemory`): the same file set, discovered
    /// through two separately-built trees with a hostile (non-alphabetical)
    /// insertion order, must yield identical `FileId`s *and* identical
    /// derived module paths for every key — save-key identity must not
    /// depend on discovery order.
    #[test]
    fn hostile_key_order_yields_identical_file_ids_and_module_paths() {
        let mut db_a = ProjectDb::new();
        let t_a = tree(&[
            ("z.brink", "flow z() {}"),
            ("a.brink", "flow a() {}"),
            ("nested/m.brink", "flow m() {}"),
        ]);
        discover_native(&mut db_a, &t_a, Path::new(".")).expect("discovery succeeds");

        let mut db_b = ProjectDb::new();
        // Same files, built via a map that (were it not for `BTreeMap`'s own
        // ordering) would insert in a completely different order.
        let t_b = tree(&[
            ("nested/m.brink", "flow m() {}"),
            ("z.brink", "flow z() {}"),
            ("a.brink", "flow a() {}"),
        ]);
        discover_native(&mut db_b, &t_b, Path::new(".")).expect("discovery succeeds");

        for key in ["a.brink", "nested/m.brink", "z.brink"] {
            let id_a = db_a.file_id(key).expect("{key} in db_a");
            let id_b = db_b.file_id(key).expect("{key} in db_b");
            assert_eq!(id_a, id_b, "FileId for {key} must match across trees");
        }

        // Module identity: every declared knot's qualified module name must
        // match between the two databases too (proving the identity isn't
        // silently keyed off `FileId`, only off the path).
        let modules_a = knot_modules(&db_a);
        let modules_b = knot_modules(&db_b);
        assert_eq!(
            modules_a, modules_b,
            "derived module paths must match regardless of discovery order"
        );
        assert_eq!(
            modules_a.get("m").map(String::as_str),
            Some("story::nested::m"),
            "sanity: nested/m.brink's module must be story::nested::m"
        );
    }

    /// Knot name → declaring module, for every knot symbol in `db`'s
    /// analysis. Used only to compare module identity across two databases.
    fn knot_modules(db: &ProjectDb) -> BTreeMap<String, String> {
        db.analysis()
            .index
            .symbols
            .values()
            .filter(|s| s.kind == brink_ir::SymbolKind::Knot)
            .filter_map(|s| Some((s.name.clone(), s.module.clone()?)))
            .collect()
    }

    /// A single-key tree with no directory structure still discovers.
    #[test]
    fn single_file_discovers() {
        let mut db = ProjectDb::new();
        let t = tree(&[("main.brink", "flow main() {}")]);

        discover_native(&mut db, &t, Path::new(".")).expect("discovery succeeds");

        assert_eq!(db.file_ids().count(), 1);
        assert!(db.file_id("main.brink").is_some());
    }

    /// An empty tree discovers as zero files, not an error.
    #[test]
    fn empty_tree_discovers_nothing() {
        let mut db = ProjectDb::new();
        let t = tree(&[]);

        discover_native(&mut db, &t, Path::new(".")).expect("discovery succeeds");

        assert_eq!(db.file_ids().count(), 0);
    }

    /// A `SourceTree` that (contrary to the contract) returns a `..`-polluted
    /// key must be rejected wholesale, before any file is loaded — a
    /// save-key-identity guardrail (issue #1288 review note (a)), not a
    /// scenario any current `SourceTree` impl can trigger.
    #[test]
    fn dotdot_key_is_rejected_before_any_load() {
        struct Hostile;
        impl SourceTree for Hostile {
            fn list(&self, _root: &Path) -> std::io::Result<Vec<String>> {
                Ok(vec!["a.brink".to_string(), "../escape.brink".to_string()])
            }
            fn read(&self, key: &str) -> std::io::Result<String> {
                Ok(format!("-- {key} --"))
            }
        }

        let mut db = ProjectDb::new();
        let err = discover_native(&mut db, &Hostile, Path::new(".")).expect_err("must be rejected");
        assert!(matches!(err, DiscoverError::InvalidKey(k) if k == "../escape.brink"));
        assert_eq!(
            db.file_ids().count(),
            0,
            "no partial load: a.brink must not have been set either"
        );
    }
}
