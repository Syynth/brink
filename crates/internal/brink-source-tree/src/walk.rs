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
//! remember.
//!
//! This is host-side code (it touches the real filesystem), sitting here
//! rather than in `brink-driver` because it is the enforcement half of a
//! policy this crate already owns. Like `RealFs`/`GitRev`, it is never
//! *constructed* on a wasm-reachable path — the crate link is not the
//! constraint (see the [module docs](crate)).
//!
//! # Issue #1407: escape hatch, gitignore-awareness, diagnostic
//!
//! Before #1407, [`Walk`] deliberately offered **no unpruned mode at all** —
//! a project legitimately keeping sources under a directory named `target/`,
//! `.git/`, or `node_modules/` had no way to opt out, got no error, and got
//! no file. #1407 closes that gap with three decisions:
//!
//! 1. **Escape hatch: [`Walk::allow`].** Un-prunes specific directory names
//!    for one `Walk` — the one legal way to widen past the by-construction
//!    policy (every other builder, [`Walk::prune_also`], can only narrow
//!    further). `brink-driver`'s `RealFs` wires this to a new `brink.toml`
//!    key, `[project] unprune-dirs`
//!    (`brink_project_config::ProjectConfig::unprune_dirs`) — an explicit,
//!    checked-in, per-project override, not an environment variable or CLI
//!    flag, so the escape hatch itself stays a deterministic-compilation
//!    input (#1306): the same tree, compiled by anyone, unprunes the same
//!    directories.
//! 2. **Gitignore-awareness: deliberately NOT implemented.** `.gitignore` is
//!    not consulted anywhere in this crate, and that is a decision, not an
//!    oversight. Two reasons, both rooted in #1306 (discovery is a
//!    deterministic-compilation input):
//!    - `.gitignore` resolution is not fully determined by the *tracked*
//!      contents of a repository — a local uncommitted edit to `.gitignore`,
//!      a per-clone `.git/info/exclude`, and a user's global
//!      `core.excludesFile` can all change what it matches, so two checkouts
//!      of byte-identical tracked source could discover a different file set
//!      and silently compile differently. `unprune-dirs` avoids exactly this:
//!      it lives in `brink.toml`, which is itself tracked, versioned source —
//!      the same input on every clone.
//!    - Correctly implementing gitignore's matching semantics (nested
//!      `.gitignore` files, `!`-negation, anchoring, `.git/info/exclude`,
//!      global excludes) is a substantial, easy-to-get-subtly-wrong
//!      reimplementation of git's own resolution logic; a divergence would
//!      itself become a silent, hard-to-diagnose "files came and went"
//!      determinism bug — the same failure class #1306 exists to prevent, not
//!      a fix for it.
//!
//!    The actual pain point the issue names — a legitimately-authored source
//!    file going silently missing — is closed by items 1 and 3 instead,
//!    without taking on either cost.
//! 3. **Diagnostic: [`Walk::warn_on_pruned_sources`] /
//!    [`Walk::pruned_with_sources`].** A caller opts a `Walk` into watching
//!    for source-shaped files (by extension) sitting directly inside a
//!    pruned directory; after the walk is drained,
//!    [`Walk::pruned_with_sources`] names every pruned directory that
//!    plausibly held something the author wanted. The check is **shallow**
//!    (the pruned directory's own immediate children only, not a recursive
//!    descent) — deliberately bounded so noticing a stray source file inside
//!    a huge `target/` never turns a cheap prune into an expensive walk of
//!    the very tree being skipped.
//!
//! Every pruned directory is still skipped exactly as before unless
//! [`Walk::allow`] names it — items 1 and 3 change what the walk *reports*,
//! never what it silently does by default.

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
    allowed: Vec<OsString>,
    watch_extensions: Vec<OsString>,
    pruned_with_sources: Vec<PathBuf>,
}

