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
//! `brink-db`: `brink-web` links `brink-db` for its (portable)
//! `InMemory` seam but never links `brink-driver`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use brink_db::SourceTree;

/// `.brink` is the native surface's source extension (as opposed to ink's
/// `.ink`) — see `crates/internal/brink-ir/src/hir/lower_native/mod.rs`.
const NATIVE_EXTENSION: &str = "brink";

/// Real-filesystem [`SourceTree`]: walks a root directory and enumerates
/// every `.brink` file under it, keyed by root-relative path.
///
/// `read` resolves a key against the root this instance was constructed
/// with, so callers should pass that same root to [`list`](SourceTree::list)
/// — `RealFs` does not itself cross-check the two.
#[derive(Debug, Clone)]
pub struct RealFs {
    root: PathBuf,
}

impl RealFs {
    /// Construct a `RealFs` seam rooted at `root`. `read` resolves keys
    /// (as returned by `list`) relative to this root.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl SourceTree for RealFs {
    fn list(&self, root: &Path) -> io::Result<Vec<String>> {
        let mut keys = Vec::new();
        walk(root, root, &mut keys)?;
        keys.sort();
        Ok(keys)
    }

    fn read(&self, key: &str) -> io::Result<String> {
        fs::read_to_string(self.root.join(key))
    }
}

/// Recursively collect root-relative `.brink` keys under `dir` into `keys`.
/// Directory-entry iteration order is filesystem/OS-dependent — callers
/// (`RealFs::list`) sort the accumulated result, so this helper does not
/// need to sort as it goes.
fn walk(root: &Path, dir: &Path, keys: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk(root, &path, keys)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == NATIVE_EXTENSION)
        {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| io::Error::other(e.to_string()))?;
            keys.push(to_key(rel));
        }
    }
    Ok(())
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
#[must_use]
pub fn native_source_root(entry: &Path) -> PathBuf {
    let entry_dir = entry
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    brink_project_config::find_config(entry_dir)
        .and_then(|config_path| config_path.parent().map(Path::to_path_buf))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| entry_dir.to_path_buf())
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
/// construction for the same reason `RealFs` stores its root: `read` has no
/// `root` parameter, so it must already know how to turn a root-relative
/// key back into a repo-relative git pathspec.
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
    fn list(&self, root: &Path) -> io::Result<Vec<String>> {
        let pathspec = to_key(root);
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
        let keys = tree.list(&root).expect("list succeeds");

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
        let keys = tree.list(&root).expect("list succeeds");
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
        let keys = tree.list(&root).expect("list succeeds");

        assert_eq!(keys, Vec::<String>::new());

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
        let keys = tree.list(Path::new(".")).expect("list succeeds");

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
