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
use crate::source_tree::is_native;

/// Discover a native project via `tree`: [`SourceTree::list`] (sorted,
/// root-relative keys, scoped to `tree`'s own constructor-held root — the
/// seam's own determinism contract), then [`SourceTree::read`] +
/// [`ProjectDb::set_file`] for each key **in that order** — `FileId`s mint
/// in sorted-key order, which is the whole point: the same tree contents,
/// discovered through any [`SourceTree`] impl in any insertion order,
/// always mint the same `FileId`s (and, since `native_module_path` is a
/// pure function of the path, the same module identity).
///
/// Every key is checked for a `..` segment and for a `.brink` extension
/// before any file is loaded, and discovery is rejected wholesale (no
/// partial load) if either check fails — see [`DiscoverError::InvalidKey`]
/// and [`DiscoverError::NonNativeKey`]. The extension guard exists because
/// `tree` is a `&dyn SourceTree`, not necessarily one scoped to `.brink`
/// alone, and nothing at the type level stops a wider-scoped tree from being
/// passed here by mistake (issue #1371). Rejecting a non-native key here,
/// rather than silently parsing `.ink` text as brink source, is the guard
/// that keeps the two discovery paths from crossing.
pub fn discover_native(db: &mut ProjectDb, tree: &dyn SourceTree) -> Result<(), DiscoverError> {
    let keys = tree.list()?;
    if let Some(bad) = keys.iter().find(|key| is_dotdot_polluted(key)) {
        return Err(DiscoverError::InvalidKey(bad.clone()));
    }
    if let Some(bad) = keys.iter().find(|key| !is_native(Path::new(key.as_str()))) {
        return Err(DiscoverError::NonNativeKey(bad.clone()));
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
/// in (see `brink_source_tree`'s crate docs).
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

        discover_native(&mut db, &t).expect("discovery succeeds");

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

        discover_native(&mut db, &t).expect("discovery succeeds");

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
        discover_native(&mut db_a, &t_a).expect("discovery succeeds");

        let mut db_b = ProjectDb::new();
        // Same files, built via a map that (were it not for `BTreeMap`'s own
        // ordering) would insert in a completely different order.
        let t_b = tree(&[
            ("nested/m.brink", "flow m() {}"),
            ("z.brink", "flow z() {}"),
            ("a.brink", "flow a() {}"),
        ]);
        discover_native(&mut db_b, &t_b).expect("discovery succeeds");

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

        discover_native(&mut db, &t).expect("discovery succeeds");

        assert_eq!(db.file_ids().count(), 1);
        assert!(db.file_id("main.brink").is_some());
    }

    /// An empty tree discovers as zero files, not an error.
    #[test]
    fn empty_tree_discovers_nothing() {
        let mut db = ProjectDb::new();
        let t = tree(&[]);

        discover_native(&mut db, &t).expect("discovery succeeds");

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
            fn list(&self) -> std::io::Result<Vec<String>> {
                Ok(vec!["a.brink".to_string(), "../escape.brink".to_string()])
            }
            fn read(&self, key: &str) -> std::io::Result<String> {
                Ok(format!("-- {key} --"))
            }
        }

        let mut db = ProjectDb::new();
        let err = discover_native(&mut db, &Hostile).expect_err("must be rejected");
        assert!(matches!(err, DiscoverError::InvalidKey(k) if k == "../escape.brink"));
        assert_eq!(
            db.file_ids().count(),
            0,
            "no partial load: a.brink must not have been set either"
        );
    }

    /// A `SourceTree` scoped wider than `.brink` alone — e.g. one that lists
    /// `.brink` + `.ink` for `brink-environment`'s `Project::load` (no
    /// implementation ever HANDED TO `discover_native` — `RealFs`, `GitRev`
    /// — lists wider than `.brink` since issue #1404 deleted `RealFs`'s
    /// second, wider list scope; but `brink_source_tree::InMemory`, the tree
    /// `brink-web`'s `compile()` builds and hands to
    /// `brink_environment::Project::load`, still lists arbitrary `.ink`
    /// keys today, and nothing at the type level stops it — or a future
    /// `discover_native` caller — from handing such a tree here instead) —
    /// must be rejected wholesale, before any file is loaded, if it is ever
    /// handed to `discover_native` instead. This is the #1371 guard:
    /// `discover_native` itself must refuse any non-`.brink` key rather than
    /// silently parsing `.ink`/other text as brink source. The fixture below
    /// also throws in a `brink.toml`-named key to prove the guard rejects
    /// *any* non-`.brink` key generically, not just `.ink`.
    #[test]
    fn non_native_key_is_rejected_before_any_load() {
        struct WiderThanNative;
        impl SourceTree for WiderThanNative {
            fn list(&self) -> std::io::Result<Vec<String>> {
                Ok(vec![
                    "a.brink".to_string(),
                    "brink.toml".to_string(),
                    "main.ink".to_string(),
                ])
            }
            fn read(&self, key: &str) -> std::io::Result<String> {
                Ok(format!("-- {key} --"))
            }
        }

        let mut db = ProjectDb::new();
        let err = discover_native(&mut db, &WiderThanNative).expect_err("must be rejected");
        assert!(matches!(err, DiscoverError::NonNativeKey(k) if k == "brink.toml"));
        assert_eq!(
            db.file_ids().count(),
            0,
            "no partial load: a.brink must not have been set either"
        );
    }

    /// EDITOR-VS-COMPILE IDENTITY PARITY (issue #1572).
    ///
    /// `native_module_path` is contractually a function of a **root-relative**
    /// key, and `discover_native` above is the only loader that produces such
    /// keys. A long-lived editor session cannot: `brink-lsp` keys `ProjectDb`
    /// by **absolute OS path**, because every path it holds round-trips
    /// through a `file://` URI. Before #1572 that meant every native module
    /// name (and therefore every `DefinitionId`) the editor minted embedded
    /// the machine's directory layout and silently diverged from a real
    /// compile of the very same tree — self-consistent, but fatal to any
    /// save-key, `@[was]`-migration, or editor-vs-compile comparison.
    ///
    /// So: run a **real compile** of a real on-disk tree through the real
    /// [`RealFs`] seam, register the same tree the way the LSP does
    /// (absolute keys + `set_native_root`), and require the two to mint
    /// byte-identical module names *and* `DefinitionId`s. The third database
    /// — absolute keys with **no** declared root, i.e. the pre-#1572 editor —
    /// pins that this is not vacuous: it must still diverge.
    #[test]
    fn absolute_keys_plus_native_root_mint_compile_identical_identity() {
        use std::fs;

        use crate::source_tree::RealFs;

        const FILES: [(&str, &str); 3] = [
            ("main.brink", "flow start() {\n  The market is busy.\n}\n"),
            (
                "market/barter.brink",
                "flow haggle() {\n  You haggle over the price.\n}\n",
            ),
            (
                "npcs/quests/intro.brink",
                "flow intro() {\n  A stranger waves.\n}\n",
            ),
        ];

        let root = temp_dir("native-identity-parity");
        for (key, source) in FILES {
            let path = root.join(key);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create fixture dir");
            }
            fs::write(&path, source).expect("write fixture file");
        }

        // (1) The real compile: root-relative keys, straight off disk.
        let mut compiled = native_db();
        discover_native(&mut compiled, &RealFs::new(&root)).expect("discovery succeeds");

        // (2) The editor: absolute keys, root declared.
        let mut editor = native_db();
        editor.set_native_root(Some(root.to_string_lossy().into_owned()));
        register_absolute(&mut editor, &root, &FILES);

        // (3) The pre-#1572 editor: absolute keys, no root declared.
        let mut drifted = native_db();
        register_absolute(&mut drifted, &root, &FILES);

        let compiled_identity = flow_identity(&compiled);
        let editor_identity = flow_identity(&editor);
        let drifted_identity = flow_identity(&drifted);

        fs::remove_dir_all(&root).expect("clean up fixture tree");

        // Sanity: the compile really did derive path-shaped module names, so
        // the equality below is comparing something meaningful.
        assert_eq!(
            compiled_identity
                .get("haggle")
                .map(|(module, _)| module.as_str()),
            Some("story::market::barter"),
            "compile-side module identity, got {compiled_identity:?}"
        );

        assert_eq!(
            editor_identity, compiled_identity,
            "an editor keying by absolute path must mint the SAME native module \
             names and `DefinitionId`s a real compile of the same tree mints"
        );
        assert_ne!(
            drifted_identity, compiled_identity,
            "guard against a vacuous test: without a declared native root the \
             absolute-keyed database must still diverge, got {drifted_identity:?}"
        );
    }

    /// A `ProjectDb` under the analysis posture the native harness uses.
    fn native_db() -> ProjectDb {
        let mut db = ProjectDb::new();
        db.set_analysis_options(brink_analyzer::AnalysisOptions {
            dialect: brink_analyzer::Dialect::Brink,
            ..brink_analyzer::AnalysisOptions::default()
        });
        db
    }

    /// Register `files` (root-relative key + source) under their **absolute**
    /// paths beneath `root` — exactly how `brink-lsp` admits a workspace file.
    fn register_absolute(db: &mut ProjectDb, root: &Path, files: &[(&str, &str)]) {
        for (key, source) in files {
            let path = root.join(key);
            db.set_file(&path.to_string_lossy(), (*source).to_string());
        }
    }

    /// Flow name → (declaring module, `DefinitionId`) for every knot symbol in
    /// `db`'s analysis — the identity pair a save key is built from.
    fn flow_identity(db: &ProjectDb) -> BTreeMap<String, (String, brink_format::DefinitionId)> {
        let index = db.symbol_index();
        index
            .symbols
            .iter()
            .filter(|(_, s)| s.kind == brink_ir::SymbolKind::Knot)
            .filter_map(|(id, s)| Some((s.name.clone(), (s.module.clone()?, *id))))
            .collect()
    }

    /// A fresh, empty temp directory, unique per call (pid + a monotonic
    /// counter + a nanosecond timestamp) so parallel test runs never collide.
    /// Mirrors `source_tree`'s own test helper — both are `#[cfg(test)]`
    /// module-private, so neither can borrow the other's.
    fn temp_dir(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "brink-discover-native-test-{label}-{}-{n}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
