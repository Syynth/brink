//! Native `.brink` project discovery (B0.10b, `docs/b0-sequencing.md`
//! §B0.10; charter §13.2 / NF-3).
//!
//! This is the native sibling of [`crate::discover`]'s `INCLUDE` BFS — a
//! **fresh, sorted, recursive filesystem walk**, deliberately *not* the
//! ink discovery machinery. The two frontends have fundamentally different
//! discovery models and share no code here:
//!
//! - **Ink** ([`crate::discover`]): a graph walk over `INCLUDE` edges,
//!   reachability-scoped to the entry file. Files are named by whatever path
//!   the includer wrote; `resolve_include_path`/`rebuild_include_graph`/
//!   `find_cycle` are its machinery.
//! - **Native** (here): "THE TREE IS THE COMPILATION UNIVERSE" — every
//!   `.brink` file under the declared source root is enumerated and loaded;
//!   there is no `INCLUDE`, no reachability closure, no cycle check. `use`
//!   grants source-visible names only (naming, not discovery). None of the
//!   ink INCLUDE helpers are reused.
//!
//! # Determinism (mandatory)
//!
//! `FileId`s are the substrate of `DefinitionId` save-key derivation, so their
//! assignment must be **deterministic across runs**, never dependent on OS
//! directory-iteration order (which `std::fs::read_dir` does not guarantee).
//! This walk collects every `.brink` path, **sorts by the project-relative
//! key**, and only then feeds `db.set_file` in that sorted order — so file `N`
//! in sorted order always gets `FileId(N)`, run after run, regardless of the
//! order the filesystem hands entries back. The relative key is also exactly
//! what the module-identity derivation consumes
//! (`brink-db::modules::native_module_path`).

use std::io;
use std::path::{Path, PathBuf};

use brink_db::ProjectDb;
use tracing::{debug, info};

use crate::discover::DiscoverError;

/// Discover every `.brink` file in the native project rooted at `entry`'s
/// declared source root, loading them into `db` in a deterministic,
/// sorted-by-relative-path order.
///
/// The source root is the directory containing `brink.toml` (found by walking
/// up from `entry`, via `brink-project-config`); with no `brink.toml`, it
/// falls back to `entry`'s own parent directory. Every discovered file's db
/// key is its path **relative to that root** (`market/barter.brink`), the
/// exact input the filesystem-derived module identity is keyed on.
///
/// Returns the set of loaded relative keys in sorted (== `FileId`) order — a
/// convenience for callers that need to map the entry back to its key.
pub fn discover_native(db: &mut ProjectDb, entry: &str) -> Result<Vec<String>, DiscoverError> {
    let entry_path = Path::new(entry);
    // Source root: the `brink.toml` directory (walk up from the entry), else
    // the entry file's own directory — a lone `.brink` with no project config
    // is still a (single-directory) project.
    let source_root = brink_project_config::find_source_root(entry_path).unwrap_or_else(|| {
        entry_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    });
    debug!(root = %source_root.display(), "native discovery: source root");

    // Recursively collect every `.brink` file under the root.
    let mut files: Vec<PathBuf> = Vec::new();
    collect_brink_files(&source_root, &mut files)?;

    // Key each by its project-relative path (normalized to `/`), then sort by
    // that key — the determinism guarantee. `FileId`s are then minted in this
    // exact order by `set_file`.
    let mut keyed: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|abs| {
            let rel = abs.strip_prefix(&source_root).unwrap_or(&abs);
            let key = rel.to_string_lossy().replace('\\', "/");
            (key, abs)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));

    let mut keys = Vec::with_capacity(keyed.len());
    for (key, abs) in keyed {
        let source = std::fs::read_to_string(&abs)?;
        db.set_file(&key, source);
        debug!(key, "native discovery: loaded");
        keys.push(key);
    }

    info!(files = keys.len(), "native discovery complete");
    Ok(keys)
}

/// Recursively collect every `.brink` file under `dir`, appending absolute (or
/// `dir`-relative, matching however `dir` was given) paths to `out`.
///
/// `read_dir` entries are consumed in whatever order the OS yields — the
/// determinism guarantee is enforced by the caller sorting the collected set,
/// not by the walk order here.
fn collect_brink_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), io::Error> {
    // A missing root is not fatal — an empty project loads nothing.
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_brink_files(&path, out)?;
        } else if file_type.is_file()
            && path.extension().is_some_and(|ext| ext == "brink")
        {
            out.push(path);
        }
    }
    Ok(())
}
