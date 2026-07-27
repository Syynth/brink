//! Host-side [`SourceTree`](brink_db::SourceTree) implementations: the real
//! filesystem and a git revision.
//!
//! Consumed by [`crate::discover_native::discover_native`] (issue #1288):
//! `RealFs` backs a normal native compile (`brink-compiler`'s
//! `prepare_driver`), `GitRev` backs the git-baseline diff path
//! (`brink-cli`'s `load_git_baseline`, closing #1224) — see
//! [`native_source_root`] for how a caller derives the `root` both
//! constructors need from an entry path (decision-log "Native
//! source-loading seam: a `SourceTree` trait with a map-backed impl; the
//! root is caller-supplied", 2026-07-22; issue #1278).
//!
//! Both types are host-only (they touch the real filesystem and spawn a
//! `git` subprocess), which is why they live here rather than in
//! `brink-db`: `RealFs`/`GitRev` are never constructed on any
//! wasm-reachable path — `brink-web`'s `compile`/`compile_fragment` build
//! `brink_source_tree::InMemory` directly and feed it to
//! `brink_environment::Project::load`, which never touches a
//! `SourceTree` at all when driven from `Environment`'s inline content;
//! `brink-compiler`'s `RealFs` branch is CLI-only. (`brink-driver` itself
//! *is* linked into the wasm build transitively, via `brink-compiler` and
//! `brink-environment` — it is `RealFs`/`GitRev` construction, not the
//! crate link, that stays host-only.)

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use brink_db::SourceTree;
use brink_source_tree::Walk;

/// `.brink` is the native surface's source extension (as opposed to ink's
/// `.ink`) — see `crates/internal/brink-ir/src/hir/lower_native/mod.rs`.
const NATIVE_EXTENSION: &str = "brink";

/// Real-filesystem [`SourceTree`]: walks a root directory and enumerates
/// `.brink` keys, keyed by root-relative path. Enumeration goes through the
/// shared [`brink_source_tree::Walk`], so it never descends into
/// [`brink_source_tree::IGNORED_DIR_NAMES`] (`target/`, `.git/`,
/// `node_modules/` — issue #1381 hand-rolled that prune here; issue #1433
/// moved the enforcement into the walk itself, where it can't be forgotten),
/// so a stray build-output or dependency tree under `root` is never
/// enumerated. `read` serves any key lazily off disk — it never eagerly
/// reads the tree, so one malformed/unreadable file elsewhere under `root`
/// cannot fail a `read` of an unrelated key (issue #1357).
///
/// `list`'s `.brink`-only scope is fixed — there used to be a second,
/// `.brink` + `.ink` scope reachable via a `RealFs::project` constructor
/// (issue #1357's CLI producer mount), but every caller of that wider scope
/// either filtered `list()`'s output back down to `.brink` itself
/// (`brink-environment`'s `collect_sources`, for a native entry) or never
/// called `list()` at all (the same function's ink-entry branch, which reads
/// through the tree via `INCLUDE` BFS instead). The extra `.ink` keys were
/// therefore never observable through any real call path, so issue #1404
/// deleted the second scope and collapsed `RealFs::project`'s callers onto
/// this single `.brink`-only constructor.
///
/// Both `list` and `read` resolve against the root this instance was
/// constructed with — neither takes a `root` parameter (issue #1371:
/// `SourceTree::list` used to take one, but `RealFs` always ignored it in
/// favor of its own constructor-held root, while [`GitRev`]'s pre-#1371
/// `list` used the passed-in `root` *instead of* its own constructor-held
/// one — two impls silently disagreeing on which root governed the same
/// call. Dropping the parameter everywhere makes "root is constructor-held"
/// the only contract left to honor).
#[derive(Debug, Clone)]
pub struct RealFs {
    root: PathBuf,
}

