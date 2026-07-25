//! The shared recursive directory walk (issue #1433).
//!
//! [`crate::is_ignored_dir`] has existed since #1402, but only as a
//! *predicate*: every recursive traversal in the workspace still wrote its
//! own `read_dir` recursion and had to remember to call the predicate at the
//! right moment. It didn't — five separate issues (#1370, #1381, #1402,
//! #1415, #1424) each fixed one traversal that had skipped the check,
//! because nothing structural made pruning the default. The predicate was
//! never the hard part; remembering to call it was.
//!
//! [`Walk`] closes that by applying the policy **by construction**: it is
//! the only recursive `read_dir` loop in the workspace's library code, so a
//! *new* traversal is pruned the moment it is written, with nothing to
//! remember. It deliberately offers **no unpruned mode** — the escape hatch
//! and gitignore-awareness questions are issue #1407's, not this seam's, so
//! there is currently no way to ask [`Walk`] to descend into `target/`,
//! `.git/` or `node_modules/` at all.
//!
//! This is host-side code (it touches the real filesystem), sitting here
//! rather than in `brink-driver` because it is the enforcement half of a
//! policy this crate already owns. Like `RealFs`/`GitRev`, it is never
//! *constructed* on a wasm-reachable path — the crate link is not the
//! constraint (see the [module docs](crate)).

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::is_ignored_dir;

/// One entry yielded by a [`Walk`]: a path plus the file type the directory
/// listing reported for it.
///
/// The file type is the one from [`fs::DirEntry::file_type`], which does
/// **not** follow symlinks — a symlink is neither [`is_dir`](Self::is_dir)
/// nor [`is_file`](Self::is_file), so a symlinked directory is yielded as a
/// plain entry and never descended into. That bounds the walk: a symlink
/// cycle cannot make it run forever (CLAUDE.md's guard-against-unbounded-
/// growth rule).
#[derive(Debug)]
pub struct WalkEntry {
    path: PathBuf,
    file_type: fs::FileType,
}

impl WalkEntry {
    /// The entry's full path — the walk's root joined with everything
    /// descended through to reach it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Consume the entry for its path, avoiding a clone when the path is all
    /// the caller wanted.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// The entry's own file name (its last path component).
    #[must_use]
    pub fn file_name(&self) -> &OsStr {
        self.path.file_name().unwrap_or_else(|| OsStr::new(""))
    }

    /// Whether this entry is a directory the walk will descend into.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    /// Whether this entry is a regular file. False for symlinks — see
    /// [`WalkEntry`]'s own doc.
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.file_type.is_file()
    }

    /// The unfollowed file type the directory listing reported.
    #[must_use]
    pub fn file_type(&self) -> fs::FileType {
        self.file_type
    }
}

/// A stack slot: either a directory still to be expanded, or an item ready
/// to be yielded.
#[derive(Debug)]
enum Pending {
    Descend(PathBuf),
    Item(io::Result<WalkEntry>),
}

/// Recursive directory walk that prunes [`crate::IGNORED_DIR_NAMES`]
/// (`target/`, `.git/`, `node_modules/`) **by construction** — there is no
/// way to construct one that descends into them (issue #1433; see the
/// [module docs](self)).
///
/// # Contract
///
/// - **Pre-order, depth-first**: a directory is yielded before its contents.
/// - **Deterministic**: entries within each directory are visited sorted by
///   file name, never in filesystem iteration order, which is unspecified
///   and varies between runs (CLAUDE.md's determinism rule). Note that a
///   pre-order traversal of per-directory-sorted entries is *not* the same
///   as a globally sorted list of paths (`a.brink` sorts before `a/z.brink`,
///   but the walk yields `a/` and its contents first) — a caller that needs
///   globally sorted output sorts the collected result itself.
/// - **The root is never pruned**: the policy is applied to entries found
///   *while descending*, never to the root the caller handed in, so a
///   workspace legitimately rooted at e.g. `node_modules/vendor-ink` still
///   walks its own contents (issue #1424). The root itself is not yielded.
/// - **A pruned directory is neither yielded nor descended into.**
/// - **Errors are per-item, and the walk continues**: an unreadable
///   directory or entry yields one `Err` and the traversal moves on to the
///   next branch, so a caller can choose between propagating (`?` in a
///   `Result`-returning function) and skipping (`.flatten()`).
///
/// # Examples
///
/// ```
/// # use brink_source_tree::Walk;
/// # fn demo(root: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
/// let mut inks = Vec::new();
/// for entry in Walk::new(root) {
///     let entry = entry?;
///     if entry.is_file() && entry.path().extension().is_some_and(|e| e == "ink") {
///         inks.push(entry.into_path());
///     }
/// }
/// # Ok(inks)
/// # }
/// ```
#[derive(Debug)]
pub struct Walk {
    stack: Vec<Pending>,
    also_pruned: Vec<OsString>,
}

