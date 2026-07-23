//! Host-side [`SourceTree`](brink_db::SourceTree) implementations: the real
//! filesystem and a git revision.
//!
//! # Status: unconsumed infrastructure
//!
//! Neither [`RealFs`] nor [`GitRev`] is called anywhere in this crate yet —
//! see the [`brink_db::source_tree`](brink_db) module docs for the full
//! rationale (decision-log "Native source-loading seam: a `SourceTree`
//! trait with a map-backed impl; the root is caller-supplied", 2026-07-22;
//! issue #1278). Wiring native discovery to consume these is a separate,
//! deliberately deferred change.
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
/// Both `list` and `read` resolve against the root this instance was
/// constructed with — there is one root per seam, set once at construction
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
    fn list(&self) -> io::Result<Vec<String>> {
        let mut keys = Vec::new();
        walk(&self.root, &self.root, &mut keys)?;
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
}