impl RealFs {
    /// Construct a `RealFs` seam rooted at `root`, listing `.brink` keys
    /// only. `read` resolves keys (as returned by `list`) relative to this
    /// root.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl SourceTree for RealFs {
    fn list(&self) -> io::Result<Vec<String>> {
        let mut keys = Vec::new();
        for entry in Walk::new(&self.root) {
            let entry = entry?;
            if !entry.is_file() || !is_native(entry.path()) {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&self.root)
                .map_err(|e| io::Error::other(e.to_string()))?;
            keys.push(to_key(rel));
        }
        // `Walk` is pre-order and per-directory sorted, which is not the
        // same as globally key-sorted (`a.brink` < `a/z.brink`, but the walk
        // yields `a/`'s contents first) — and `list`'s contract is the
        // latter, so sort the collected keys.
        keys.sort();
        Ok(keys)
    }

    fn read(&self, key: &str) -> io::Result<String> {
        fs::read_to_string(self.root.join(key))
    }
}

/// Join a relative path's components with `/`, so keys are stable across
/// platforms (Windows' `\` component separator would otherwise leak into
/// module-path derivation downstream).
fn to_key(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether `path` is a native `.brink` source file — an extension test only,
/// matching `brink-db`'s internal `file_language` classification. This is
/// the dispatch every discovery caller (`brink-compiler`'s `prepare_driver`,
/// `brink-cli`'s `load_git_baseline`) uses to pick [`discover_native`] +
/// [`RealFs`]/[`GitRev`] (native) over [`crate::Driver::discover`] (ink,
/// `INCLUDE` BFS).
///
/// [`discover_native`]: crate::discover_native::discover_native
#[must_use]
pub fn is_native(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == NATIVE_EXTENSION)
}

/// Resolve a native project's source root from an entry file's path: the
/// directory containing the nearest `brink.toml` found by walking up from
/// the entry (`brink-project-config`'s discovery), or — if none exists —
/// the entry's own directory (decision-log 2026-07-22 "native module
/// identity ... source root": the explicit, documented single-file-project
/// mode, not a silent fallback).
///
/// A *relative* multi-component `entry_dir` like `chapters` has
/// `Path::parent` return `Some("")` — not `None` — once the walk-up inside
/// [`brink_project_config::find_config`] reaches it, so `find_config` can
/// return a *bare* `brink.toml` (found relative to the process cwd, e.g.
/// `PathBuf::from("brink.toml")`) whose own `.parent()` is that same empty
/// path. Naively filtering that empty parent out and falling back to
/// `entry_dir` (as this function used to) silently discards a config that
/// *was* found — so `brink ide check -e chapters/story.ink`, run from a cwd
/// containing `brink.toml`, missed the config and mis-rooted at `chapters`
/// instead of the true root (review finding on #1403/PR #1412). An empty
/// parent always means "found in the directory the walk started from" —
/// i.e. the current directory — so it maps to `Path::new(".")` instead of
/// being discarded.
///
/// A *relative* `entry_dir` still can't see past the process's cwd, though,
/// even after that fix: `find_config`'s walk-up is `Path::parent`, which is
/// purely lexical — for a relative path it bottoms out at `""` (cwd itself)
/// and has no way to synthesize a `".."` to keep climbing, unlike an
/// *absolute* `entry_dir`, whose `Path::parent` chain walks all the way to
/// the filesystem root for free. So `brink compile story.ink`, run from a
/// cwd whose `brink.toml` lives one directory *above* cwd (not in cwd
/// itself), never even attempts that ancestor — `entry_dir` is `"."`,
/// `find_config` checks `"./brink.toml"` and the bare `"brink.toml"` (both
/// resolve to the same cwd-relative candidate) and then has nowhere lexical
/// left to go, so it returns `None` and this function falls back to
/// `entry_dir` itself, mis-rooting at cwd instead of the true project root
/// (issue #1413) — even though the identical project laid out with an
/// absolute or `chapters/`-nested entry resolves correctly. When the
/// relative walk comes up empty, retry once from an absolutized
/// `entry_dir` so a `brink.toml` above cwd is still found, exactly as it
/// would be for an absolute-path entry. The retry is skipped whenever
/// absolutizing `entry_dir` changes nothing (i.e. `entry_dir` was already
/// absolute *and* already normalized — the first pass already walked to the
/// filesystem root, so a byte-identical second walk would be wasted work)
/// and never runs when the relative walk already found an
/// answer — so the fast, already-correct relative result (including the
/// `"."`-for-cwd spelling [`GitRev::repo_relative`](GitRev)'s shortcut
/// depends on, per the #1403/PR #1412 trap) is untouched in the common
/// case.
///
/// Neither pass climbs past a workspace/git boundary (#1425):
/// [`brink_project_config::find_config`] itself now stops ascending once it
/// passes a directory containing a `.git` entry, so this function can never
/// resolve `root` to somewhere outside the repository the entry lives in —
/// closing the gap the absolutized retry above opened (an absolute walk used
/// to reach the filesystem root "for free," which meant a stray `brink.toml`
/// anywhere above the repo — even in `$HOME` — could get picked up). A
/// `brink.toml` sitting outside a repository entirely (as opposed to merely
/// above `entry_dir` but still inside it) is now treated exactly like no
/// `brink.toml` at all: this function falls back to `entry_dir`.
#[must_use]
pub fn native_source_root(entry: &Path) -> PathBuf {
    let entry_dir = entry
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    if let Some(root) = source_root_from_config(entry_dir) {
        return root;
    }

    let entry_dir_abs = std::path::absolute(entry_dir).unwrap_or_else(|_| entry_dir.to_path_buf());
    if entry_dir_abs != entry_dir
        && let Some(root) = source_root_from_config(&entry_dir_abs)
    {
        return root;
    }

    entry_dir.to_path_buf()
}

