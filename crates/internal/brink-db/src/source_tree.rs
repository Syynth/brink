//! The `SourceTree` seam (decision-log "Native source-loading seam: a
//! `SourceTree` trait with a map-backed impl; the root is caller-supplied",
//! 2026-07-22; issue #1278): a host-agnostic way to enumerate and read
//! native `.brink` source files.
//!
//! `InMemory` is `brink-web`'s discovery seam directly; the host-only
//! implementations (`RealFs`, `GitRev`) live in `brink-driver` and back
//! `brink_driver::discover_native` (issue #1288) — a normal native compile
//! and the `brink ide` git-baseline diff path, respectively.
//!
//! # The contract
//!
//! [`SourceTree::list`] enumerates every source key under a caller-supplied
//! `root`, **sorted deterministically by key** — never in filesystem/OS
//! iteration order, which is unspecified and can vary between runs. Keys are
//! root-relative (forward-slash-joined, matching how `.brink` module paths
//! are derived downstream). [`SourceTree::read`] reads the source text for a
//! key previously returned by `list`.
//!
//! The root itself is never discovered inside the seam (no implementation
//! walks upward looking for a project marker) — it is always supplied by the
//! caller, which resolves it however is appropriate for that host (a
//! `brink.toml` walk-up for the CLI, a pushed project root for web/LSP).

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

/// A source of `.brink` files: enumerate what exists under a root, and read
/// any key that enumeration returned.
///
/// See the [module docs](self) for the full contract. Implementations must
/// return `list()` results sorted by key, regardless of what order the
/// underlying storage (filesystem, git tree, in-memory map) happens to
/// iterate in.
pub trait SourceTree {
    /// Enumerate every source key under `root`, sorted deterministically by
    /// key.
    fn list(&self, root: &Path) -> io::Result<Vec<String>>;

    /// Read the source text for `key` (a key previously returned by
    /// [`list`](Self::list)).
    fn read(&self, key: &str) -> io::Result<String>;
}

/// Map-backed [`SourceTree`]: the test and web seam.
///
/// Built from a `BTreeMap<key, source>`, so `list()`'s sortedness falls out
/// of `BTreeMap`'s own ordering guarantee rather than an extra sort step —
/// the map stays sorted by key no matter what order entries were inserted
/// in.
#[derive(Debug, Clone, Default)]
pub struct InMemory {
    files: BTreeMap<String, String>,
}

impl InMemory {
    /// Build an in-memory `SourceTree` from a root-relative key → source map.
    #[must_use]
    pub fn new(files: BTreeMap<String, String>) -> Self {
        Self { files }
    }
}

impl SourceTree for InMemory {
    /// `root` is unused: an `InMemory` tree's keys are already root-relative
    /// by construction, so there is nothing further to scope by directory.
    fn list(&self, _root: &Path) -> io::Result<Vec<String>> {
        Ok(self.files.keys().cloned().collect())
    }

    fn read(&self, key: &str) -> io::Result<String> {
        self.files
            .get(key)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{key}: not found")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeding keys in a hostile (reverse-sorted) insertion order must not
    /// affect `list()`'s output — it always comes back key-sorted.
    #[test]
    fn in_memory_list_is_sorted_despite_hostile_reverse_insertion_order() {
        let mut files = BTreeMap::new();
        // Insert in reverse-sorted order.
        for key in ["c/z.brink", "b/m.brink", "a/a.brink"] {
            files.insert(key.to_string(), format!("-- {key} --"));
        }
        let tree = InMemory::new(files);

        let keys = tree.list(Path::new(".")).expect("list succeeds");

        assert_eq!(keys, vec!["a/a.brink", "b/m.brink", "c/z.brink"]);
    }

    /// Feeding keys in a hostile (shuffled, non-monotonic) insertion order
    /// must also not affect `list()`'s output.
    #[test]
    fn in_memory_list_is_sorted_despite_hostile_shuffled_insertion_order() {
        let mut files = BTreeMap::new();
        for key in ["m/mid.brink", "a/first.brink", "z/last.brink", "b/b.brink"] {
            files.insert(key.to_string(), format!("-- {key} --"));
        }
        let tree = InMemory::new(files);

        let keys = tree.list(Path::new(".")).expect("list succeeds");

        assert_eq!(
            keys,
            vec!["a/first.brink", "b/b.brink", "m/mid.brink", "z/last.brink"]
        );
    }

    /// `read()` returns exactly the source text a key was constructed with.
    #[test]
    fn in_memory_read_round_trips() {
        let mut files = BTreeMap::new();
        files.insert(
            "market/barter.brink".to_string(),
            "flow barter() {}".to_string(),
        );
        files.insert("main.brink".to_string(), "flow main() {}".to_string());
        let tree = InMemory::new(files);

        assert_eq!(
            tree.read("market/barter.brink").expect("key exists"),
            "flow barter() {}"
        );
        assert_eq!(
            tree.read("main.brink").expect("key exists"),
            "flow main() {}"
        );
    }

    /// Reading a key that was never inserted is a `NotFound` I/O error, not
    /// a panic — `InMemory` is a real `SourceTree`, not a test-only stub
    /// that can assume well-formed callers.
    #[test]
    fn in_memory_read_missing_key_is_not_found() {
        let tree = InMemory::new(BTreeMap::new());

        let err = tree.read("missing.brink").expect_err("key absent");

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// `list()` on an empty tree is `Ok(vec![])`, not an error.
    #[test]
    fn in_memory_list_empty_is_ok_empty() {
        let tree = InMemory::new(BTreeMap::new());

        assert_eq!(
            tree.list(Path::new(".")).expect("list succeeds"),
            Vec::<String>::new()
        );
    }
}