impl Walk {
    /// Start a pruned walk rooted at `root`. The
    /// [`IGNORED_DIR_NAMES`](crate::IGNORED_DIR_NAMES) policy applies with
    /// no opt-in and no opt-out — unless [`Walk::allow`] names an entry
    /// explicitly (issue #1407).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            stack: vec![Pending::Descend(root.into())],
            also_pruned: Vec::new(),
            allowed: Vec::new(),
            watch_extensions: Vec::new(),
            pruned_with_sources: Vec::new(),
        }
    }

    /// Prune these additional directory names on top of the standing policy
    /// — strictly narrowing, never widening on its own (there is no way for
    /// `prune_also` itself to un-prune an
    /// [`IGNORED_DIR_NAMES`](crate::IGNORED_DIR_NAMES) entry; see
    /// [`Walk::allow`] for the one builder that can). For callers with a
    /// fixture-layout convention of their own, e.g. the test harness's
    /// `oracle/`/`episodes/` case directories.
    #[must_use]
    pub fn prune_also<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.also_pruned.extend(names.into_iter().map(Into::into));
        self
    }

    /// Un-prune these directory names for this `Walk` — the escape hatch
    /// issue #1407 asked for. A name passed here is never pruned, regardless
    /// of the standing [`IGNORED_DIR_NAMES`](crate::IGNORED_DIR_NAMES)
    /// policy or [`Walk::prune_also`]; this is the one legal way to widen a
    /// `Walk` past its by-construction pruning (see the [module docs](self))
    /// — every other constructor/builder can only narrow further.
    /// `brink-driver`'s `RealFs` wires this to `brink.toml`'s
    /// `[project] unprune-dirs`, so the widening stays an explicit,
    /// checked-in per-project choice rather than something ambient.
    #[must_use]
    pub fn allow<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.allowed.extend(names.into_iter().map(Into::into));
        self
    }

    /// Watch for pruned directories that plausibly held a source file (issue
    /// #1407's diagnostic half): after the walk is drained,
    /// [`Walk::pruned_with_sources`] names every pruned directory whose own
    /// **immediate** children include a file with one of these extensions
    /// (e.g. `"brink"`, no leading dot). Every pruned directory is still
    /// skipped exactly as before — this only makes
    /// [`Walk::pruned_with_sources`] non-empty; it never changes what is
    /// yielded.
    ///
    /// The check is deliberately shallow — the pruned directory's immediate
    /// children only, never a recursive descent into it — so flagging a
    /// stray source file inside a huge pruned `target/` never turns a cheap
    /// prune into an expensive walk of the very tree being skipped.
    #[must_use]
    pub fn warn_on_pruned_sources<I, S>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.watch_extensions
            .extend(extensions.into_iter().map(Into::into));
        self
    }

    /// Every pruned directory this walk has skipped so far whose own
    /// immediate children include a file with one of the extensions passed
    /// to [`Walk::warn_on_pruned_sources`] — empty unless that builder was
    /// called. Populated incrementally as iteration proceeds (a directory
    /// not yet reached hasn't been checked yet), so read this only after the
    /// walk has been fully drained.
    #[must_use]
    pub fn pruned_with_sources(&self) -> &[PathBuf] {
        &self.pruned_with_sources
    }

    /// Whether a directory named `name`, found while descending, is pruned.
    /// [`Walk::allow`] takes priority over both the standing policy and
    /// [`Walk::prune_also`] — an allowed name is never pruned by this `Walk`.
    fn is_pruned(&self, name: &OsStr) -> bool {
        if self.allowed.iter().any(|allowed| allowed == name) {
            return false;
        }
        is_ignored_dir(name) || self.also_pruned.iter().any(|pruned| pruned == name)
    }

    /// Whether `dir`'s own immediate children (not a recursive descent — see
    /// [`Walk::warn_on_pruned_sources`]) include a file whose extension is
    /// one of `self.watch_extensions`. An unreadable `dir` reports `false`
    /// rather than propagating an error — this is a best-effort diagnostic
    /// check on a directory the walk has already decided to skip, not a
    /// traversal step that must succeed.
    fn shallow_contains_watched_extension(&self, dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| self.watch_extensions.iter().any(|watched| watched == ext))
        })
    }

    /// List `dir`'s entries, sorted by file name, with pruned directories
    /// already dropped. A failure to read the directory itself is one error
    /// for the whole directory; a failure to stat a single entry is an error
    /// for that entry alone. Takes `&mut self` (not `&self`) because a
    /// pruned directory that shallowly contains a watched extension is
    /// recorded into `self.pruned_with_sources` as it's found.
    fn children(&mut self, dir: &Path) -> io::Result<Vec<Pending>> {
        let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let mut pending = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry.file_type() {
                Ok(file_type) => {
                    if file_type.is_dir() && self.is_pruned(&entry.file_name()) {
                        if !self.watch_extensions.is_empty()
                            && self.shallow_contains_watched_extension(&entry.path())
                        {
                            self.pruned_with_sources.push(entry.path());
                        }
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

    /// `allow` un-prunes a standing [`crate::IGNORED_DIR_NAMES`] entry (issue
    /// #1407's escape hatch): a `node_modules/` directory that would
    /// otherwise be pruned entirely is walked and yielded like any other
    /// directory once its name is passed to `allow`, while a *sibling*
    /// ignored directory not named in `allow` (`target/`) is still pruned.
    #[test]
    fn walk_allow_unprunes_a_named_standing_policy_entry() {
        let root = temp_dir("allow");
        write(root.join("main.ink"), "main");
        write(root.join("node_modules/vendor-ink/lib.ink"), "vendored");
        write(root.join("target/stray.ink"), "stray");

        assert_eq!(
            relative(&root, Walk::new(&root).allow(["node_modules"])),
            vec![
                "main.ink",
                "node_modules",
                "node_modules/vendor-ink",
                "node_modules/vendor-ink/lib.ink",
            ],
            "node_modules/ must be un-pruned by `allow`, target/ must stay pruned"
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `warn_on_pruned_sources` + `pruned_with_sources`: a pruned directory
    /// whose immediate children include a watched-extension file is reported
    /// once drained; a pruned directory with no matching file is not, and
    /// neither report changes what the walk actually yields (still nothing
    /// from inside either pruned directory).
    #[test]
    fn walk_pruned_with_sources_reports_only_directories_shallowly_holding_watched_files() {
        let root = temp_dir("pruned-with-sources");
        write(root.join("main.ink"), "main");
        write(root.join("node_modules/stray.ink"), "stray");
        write(root.join(".git/HEAD"), "ref");

        let mut walk = Walk::new(&root).warn_on_pruned_sources(["ink"]);
        let yielded: Vec<String> = relative_lossy(&root, walk.by_ref());

        assert_eq!(
            yielded,
            vec!["main.ink"],
            "reporting a pruned directory must not change what is yielded"
        );

        let pruned: Vec<String> = walk
            .pruned_with_sources()
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .expect("pruned path is under root")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            pruned,
            vec!["node_modules"],
            "only node_modules/ shallowly holds a watched .ink file; .git/ (HEAD, no \
             extension) must not be reported"
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `warn_on_pruned_sources` only checks a pruned directory's *immediate*
    /// children, never descending further — a watched file nested two levels
    /// deep inside the pruned directory is not detected. Documents the
    /// deliberate shallow-check bound (never an expensive recursive scan of
    /// a skipped subtree).
    #[test]
    fn walk_pruned_with_sources_does_not_recurse_into_the_pruned_directory() {
        let root = temp_dir("pruned-with-sources-shallow");
        write(root.join("main.ink"), "main");
        write(root.join("node_modules/pkg/nested.ink"), "nested");

        let mut walk = Walk::new(&root).warn_on_pruned_sources(["ink"]);
        let _: Vec<String> = relative_lossy(&root, walk.by_ref());

        assert!(
            walk.pruned_with_sources().is_empty(),
            "a watched file two levels deep must not be found by the shallow check, got {:?}",
            walk.pruned_with_sources()
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// With no call to `warn_on_pruned_sources`, `pruned_with_sources` stays
    /// empty even though pruned directories with matching files exist — the
    /// diagnostic is opt-in, never ambient.
    #[test]
    fn walk_pruned_with_sources_is_empty_when_never_requested() {
        let root = temp_dir("pruned-with-sources-opt-in");
        write(root.join("main.ink"), "main");
        write(root.join("node_modules/stray.ink"), "stray");

        let mut walk = Walk::new(&root);
        let _: Vec<String> = relative_lossy(&root, walk.by_ref());

        assert!(walk.pruned_with_sources().is_empty());

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// A nonexistent root yields exactly one error and then ends — callers
    /// that `?` it get the I/O error, callers that `.flatten()` get an empty
    /// walk, and neither loops.
    #[test]
    fn walk_of_a_missing_root_yields_one_error_then_ends() {
        let wrapper = temp_dir("missing");
        let root = wrapper.join("nope");

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

        fs::remove_dir_all(&wrapper).expect("cleanup temp dir");
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

    /// Documents the disclosed behavior delta from `Path::is_dir()`-based
    /// hand-rolled walks (the LSP's pre-#1433 `collect_ink_files`): a
    /// [`WalkEntry`]'s kind comes from `DirEntry::file_type`, which does not
    /// follow symlinks, so a symlinked directory is yielded once as a plain
    /// (non-dir) entry and never descended into — bounding the walk against
    /// symlink cycles — while a symlinked `.ink` file is still yielded (with
    /// `is_dir() == false`, `is_file() == false`), which is exactly what
    /// lets a caller filtering on `!entry.is_dir()` (as `collect_ink_files`
    /// does) still admit it.
    #[cfg(unix)]
    #[test]
    fn walk_does_not_descend_into_a_symlinked_directory_but_admits_a_symlinked_file() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("symlink");
        write(root.join("real/nested.ink"), "nested");
        write(root.join("real-file.ink"), "real");
        symlink(root.join("real"), root.join("link-dir")).expect("symlink dir");
        symlink(root.join("real-file.ink"), root.join("link-file.ink")).expect("symlink file");

        let entries: Vec<(String, bool, bool)> = Walk::new(&root)
            .map(|entry| {
                let entry = entry.expect("entry reads");
                (
                    entry
                        .path()
                        .strip_prefix(&root)
                        .expect("entry is under root")
                        .to_string_lossy()
                        .into_owned(),
                    entry.is_dir(),
                    entry.is_file(),
                )
            })
            .collect();

        assert_eq!(
            entries,
            vec![
                ("link-dir".to_string(), false, false),
                ("link-file.ink".to_string(), false, false),
                ("real".to_string(), true, false),
                ("real/nested.ink".to_string(), false, true),
                ("real-file.ink".to_string(), false, true),
            ],
            "the symlinked directory is yielded once and never descended into; \
             the symlinked file is still yielded, with is_dir()==false"
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// [`relative`] but dropping errored entries — for fixtures that
    /// deliberately contain an unreadable branch, or for a caller that needs
    /// to keep the `Walk` alive afterward (via `walk.by_ref()`) to read
    /// state it accumulated during iteration (e.g. `pruned_with_sources`).
    fn relative_lossy(
        root: &Path,
        walk: impl Iterator<Item = io::Result<WalkEntry>>,
    ) -> Vec<String> {
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