/// Walk up from `entry_dir` for a `brink.toml` via
/// [`brink_project_config::find_config`], returning the directory that
/// governs it — `None` when no config is found anywhere above `entry_dir`.
/// An empty parent (a bare `brink.toml` found exactly at `entry_dir`, e.g.
/// the process cwd for a relative walk) maps to `Path::new(".")` rather
/// than being discarded — see [`native_source_root`]'s doc for why.
fn source_root_from_config(entry_dir: &Path) -> Option<PathBuf> {
    let config_path = brink_project_config::find_config(entry_dir)?;
    let parent = config_path.parent().unwrap_or_else(|| Path::new(""));
    Some(if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    })
}

/// Convert `path` to the root-relative key [`RealFs`]/[`GitRev`] would key it
/// under — the inverse of "join `root` with a key," used by discovery
/// callers to look up the `FileId` a just-discovered entry landed on. Both
/// `root` and `path` are lexically absolutized first (via
/// [`std::path::absolute`], which resolves `.`/`..` without touching the
/// filesystem) so the strip is exact regardless of how each was spelled
/// relative to the process's cwd — e.g. `root = "."`, `path = "story/main.brink"`
/// and `root = "story"`, `path = "./story/main.brink"` both key as
/// `"story/main.brink"`.
#[must_use]
pub fn relative_key(root: &Path, path: &Path) -> String {
    let root_abs = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let path_abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let rel = path_abs.strip_prefix(&root_abs).unwrap_or(&path_abs);
    to_key(rel)
}

/// Git-revision [`SourceTree`]: reads keys/contents from a git revision via
/// `git show <rev>:<path>` — the fix path for #1224's baseline-diff bug
/// (`brink ide effects-diff --rev` reading nothing because the old
/// closure-only seam couldn't enumerate).
///
/// `git` runs with `repo_dir` as its working directory. `root` (a path
/// relative to `repo_dir`, `.` for the whole repo) is stored at
/// construction for the same reason `RealFs` stores its root: neither `list`
/// nor `read` takes a `root` parameter (issue #1371), so both must already
/// know how to turn a root-relative key back into a repo-relative git
/// pathspec from `self.root` alone.
#[derive(Debug, Clone)]
pub struct GitRev {
    repo_dir: PathBuf,
    rev: String,
    root: PathBuf,
}

