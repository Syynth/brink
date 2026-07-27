//! BFS discovery of files reachable via INCLUDEs.

use std::collections::HashSet;
use std::io;

use brink_db::{ProjectDb, resolve_include_path};
use tracing::{debug, info};

/// Errors from file discovery.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    /// File I/O error during discovery.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// Circular INCLUDE dependency detected.
    #[error("circular INCLUDE: {0}")]
    CircularInclude(String),
    /// A native discovery key is not root-relative (contains a `..`
    /// segment). `native_module_path` treats `..` literally, so letting one
    /// through would mint a bogus module (`../x.brink` → `story::..::x`) —
    /// save-key-identity-critical (issue #1288 review note (a)). Every
    /// current `SourceTree` (`RealFs`, `GitRev`, `InMemory`) already
    /// produces root-relative, `..`-free keys; this guards against a future
    /// implementation that doesn't.
    #[error("source key `{0}` is not root-relative (contains `..`)")]
    InvalidKey(String),
    /// A key `discover_native` was handed does not have the `.brink`
    /// extension. `discover_native` must only ever see native source (issue
    /// #1371): `tree` is a `&dyn SourceTree`, and nothing at the type level
    /// stops a caller from handing it an implementation scoped wider than
    /// `.brink` alone — e.g. `brink_source_tree::InMemory`, the tree
    /// `brink-web`'s `compile()` builds (`.ink`-keyed) and hands to
    /// `brink_environment::Project::load`, not to native discovery — which
    /// would let `.ink` text be parsed as brink source. Checked (like
    /// [`InvalidKey`](Self::InvalidKey)) before
    /// any file is loaded, so a violation rejects the whole discovery, not
    /// just the offending key.
    #[error("source key `{0}` is not a native `.brink` file")]
    NonNativeKey(String),
}

