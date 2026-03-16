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
    fn resolve_parent_traversal_not_normalized() {
        // No normalization — matches ink behavior
        assert_eq!(
            resolve_include_path("a/b/c.ink", "../d.ink"),
            "a/b/../d.ink"
        );
    }

    #[test]
    fn resolve_deep_nesting() {
        assert_eq!(resolve_include_path("a/b/c.ink", "d/e.ink"), "a/b/d/e.ink");
    }
}