impl GitRev {
    /// Construct a `GitRev` seam that reads `root` (relative to `repo_dir`)
    /// at revision `rev`.
    #[must_use]
    pub fn new(
        repo_dir: impl Into<PathBuf>,
        rev: impl Into<String>,
        root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            repo_dir: repo_dir.into(),
            rev: rev.into(),
            root: root.into(),
        }
    }

    /// The repo-relative pathspec for `key` (root-relative), i.e. `root`
    /// joined with `key` and normalized to `/`-separated components.
    fn repo_relative(&self, key: &str) -> String {
        if self.root == Path::new(".") {
            key.to_string()
        } else {
            format!("{}/{key}", to_key(&self.root))
        }
    }
}

impl SourceTree for GitRev {
    fn list(&self) -> io::Result<Vec<String>> {
        let pathspec = to_key(&self.root);
        let output = Command::new("git")
            .current_dir(&self.repo_dir)
            .args([
                "ls-tree",
                "-r",
                "--name-only",
                "--full-name",
                &self.rev,
                "--",
            ])
            .arg(&pathspec)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "git ls-tree {} -- {pathspec} failed: {}",
                self.rev,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        let prefix = if pathspec == "." {
            String::new()
        } else {
            format!("{pathspec}/")
        };
        let mut keys: Vec<String> = text
            .lines()
            .filter(|line| line.ends_with(&format!(".{NATIVE_EXTENSION}")))
            .map(|line| line.strip_prefix(&prefix).unwrap_or(line).to_string())
            .collect();
        keys.sort();
        Ok(keys)
    }