/// Discover all files reachable via INCLUDEs from the entry point.
///
/// Performs BFS: reads each file, parses it via `db.set_file()`, then follows
/// its INCLUDEs. After all files are loaded, rebuilds the include graph and
/// checks for cycles.
pub fn discover<F>(db: &mut ProjectDb, entry: &str, read_file: &mut F) -> Result<(), DiscoverError>
where
    F: FnMut(&str) -> Result<String, io::Error>,
{
    let mut queue: Vec<String> = vec![entry.to_string()];
    let mut seen: HashSet<String> = HashSet::new();

    while let Some(path) = queue.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }

        let source = read_file(&path)?;
        let file_id = db.set_file(&path, source);

        // Discover INCLUDEs
        if let Some(hir) = db.hir(file_id) {
            for include in &hir.includes {
                // A bare `INCLUDE` (no path) lowers to an `IncludeSite` with
                // an empty `file_path` — the parser already flagged this as
                // E037 ("expected file path"). Reading the empty path here
                // would surface an `Io` error before that diagnostic ever
                // reaches the user, so skip it and let discovery continue.
                if include.file_path.is_empty() {
                    debug!(from = path, "skipping empty INCLUDE path (E037)");
                    continue;
                }
                let resolved = resolve_include_path(&path, &include.file_path);
                if !seen.contains(&resolved) {
                    debug!(from = path, include = resolved, "discovered INCLUDE");
                    queue.push(resolved);
                }
            }
        }
    }

    // Rebuild include graph now that all files are loaded
    db.rebuild_include_graph();

    // Detect circular includes
    if let Some(cycle) = db.find_cycle() {
        let names: Vec<_> = cycle.iter().filter_map(|id| db.file_path(*id)).collect();
        return Err(DiscoverError::CircularInclude(names.join(" -> ")));
    }

    info!(files = seen.len(), "discovery complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use brink_db::resolve_include_path;

    #[test]
    fn resolve_relative_include() {
        assert_eq!(
            resolve_include_path("src/main.ink", "utils.ink"),
            "src/utils.ink"
        );
    }

    #[test]
    fn resolve_no_directory() {
        assert_eq!(resolve_include_path("story.ink", "other.ink"), "other.ink");
    }

    #[test]
    fn resolve_nested_directory() {
        assert_eq!(
            resolve_include_path("story.ink", "lib/helpers.ink"),
            "lib/helpers.ink"
        );
    }

    #[test]
    fn resolve_parent_traversal_normalized() {
        // `..` collapses to a clean key so upward includes resolve to real
        // files (system-wide; see docs/decision-log.md).
        assert_eq!(resolve_include_path("a/b/c.ink", "../d.ink"), "a/d.ink");
    }

    #[test]
    fn resolve_deep_nesting() {
        assert_eq!(resolve_include_path("a/b/c.ink", "d/e.ink"), "a/b/d/e.ink");
    }

    /// #1504(b), reachable form: an editor session that admits an
    /// `INCLUDE` target before the entry file itself (`brink-lsp`'s
    /// `load_file_from_disk`, `backend.rs:624`, which can walk-and-load a
    /// sibling ahead of an explicit `did_open` on the entry) mints the
    /// entry a different numeric `FileId` than [`super::discover`] does —
    /// `discover` always seeds its BFS queue with the entry
    /// (`crate::discover::discover`, `discover.rs:51`), so a from-scratch
    /// compile always mints the entry `FileId(0)`. The synthesized root
    /// terminus is keyed by that numeric id
    /// (`attach_root_final_gather`, `brink-ir/src/lir/lower/mod.rs:1957`),
    /// not by anything content-derived, so the container-id set an
    /// editor-order load produces diverges from a real compile of the
    /// identical tree — the ink-mode sibling of the editor-vs-compile
    /// identity parity `discover_native.rs:349` already guards for native.
    #[test]
    #[ignore = "known bug #1504(b), reachable form: the root-terminus \
                DefinitionId is keyed by numeric FileId, so loading an \
                INCLUDE target before the entry file (editor/LSP order) \
                mints a different id space than a real `discover` compile \
                of the same tree; fix is blocked on the FG-4d identity \
                ruling"]
    fn root_content_ids_agree_between_discover_and_editor_order() {
        use std::collections::BTreeSet;
        use std::collections::HashMap;

        use brink_format::DefinitionId;

        const ENTRY: &str = "INCLUDE sibling.ink\n* one\n* two\n- gathered\n";
        const SIBLING: &str = "=== helper ===\nhelper text\n-> DONE\n";

        fn container_ids(container: &brink_ir::lir::Container, out: &mut BTreeSet<DefinitionId>) {
            out.insert(container.id);
            for child in &container.children {
                container_ids(child, out);
            }
        }

        fn ids_via(db: &brink_db::ProjectDb) -> BTreeSet<DefinitionId> {
            let mut out = BTreeSet::new();
            let program = db
                .lir_product()
                .and_then(|p| p.program.as_ref())
                .expect("lowering succeeds");
            container_ids(&program.root, &mut out);
            out
        }

        // (1) Compile order: `discover` seeds the BFS from the entry, so
        // `entry.ink` mints `FileId(0)`.
        let mut compiled = crate::Driver::new();
        let files: HashMap<&str, &str> = [("entry.ink", ENTRY), ("sibling.ink", SIBLING)].into();
        compiled
            .discover("entry.ink", |path: &str| {
                files
                    .get(path)
                    .map(|s| (*s).to_string())
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
            })
            .expect("discovery succeeds");
        compiled.db_mut().set_entry("entry.ink");

        // (2) Editor order: the sibling is admitted first (e.g. a workspace
        // walk, or an `INCLUDE` chased before the entry itself is opened) —
        // `entry.ink` mints `FileId(1)` instead.
        let mut edited = crate::Driver::new();
        edited.db_mut().set_file("sibling.ink", SIBLING.to_string());
        edited.db_mut().set_file("entry.ink", ENTRY.to_string());
        edited.db_mut().set_entry("entry.ink");

        assert_eq!(
            ids_via(edited.db()),
            ids_via(compiled.db()),
            "editor-order file registration must mint the SAME root-content \
             DefinitionIds a real `discover` compile of the same tree mints"
        );
    }
}