impl Walk {
    /// Start a pruned walk rooted at `root`. The
    /// [`IGNORED_DIR_NAMES`](crate::IGNORED_DIR_NAMES) policy applies with
    /// no opt-in and no opt-out.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            stack: vec![Pending::Descend(root.into())],
            also_pruned: Vec::new(),
        }
    }

    /// Prune these additional directory names on top of the standing policy
    /// — strictly narrowing, never widening (there is no way to un-prune an
    /// [`IGNORED_DIR_NAMES`](crate::IGNORED_DIR_NAMES) entry). For callers
    /// with a fixture-layout convention of their own, e.g. the test
    /// harness's `oracle/`/`episodes/` case directories.
    #[must_use]
    pub fn prune_also<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.also_pruned.extend(names.into_iter().map(Into::into));
        self
    }

    /// Whether a directory named `name`, found while descending, is pruned.
    fn is_pruned(&self, name: &OsStr) -> bool {
        is_ignored_dir(name) || self.also_pruned.iter().any(|pruned| pruned == name)
    }

    /// List `dir`'s entries, sorted by file name, with pruned directories
    /// already dropped. A failure to read the directory itself is one error
    /// for the whole directory; a failure to stat a single entry is an error
    /// for that entry alone.
    fn children(&self, dir: &Path) -> io::Result<Vec<Pending>> {
        let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let mut pending = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry.file_type() {
                Ok(file_type) => {
                    if file_type.is_dir() && self.is_pruned(&entry.file_name()) {
                        continue;
                    }
                    pending.push(Pending::Item(Ok(WalkEntry {
                        path: entry.path(),
                        file_type,
                    })));
                }
                Err(err) => pending.push(Pending::Item(Err(err))),
            }
        }
        Ok(pending)
    }
}