    fn read(&self, key: &str) -> io::Result<String> {
        let spec = format!("{}:{}", self.rev, self.repo_relative(key));
        let output = Command::new("git")
            .current_dir(&self.repo_dir)
            .args(["show", &spec])
            .output()?;
        if output.status.success() {
            String::from_utf8(output.stdout)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{key} not in {}", self.rev),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A fresh, empty temp directory under the OS temp dir, unique per call
    /// (pid + a monotonic counter + a nanosecond timestamp) so parallel test
    /// runs never collide. No external crate needed for this — the tests
    /// clean up after themselves.
    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "brink-source-tree-test-{label}-{}-{n}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// `RealFs::list` over a tempdir enumerates only `.brink` files, and
    /// returns them in sorted order even though they are created on disk in
    /// a hostile (non-sorted) order.
    #[test]
    fn real_fs_list_enumerates_only_brink_files_in_sorted_order() {
        let root = temp_dir("realfs-list");

        // Created in a hostile (non-sorted) order, interleaved with
        // non-`.brink` files and a nested directory.
        fs::write(root.join("z.brink"), "-- z --").expect("write z.brink");
        fs::write(root.join("z.ink"), "not brink").expect("write z.ink");
        fs::write(root.join("README.md"), "not brink").expect("write README.md");
        fs::create_dir_all(root.join("nested")).expect("mkdir nested");
        fs::write(root.join("nested/a.brink"), "-- nested/a --").expect("write nested/a.brink");
        fs::write(root.join("a.brink"), "-- a --").expect("write a.brink");

        let tree = RealFs::new(&root);
        let keys = tree.list().expect("list succeeds");

        assert_eq!(keys, vec!["a.brink", "nested/a.brink", "z.brink"]);

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `RealFs::read` round-trips exactly the bytes written to disk, using a
    /// key as returned by `list`.
    #[test]
    fn real_fs_read_round_trips() {
        let root = temp_dir("realfs-read");
        fs::write(root.join("main.brink"), "flow main() {}").expect("write main.brink");

        let tree = RealFs::new(&root);
        let keys = tree.list().expect("list succeeds");
        assert_eq!(keys, vec!["main.brink"]);

        let source = tree.read(&keys[0]).expect("read succeeds");
        assert_eq!(source, "flow main() {}");

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// An empty root directory lists as empty, not an error.
    #[test]
    fn real_fs_list_empty_dir_is_ok_empty() {
        let root = temp_dir("realfs-empty");

        let tree = RealFs::new(&root);
        let keys = tree.list().expect("list succeeds");

        assert_eq!(keys, Vec::<String>::new());

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    // ── ignored-directory pruning (issue #1381) ─────────────────────────

    /// `RealFs::list` never descends into a `target/` subtree — a `.brink`
    /// file inside it is never enumerated, matching `.git/` and
    /// `node_modules/` in scope. Proves the walk *prunes* the directory
    /// (rather than merely filtering matched keys after the fact): the
    /// fixture also plants a sibling `.brink` file directly under `target/`
    /// to guarantee a bug that stopped pruning at the top level, but still
    /// walked in, would be caught.
    #[test]
    fn real_fs_list_skips_ignored_dirs() {
        let root = temp_dir("realfs-ignored-dirs");

        fs::write(root.join("main.brink"), "flow main() {}").expect("write main.brink");
        fs::create_dir_all(root.join("target/debug")).expect("mkdir target/debug");
        fs::write(root.join("target/stray.brink"), "-- stray --").expect("write target/stray");
        fs::write(root.join("target/debug/build.brink"), "-- build --")
            .expect("write target/debug/build");
        fs::create_dir_all(root.join(".git/objects")).expect("mkdir .git/objects");
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write .git/HEAD");
        fs::write(root.join(".git/objects/pack.brink"), "-- pack --")
            .expect("write .git/objects/pack.brink");
        fs::create_dir_all(root.join("node_modules/some-pkg")).expect("mkdir node_modules");
        fs::write(root.join("node_modules/some-pkg/index.brink"), "-- pkg --")
            .expect("write node_modules/some-pkg/index.brink");

        let tree = RealFs::new(&root);
        let keys = tree.list().expect("list succeeds");

        assert_eq!(
            keys,
            vec!["main.brink"],
            "target/, .git/, and node_modules/ must be pruned entirely"
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `RealFs::list` excludes `.ink` and `brink.toml` keys even when they
    /// sit alongside `.brink` files — `discover_native`/`EditOverlay` must
    /// never see them, and `read` still serves them (see below).
    #[test]
    fn real_fs_native_list_still_excludes_ink_and_config() {
        let root = temp_dir("realfs-native-scope");

        fs::write(root.join("a.brink"), "-- a --").expect("write a.brink");
        fs::write(root.join("main.ink"), "-> END\n").expect("write main.ink");
        fs::write(root.join("brink.toml"), "[project]\n").expect("write brink.toml");

        let tree = RealFs::new(&root);
        let keys = tree.list().expect("list succeeds");

        assert_eq!(keys, vec!["a.brink"]);

        // `read` carries no equivalent scoping (brink-source-tree's "policy
        // asymmetry" doc section): a `.brink`-scoped `list()` still leaves
        // `read` willing to serve the non-native keys sitting right next to
        // it, because `find_config_in_tree`'s ancestor probe depends on
        // exactly that.
        assert_eq!(
            tree.read("brink.toml").expect("read is not list-scoped"),
            "[project]\n"
        );
        assert_eq!(
            tree.read("main.ink").expect("read is not list-scoped"),
            "-> END\n"
        );

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `RealFs::list` takes no `root` parameter at all (issue #1371) and
    /// always walks the root it was constructed with — the CLI producer
    /// mount (`brink_environment::Project::load`) calls `list` with no
    /// arguments at all now, the #1312 "tree is rooted at `.`" convention
    /// realized as "there is nothing else to pass," and must still see the
    /// real keys.
    #[test]
    fn real_fs_list_uses_only_its_constructor_root() {
        let root = temp_dir("realfs-list-constructor-root");
        fs::write(root.join("main.brink"), "flow main() {}").expect("write main.brink");

        let tree = RealFs::new(&root);
        let keys = tree.list().expect("list succeeds");

        assert_eq!(keys, vec!["main.brink"]);

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `RealFs::read` is lazy per-key: `list` never reads file contents, so
    /// an unrelated file elsewhere under root that is not valid UTF-8 does
    /// not fail `list`, and does not fail `read` of a *different*,
    /// well-formed key (issue #1357's core fix — a whole-tree eager drain
    /// would have failed both).
    #[test]
    fn real_fs_read_is_lazy_so_an_unrelated_malformed_file_does_not_fail_other_reads() {
        let root = temp_dir("realfs-lazy-read");
        fs::write(root.join("good.brink"), "flow main() {}").expect("write good.brink");
        // Invalid UTF-8 bytes with a `.brink` extension — would fail
        // `fs::read_to_string` if ever read.
        fs::write(root.join("bad.brink"), [0xFF, 0xFE, 0xFD]).expect("write bad.brink");

        let tree = RealFs::new(&root);
        let keys = tree.list().expect("list succeeds without reading contents");
        assert_eq!(keys, vec!["bad.brink", "good.brink"]);

        let source = tree
            .read("good.brink")
            .expect("reading an unrelated, well-formed key must not be affected");
        assert_eq!(source, "flow main() {}");

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// `RealFs::read` resolves a key that escapes the root (a leading `..`
    /// segment) by joining it onto the root and reading through to disk —
    /// the read-through behavior an ink `INCLUDE` above the resolved project
    /// root needs (issue #1356's regression, preserved by #1357's
    /// `DrainedRoot` replacement).
    #[test]
    fn real_fs_read_resolves_a_key_that_escapes_the_root() {
        let wrapper = temp_dir("realfs-escape-root");
        let root = wrapper.join("proj");
        fs::create_dir_all(&root).expect("mkdir proj");
        fs::write(wrapper.join("shared.ink"), "Shared content.\n").expect("write shared.ink");

        let tree = RealFs::new(&root);
        let source = tree
            .read("../shared.ink")
            .expect("read resolves an above-root key relative to the constructed root");

        assert_eq!(source, "Shared content.\n");

        fs::remove_dir_all(&wrapper).expect("cleanup temp dir");
    }

    /// Issue #1387 (2/3): `find_config_in_tree`'s #1370 direct probe (no
    /// `list()` walk — see its doc comment) works by calling `RealFs::read`
    /// on each `{ancestor}/brink.toml` candidate and treating anything but
    /// `NotFound` as "found". `find_config_in_tree_reports_found_when_the_
    /// candidate_read_errors_non_not_found` (brink-project-config) pins that
    /// contract against a hand-written mock `SourceTree`; this pins the same
    /// contract against a *real* `RealFs`, with an actual symlink: a
    /// `brink.toml` that is a symlink to a real file elsewhere must resolve
    /// exactly as a plain file would (`fs::read_to_string`, which `RealFs::
    /// read` wraps, follows symlinks) — both `RealFs::read` and the probe
    /// that depends on it must see the target's real content, not treat the
    /// symlink as absent or unreadable.
    #[cfg(unix)]
    #[test]
    fn real_fs_read_and_find_config_in_tree_follow_a_symlinked_brink_toml() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("realfs-symlink-config");
        fs::write(
            root.join("real-brink.toml"),
            "[project]\ndialect = \"brink\"\n",
        )
        .expect("write real-brink.toml");
        symlink(root.join("real-brink.toml"), root.join("brink.toml"))
            .expect("symlink brink.toml -> real-brink.toml");
        fs::write(root.join("main.brink"), "flow main() {}").expect("write main.brink");

        let tree = RealFs::new(&root);
        assert_eq!(
            tree.read("brink.toml").expect("read follows the symlink"),
            "[project]\ndialect = \"brink\"\n"
        );

        let found = brink_project_config::discover_from_entry_in_tree(&tree, "main.brink")
            .expect("probe read succeeds")
            .expect("the symlinked brink.toml is discovered");
        assert_eq!(found, "brink.toml");

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// Issue #1387 (2/3), the other edge case: a `brink.toml` that is
    /// actually a *directory* on disk (a plausible authoring mistake — an
    /// empty `mkdir brink.toml` instead of a file, or a half-finished
    /// project scaffold). `RealFs::read` must surface this as an `Err`
    /// (`fs::read_to_string` on a directory never panics — it errors), and
    /// `find_config_in_tree`'s probe must still treat that error as "found"
    /// per its documented contract ("any other error kind ... means a
    /// `brink.toml` *exists* at this candidate but this probe couldn't read
    /// it — treated as found"): the caller's own subsequent `read` of the
    /// returned key is what turns this into a path-attributed load error
    /// (`brink-environment`'s `LoadError::ConfigRead`), not a silent
    /// "no config" fallback to defaults.
    #[test]
    fn real_fs_read_and_find_config_in_tree_report_a_directory_shaped_brink_toml_as_found() {
        let root = temp_dir("realfs-dir-config");
        fs::create_dir_all(root.join("brink.toml")).expect("mkdir brink.toml (directory)");
        fs::write(root.join("main.brink"), "flow main() {}").expect("write main.brink");

        let tree = RealFs::new(&root);
        tree.read("brink.toml")
            .expect_err("reading a directory as a config file must error, not panic");

        let found = brink_project_config::discover_from_entry_in_tree(&tree, "main.brink")
            .expect("the directory's read error is not propagated as an Err")
            .expect("a directory-shaped brink.toml is still reported as found");
        assert_eq!(found, "brink.toml");

        fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// Build a throwaway git repo with one commit containing `files`
    /// (relative path -> content), and return its directory plus the
    /// resulting commit sha.
    fn git_repo_with_commit(label: &str, files: &[(&str, &str)]) -> (PathBuf, String) {
        let dir = temp_dir(label);
        let git = |args: &[&str]| {
            let output = StdCommand::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .expect("spawn git");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.email", "sourcetree-test@example.invalid"]);
        git(&["config", "user.name", "SourceTree Test"]);
        for (path, content) in files {
            let full = dir.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("mkdir parent");
            }
            fs::write(&full, content).expect("write fixture file");
        }
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "sourcetree test fixture"]);

        let output = StdCommand::new("git")
            .current_dir(&dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("spawn git rev-parse");
        assert!(output.status.success(), "git rev-parse HEAD failed");
        let sha = String::from_utf8(output.stdout)
            .expect("HEAD sha is utf8")
            .trim()
            .to_string();
        (dir, sha)
    }

    /// `GitRev::list`/`read` over a real (throwaway) git repo: enumerates
    /// only `.brink` blobs in sorted order, and `read` round-trips their
    /// committed content.
    #[test]
    fn git_rev_list_and_read_round_trip_a_real_commit() {
        let (repo_dir, sha) = git_repo_with_commit(
            "gitrev",
            &[
                ("z.brink", "-- z --"),
                ("a.brink", "-- a --"),
                ("nested/b.brink", "-- nested/b --"),
                ("README.md", "not brink"),
            ],
        );

        let tree = GitRev::new(&repo_dir, sha.clone(), ".");
        let keys = tree.list().expect("list succeeds");

        assert_eq!(keys, vec!["a.brink", "nested/b.brink", "z.brink"]);
        assert_eq!(tree.read("a.brink").expect("read succeeds"), "-- a --");
        assert_eq!(
            tree.read("nested/b.brink").expect("read succeeds"),
            "-- nested/b --"
        );

        fs::remove_dir_all(&repo_dir).expect("cleanup temp dir");
    }

    /// A key that does not exist at the given revision reads as a
    /// `NotFound` I/O error.
    #[test]
    fn git_rev_read_missing_key_is_not_found() {
        let (repo_dir, sha) = git_repo_with_commit("gitrev-missing", &[("a.brink", "-- a --")]);

        let tree = GitRev::new(&repo_dir, sha, ".");
        let err = tree.read("missing.brink").expect_err("key absent at rev");

        assert_eq!(err.kind(), io::ErrorKind::NotFound);

        fs::remove_dir_all(&repo_dir).expect("cleanup temp dir");
    }

    /// Regression for #1371: `GitRev::list` used to take a `root: &Path`
    /// trait parameter and scope its `git ls-tree` pathspec off *that*
    /// argument, ignoring its own constructor-held `root` entirely — the
    /// opposite bug from `RealFs` (which ignored the argument and always
    /// used its constructor root). A tree constructed with a subdirectory
    /// root must list only that subdirectory's `.brink` files, with no
    /// `root` argument available to override it.
    #[test]
    fn git_rev_list_uses_only_its_constructor_root_not_a_call_site_argument() {
        let (repo_dir, sha) = git_repo_with_commit(
            "gitrev-constructor-root",
            &[("sub/a.brink", "-- sub/a --"), ("top.brink", "-- top --")],
        );

        let tree = GitRev::new(&repo_dir, sha, "sub");
        let keys = tree.list().expect("list succeeds");

        assert_eq!(
            keys,
            vec!["a.brink"],
            "must scope to the constructor-held root (sub/), never see top.brink"
        );
        assert_eq!(tree.read("a.brink").expect("read succeeds"), "-- sub/a --");

        fs::remove_dir_all(&repo_dir).expect("cleanup temp dir");
    }

    // ── is_native ────────────────────────────────────────────────────

    #[test]
    fn is_native_matches_brink_extension_only() {
        assert!(is_native(Path::new("foo.brink")));
        assert!(is_native(Path::new("nested/foo.brink")));
        assert!(!is_native(Path::new("foo.ink")));
        assert!(!is_native(Path::new("foo")));
    }

    // ── native_source_root ──────────────────────────────────────────────

    /// A `brink.toml` above the entry's directory (walked up to) makes its
    /// *parent* directory the source root — not the entry's own directory.
    #[test]
    fn native_source_root_walks_up_to_brink_toml() {
        let dir = temp_dir("root-walkup");
        fs::create_dir_all(dir.join("sub")).expect("mkdir sub");
        fs::write(dir.join("brink.toml"), "[project]\n").expect("write brink.toml");

        let entry = dir.join("sub").join("main.brink");
        let root = native_source_root(&entry);

        assert_eq!(
            root, dir,
            "root must be brink.toml's directory, not entry's"
        );

        fs::remove_dir_all(&dir).expect("cleanup temp dir");
    }

    /// No `brink.toml` anywhere above the entry: root falls back to the
    /// entry's own directory — the documented single-file-project mode.
    #[test]
    fn native_source_root_falls_back_to_entry_dir_without_brink_toml() {
        let dir = temp_dir("root-fallback");

        let entry = dir.join("main.brink");
        let root = native_source_root(&entry);

        assert_eq!(root, dir);

        fs::remove_dir_all(&dir).expect("cleanup temp dir");
    }

    // ── relative_key ─────────────────────────────────────────────────

    #[test]
    fn relative_key_strips_root_prefix() {
        let dir = temp_dir("relative-key");
        let path = dir.join("story").join("main.brink");

        assert_eq!(relative_key(&dir, &path), "story/main.brink");

        fs::remove_dir_all(&dir).expect("cleanup temp dir");
    }

    /// `root = "."` and an already-cwd-relative `path` key identically to
    /// the plain path — the common case for a CLI invocation with no
    /// `brink.toml` above the entry.
    #[test]
    fn relative_key_root_dot_keys_a_relative_path_as_is() {
        assert_eq!(
            relative_key(Path::new("."), Path::new("main.brink")),
            "main.brink"
        );
    }
}