impl Iterator for Walk {
    type Item = io::Result<WalkEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.stack.pop()? {
                Pending::Item(Ok(entry)) => {
                    if entry.is_dir() {
                        // Descend before the remaining siblings on the stack
                        // — that is what makes this pre-order.
                        self.stack.push(Pending::Descend(entry.path.clone()));
                    }
                    return Some(Ok(entry));
                }
                Pending::Item(Err(err)) => return Some(Err(err)),
                Pending::Descend(dir) => match self.children(&dir) {
                    Ok(children) => self.stack.extend(children.into_iter().rev()),
                    Err(err) => return Some(Err(err)),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A fresh, empty temp directory, unique per call (pid + counter +
    /// nanoseconds) so parallel test runs never collide. Mirrors the same
    /// helper in `brink-driver`'s `source_tree` tests — this crate is an L0
    /// leaf with no dev-dependencies.
    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "brink-walk-test-{label}-{}-{n}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// Collect a walk's entries as root-relative `/`-joined strings, so
    /// assertions read as a literal expected traversal.
    fn relative(root: &Path, walk: Walk) -> Vec<String> {
        walk.map(|entry| {
            let entry = entry.expect("entry reads");
            entry
                .path()
                .strip_prefix(root)
                .expect("entry is under root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect()
    }

    fn write(path: PathBuf, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    /// The whole point of the helper: a walk written with no prune code of
    /// its own still prunes every [`crate::IGNORED_DIR_NAMES`] directory,
    /// at any depth, including files sitting directly inside one.
    #[test]
    fn walk_prunes_ignored_dirs_by_construction() {
        let root = temp_dir("prune");
        write(root.join("main.ink"), "main");
        write(root.join("target/stray.ink"), "stray");
        write(root.join("target/debug/build.ink"), "build");
        write(root.join(".git/HEAD"), "ref");
        write(root.join(".git/objects/pack.ink"), "pack");
        write(root.join("node_modules/pkg/index.ink"), "pkg");
        write(root.join("src/nested/deep/target/out.ink"), "deep");
        write(root.join("src/nested/deep/keep.ink"), "keep");

        assert_eq!(
            relative(&root, Walk::new(&root)),
            vec![
                "main.ink",
                "src",
                "src/nested",
                "src/nested/deep",
                "src/nested/deep/keep.ink",
            ],
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// Pruning is name-equality on the directory entry, matching
    /// [`crate::is_ignored_dir`] — a directory whose name merely starts with
    /// an ignored name is walked normally, and a *file* named `target` is
    /// yielded rather than skipped.
    #[test]
    fn walk_prunes_by_exact_directory_name_only() {
        let root = temp_dir("prune-exact");
        write(root.join("targets/a.ink"), "a");
        write(root.join("target.brink"), "not a dir");
        write(root.join("my-node_modules/b.ink"), "b");

        assert_eq!(
            relative(&root, Walk::new(&root)),
            vec![
                "my-node_modules",
                "my-node_modules/b.ink",
                "target.brink",
                "targets",
                "targets/a.ink",
            ],
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// The root is never tested against the policy (issue #1424): a walk
    /// rooted *at* an ignored-named directory still enumerates its contents,
    /// while a genuinely nested ignored directory below it is still pruned.
    #[test]
    fn walk_never_prunes_its_own_root() {
        let wrapper = temp_dir("prune-root");
        let root = wrapper.join("node_modules/vendor-ink");
        write(root.join("main.ink"), "main");
        write(root.join("target/debug/build.ink"), "build");

        assert_eq!(relative(&root, Walk::new(&root)), vec!["main.ink"]);

        fs::remove_dir_all(&wrapper).expect("cleanup temp dir");
    }

    /// Traversal is pre-order and per-directory sorted regardless of the
    /// order entries were created on disk.
    #[test]
    fn walk_is_pre_order_and_sorted_despite_hostile_creation_order() {
        let root = temp_dir("order");
        write(root.join("z.ink"), "z");
        write(root.join("b/z.ink"), "bz");
        write(root.join("b/a.ink"), "ba");
        write(root.join("a.ink"), "a");
        write(root.join("b/c/inner.ink"), "inner");

        assert_eq!(
            relative(&root, Walk::new(&root)),
            vec![
                "a.ink",
                "b",
                "b/a.ink",
                "b/c",
                "b/c/inner.ink",
                "b/z.ink",
                "z.ink",
            ],
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `prune_also` narrows further, on top of (never instead of) the
    /// standing policy.
    #[test]
    fn walk_prune_also_narrows_on_top_of_the_standing_policy() {
        let root = temp_dir("prune-also");
        write(root.join("case/story.ink"), "story");
        write(root.join("case/oracle/e0.oracle.json"), "{}");
        write(root.join("case/target/out.ink"), "out");

        assert_eq!(
            relative(&root, Walk::new(&root).prune_also(["oracle"])),
            vec!["case", "case/story.ink"],
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// A nonexistent root yields exactly one error and then ends — callers
    /// that `?` it get the I/O error, callers that `.flatten()` get an empty
    /// walk, and neither loops.
    #[test]
    fn walk_of_a_missing_root_yields_one_error_then_ends() {
        let root = temp_dir("missing").join("nope");

        let mut walk = Walk::new(&root);
        let first = walk.next().expect("one item");
        assert_eq!(
            first.expect_err("missing root is an error").kind(),
            io::ErrorKind::NotFound
        );
        assert!(
            walk.next().is_none(),
            "the walk must not loop after an error"
        );

        assert_eq!(Walk::new(&root).flatten().count(), 0);
    }

    /// An unreadable subdirectory doesn't abort the whole walk: it yields
    /// one error, and the remaining siblings are still visited (a caller
    /// using `.flatten()` simply skips the branch).
    #[cfg(unix)]
    #[test]
    fn walk_continues_past_an_unreadable_subdirectory() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir("unreadable");
        write(root.join("a/keep.ink"), "keep");
        fs::create_dir_all(root.join("b")).expect("mkdir b");
        write(root.join("c/also-keep.ink"), "also");
        fs::set_permissions(root.join("b"), fs::Permissions::from_mode(0o000))
            .expect("chmod b unreadable");

        let entries: Vec<String> = relative_lossy(&root, Walk::new(&root));

        // Restore permissions before asserting so cleanup always works.
        fs::set_permissions(root.join("b"), fs::Permissions::from_mode(0o755))
            .expect("restore b permissions");

        assert_eq!(
            entries,
            vec!["a", "a/keep.ink", "b", "c", "c/also-keep.ink"],
            "the unreadable branch is skipped, later siblings still walked"
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// [`relative`] but dropping errored entries — for fixtures that
    /// deliberately contain an unreadable branch.
    #[cfg(unix)]
    fn relative_lossy(root: &Path, walk: Walk) -> Vec<String> {
        walk.flatten()
            .map(|entry| {
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("entry is under root")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect()
    }

    /// Directories are distinguishable from files, and `file_name` reports
    /// the entry's own last component.
    #[test]
    fn walk_entry_reports_kind_and_file_name() {
        let root = temp_dir("entry-kind");
        write(root.join("dir/file.ink"), "f");

        let entries: Vec<(String, bool, bool)> = Walk::new(&root)
            .map(|entry| {
                let entry = entry.expect("entry reads");
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.is_dir(),
                    entry.is_file(),
                )
            })
            .collect();

        assert_eq!(
            entries,
            vec![
                ("dir".to_string(), true, false),
                ("file.ink".to_string(), false, true),
            ],
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }
}
