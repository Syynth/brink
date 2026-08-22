//! The Tauri shell for brink-desktop (docs/desktop-shell-spec.md, D1).
//!
//! Custom commands instead of the fs plugin: the shell owns filesystem
//! policy, and the stay-inside-the-project-root guard lives here, next to
//! the I/O it guards. The frontend passes the project root (obtained from
//! our own `pick_project_folder` dialog) plus project-relative paths; this
//! module rejects anything absolute or `..`-carrying for every project-file
//! path it resolves (`resolve`, used by `read_file`/`write_file`/
//! `rename_file`/`delete_file`/`append_backups`/`run_cli`'s input path). The
//! one deliberate exception is `run_cli`'s trailing `rest` args (e.g.
//! `export-xliff`'s `--output <path>`), which may still be absolute — that
//! path comes from a native save dialog, not from a project-relative key,
//! so it is never run through `resolve` at all (see `prepare_cli_invocation`).

use std::path::{Component, Path, PathBuf};

use tauri_plugin_dialog::DialogExt;

/// Shell I/O errors. Serialized as their display string across the IPC
/// boundary (Tauri command errors must be `Serialize`).
#[derive(Debug, thiserror::Error)]
enum ShellError {
    #[error("path escapes the project root: {0}")]
    PathEscape(String),
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("no subcommand given")]
    MissingSubcommand,
    #[error("subcommand not in the sidecar allowlist: {0}")]
    DisallowedCommand(String),
    #[error("brink-cli sidecar error: {0}")]
    Sidecar(String),
}

impl serde::Serialize for ShellError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

fn io_err(path: &Path, source: std::io::Error) -> ShellError {
    ShellError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Join a project-relative key onto the root, rejecting absolute paths and
/// any `..` component. The root itself came from our own folder dialog and
/// is trusted; the relative key came from the webview and is not.
fn resolve(root: &str, rel: &str) -> Result<PathBuf, ShellError> {
    let rel_path = Path::new(rel);
    let escapes = rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)));
    if escapes {
        return Err(ShellError::PathEscape(rel.to_owned()));
    }
    Ok(Path::new(root).join(rel_path))
}

/// Whether a directory entry participates in the project listing. Keep in
/// sync with `ENTRY_FALLBACKS` / the loadable set in `src/main.tsx`.
fn is_project_file(path: &Path) -> bool {
    if path.file_name().and_then(|n| n.to_str()) == Some("brink.toml") {
        return true;
    }
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("brink" | "ink")
    )
}

fn is_skipped_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist"
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<(), ShellError> {
    let entries = std::fs::read_dir(dir).map_err(|e| io_err(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_err(dir, e))?;
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if path.is_dir() {
            if !is_skipped_dir(name) {
                walk(&path, root, out)?;
            }
        } else if is_project_file(&path) {
            if let Ok(rel) = path.strip_prefix(root) {
                // Provider keys are `/`-separated regardless of OS.
                let key = rel
                    .components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push(key);
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn list_files(root: String) -> Result<Vec<String>, ShellError> {
    let root_path = Path::new(&root);
    let mut out = Vec::new();
    walk(root_path, root_path, &mut out)?;
    // Deterministic listing regardless of directory iteration order.
    out.sort();
    Ok(out)
}

#[tauri::command]
async fn read_file(root: String, rel: String) -> Result<String, ShellError> {
    let path = resolve(&root, &rel)?;
    std::fs::read_to_string(&path).map_err(|e| io_err(&path, e))
}

#[tauri::command]
async fn write_file(root: String, rel: String, content: String) -> Result<(), ShellError> {
    let path = resolve(&root, &rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    atomic_write(&path, content.as_bytes()).map_err(|e| io_err(&path, e))
}

/// A same-directory temp-file path derived from `path`, unique per call
/// (issue #2445: concurrent writes to the same `path` must not race each
/// other over the same temp file). Leading-`.` hidden by convention, but
/// that dot is NOT what keeps it out of the project listing: `is_skipped_dir`'s
/// dot-rule applies only to directory names, both in `walk` and in
/// `watch_key`. What actually excludes the temp file is that
/// `is_project_file` matches only `brink.toml` or a `.brink`/`.ink`
/// extension, and this file's extension is the trailing disambiguator
/// (`.tmp.<pid>.<nanos>.<counter>`), never one of those — disambiguated by
/// process id, a monotonic in-process counter, and a nanosecond timestamp:
/// the counter alone already guarantees uniqueness within one process, but
/// the pid keeps two independently-launched instances (a dev rebuild racing
/// a still-running app, say) from ever colliding on a leftover temp name.
fn temp_path_for(path: &Path) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    path.with_file_name(format!(".{file_name}.tmp.{pid}.{nanos}.{counter}"))
}

/// Write `content` to `path` so an interruption — most concretely, desktop
/// quit racing an in-flight `requestSave` (#2445: `destroy()` can run while
/// another queued write is still inside what used to be a plain
/// `std::fs::write`) — can never leave `path` holding a half-written file.
///
/// The technique is temp-write-then-rename: `content` lands first at a
/// fresh, uniquely-named temp file in `path`'s OWN directory (see
/// `temp_path_for`), then `path` is atomically replaced by renaming
/// the temp file over it. Same-directory is what makes the rename
/// same-filesystem, which is what makes it atomic on every OS Tauri
/// targets: POSIX `rename(2)` on Linux/macOS, and on Windows the
/// `MoveFileExW` call `std::fs::rename` issues there with
/// `MOVEFILE_REPLACE_EXISTING` — Windows rename-over-existing is handled by
/// that flag, not left to a bare POSIX-only assumption; it overwrites an
/// existing destination rather than erroring on it. At every instant,
/// `path` refers either to its old, complete content or to its new,
/// complete content — an interruption can only ever land before the rename
/// (old content stays; the temp file is simply orphaned) or after it
/// completes (new content); the rename syscall itself is not interruptible
/// mid-effect by the process being torn down, so there is no window where
/// `path` is partially written.
///
/// This does NOT come for free relative to the plain `std::fs::write` (open
/// existing + truncate + write, preserving the original inode) it replaced.
/// Rename-over swaps the inode instead, which changes three things:
/// - **Permissions**: the temp file is created with the process's default
///   mode (`0666 & ~umask` on POSIX), not `path`'s existing mode, so a
///   read-only target (`0444`) comes back writable after being overwritten.
///   This function closes that gap by copying `path`'s permissions onto the
///   temp file before the rename, when `path` already exists — see below.
/// - **Hard links**: any hard link to `path` still points at the old inode
///   with its old content; it is not updated by the rename, whereas
///   `std::fs::write`'s in-place truncate would have been visible through
///   every link.
/// - **Symlinks-in**: if `path` is itself a symlink (e.g. a shared file
///   symlinked into a project folder, which `walk` lists like any other
///   entry), the rename replaces the symlink itself with the new file
///   rather than writing through it — the real target file is left stale
///   and the symlink no longer points at it.
///
/// One caveat this does NOT paper over: on Windows, `rename` can fail if
/// another process holds `path` open without `FILE_SHARE_DELETE` (e.g. a
/// third-party indexer with an exclusive lock), and separately,
/// `MOVEFILE_REPLACE_EXISTING` itself fails outright against a target with
/// `FILE_ATTRIBUTE_READONLY` set — so Windows and POSIX disagree on a
/// read-only target: POSIX only requires write permission on the
/// *directory* to rename over it, while Windows refuses. Both surface as an
/// ordinary `Err` here, exactly as a plain `std::fs::write` failing outright
/// would have.
///
/// **fsync judgment (deliberately NOT fsync'd before the rename):** the
/// corruption this function exists to prevent — a half-written target left
/// by a killed *process* — is fully closed by the temp+rename swap alone,
/// with no dependency on fsync: until the rename lands, `path` is
/// untouched; the moment it lands, it holds the complete new content. What
/// fsync would add on top is a different property — durability across an
/// OS/power-loss crash between the write and the filesystem journal's next
/// flush — which is not the failure mode #2445 is about (a controlled app
/// quit, not a power cut). No write in this shell — this one, the plain
/// `std::fs::write` it replaced, or `append_backups`' own ring writes
/// (`append_backups`, below) — calls `sync_all`/`sync_data` today, so this
/// function is exactly as exposed to a power-loss crash as everything
/// around it always has been; it neither adds that exposure nor closes it.
/// Hardening power-loss durability is a separate decision from this PR's
/// #2445 fix, and if taken up it would have to start with the backup ring,
/// not here. Desktop writes happen on every autosave tick and every
/// `saveAll`/quit dispatch, so paying an fsync's latency on each one is a
/// tradeoff to make deliberately, not a byproduct of this fix.
fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let tmp_path = temp_path_for(path);
    std::fs::write(&tmp_path, content)?;
    // Best-effort: carry the existing target's permissions onto the temp
    // file before the rename replaces it, so overwriting a read-only file
    // does not silently make it writable again (see the doc comment above).
    // A fresh target (no existing file) has nothing to carry — the process
    // default mode applies, same as `std::fs::write` creating a new file.
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
    }
    let renamed = std::fs::rename(&tmp_path, path);
    if renamed.is_err() {
        // Best-effort: don't leave the temp file behind when the rename
        // itself failed. The rename's error is what the caller needs to
        // see, so this cleanup's own outcome is deliberately ignored.
        let _ = std::fs::remove_file(&tmp_path);
    }
    renamed
}

#[tauri::command]
async fn delete_file(root: String, rel: String) -> Result<(), ShellError> {
    let path = resolve(&root, &rel)?;
    std::fs::remove_file(&path).map_err(|e| io_err(&path, e))
}

#[tauri::command]
async fn rename_file(root: String, from: String, to: String) -> Result<(), ShellError> {
    let from_path = resolve(&root, &from)?;
    let to_path = resolve(&root, &to)?;
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    std::fs::rename(&from_path, &to_path).map_err(|e| io_err(&from_path, e))
}

// ── Filesystem watcher (docs/desktop-shell-spec.md D2) ──────────────
//
// Watches the open project root and emits `fs:external-change` events to
// the webview: `{ path: <project-relative key>, content: <text|null> }`,
// null meaning deleted. Events are debounced (300 ms of quiet) and
// filtered to project files, mirroring `list_files`' rules. The provider
// forwards them into `ProjectSession`'s #320 machinery — never-clobber
// 3-way against the last canonical save — which this watcher gives its
// first real filesystem.

/// The live watcher for the (single) open project. Dropping the watcher
/// stops event delivery; the debounce thread then exits on channel close.
struct WatchState(std::sync::Mutex<Option<notify::RecommendedWatcher>>);

#[derive(Clone, serde::Serialize)]
struct ExternalChangeOut {
    path: String,
    content: Option<String>,
}

/// Project-relative key for an absolute path inside `root`, applying the
/// same skip rules as `list_files` (dotdirs, `node_modules`, target, dist)
/// and the same file filter. None ⇒ not a project file, ignore.
fn watch_key(root: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(root).ok()?;
    let mut parts: Vec<&str> = Vec::new();
    for c in rel.components() {
        let s = c.as_os_str().to_str()?;
        parts.push(s);
    }
    let (file, dirs) = parts.split_last()?;
    if dirs.iter().any(|d| is_skipped_dir(d)) {
        return None;
    }
    if !is_project_file(Path::new(file)) {
        return None;
    }
    Some(parts.join("/"))
}

#[tauri::command]
async fn start_watch(
    app: tauri::AppHandle,
    state: tauri::State<'_, WatchState>,
    root: String,
) -> Result<(), ShellError> {
    use notify::Watcher;
    use tauri::Emitter;

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| ShellError::Io {
        path: root.clone(),
        source: std::io::Error::other(e),
    })?;
    watcher
        .watch(Path::new(&root), notify::RecursiveMode::Recursive)
        .map_err(|e| ShellError::Io {
            path: root.clone(),
            source: std::io::Error::other(e),
        })?;

    // Replacing any previous watcher stops its delivery; its debounce
    // thread exits when the dropped watcher's channel disconnects.
    *state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(watcher);

    let root_path = PathBuf::from(root);
    std::thread::spawn(move || {
        let mut pending: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(300)) {
                Ok(Ok(event)) => pending.extend(event.paths),
                Ok(Err(_)) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    for abs in std::mem::take(&mut pending) {
                        let Some(key) = watch_key(&root_path, &abs) else {
                            continue;
                        };
                        let content = match std::fs::read_to_string(&abs) {
                            Ok(text) => Some(text),
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                            Err(_) => continue, // transient; a later event retries
                        };
                        let _ = app.emit(
                            "fs:external-change",
                            ExternalChangeOut { path: key, content },
                        );
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    Ok(())
}

#[tauri::command]
async fn stop_watch(state: tauri::State<'_, WatchState>) -> Result<(), ShellError> {
    *state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    Ok(())
}

// ── Backup ring (docs/desktop-shell-spec.md D2; 2026-08-07 overlay ruling) ──

/// One ring entry from the webview (`TauriBackupSink.append`).
#[derive(serde::Deserialize)]
struct BackupEntryIn {
    path: String,
    content: String,
    /// Milliseconds since epoch, from the frontend's clock.
    at: u64,
}

/// Ring bounds — enforced HERE, next to the storage (the sink owns its
/// bounds per the `OverlayPersistence` contract). Working defaults from the
/// 2026-08-07 ruling; a Settings surface can adjust later.
const RING_MAX_ENTRIES: usize = 25;
const RING_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// A filesystem-safe, deterministic key for one project's ring directory.
/// The full sanitized path (not a hash): stable forever, debuggable by eye.
fn project_ring_key(root: &str) -> String {
    root.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Append crash-protection snapshots to the project's backup ring, then
/// prune oldest-first until the bounds hold. Filenames are
/// `{at:013}_{seq:02}_{sanitized-rel}.txt` — zero-padded millis make
/// lexicographic order chronological, so pruning is a sort + truncate.
#[tauri::command]
async fn append_backups(
    app: tauri::AppHandle,
    root: String,
    entries: Vec<BackupEntryIn>,
) -> Result<(), ShellError> {
    use tauri::Manager;

    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| ShellError::Io {
            path: "app-data dir".to_owned(),
            source: std::io::Error::other(e),
        })?
        .join("backups")
        .join(project_ring_key(&root));
    std::fs::create_dir_all(&base).map_err(|e| io_err(&base, e))?;

    for (i, entry) in entries.iter().enumerate() {
        // Guard the rel path exactly like the project commands do; a ring
        // write must never become a path-escape vector either.
        resolve(&root, &entry.path)?;
        let name = format!(
            "{:013}_{:02}_{}.txt",
            entry.at,
            i,
            project_ring_key(&entry.path)
        );
        let file = base.join(name);
        std::fs::write(&file, &entry.content).map_err(|e| io_err(&file, e))?;
    }

    // Prune: oldest first (lexicographic == chronological by construction).
    let mut files: Vec<(PathBuf, u64)> = std::fs::read_dir(&base)
        .map_err(|e| io_err(&base, e))?
        .filter_map(std::result::Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| (e.path(), meta.len()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|(_, len)| len).sum();
    let mut count = files.len();
    for (path, len) in &files {
        if count <= RING_MAX_ENTRIES && total <= RING_MAX_BYTES {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            count -= 1;
            total -= len;
        }
    }
    Ok(())
}

/// Export a compiled story to a user-chosen file (D3 slice 1, #2391): a
/// native save-file dialog defaulting to `default_name`, then a direct
/// `fs::write` of the already-compiled bytes. Returns the chosen path, or
/// `None` if the user cancelled.
///
/// ⚠ This command (like every command here) MUST be `async` — see
/// `pick_project_folder` below for why a blocking dialog call on a sync
/// command deadlocks the main thread.
///
/// ⚠ The picked path is deliberately NOT passed through `resolve()`. That
/// guard exists to keep webview-supplied *project-relative* keys from
/// escaping the trusted project root; here there is no root to stay inside
/// of — the user just chose an arbitrary destination via the native dialog,
/// which is exactly the point of an export. Routing this through `resolve()`
/// would incorrectly reject every destination outside the open project,
/// i.e. every normal use of this command.
#[tauri::command]
async fn save_bytes_dialog(
    app: tauri::AppHandle,
    default_name: String,
    bytes: Vec<u8>,
) -> Result<Option<String>, ShellError> {
    let Some(picked) = app
        .dialog()
        .file()
        .set_file_name(&default_name)
        .blocking_save_file()
    else {
        return Ok(None); // user cancelled
    };
    let path = picked.into_path().map_err(|e| ShellError::Io {
        path: default_name,
        source: std::io::Error::other(e),
    })?;
    std::fs::write(&path, &bytes).map_err(|e| io_err(&path, e))?;
    Ok(Some(path.display().to_string()))
}

// ── CLI sidecar (docs/desktop-shell-spec.md D3; #2392) ──────────────
//
// `brink-cli` ships as a Tauri sidecar (`bundle.externalBin` in
// `tauri.conf.json`, staged by `scripts/ensure-cli-sidecar.mjs`) so batch
// xliff/locale operations run against the exact workspace version the
// shell was built from, never whatever `brink` happens to be on the
// user's PATH. `run_cli` is the ONLY way the webview can reach it, and it
// is deliberately not a passthrough: the first argument must be one of a
// fixed subcommand allowlist. A webview that can run arbitrary sidecar
// args is a webview that can run arbitrary code with the app's
// filesystem reach — this allowlist is the real security boundary
// (`Shell::sidecar()` never consults the `shell:allow-execute` capability
// scope at all, so that permission does not belong in `capabilities/
// default.json` — 2026-08 review finding).
//
// The webview never hands this command a raw input path: `rel` is a
// project-relative key resolved against `root` through the same
// [`resolve`] guard every other filesystem command in this module uses,
// exactly like `read_file`/`write_file` above. Only the *trailing* `rest`
// args may still carry an absolute path — e.g. `export-xliff`'s
// `--output <path>` — and that is fine, because that path comes from a
// native save dialog (`src/main.tsx`'s `exportXliff`), never parsed out of
// arbitrary webview input the way the old `args: Vec<String>` shape let
// the *input* path be (2026-08 review finding: the old shape gave a
// compromised webview an arbitrary-file-read/write primitive by passing
// an absolute path as the positional input argument).
//
// ⚠ House rule: the intl pipeline never consumes `.ink.json` — every
// allowed subcommand here (mirroring `brink-cli`'s own surface) operates
// on `.ink`/`.brink`/`.inkb`/`.inkt` inputs only.
//
// This list is a hand-maintained SUBSET of `brink-cli`'s real `clap`
// subcommand surface (`crates/brink-cli/src/main.rs`'s `enum Commands`) —
// `tests::cli_allowlist_subcommands_exist_in_brink_cli_surface` below is the
// cross-workspace guard that fails if an entry here is renamed or removed
// on the `brink-cli` side (docs/desktop-shell-spec.md "Workspace
// placement", #2507).
const ALLOWED_CLI_SUBCOMMANDS: &[&str] = &[
    "export-xliff",
    "compile-locale",
    "regenerate-xliff",
    "compile",
];

/// One line of sidecar output, forwarded to the webview as it streams
/// rather than buffered until exit — `compile-locale` on a large story can
/// run for seconds, and a future fuller intl UI wants live progress.
#[derive(Clone, serde::Serialize)]
struct CliOutputLine {
    /// `"stdout"` or `"stderr"`.
    stream: &'static str,
    line: String,
}

/// The allowlist check, pulled out of [`prepare_cli_invocation`] so it's
/// testable in isolation: `subcommand` must be one of
/// [`ALLOWED_CLI_SUBCOMMANDS`], checked before the sidecar is ever spawned.
fn validate_cli_subcommand(subcommand: &str) -> Result<(), ShellError> {
    if subcommand.is_empty() {
        return Err(ShellError::MissingSubcommand);
    }
    if !ALLOWED_CLI_SUBCOMMANDS.contains(&subcommand) {
        return Err(ShellError::DisallowedCommand(subcommand.to_owned()));
    }
    Ok(())
}

/// Build the full sidecar argv for one CLI invocation, pulled out of
/// [`run_cli`] so it's testable without an `AppHandle`/sidecar (mirrors
/// `resolve`/`project_ring_key` above): validate `subcommand` against the
/// allowlist, resolve `rel` against `root` through the same [`resolve`]
/// guard `read_file`/`write_file` use, and append `rest` verbatim after
/// the resolved input path. `rest` may still contain an absolute path
/// (e.g. `export-xliff`'s dialog-chosen `--output <path>`) — see this
/// section's module doc for why that is the intended remaining shape.
fn prepare_cli_invocation(
    root: &str,
    rel: &str,
    subcommand: &str,
    rest: &[String],
) -> Result<Vec<String>, ShellError> {
    validate_cli_subcommand(subcommand)?;
    let input = resolve(root, rel)?;
    let mut args = vec![subcommand.to_owned(), input.display().to_string()];
    args.extend(rest.iter().cloned());
    Ok(args)
}

/// Run an allowlisted `brink-cli` subcommand as a Tauri sidecar, streaming
/// its stdout/stderr to the webview as `cli:output` events and resolving
/// to the process exit code once it terminates. See
/// [`prepare_cli_invocation`] for the argument-shaping/guard rules;
/// anything it rejects is returned here before the sidecar is ever
/// spawned.
#[tauri::command]
async fn run_cli(
    app: tauri::AppHandle,
    root: String,
    rel: String,
    subcommand: String,
    rest: Vec<String>,
) -> Result<i32, ShellError> {
    use tauri::Emitter;
    use tauri_plugin_shell::process::CommandEvent;
    use tauri_plugin_shell::ShellExt;

    let args = prepare_cli_invocation(&root, &rel, &subcommand, &rest)?;

    let sidecar = app
        .shell()
        .sidecar("brink-cli")
        .map_err(|e| ShellError::Sidecar(e.to_string()))?;
    let (mut rx, _child) = sidecar
        .args(&args)
        .spawn()
        .map_err(|e| ShellError::Sidecar(e.to_string()))?;

    let mut exit_code: i32 = -1;
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(bytes) => {
                let _ = app.emit(
                    "cli:output",
                    CliOutputLine {
                        stream: "stdout",
                        line: String::from_utf8_lossy(&bytes).into_owned(),
                    },
                );
            }
            CommandEvent::Stderr(bytes) => {
                let _ = app.emit(
                    "cli:output",
                    CliOutputLine {
                        stream: "stderr",
                        line: String::from_utf8_lossy(&bytes).into_owned(),
                    },
                );
            }
            CommandEvent::Terminated(payload) => {
                exit_code = payload.code.unwrap_or(-1);
            }
            CommandEvent::Error(message) => return Err(ShellError::Sidecar(message)),
            _ => {}
        }
    }
    Ok(exit_code)
}

// ── File associations (docs/desktop-shell-spec.md D3; #2393) ───────────
//
// `bundle.fileAssociations` (tauri.conf.json) registers `.ink`/`.brink`
// with the OS — this ONLY bites in the bundled `.app`; a dev run
// (`pnpm tauri dev`) never gets a file-open launch, so this whole path is
// simply unreached there. On macOS, double-clicking (or dragging onto the
// Dock icon) an associated file delivers `RunEvent::Opened` — at COLD
// launch that fires before the webview has loaded `main.tsx` and attached
// its listener, so events would otherwise be silently dropped (Tauri's JS
// event bus has no built-in replay). `PendingOpens` bridges that gap: it
// buffers paths until the frontend's one-time `take_pending_opens` pull at
// boot, after which it switches to live `shell:file-open` emits for any
// later opens delivered to the already-running app (Dock re-open, a second
// double-click) — never both for the same path, and never unbounded, since
// nothing is pushed once the buffer has been taken.
struct PendingOpens(std::sync::Mutex<Option<Vec<String>>>);

/// Drain paths buffered before the frontend was ready to receive them, and
/// flip the state to "ready" (`None`) so any *subsequent* `Opened` event
/// goes out as a live `shell:file-open` emit instead of piling up here.
/// Called exactly once by `src/main.tsx` at startup; safe to call again
/// (returns empty) but nothing else in the app does.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri implements `CommandArg` only for `State<'_, T>` taken by value \
              (tauri::state), so a command cannot declare `&State` — the by-value \
              parameter is the command ABI, not an avoidable move."
)]
fn take_pending_opens(state: tauri::State<'_, PendingOpens>) -> Vec<String> {
    state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .unwrap_or_default()
}

// ⚠ CI BLIND SPOT (#2428). Everything behind this cfg — `opened_url_to_path`,
// `handle_opened`, the `RunEvent::Opened` arm in `run()`, and the three tests
// at the bottom of this file that cover them — is compiled by NO CI lane:
// `.github/workflows/desktop-smoke.yml` runs `ubuntu-latest` only, so
// `cargo check`/`clippy`/`test` there all take the `#[cfg]`-out path. This is
// the file-association surface D3 keeps growing, and it is reviewed by eye
// until a macOS runner is ruled in — a cost question that is NOT settled
// here (docs/desktop-shell-spec.md, "CI coverage blind spots").
//
// The first mobile target additionally owes an `#[expect]` OUTSIDE this
// block: `tauri-macros`' `mobile_entry_point` expansion (2.6.3,
// src/mobile.rs) calls `stop_unwind(run)` in statement position, discarding
// `run()`'s `tauri::Result<()>` (`unused_must_use`, fatal under this lane's
// `-D warnings`), and its panic arm uses `eprintln!` — which #2415's deny
// set turns into `clippy::print_stderr`, since a proc macro's output carries
// call-site spans and so is not treated as an external-macro expansion (the
// same reason `run()` already carries an `#[expect(clippy::exit)]` for
// `generate_context!`). Its `std::process::abort()` is not caught by the
// current deny set. None of this is reachable today: nothing in this repo
// builds a mobile target.

/// Convert one `Opened` URL to an absolute filesystem path. `file://` is
/// the only scheme macOS delivers for a file association or Dock drop;
/// anything else (there shouldn't be anything else here) is dropped rather
/// than guessed at.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
fn opened_url_to_path(url: &tauri::Url) -> Option<String> {
    url.to_file_path().ok().map(|p| p.display().to_string())
}

/// Route one `RunEvent::Opened` batch: buffer it if the frontend hasn't
/// taken over yet, otherwise emit it live. See the `PendingOpens` doc
/// comment for why exactly one of those two happens per path.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
fn handle_opened(app: &tauri::AppHandle, urls: &[tauri::Url]) {
    use tauri::{Emitter, Manager};

    let paths: Vec<String> = urls.iter().filter_map(opened_url_to_path).collect();
    if paths.is_empty() {
        return;
    }
    let pending = app.state::<PendingOpens>();
    let mut guard = pending
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.as_mut() {
        Some(buffered) => buffered.extend(paths),
        None => {
            drop(guard);
            let _ = app.emit("shell:file-open", paths);
        }
    }
}

// ── Recent projects (docs/desktop-shell-spec.md D2; #2394) ─────────────
//
// `recents.json` in app-data: a JSON array of project-root paths,
// most-recent-first, capped at `RECENTS_MAX`, deduplicated by exact path.
// Same app-data-dir precedent as `append_backups`. `push_recent` is called
// by the frontend after every successful project open; `prune_recent` is
// called lazily — only when the frontend actually tried to open a recent
// entry and the open failed (e.g. the folder was deleted or moved) — never
// by a proactive existence sweep, so a normal `read_recents` stays a single
// fast file read with no filesystem stat per entry.

/// Cap on the persisted recents list (#2394's "~10").
const RECENTS_MAX: usize = 10;

/// The menu-id prefix for an Open Recent entry. Shared by `build_menu`
/// (producer, `format!("{OPEN_RECENT_ID_PREFIX}{path}")`) and
/// `on_menu_event` (consumer, `id.strip_prefix(OPEN_RECENT_ID_PREFIX)`) so
/// the two can never drift out of sync — this crate's only cargo coverage
/// is the deliberately NON-required `.github/workflows/desktop-smoke.yml`
/// lane (`docs/desktop-shell-spec.md` "Workspace placement"), so a typo'd
/// duplicate literal at either site would still ship a silently inert menu
/// behind a fully green *required* gate. See `open_recent_id_round_trips_posix_and_windows_paths`
/// for the round-trip this prefix is expected to survive.
const OPEN_RECENT_ID_PREFIX: &str = "open-recent:";

/// The app-data path for `recents.json`.
fn recents_path(app: &tauri::AppHandle) -> Result<PathBuf, ShellError> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| ShellError::Io {
            path: "app-data dir".to_owned(),
            source: std::io::Error::other(e),
        })?
        .join("recents.json"))
}

/// Load the persisted list, most-recent-first. A missing file is a fresh
/// install, not an error. A corrupt file is non-critical cached state (not
/// compiled project data — nothing here is a silent drop of anything the
/// user authored) so it self-heals to an empty list rather than surfacing a
/// parse error that would otherwise block every future project open.
fn load_recents(path: &Path) -> Result<Vec<String>, ShellError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_err(path, e)),
    };
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

/// Persist the list.
fn save_recents(path: &Path, list: &[String]) -> Result<(), ShellError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|e| ShellError::Io {
        path: path.display().to_string(),
        source: std::io::Error::other(e),
    })?;
    std::fs::write(path, json).map_err(|e| io_err(path, e))
}

/// Recents policy, pure and unit-testable without an `AppHandle` (mirrors
/// `resolve` / `project_ring_key` above): move `path` to the front,
/// deduplicating any existing entry for the same path, then cap.
fn recents_after_push(mut existing: Vec<String>, path: String, cap: usize) -> Vec<String> {
    existing.retain(|p| p != &path);
    existing.insert(0, path);
    existing.truncate(cap);
    existing
}

/// Drop one path from the list. Only ever called on a failed open — the
/// lazy half of "lazy-prune missing paths".
fn recents_after_prune(mut existing: Vec<String>, path: &str) -> Vec<String> {
    existing.retain(|p| p != path);
    existing
}

/// Rebuild the whole native menu bar with a fresh Open Recent submenu and
/// install it in place of the current one.
///
/// **Decision: rebuild-on-change, not a dynamic submenu.** Tauri v2 (muda)
/// menus have no "insert/remove item from this live submenu, in place"
/// affordance that plays nicely with a scalar list rebuilt from disk on
/// every push/prune — the closest primitives (`append_items` /
/// `remove_item`) would need us to track item identity across calls
/// ourselves for zero benefit at this size (a handful of items). Rebuilding
/// the entire `Menu` from scratch on every recents change and calling
/// `app.set_menu` is simpler, always correct (the new menu is built from
/// the just-persisted list, so it can never drift from `recents.json`), and
/// unmeasurably cheap next to the fs write that already happened in the
/// same command. `on_menu_event` is registered once on the `App` in
/// `run()`, not per-`Menu`, so it keeps firing correctly across rebuilds.
fn rebuild_menu(app: &tauri::AppHandle, recents: &[String]) -> Result<(), ShellError> {
    let to_shell_err = |e: tauri::Error| ShellError::Io {
        path: "menu".to_owned(),
        source: std::io::Error::other(e),
    };
    let menu = build_menu(app, recents).map_err(to_shell_err)?;
    app.set_menu(menu).map_err(to_shell_err)?;
    Ok(())
}

#[tauri::command]
async fn read_recents(app: tauri::AppHandle) -> Result<Vec<String>, ShellError> {
    load_recents(&recents_path(&app)?)
}

#[tauri::command]
async fn push_recent(app: tauri::AppHandle, path: String) -> Result<Vec<String>, ShellError> {
    let file = recents_path(&app)?;
    let list = recents_after_push(load_recents(&file)?, path, RECENTS_MAX);
    save_recents(&file, &list)?;
    rebuild_menu(&app, &list)?;
    Ok(list)
}

#[tauri::command]
async fn prune_recent(app: tauri::AppHandle, path: String) -> Result<Vec<String>, ShellError> {
    let file = recents_path(&app)?;
    let list = recents_after_prune(load_recents(&file)?, &path);
    save_recents(&file, &list)?;
    rebuild_menu(&app, &list)?;
    Ok(list)
}

/// Whether a project root still exists as a directory. Used by the frontend
/// to gate lazy recents pruning (#2394 review finding): `openRecent`'s catch
/// handler must only prune an entry when the folder itself is actually
/// gone, not on every failure `openProject` can raise (a transient
/// `mountStudio` error, a permission error, a file deleted mid-listing) —
/// those must surface to the user, not silently delete a valid project from
/// `recents.json` and the native Open Recent submenu.
#[tauri::command]
async fn project_root_exists(path: String) -> bool {
    Path::new(&path).is_dir()
}

/// Native folder picker. ⚠ This command (like every command here) MUST be
/// `async`: Tauri v2 runs **sync** commands on the **main thread**, and
/// `blocking_pick_folder` on the main thread deadlocks — the dialog needs
/// the main thread free to pump its own events. (Observed live: the first
/// D1 run hung inside the native dialog.) `async` commands run on the
/// runtime's worker pool, where blocking on the dialog is the documented
/// pattern; the plugin dispatches the actual `NSOpenPanel` to the main
/// thread itself.
#[tauri::command]
async fn pick_project_folder(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
}

/// Build the native menu bar. Project lifecycle (Open/Close) is
/// SHELL-owned — it sits above `mountStudio`, so hand-wiring it here does
/// not conflict with the D2 ruling that *studio* commands reach menus via
/// the command registry (docs/desktop-shell-spec.md "Menus"); Save/Play
/// items arrive in D2 through that registry. The Edit submenu's predefined
/// roles are load-bearing: without them the webview loses ⌘C/⌘V/⌘X on
/// macOS.
///
/// The app-menu Quit item is a plain `MenuItem`, not
/// `PredefinedMenuItem::quit` (#2370): the predefined item's native
/// teardown does not reliably reach the webview (`on_menu_event` /
/// `WindowEvent::CloseRequested`) on every platform, which would bypass the
/// await-the-final-save quit path entirely. Routing it as `menu:quit`, the
/// same way Open/Close Project already forward, guarantees ⌘Q funnels
/// through the identical guarded path as closing the window.
///
/// `recents` (#2394) drives the File → Open Recent submenu; see
/// `rebuild_menu`'s doc comment for why this whole function is re-run on
/// every recents change rather than mutating a live submenu. Each entry's
/// item id is `open-recent:{path}`; `on_menu_event` in `run()` strips that
/// prefix back off to recover the path (paths may themselves contain `:`
/// on Windows, but `strip_prefix` only ever touches the leading match).
fn build_menu(
    handle: &tauri::AppHandle,
    recents: &[String],
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

    let quit = MenuItem::with_id(
        handle,
        "quit",
        "Quit Brink Studio",
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let app_menu = Submenu::with_items(
        handle,
        "Brink Studio",
        true,
        &[
            &PredefinedMenuItem::about(handle, None, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &quit,
        ],
    )?;
    let open = MenuItem::with_id(
        handle,
        "open-project",
        "Open Project…",
        true,
        Some("CmdOrCtrl+O"),
    )?;
    // Shift-modified so plain ⌘W keeps its native close-window role.
    let close = MenuItem::with_id(
        handle,
        "close-project",
        "Close Project",
        true,
        Some("CmdOrCtrl+Shift+W"),
    )?;
    // D3 slice 1 (#2391): compile the open project and write the bytes
    // through a native save dialog. Unmodified-key-free (no obvious
    // platform-conventional accelerator for "export"); reachable from the
    // menu and the command palette isn't in scope for this shell-owned item.
    let export_inkb = MenuItem::with_id(
        handle,
        "export-inkb",
        "Export Story (.inkb)…",
        true,
        None::<&str>,
    )?;
    // Proves the sidecar path end-to-end (D3, #2392); the fuller intl UI
    // (locale picker, progress, batch ops beyond xliff export) is future
    // work — this item exists to exercise one real path, not to be it.
    let export_xliff =
        MenuItem::with_id(handle, "export-xliff", "Export XLIFF…", true, None::<&str>)?;
    let recent_items: Vec<MenuItem<tauri::Wry>> = if recents.is_empty() {
        vec![MenuItem::with_id(
            handle,
            "no-recents",
            "No Recent Projects",
            false,
            None::<&str>,
        )?]
    } else {
        recents
            .iter()
            .map(|path| {
                MenuItem::with_id(
                    handle,
                    format!("{OPEN_RECENT_ID_PREFIX}{path}"),
                    path,
                    true,
                    None::<&str>,
                )
            })
            .collect::<tauri::Result<Vec<_>>>()?
    };
    // Unsized coercion via the closure's return type rather than an `as`
    // cast: `trivial_casts` (deny, repo-wide) rejects the cast spelling.
    let recent_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = recent_items
        .iter()
        .map(|item| -> &dyn IsMenuItem<tauri::Wry> { item })
        .collect();
    let open_recent = Submenu::with_items(handle, "Open Recent", true, &recent_refs)?;
    let file_menu = Submenu::with_items(
        handle,
        "File",
        true,
        &[
            &open,
            &open_recent,
            &close,
            &PredefinedMenuItem::separator(handle)?,
            &export_inkb,
            &export_xliff,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::close_window(handle, None)?,
        ],
    )?;
    let edit_menu = Submenu::with_items(
        handle,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(handle, None)?,
            &PredefinedMenuItem::redo(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::cut(handle, None)?,
            &PredefinedMenuItem::copy(handle, None)?,
            &PredefinedMenuItem::paste(handle, None)?,
            &PredefinedMenuItem::select_all(handle, None)?,
        ],
    )?;
    Menu::with_items(handle, &[&app_menu, &file_menu, &edit_menu])
}

/// Build and run the shell. Returns the builder's error rather than
/// panicking on it (#2415): a failure here means the app cannot start at
/// all, and `main` propagating it exits non-zero with the error printed by
/// the standard `Termination` impl — strictly more useful than an
/// `.expect()` abort, and it is the shape the repo-wide `expect_used` deny
/// asks for now that this crate is finally covered by it.
///
/// (Under `mobile` the generated entry point calls this through
/// `stop_unwind`, which is generic over the return type and discards it —
/// an `unused_must_use` this crate's deny set will reject on the first
/// mobile build; see the ⚠ marker above `opened_url_to_path`, #2428.)
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[expect(
    clippy::exit,
    reason = "the flagged `process::exit(101)` is inside `tauri::generate_context!`'s \
              own expansion — tauri-codegen's `inner()` fallback for a panicking \
              context-creation thread — not code this crate writes. Unlike \
              `expect_used`, `clippy::exit` does not suppress itself inside an \
              external macro's expansion, so the site has to be silenced here."
)]
pub fn run() -> tauri::Result<()> {
    use tauri::Emitter;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(WatchState(std::sync::Mutex::new(None)))
        .manage(PendingOpens(std::sync::Mutex::new(Some(Vec::new()))))
        .setup(|app| {
            // Load whatever recents.json already holds so a relaunch shows
            // the File → Open Recent submenu populated immediately, not
            // just after the first push in this session (#2394).
            let initial_recents = load_recents(&recents_path(app.handle())?)?;
            let menu = build_menu(app.handle(), &initial_recents)?;
            app.set_menu(menu)?;
            // Menu events forward to the webview as plain events; the
            // frontend owns what "open" and "close" mean (it holds the
            // StudioHandle). The shell stays policy-free.
            app.on_menu_event(|app, event| {
                let id = event.id().as_ref();
                if let Some(path) = id.strip_prefix(OPEN_RECENT_ID_PREFIX) {
                    let _ = app.emit("menu:open-recent", path.to_owned());
                    return;
                }
                let forwarded = match id {
                    "open-project" => Some("menu:open-project"),
                    "close-project" => Some("menu:close-project"),
                    "export-inkb" => Some("menu:export-inkb"),
                    "export-xliff" => Some("menu:export-xliff"),
                    "quit" => Some("menu:quit"),
                    _ => None,
                };
                if let Some(name) = forwarded {
                    let _ = app.emit(name, ());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_files,
            read_file,
            write_file,
            delete_file,
            rename_file,
            append_backups,
            start_watch,
            stop_watch,
            pick_project_folder,
            save_bytes_dialog,
            run_cli,
            take_pending_opens,
            read_recents,
            push_recent,
            prune_recent,
            project_root_exists,
        ])
        .build(tauri::generate_context!())?
        .run(move |app_handle, event| {
            // File associations (#2393) only exist on macOS/iOS/Android —
            // `RunEvent::Opened` itself is cfg-gated out of the enum on
            // other platforms, so the arm is compiled out there too rather
            // than matching a variant that doesn't exist.
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
            if let tauri::RunEvent::Opened { urls } = &event {
                handle_opened(app_handle, urls);
            }
            // NOTE (2026-08-21 review, #2927): this arm previously called
            // `api.prevent_exit()` unconditionally on every
            // `RunEvent::ExitRequested`, intending to funnel macOS Dock
            // Quit through the same guarded `menu:quit` webview path as
            // app-menu ⌘Q. It was removed for two independent reasons —
            // either one alone would be enough:
            //
            // 1. It did not actually fix Dock Quit (#2400 stays open).
            //    `tauri-runtime-wry` only emits `ExitRequested{code: None}`
            //    when the LAST WINDOW IS DESTROYED (tao's
            //    `WindowEvent::Destroyed` -> empty window list ->
            //    `callback(RunEvent::ExitRequested{code:None,tx})`); Dock
            //    Quit reaches macOS as `applicationShouldTerminate:` /
            //    `LoopDestroyed` -> `RunEvent::Exit`, a path this arm never
            //    saw. Nothing in this crate's dependency tree implements
            //    `applicationShouldTerminate:` to redirect it.
            //
            // 2. On every path this arm DOES see (⌘Q via `menu:quit`, and
            //    red-button close via the webview's `onCloseRequested`),
            //    `ExitRequested` fires only as a SIDE EFFECT of
            //    `handleQuitRequested`'s own `getCurrentWindow().destroy()`
            //    call — i.e. strictly after the guarded save had already
            //    run. Calling `prevent_exit()` there halted an exit the app
            //    itself had just finished asking for, leaving a windowless
            //    zombie process (Force Quit only) while re-emitting
            //    `menu:quit` to an already-destroyed webview.
            //
            // A correct Dock Quit fix needs a mechanism Dock Quit actually
            // reaches (e.g. overriding `applicationShouldTerminate:` /
            // `NSTerminateLater`) — a maintainer design call, tracked on
            // #2400 — not a guard on this event. Until then, letting
            // `ExitRequested` fall through to its default (`ControlFlow::
            // Exit`) is correct: the two funnels that matter (⌘Q,
            // red-button close) both already await the guarded save via
            // `handleQuitRequested` before they ever destroy the window.
            let _ = (app_handle, &event);
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo root, three levels up from `src-tauri`
    /// (`packages/brink-desktop/src-tauri`).
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    /// The significant (non-comment, non-blank) lines of every table in
    /// `manifest` whose header starts with `prefix`, with the prefix
    /// normalized away so a `[workspace.lints.clippy]` block and a
    /// `[lints.clippy]` block compare equal.
    fn lint_table(manifest: &str, prefix: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut inside = false;
        for line in manifest.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                inside = line.starts_with(prefix);
                if inside {
                    out.push(format!("[{}", &line[prefix.len()..]));
                }
            } else if inside && !line.is_empty() && !line.starts_with('#') {
                out.push(line.to_owned());
            }
        }
        out
    }

    fn significant_lines(text: &str) -> Vec<String> {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    }

    /// This crate is its own cargo workspace (docs/desktop-shell-spec.md
    /// "Workspace placement"), so `[lints] workspace = true` is unavailable
    /// and its `[lints]` table is a hand-maintained COPY of root
    /// `Cargo.toml`'s `[workspace.lints]` (#2415). A copy drifts silently —
    /// this test is the only thing that notices when the root policy gains
    /// or loses a lint and this crate's copy does not follow. Before #2415
    /// there was no `[lints]` table here at all, so this test fails on that
    /// state too.
    #[test]
    fn lint_policy_matches_the_root_workspace() {
        let root_manifest = repo_root().join("Cargo.toml");
        assert!(
            root_manifest.is_file(),
            "root manifest should exist at {root_manifest:?}"
        );
        let root = std::fs::read_to_string(&root_manifest)
            .expect("just asserted the root manifest exists");
        let mine =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
                .expect("this crate's own manifest is always readable from its own test");

        let root_lints = lint_table(&root, "[workspace.lints");
        let my_lints = lint_table(&mine, "[lints");
        assert!(
            !root_lints.is_empty(),
            "root Cargo.toml should still declare [workspace.lints]"
        );
        assert_eq!(
            my_lints, root_lints,
            "src-tauri's [lints] table has drifted from root's [workspace.lints]"
        );
    }

    /// Same drift class for the lint *configuration*: clippy searches for
    /// `clippy.toml` no further up than the workspace root, and this crate
    /// IS its own workspace root, so the repo root's test carve-outs
    /// (`allow-unwrap-in-tests` and friends) never reach it either.
    #[test]
    fn clippy_config_matches_the_root_workspace() {
        let root_config = repo_root().join("clippy.toml");
        assert!(
            root_config.is_file(),
            "root clippy.toml should exist at {root_config:?}"
        );
        let root =
            std::fs::read_to_string(&root_config).expect("just asserted the root config exists");
        let my_config = Path::new(env!("CARGO_MANIFEST_DIR")).join("clippy.toml");
        assert!(
            my_config.is_file(),
            "src-tauri needs its own clippy.toml at {my_config:?} — the root one does not reach it"
        );
        let mine = std::fs::read_to_string(&my_config).expect("just asserted this config exists");
        assert_eq!(
            significant_lines(&mine),
            significant_lines(&root),
            "src-tauri's clippy.toml has drifted from the root one"
        );
    }

    /// The `name = value` entries of the first table headed exactly
    /// `header`, as `(name, value)` pairs with the raw TOML value text.
    /// Deliberately naive: both manifests this reads are plain one-entry-
    /// per-line dependency tables with no nested sub-tables.
    fn manifest_table(manifest: &str, header: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut inside = false;
        for line in manifest.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                inside = line == header;
            } else if inside && !line.is_empty() && !line.starts_with('#') {
                if let Some((name, value)) = line.split_once('=') {
                    out.push((name.trim().to_owned(), value.trim().to_owned()));
                }
            }
        }
        out
    }

    /// The version requirement out of a dependency table's value, for both
    /// spellings the two manifests use: a bare `"1"` and an inline table
    /// `{ version = "1", features = [...] }`. `None` for anything else
    /// (a path/git dependency, `workspace = true`), which the caller skips.
    fn version_requirement(value: &str) -> Option<&str> {
        let rest = if value.starts_with('"') {
            value
        } else {
            let at = value.find("version")?;
            value[at..].split_once('=')?.1.trim_start()
        };
        rest.strip_prefix('"')?.split('"').next()
    }

    /// A version's numeric `(major, minor, patch)`, ignoring any
    /// pre-release or build metadata (`1.1.4+spec-1.1.0` -> `(1, 1, 4)`).
    /// Missing components read as zero, so a requirement of `"2"` parses.
    fn numeric_version(version: &str) -> Option<(u64, u64, u64)> {
        let core = version.split(['-', '+']).next().unwrap_or(version);
        let mut parts = core.split('.');
        let major: u64 = parts.next()?.parse().ok()?;
        let minor: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let patch: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Some((major, minor, patch))
    }

    /// Cargo's semver-compatibility unit: the major for `1.x` and above,
    /// the minor for a `0.x` release. Two versions sharing a unit are
    /// interchangeable to cargo, so they are the ones worth comparing —
    /// `0.39.4` and `0.41.0` are simply different dependencies.
    fn compatibility_unit(version: &str) -> Option<(u64, u64)> {
        let (major, minor, _) = numeric_version(version)?;
        Some(if major > 0 { (major, 0) } else { (0, minor) })
    }

    /// Every `name = version` pair in a `Cargo.lock`. A name can repeat —
    /// both lockfiles carry several crates at two incompatible versions.
    fn lock_versions(lock: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut name = String::new();
        for line in lock.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("name = ") {
                name = unquote(value).to_owned();
            } else if let Some(value) = line.strip_prefix("version = ") {
                if !name.is_empty() {
                    out.push((std::mem::take(&mut name), unquote(value).to_owned()));
                }
            }
        }
        out
    }

    /// The highest version of `name` in `lock` that is semver-compatible
    /// with `unit`, or `None` when the lock carries no such version.
    fn resolved_in_unit(lock: &[(String, String)], name: &str, unit: (u64, u64)) -> Option<String> {
        lock.iter()
            .filter(|(crate_name, version)| {
                crate_name == name && compatibility_unit(version) == Some(unit)
            })
            .map(|(_, version)| version.clone())
            .max_by_key(|version| numeric_version(version))
    }

    /// The highest version of `name` in `lock` whose compatibility unit is
    /// ABOVE `unit`, or `None` when the lock carries nothing past it.
    ///
    /// `resolved_in_unit` returning `None` for the root's unit is ambiguous
    /// on its own: it means either src-tauri hasn't caught up to a root
    /// bump yet, or src-tauri already bumped past root's current
    /// requirement. This distinguishes the two, so the caller can blame
    /// whichever side actually hasn't moved.
    fn resolved_above_unit(
        lock: &[(String, String)],
        name: &str,
        unit: (u64, u64),
    ) -> Option<String> {
        lock.iter()
            .filter(|(crate_name, _)| crate_name == name)
            .filter_map(|(_, version)| {
                let their_unit = compatibility_unit(version)?;
                (their_unit > unit).then(|| version.clone())
            })
            .max_by_key(|version| numeric_version(version))
    }

    /// One dependency's drift check, pure and synthetic-data driven so it
    /// is unit-testable without mutating real manifests/lockfiles.
    /// `Err(message)` is exactly the panic message
    /// `dependency_versions_track_the_root_workspace` should fail with;
    /// `Ok(())` means `name` is in sync.
    ///
    /// `my_lock` carrying nothing in `unit` is ambiguous by itself: either
    /// src-tauri hasn't caught up to a root bump yet, or src-tauri already
    /// bumped past the version `unit` names and root hasn't caught up. This
    /// checks `resolved_above_unit` first so the remediation blames
    /// whichever side actually hasn't moved (PR #2462 review finding).
    fn dependency_drift(
        name: &str,
        root_requirement: &str,
        root_resolved: &str,
        unit: (u64, u64),
        my_lock: &[(String, String)],
    ) -> Result<(), String> {
        let Some(my_resolved) = resolved_in_unit(my_lock, name, unit) else {
            if let Some(ahead) = resolved_above_unit(my_lock, name, unit) {
                return Err(format!(
                    "src-tauri's Cargo.lock already pins {name} {ahead}, ahead of the root \
                     workspace's {root_requirement:?} requirement (resolved to \
                     {root_resolved}) — src-tauri is ahead here. Bump {name} in the root \
                     workspace's Cargo.toml instead."
                ));
            }
            return Err(format!(
                "the root workspace declares {name} {root_requirement:?} and resolves it to \
                 {root_resolved}, but src-tauri's Cargo.lock carries no compatible version — \
                 the root's major bump never propagated across the workspace fence. Bump \
                 {name} in src-tauri/Cargo.toml as well."
            ));
        };

        if numeric_version(&my_resolved) < numeric_version(root_resolved) {
            return Err(format!(
                "src-tauri's Cargo.lock pins {name} {my_resolved}, behind the root \
                 workspace's {root_resolved}. Its excluded workspace does not follow root \
                 bumps on its own — run `cargo update -p {name}` in src-tauri."
            ));
        }
        Ok(())
    }

    /// Every `[dependencies]`/`[build-dependencies]` entry `my_deps`
    /// declares that root's `[workspace.dependencies]` also declares — the
    /// deliberate overlap, where a root bump is meant to propagate. (Not
    /// first-party crates: #2451's body names `brink-runtime`/
    /// `brink-format`, but `src-tauri` depends on no workspace crate at
    /// all — it reaches the compiler only through the `brink-cli` sidecar
    /// binary. Its lock holds exactly one `brink-*` package, itself.)
    /// Transitive crates are out of scope because the two graphs
    /// legitimately resolve differently; see the `toml` divergence
    /// recorded on #2451.
    ///
    /// `Ok(names)` on success; `Err(message)` on the first drift found OR
    /// when a root value in the overlap could not be parsed as a version
    /// requirement — the latter used to `continue` past silently, quietly
    /// shrinking the checked set instead of failing (PR #2462 review
    /// finding). Pure and synthetic-data driven so both branches are
    /// unit-testable without mutating real manifests/lockfiles; see
    /// `dependency_versions_track_the_root_workspace`, which drives this
    /// over the real root/src-tauri manifests and locks.
    fn overlap_drift_check(
        root_deps: &[(String, String)],
        my_deps: &[(String, String)],
        root_lock: &[(String, String)],
        my_lock: &[(String, String)],
    ) -> Result<Vec<String>, String> {
        let mut checked = Vec::new();
        let mut unparsed_root_values = Vec::new();

        for (name, my_value) in my_deps {
            let Some((_, root_value)) = root_deps.iter().find(|(dep, _)| dep == name) else {
                continue;
            };
            let Some(root_requirement) = version_requirement(root_value) else {
                unparsed_root_values.push(format!("{name} (root value {root_value:?})"));
                continue;
            };
            if version_requirement(my_value).is_none() {
                return Err(format!(
                    "{name} is declared in both manifests, so this one should name a \
                     version requirement; it reads {my_value:?}"
                ));
            }
            let Some(unit) = compatibility_unit(root_requirement) else {
                continue;
            };

            let Some(root_resolved) = resolved_in_unit(root_lock, name, unit) else {
                return Err(format!(
                    "root Cargo.lock has no {name} compatible with the {root_requirement:?} \
                     it declares — the root lock is itself stale"
                ));
            };

            dependency_drift(name, root_requirement, &root_resolved, unit, my_lock)?;
            checked.push(name.clone());
        }

        if !unparsed_root_values.is_empty() {
            return Err(format!(
                "these dependencies are declared by both manifests but this test could not \
                 parse their root [workspace.dependencies] version requirement, so they \
                 would have silently skipped the drift check: {unparsed_root_values:?}"
            ));
        }

        Ok(checked)
    }

    /// This crate's own workspace means its own `Cargo.lock` (#2451), and
    /// `cargo test --locked` in the smoke lane only proves that lock is
    /// internally consistent with the `Cargo.toml` NEXT TO IT — never that
    /// it still tracks the root workspace's versions. PR #2446 widened
    /// `desktop-smoke.yml`'s path filter so a root `Cargo.toml`/`Cargo.lock`
    /// bump at least RUNS this lane; this is the assertion inside it that
    /// the widened filter had nothing to trigger.
    #[test]
    fn dependency_versions_track_the_root_workspace() {
        let root_manifest = repo_root().join("Cargo.toml");
        let root_lockfile = repo_root().join("Cargo.lock");
        assert!(
            root_lockfile.is_file(),
            "root Cargo.lock should exist at {root_lockfile:?}"
        );
        let root = std::fs::read_to_string(&root_manifest)
            .expect("the root manifest is read by lint_policy_matches_the_root_workspace too");
        let root_lock = lock_versions(
            &std::fs::read_to_string(&root_lockfile).expect("just asserted the root lock exists"),
        );

        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mine = std::fs::read_to_string(manifest_dir.join("Cargo.toml"))
            .expect("this crate's own manifest is always readable from its own test");
        let my_lock = lock_versions(
            &std::fs::read_to_string(manifest_dir.join("Cargo.lock"))
                .expect("this crate's own lock is always readable from its own test"),
        );

        let root_deps = manifest_table(&root, "[workspace.dependencies]");
        let mut my_deps = manifest_table(&mine, "[dependencies]");
        my_deps.extend(manifest_table(&mine, "[build-dependencies]"));

        let result = overlap_drift_check(&root_deps, &my_deps, &root_lock, &my_lock);
        assert!(
            result.is_ok(),
            "{}",
            result.as_ref().err().cloned().unwrap_or_default()
        );
        let checked = result.expect("just asserted the drift check succeeded");

        assert!(
            !checked.is_empty(),
            "no dependency is declared by both root Cargo.toml's \
             [workspace.dependencies] and src-tauri's — either the overlap really is empty \
             (then this test is dead and should say so) or one of the two tables stopped \
             parsing"
        );
    }

    /// Reproduces the PR #2462 review finding directly: with src-tauri at
    /// `thiserror = "3"` / lock 3.0.0 and root still `"2"` / 2.0.18, the
    /// old code asserted `resolved_in_unit(&my_lock, "thiserror", (2, 0))`
    /// was `Some` and always failed with the "root's major bump never
    /// propagated" message — even though no root bump happened and
    /// src-tauri is the side that is ahead.
    #[test]
    fn dependency_drift_blames_root_when_src_tauri_is_already_ahead() {
        let my_lock = [("thiserror".to_owned(), "3.0.0".to_owned())];
        let err = dependency_drift("thiserror", "\"2\"", "2.0.18", (2, 0), &my_lock)
            .expect_err("src-tauri's lock has no 2.x thiserror to compare against root's unit");
        assert!(
            err.contains("src-tauri is ahead here"),
            "expected the root-side remediation, got: {err}"
        );
        assert!(
            err.contains("Bump thiserror in the root workspace's Cargo.toml instead"),
            "expected the root-side remediation, got: {err}"
        );
    }

    /// The companion case: src-tauri really hasn't caught up to a root
    /// bump, so the original "bump src-tauri" remediation is the correct
    /// one and must still fire.
    #[test]
    fn dependency_drift_blames_src_tauri_when_it_has_not_caught_up() {
        let my_lock = [("thiserror".to_owned(), "2.0.18".to_owned())];
        let err = dependency_drift("thiserror", "\"3\"", "3.0.0", (3, 0), &my_lock)
            .expect_err("src-tauri's lock has no 3.x thiserror yet");
        assert!(
            err.contains("Bump thiserror in src-tauri/Cargo.toml as well"),
            "expected the src-tauri-side remediation, got: {err}"
        );
    }

    /// Reproduces the PR #2462 review finding directly: reformatting a
    /// root `[workspace.dependencies]` entry so `version_requirement` can't
    /// parse it (here, a dependency with no `version` key at all) used to
    /// just `continue` past it, shrinking `checked` with no failure.
    #[test]
    fn overlap_drift_check_fails_on_an_unparsed_root_value_instead_of_dropping_it() {
        let root_deps = [
            (
                "serde".to_owned(),
                "{ git = \"https://example.com/serde\" }".to_owned(),
            ),
            ("thiserror".to_owned(), "\"2\"".to_owned()),
        ];
        let my_deps = [
            ("serde".to_owned(), "\"1\"".to_owned()),
            ("thiserror".to_owned(), "\"2\"".to_owned()),
        ];
        let root_lock = [
            ("serde".to_owned(), "1.0.228".to_owned()),
            ("thiserror".to_owned(), "2.0.18".to_owned()),
        ];
        let my_lock = [
            ("serde".to_owned(), "1.0.229".to_owned()),
            ("thiserror".to_owned(), "2.0.19".to_owned()),
        ];

        let err = overlap_drift_check(&root_deps, &my_deps, &root_lock, &my_lock)
            .expect_err("serde's root value has no `version` key for version_requirement to find");
        assert!(
            err.contains("serde"),
            "expected the failure to name the dropped dependency, got: {err}"
        );
    }

    /// Read one workflow out of `.github/workflows`.
    fn workflow(file: &str) -> String {
        let path = repo_root().join(".github/workflows").join(file);
        assert!(path.is_file(), "workflow should exist at {path:?}");
        std::fs::read_to_string(&path).expect("just asserted the workflow exists")
    }

    /// Strip one matching pair of surrounding quotes. Only a pair: a
    /// double-quoted `if:` expression keeps the single quotes inside it
    /// (`steps.x.outcome == 'success'`), which a blanket trim would eat.
    fn unquote(value: &str) -> &str {
        for quote in ['"', '\''] {
            if let Some(inner) = value
                .strip_prefix(quote)
                .and_then(|v| v.strip_suffix(quote))
            {
                return inner;
            }
        }
        value
    }

    /// The entries of a workflow's single `paths:` filter, unquoted.
    /// `desktop-smoke.yml` has exactly one (under `pull_request`).
    fn path_filter(workflow: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut inside = false;
        for line in workflow.lines() {
            let line = line.trim();
            if line == "paths:" {
                inside = true;
            } else if inside {
                if let Some(entry) = line.strip_prefix("- ") {
                    out.push(unquote(entry).to_owned());
                } else if !line.is_empty() && !line.starts_with('#') {
                    break;
                }
            }
        }
        out
    }

    /// Every step of a workflow paired with its `id:` and `if:` (each empty
    /// when the step declares none). A `- uses:` step pushes an unnamed
    /// entry so its own `id:`/`if:` are never misattributed to the step
    /// above it. Carrying `id` alongside `condition` lets a test check a
    /// prerequisite named in a dependant's `if:` actually names a step that
    /// still exists — not just that the `if:` text is well-formed.
    fn steps_with_conditions(workflow: &str) -> Vec<(String, String, String)> {
        let mut out: Vec<(String, String, String)> = Vec::new();
        for line in workflow.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix("- name: ") {
                out.push((name.to_owned(), String::new(), String::new()));
            } else if line.starts_with("- uses: ") {
                out.push((String::new(), String::new(), String::new()));
            } else if let Some(id) = line.strip_prefix("id: ") {
                if let Some(step) = out.last_mut() {
                    step.1 = id.to_owned();
                }
            } else if let Some(condition) = line.strip_prefix("if: ") {
                if let Some(step) = out.last_mut() {
                    step.2 = unquote(condition).to_owned();
                }
            }
        }
        out
    }

    /// The desktop unit suite (`vitest`) has exactly one home: a step in
    /// `ci.yml`'s `frontend` job. The smoke lane deliberately does not
    /// duplicate it (docs/desktop-shell-spec.md "Workspace placement" — it
    /// builds no Tauri graph there, so it never violated the "required
    /// lanes must not grow a Tauri build" fence), which leaves deleting
    /// that one step free to drop the whole suite in silence. This test is
    /// what notices — and it lives on the cargo side deliberately, because
    /// a test inside the vitest suite could not fail for its own removal.
    /// `desktop-smoke.yml`'s path filter now includes `ci.yml`, so the PR
    /// that removes the step is the PR this fails on (#2418 gap 3).
    #[test]
    fn ci_workflow_still_runs_the_desktop_vitest_suite() {
        let ci = workflow("ci.yml");
        assert!(
            ci.lines()
                .any(|line| line.trim() == "run: pnpm --filter @brink/desktop test"),
            "ci.yml no longer runs the desktop vitest suite — restore the step, or \
             give the suite another home and update this test plus \
             docs/desktop-shell-spec.md \"Workspace placement\""
        );
    }

    /// The smoke lane's `pull_request` filter has to list every input that
    /// can break it, not just the trees it checks (#2418 gap 2): otherwise
    /// a lockfile, root-manifest or toolchain change runs this lane only on
    /// the post-merge push to `main`, after the break has landed — and the
    /// two `*_matches_the_root_workspace` tests above, plus
    /// `dependency_versions_track_the_root_workspace` (#2451, which also
    /// compares root's `Cargo.lock`), whose entire purpose is catching
    /// root-policy drift, cannot fail the PR that causes it.
    ///
    /// `crates/brink-cli/**` is deliberately NOT in this list (#2477):
    /// `BRINK_SIDECAR_STUB: "1"` is unconditional in this workflow's `env:`
    /// block, so `ensure-cli-sidecar.mjs` never runs `cargo build -p
    /// brink-cli --release` in this lane — see `STUB_SIDECAR` in
    /// `packages/brink-desktop/scripts/ensure-cli-sidecar.mjs`, which stages
    /// a placeholder without reading any `brink-cli` source. `src-tauri` is
    /// its own excluded workspace and does not depend on the `brink-cli`
    /// crate either, so nothing left in this lane can notice a
    /// `crates/brink-cli/**` change; watching that tree here would only
    /// trigger the job for a change it can no longer detect.
    #[test]
    fn desktop_smoke_path_filter_covers_its_shared_inputs() {
        let entries = path_filter(&workflow("desktop-smoke.yml"));
        for required in [
            "packages/brink-desktop/**",
            "pnpm-lock.yaml",
            "Cargo.toml",
            "Cargo.lock",
            "clippy.toml",
            "rust-toolchain.toml",
            // #2488/#2522: `deny.toml` is the policy the `cargo-deny
            // (src-tauri)` step resolves AND the file
            // `deny_toml_admits_mpl_for_the_transitive_tauri_dependencies`
            // parses, so a PR editing only it must still trigger this lane.
            "deny.toml",
            ".github/workflows/ci.yml",
            ".github/workflows/desktop-smoke.yml",
            // #2504: the individual entries above are not enough — a
            // reordered npm-release.yml, or a brand-new workflow file
            // adding a `pnpm install --frozen-lockfile` lane, would
            // otherwise trigger this lane only on the post-merge push to
            // `main`. See `every_pnpm_install_lane_builds_wasm_first_in_the_same_job`.
            ".github/workflows/**",
        ] {
            assert!(
                entries.iter().any(|entry| entry == required),
                "desktop-smoke.yml's pull_request path filter should list {required:?}; \
                 it lists {entries:?}"
            );
        }
        assert!(
            !entries.iter().any(|entry| entry == "crates/brink-cli/**"),
            "desktop-smoke.yml's pull_request path filter should NOT list \
             \"crates/brink-cli/**\" (#2477): BRINK_SIDECAR_STUB is unconditional in this \
             workflow, so nothing left in this lane can notice a brink-cli source change \
             — re-adding the entry without also restoring something that reads brink-cli \
             sources would just resurrect the dead-weight trigger this test now guards \
             against"
        );
    }

    /// The entries of the `paths:` filter nested under `pull_request:`,
    /// specifically. `path_filter` alone returns the FIRST `paths:` block
    /// found in the file — which, in `desktop-bundle-smoke.yml` since
    /// #2716, is `push`'s (declared first in `on:`), not `pull_request`'s.
    /// Slicing the workflow text from the `pull_request:` trigger line
    /// onward makes ITS `paths:` block the first one `path_filter` finds.
    fn pull_request_path_filter(workflow: &str) -> Vec<String> {
        let idx = workflow
            .find("\n  pull_request:")
            .expect("workflow should have a top-level `pull_request:` trigger");
        path_filter(&workflow[idx..])
    }

    /// The entries of the `paths:` filter nested under `push:`, specifically
    /// — bounded to the slice between the `push:` and `pull_request:`
    /// trigger lines. A bare `path_filter(&contents)` reads whichever
    /// `paths:` block happens to appear FIRST in the file, which is only
    /// `push`'s by coincidence of `on:` ordering: delete `push`'s `paths:`
    /// block (the exact regression #2716 fixed) and the first block in the
    /// file becomes `pull_request`'s instead, so an unbounded read would
    /// silently return `pull_request`'s list for BOTH triggers — the
    /// mismatch test below would then compare a list to itself and pass
    /// even though the fix had been reverted. Bounding the read positionally
    /// to the `push:`..`pull_request:` span closes that hole: if `push`
    /// carries no `paths:` filter in that span, this returns empty rather
    /// than borrowing `pull_request`'s.
    fn push_path_filter(workflow: &str) -> Vec<String> {
        let push_idx = workflow
            .find("\n  push:")
            .expect("workflow should have a top-level `push:` trigger");
        let pull_request_idx = workflow
            .find("\n  pull_request:")
            .expect("workflow should have a top-level `pull_request:` trigger");
        assert!(
            push_idx < pull_request_idx,
            "expected `push:` to precede `pull_request:` in the `on:` block"
        );
        path_filter(&workflow[push_idx..pull_request_idx])
    }

    /// #2716: `desktop-bundle-smoke.yml`'s `push` trigger had NO `paths:`
    /// filter at all — every push to `main` re-ran the whole lane (a real
    /// `cargo build -p brink-cli --release` plus the full `src-tauri`
    /// Tauri build graph) and re-saved its ~784 MB rust-cache entry
    /// (`Cache Size: ~784 MB (822197295 B)`, confirmed against a real run,
    /// job 95324823667), against ci.yml's shared 10 GB repo-wide cache
    /// quota, regardless of whether the push touched anything this lane
    /// exercises. The fix mirrors `pull_request`'s own `paths:` list onto
    /// `push`. This test is what keeps that mirror honest going forward.
    /// Both sides use a positionally-bounded reader (`push_path_filter` /
    /// `pull_request_path_filter`), not bare `path_filter`: a bare
    /// `path_filter(&contents)` reads whichever `paths:` block appears
    /// FIRST in the file, which only happens to be `push`'s today because
    /// `push:` precedes `pull_request:` under `on:`. Delete `push`'s
    /// `paths:` block entirely (the exact regression this test guards
    /// against) or reorder the two triggers, and an unbounded read would
    /// silently fall through to `pull_request`'s block for BOTH sides —
    /// `assert_eq!` would then compare a list to itself and pass, and
    /// `!push_paths.is_empty()` would pass too, even though the fix had
    /// been reverted.
    #[test]
    fn desktop_bundle_smoke_push_and_pull_request_paths_match() {
        let contents = workflow("desktop-bundle-smoke.yml");
        let push_paths = push_path_filter(&contents);
        let pull_request_paths = pull_request_path_filter(&contents);
        assert!(
            !push_paths.is_empty(),
            "desktop-bundle-smoke.yml's push trigger should carry a real paths filter (#2716); \
             an empty filter here would mean path_filter's parsing broke, not that the fix \
             was reverted on purpose"
        );
        assert_eq!(
            push_paths, pull_request_paths,
            "desktop-bundle-smoke.yml's push and pull_request paths filters must stay \
             identical (#2716) — push: {push_paths:?}, pull_request: {pull_request_paths:?}. \
             If one legitimately needs a new entry the other doesn't, that's a deliberate \
             design change this test should be updated to reflect explicitly, not a silent \
             drift"
        );
    }

    /// Negative case for the tautology `desktop_bundle_smoke_push_and_pull_request_paths_match`
    /// guards against: over a synthetic workflow with `push`'s `paths:`
    /// block removed entirely, `push_path_filter` must report empty rather
    /// than silently borrowing `pull_request`'s block. A bare
    /// `path_filter(&contents)` would NOT catch this — with `push`'s block
    /// gone, `pull_request`'s becomes the first (and only) `paths:` block in
    /// the file, so the unbounded reader would return it for both sides and
    /// the drift test above would pass on a reverted fix.
    #[test]
    fn push_path_filter_is_empty_when_push_has_no_paths_block() {
        let synthetic = "\
name: Synthetic
on:
  push:
    branches: [main]
  pull_request:
    paths:
      - \"packages/brink-desktop/**\"
      - \"pnpm-lock.yaml\"
";
        assert!(
            push_path_filter(synthetic).is_empty(),
            "push_path_filter should report empty when push's own paths: block is missing, \
             not fall through to pull_request's block"
        );
    }

    /// Each check in the smoke lane must stay non-blocking for its SIBLINGS
    /// (`!cancelled()`, so a clippy failure cannot hide a failing test)
    /// while still being gated on the SETUP steps it depends on. A bare
    /// `!cancelled()` overrides the implicit `success()` for a failed
    /// prerequisite too, which let a dying `pnpm/action-setup` take four
    /// dependent steps down with it and bury the root cause in cascade
    /// noise (2026-08-13 finding).
    ///
    /// A condition can quote a prerequisite id that no longer names any
    /// step (e.g. a renamed or `id:`-stripped setup step): `steps.<id>`
    /// then resolves to null at runtime, `.outcome` reads as the empty
    /// string, and the guard is simply always false — the step silently
    /// never runs while the lane stays green, the exact "ran nothing but
    /// reported success" class this PR closes for the cascade case. So
    /// beyond checking the `if:` text, this also checks every prerequisite
    /// named above still occurs as some step's real `id:` in the workflow.
    #[test]
    fn desktop_smoke_gates_dependent_steps_on_setup_success() {
        let steps = steps_with_conditions(&workflow("desktop-smoke.yml"));
        let known_ids: std::collections::BTreeSet<&str> = steps
            .iter()
            .map(|(_, id, _)| id.as_str())
            .filter(|id| !id.is_empty())
            .collect();
        let dependants: [(&str, &[&str]); 8] = [
            ("cargo check (src-tauri)", &["linux_deps", "sidecar"]),
            ("Clippy (src-tauri)", &["linux_deps", "sidecar"]),
            ("cargo test (src-tauri)", &["linux_deps", "sidecar"]),
            // The step that makes the comment below's claim ("itself gated
            // on `pnpm_install`") true in the first place — without this
            // entry nothing asserted `check_wasm_pkg`'s own `if:` at all.
            (
                "Verify wasm package link (check-wasm-pkg)",
                &["pnpm_install"],
            ),
            // `check_wasm_pkg`, not `pnpm_install` (#2514): `pnpm install
            // --frozen-lockfile`'s own exit code is the lying signal #2479
            // named, so gating on `steps.pnpm_install.outcome` alone would
            // let these two steps run over a silently-broken `file:` link.
            // `check_wasm_pkg` (`pnpm run check:wasm-pkg`) independently
            // verifies the resolved link and is itself gated on
            // `pnpm_install`, so both prerequisites are still covered
            // transitively.
            (
                "Typecheck (tsc --noEmit)",
                &["wasm_build", "check_wasm_pkg"],
            ),
            ("pnpm build", &["wasm_build", "check_wasm_pkg", "sidecar"]),
            // Format check needs nothing but the runner's toolchain and the
            // checkout: `actions/checkout` has no `id` to gate the other
            // steps on, but Format check's own `working-directory` does not
            // exist without it, so it stays gated on the checkout alone.
            ("Format check (src-tauri)", &["checkout"]),
            // Same shape (#2470): the audit reads manifests and lockfiles
            // out of the working tree and resolves metadata only, so it too
            // needs the checkout and nothing else.
            ("cargo-deny (src-tauri)", &["checkout"]),
        ];
        for (name, prerequisites) in dependants {
            let step = steps.iter().find(|(step_name, _, _)| step_name == name);
            assert!(
                step.is_some(),
                "desktop-smoke.yml should still have a step named {name:?}"
            );
            let (_, _, condition) = step.expect("just asserted the step exists");
            assert!(
                condition.contains("!cancelled()"),
                "{name:?} should stay non-blocking for its sibling checks"
            );
            for prerequisite in prerequisites {
                let guard = format!("steps.{prerequisite}.outcome == 'success'");
                assert!(
                    condition.contains(&guard),
                    "{name:?} depends on the {prerequisite:?} setup step, so its `if` \
                     should contain {guard:?}; it is {condition:?}"
                );
                assert!(
                    known_ids.contains(prerequisite),
                    "{name:?}'s `if` names {prerequisite:?} as a prerequisite, but no step \
                     in desktop-smoke.yml declares `id: {prerequisite}` — the guard would \
                     always read as false at runtime"
                );
            }
        }
    }

    /// The trimmed lines of one named step, from its `- name:` header up to
    /// the next step, so a `with:` value can be checked against the step it
    /// actually belongs to rather than merely "occurs somewhere in the
    /// file".
    fn step_block(workflow: &str, name: &str) -> Vec<String> {
        let header = format!("- name: {name}");
        let mut out = Vec::new();
        let mut inside = false;
        for line in workflow.lines() {
            let trimmed = line.trim();
            if trimmed == header {
                inside = true;
            } else if inside {
                if trimmed.starts_with("- name: ") || trimmed.starts_with("- uses: ") {
                    break;
                }
                out.push(trimmed.to_owned());
            }
        }
        out
    }

    /// The commit SHA an `EmbarkStudios/cargo-deny-action` pin names, from
    /// whichever workflow line pins it.
    fn cargo_deny_action_pin(workflow: &str) -> Option<String> {
        workflow.lines().find_map(|line| {
            line.trim()
                .split_once("EmbarkStudios/cargo-deny-action@")
                .map(|(_, rest)| {
                    rest.split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                })
        })
    }

    /// #2470: `ci.yml`'s `cargo-deny` job runs `check` exactly ONCE, at the
    /// repo root. This crate is its own cargo workspace with its own
    /// `Cargo.lock` (451 `[[package]]` entries — the Tauri graph, which
    /// shares no resolution with the root lock), so until this step existed
    /// that whole graph received no RUSTSEC advisory check and no licence
    /// check at all. #2451's `dependency_versions_track_the_root_workspace`
    /// above closes a DIFFERENT hole across the same workspace fence
    /// (version drift, not audit coverage) and would stay green throughout.
    ///
    /// The step lives in this deliberately NON-required lane rather than in
    /// `ci.yml`'s required `cargo-deny` job because it cannot be a blocking
    /// gate yet: the first audit surfaced 22 real errors. The 2026-08-15
    /// maintainer ruling cleared the 5 MPL-2.0 `error[rejected]` findings
    /// (see `deny_toml_admits_mpl_for_the_transitive_tauri_dependencies`
    /// below), leaving 17: 16 unmaintained RUSTSEC advisories inherent to
    /// Tauri v2 on Linux, plus `error[unlicensed]` for this crate itself.
    /// NEITHER of those is ruled on. Accepting either class is a maintainer
    /// policy call, so the step still reports rather than blocks — see
    /// docs/desktop-shell-spec.md "Smoke-lane inputs and step gating".
    #[test]
    fn desktop_smoke_audits_the_src_tauri_dependency_graph() {
        let smoke = workflow("desktop-smoke.yml");
        let step = step_block(&smoke, "cargo-deny (src-tauri)");
        assert!(
            !step.is_empty(),
            "desktop-smoke.yml should have a \"cargo-deny (src-tauri)\" step — ci.yml's \
             cargo-deny job runs `check` only at the repo root, so without this step \
             packages/brink-desktop/src-tauri/Cargo.lock gets no advisory or licence \
             audit anywhere in CI (#2470)"
        );

        // Pinned to the exact manifest, not merely "a manifest-path is
        // set": the action defaults this to `./Cargo.toml`, so a step that
        // dropped or mistyped the value would re-audit the ROOT graph that
        // ci.yml already covers and report a cheerful pass while this
        // crate's graph stayed unaudited.
        assert!(
            step.iter()
                .any(|line| line == "manifest-path: ./packages/brink-desktop/src-tauri/Cargo.toml"),
            "the cargo-deny (src-tauri) step should scope itself with \
             `manifest-path: ./packages/brink-desktop/src-tauri/Cargo.toml`; it reads {step:?}"
        );

        // `arguments` overrides the action's own `--all-features` default
        // wholesale, so the value has to carry it back, and `--locked` is
        // this lane's standing convention for every command that reads
        // src-tauri's committed lock. No `--config`: the pinned action's
        // image ships cargo-deny 0.19.8, where `--config` is a `check`
        // SUBCOMMAND flag, not a top-level one, and `action.yml` places
        // `arguments` BEFORE `command` on the assembled command line — so a
        // `--config` here is a clap parse failure (`error: unexpected
        // argument '--config' found`, exit 2) that `continue-on-error`
        // silently swallows, not the "explicit rather than relying on the
        // fallback" hardening it looks like. cargo-deny's own
        // `<cwd>/deny.toml` fallback already resolves to the root policy
        // without the flag, because the entrypoint's only `cd` is
        // subshelled and leaves cwd at the workspace root. Which flags are
        // legal in `arguments` vs `command-arguments` is a function of the
        // cargo-deny version baked into the pinned image (0.19.8 today) —
        // this assertion is the one place a future SHA bumper will read
        // that fact, so re-check it on every pin bump.
        assert!(
            step.iter()
                .any(|line| line == "arguments: --all-features --locked"),
            "the cargo-deny (src-tauri) step should pass \
             `arguments: --all-features --locked`; it reads {step:?}"
        );

        // Reporting, not blocking — see the doc comment. Removing this
        // while the remaining 17 findings stand turns the whole smoke lane
        // permanently red, which trains everyone to ignore the checks
        // beside it that DO pass today. The 2026-08-15 ruling settled the
        // MPL-2.0 licence class only; delete this assertion (and the spec
        // bullet) only together with a ruling on the 16 unmaintained
        // advisories and the `error[unlicensed]` finding.
        assert!(
            step.iter().any(|line| line == "continue-on-error: true"),
            "the cargo-deny (src-tauri) step should stay `continue-on-error: true` until \
             the unmaintained-advisory and error[unlicensed] findings are ruled on too \
             (the 2026-08-15 ruling covered only the MPL-2.0 crates), otherwise this \
             lane is permanently red; it reads {step:?}"
        );

        // One pin for both invocations. `desktop_smoke_path_filter_covers_\
        // its_shared_inputs` already puts ci.yml in this lane's path filter,
        // so the PR that bumps one pin is the PR this fails on.
        let smoke_pin = cargo_deny_action_pin(&smoke);
        let ci_pin = cargo_deny_action_pin(&workflow("ci.yml"));
        assert!(
            ci_pin.is_some(),
            "ci.yml should still pin EmbarkStudios/cargo-deny-action by SHA"
        );
        assert_eq!(
            smoke_pin, ci_pin,
            "both cargo-deny invocations should pin the same SHA-pinned action revision \
             (supply-chain hardening, docs/decision-log.md 2026-06-04), so the two audits \
             cannot drift onto different cargo-deny versions"
        );
    }

    /// The entries of the array assigned to `key` inside `deny.toml`'s
    /// `[licenses]` table — one raw line each (trimmed, blank and comment
    /// lines dropped, trailing comma removed). Hand-rolled rather than
    /// pulled in via a `toml` dev-dependency on purpose: this crate's
    /// `Cargo.lock` is the very artefact the audit under guard reads, so a
    /// test-only dependency added here would enlarge the graph it checks.
    fn licences_array(deny: &str, key: &str) -> Vec<String> {
        let opener = format!("{key} = [");
        let mut out = Vec::new();
        let mut inside_licenses = false;
        let mut inside_array = false;
        for line in deny.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if inside_array {
                if line.starts_with(']') {
                    inside_array = false;
                } else {
                    out.push(line.trim_end_matches(',').trim().to_owned());
                }
            } else if line.starts_with('[') {
                inside_licenses = line == "[licenses]";
            } else if inside_licenses && line == opener {
                inside_array = true;
            }
        }
        out
    }

    /// MAINTAINER RULING, 2026-08-15 — `docs/decision-log.md` "MPL-2.0
    /// admitted for the five transitive Tauri dependencies", and the
    /// `cargo-deny (src-tauri)` bullet of `docs/desktop-shell-spec.md`'s
    /// "Smoke-lane inputs and step gating".
    ///
    /// `deny.toml` is the policy file the `cargo-deny (src-tauri)` step
    /// above resolves (via cargo-deny's `<cwd>/deny.toml` fallback), so
    /// this crate's audit outcome is decided by a file outside this crate
    /// that nothing else here would notice changing. Before the ruling the
    /// audit reported 22 errors, five of them `error[rejected]` for the
    /// MPL-2.0 crates named below; after it, 17 (verified by running the
    /// step's exact invocation against cargo-deny 0.19.8, the version the
    /// pinned action image ships).
    ///
    /// The crate names and the `MPL-2.0` expression are asserted as the
    /// exact strings cargo-deny matches on, not merely as "an `exceptions`
    /// key exists": a typo in either silently restores the five rejections,
    /// and `continue-on-error: true` would keep the lane green while it
    /// happened.
    ///
    /// The second assertion is a PRESERVATION guard and is expected to be
    /// green both before and after the ruling — it is not vacuous, because
    /// it is the one thing pinning the ruling's *mechanism*: adding
    /// "MPL-2.0" to the blanket `allow` list would also silence all five
    /// findings, and would equally admit copyleft for every other crate in
    /// both governed graphs. The ruling covers five crates, not a licence.
    #[test]
    fn deny_toml_admits_mpl_for_the_transitive_tauri_dependencies() {
        const MPL: &str = "MPL-2.0";

        let deny_path = repo_root().join("deny.toml");
        assert!(
            deny_path.is_file(),
            "the repo-root deny.toml is the policy this crate's cargo-deny step resolves, \
             expected at {deny_path:?}"
        );
        let deny = std::fs::read_to_string(&deny_path).expect("just asserted this file exists");

        let allowed_for = format!("allow = [\"{MPL}\"]");
        let mut admitted: Vec<String> = licences_array(&deny, "exceptions")
            .iter()
            .filter(|entry| entry.contains(&allowed_for))
            .filter_map(|entry| {
                entry
                    .split_once("crate = \"")
                    .and_then(|(_, rest)| rest.split_once('"'))
                    .map(|(name, _)| name.to_owned())
            })
            .collect();
        admitted.sort();
        let admitted: Vec<&str> = admitted.iter().map(String::as_str).collect();

        assert_eq!(
            admitted,
            [
                "cssparser",
                "cssparser-macros",
                "dtoa-short",
                "option-ext",
                "selectors"
            ],
            "deny.toml's [licenses] exceptions should admit {MPL} for exactly the five \
             crates the 2026-08-15 ruling names — they are unavoidable transitively \
             through Tauri (selectors/cssparser/cssparser-macros/dtoa-short via dom_query \
             under tauri-utils and wry; option-ext via dirs-sys -> dirs under tauri, \
             tauri-build and wry). A SIXTH MPL crate appearing here is outside that \
             ruling and needs its own."
        );

        // Precondition, or the assertion below fails OPEN: `licences_array`
        // only recognises a MULTI-LINE `allow = [` block, so a legal
        // reformat collapsing that array to one line makes it return an
        // empty vec and the `!any(...)` check vacuously true — while
        // cargo-deny happily accepts the reformatted file with MPL-2.0
        // blanket-allowed and the per-crate exceptions doing nothing. Pin
        // that the parser actually found the list before trusting its
        // contents.
        let blanket = licences_array(&deny, "allow");
        assert!(
            blanket.iter().any(|entry| unquote(entry) == "MIT"),
            "expected to parse deny.toml's multi-line [licenses] allow list (it should \
             still contain \"MIT\"); an inlined or reformatted array would make the \
             check below vacuous. It parsed as {blanket:?}"
        );

        assert!(
            !blanket.iter().any(|entry| unquote(entry) == MPL),
            "{MPL} should stay OUT of deny.toml's blanket [licenses] allow list — the \
             2026-08-15 ruling admits it per-crate via `exceptions`, which is the \
             narrowest mechanism cargo-deny offers. Allowing it graph-wide would admit \
             copyleft for every crate in BOTH workspaces this policy governs, including \
             the root one that is 100% permissive today."
        );
    }

    /// Gap 4 (#2418): the sidecar staged in this check-only lane is only
    /// there so `tauri-build`'s externalBin resolution finds a file on
    /// disk — nothing here executes it ([`run_cli`] is the only caller and
    /// it needs a running app, not a `cargo test`) — so the lane asks
    /// `ensure-cli-sidecar.mjs` for a stub and skips the build entirely
    /// (#2469). PR #2446's `CARGO_PROFILE_RELEASE_*` stopgap was set
    /// job-wide, so it was also flattening the "Build brink-web wasm
    /// package" step's `wasm-pack build` (release by default) — not only
    /// the sidecar build it was written to excuse. Removing it un-flattens
    /// that wasm build too: the lane now deliberately accepts a
    /// fully-optimised one, rather than keep the vars as dead configuration
    /// for a sidecar build that no longer happens. Both halves are asserted
    /// here so the stub cannot quietly revert to the stopgap, or accumulate
    /// both. Nothing else in this file would notice the wiring vanishing
    /// from desktop-smoke.yml; this is that guard, and it is the third of
    /// the "Three properties of `desktop-smoke.yml` ... asserted by tests"
    /// that docs/desktop-shell-spec.md's "Smoke-lane inputs and step
    /// gating" section claims.
    ///
    /// Restoring a real (non-stubbed) sidecar build here also means
    /// re-adding `crates/brink-cli/**` to the `pull_request` path filter —
    /// `desktop_smoke_path_filter_covers_its_shared_inputs` now asserts
    /// that entry stays **absent** (#2477), on the premise that this guard
    /// keeps `BRINK_SIDECAR_STUB` unconditional. Un-stub the sidecar
    /// without also touching that test and the filter goes back to
    /// watching a tree the lane silently ignores.
    #[test]
    fn desktop_smoke_stubs_the_staged_sidecar() {
        let workflow = workflow("desktop-smoke.yml");
        let sets_key = |key: &str| {
            let needle = format!("{key}:");
            workflow
                .lines()
                .any(|line| line.trim_start().starts_with(needle.as_str()))
        };

        assert!(
            // Pinned to the exact value, not merely "the key is set": only
            // the literal string "1" makes `ensureCliSidecar`'s `stub`
            // default opt in (scripts/ensure-cli-sidecar.mjs), so e.g.
            // `BRINK_SIDECAR_STUB: "0"` would satisfy a presence-only check
            // while silently restoring the full release build this guard
            // exists to keep out.
            workflow
                .lines()
                .any(|line| line.trim() == "BRINK_SIDECAR_STUB: \"1\""),
            "desktop-smoke.yml's env: block should set BRINK_SIDECAR_STUB: \"1\" (the \
             exact string ensureCliSidecar's `stub` option opts in on) so \
             ensure-cli-sidecar.mjs stages a placeholder instead of building a \
             brink-cli release binary this check-only lane never runs; an env var \
             rather than a step flag because `pnpm build` re-runs that script — if you \
             are restoring a real sidecar build, also re-add \"crates/brink-cli/**\" to \
             desktop-smoke.yml's pull_request path filter, which \
             desktop_smoke_path_filter_covers_its_shared_inputs (#2477) currently \
             forbids"
        );
        for key in [
            "CARGO_PROFILE_RELEASE_OPT_LEVEL",
            "CARGO_PROFILE_RELEASE_DEBUG",
            "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
        ] {
            assert!(
                !sets_key(key),
                "desktop-smoke.yml still sets {key}: PR #2446's stopgap targeted the \
                 sidecar build, which BRINK_SIDECAR_STUB now removes, but the var was \
                 job-wide and was also flattening the wasm-pack release build this lane \
                 still runs — keeping it would silently leave that build de-optimised, \
                 not just the (already-gone) sidecar one"
            );
        }
    }

    /// This crate's own `build.rs`.
    fn build_script() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
        assert!(path.is_file(), "build.rs should exist at {path:?}");
        std::fs::read_to_string(&path).expect("just asserted the build script exists")
    }

    /// CLAUDE.md names `cd packages/brink-desktop/src-tauri && cargo test`
    /// as this crate's gate, and until #2617 that command was false on
    /// every fresh checkout and every fresh git worktree: `tauri-build`
    /// resolves `bundle.externalBin` unconditionally, `binaries/` is
    /// gitignored (the triple suffix is host-specific), and nothing on the
    /// local path staged it — so the build script died with `resource path
    /// "binaries/brink-cli-x86_64-unknown-linux-gnu" doesn't exist` before
    /// a single test ran.
    ///
    /// `build.rs` now stages a stub for DEBUG builds by running the very
    /// script `desktop-smoke.yml`'s "Stage brink-cli sidecar" step runs,
    /// under the very variable that lane sets (asserted by
    /// [`desktop_smoke_stubs_the_staged_sidecar`] above). That reuse is the
    /// point of this guard: the stub payload, the host-triple detection and
    /// the staged filename must keep living in
    /// `packages/brink-desktop/scripts/ensure-cli-sidecar.mjs` alone. A
    /// second copy of any of them in Rust is drift waiting to happen —
    /// #2481's Windows `.exe` refusal, for one, exists in exactly one place
    /// today.
    ///
    /// The `PROFILE`/`debug` gate is the other half. `cargo tauri build`
    /// (release) must keep failing loudly on a missing sidecar: a real
    /// bundle ships the real `brink-cli`, and silently substituting a
    /// loudly-failing placeholder there would turn a build-time error into
    /// a shipped one.
    #[test]
    fn build_script_stages_the_dev_sidecar_the_way_ci_does() {
        let build_rs = build_script();

        assert!(
            build_rs.contains("scripts/ensure-cli-sidecar.mjs"),
            "build.rs should stage the missing sidecar by invoking \
             packages/brink-desktop/scripts/ensure-cli-sidecar.mjs — the same script \
             desktop-smoke.yml's \"Stage brink-cli sidecar\" step runs — so CLAUDE.md's \
             documented `cd packages/brink-desktop/src-tauri && cargo test` works on a \
             fresh tree (#2617)"
        );
        // The assertion above is a string-literal grep, so it stays green even if the
        // script it names is moved or renamed — exactly the drift that would silently
        // re-break the gate this test exists to protect. Assert the path actually
        // resolves on disk, so a rename fails this test instead of only the vitest-side
        // guard (`src/__tests__/scripts-main-guard.test.ts`), which does not cover this
        // crate's build script at all.
        assert!(
            repo_root()
                .join("packages/brink-desktop/scripts/ensure-cli-sidecar.mjs")
                .is_file(),
            "packages/brink-desktop/scripts/ensure-cli-sidecar.mjs should exist — build.rs \
             hard-codes this path as a literal string, so a rename or move must be caught here"
        );
        assert!(
            build_rs.contains("BRINK_SIDECAR_STUB"),
            "build.rs should ask ensure-cli-sidecar.mjs for a STUB via BRINK_SIDECAR_STUB, \
             exactly as desktop-smoke.yml's env: block does (#2469) — a `cargo build -p \
             brink-cli --release` out of the root workspace is not something a `cargo test` \
             in this crate should trigger, and nothing here ever executes the sidecar"
        );
        assert!(
            !build_rs.contains("#!/bin/sh"),
            "build.rs should not carry its own copy of the stub payload — STUB_SIDECAR, the \
             host-triple detection and the staged filename (including #2481's Windows `.exe` \
             refusal) belong to ensure-cli-sidecar.mjs alone; a second mechanism is what this \
             guard exists to keep out"
        );
        assert!(
            build_rs.contains("PROFILE") && build_rs.contains("\"debug\""),
            "build.rs's auto-staging should be gated on PROFILE == \"debug\": `cargo tauri \
             build` (release) must keep failing loudly on a missing sidecar rather than \
             bundling a placeholder that exits 127 in a shipped app"
        );
        // #2715 review: this function probes per-arch `brink-cli-<TARGET>`
        // (`host_matches_target` above), but ensure-cli-sidecar.mjs's
        // main-guard stages under `universal-apple-darwin` whenever
        // `TAURI_ENV_TARGET_TRIPLE=universal-apple-darwin` is in its env. An
        // ambient inherited value from an enclosing `tauri build --target
        // universal-apple-darwin` would make the child stage the wrong
        // triple for no benefit — the HOST == TARGET guard exists to
        // prevent exactly this. Must scrub it before spawning the child.
        assert!(
            build_rs.contains("env_remove(\"TAURI_ENV_TARGET_TRIPLE\")"),
            "build.rs should env_remove(\"TAURI_ENV_TARGET_TRIPLE\") before spawning \
             ensure-cli-sidecar.mjs — an inherited universal-apple-darwin value would stage \
             the wrong-triple sidecar while this function keeps probing brink-cli-<TARGET> \
             (#2715 review)"
        );
    }

    /// The doc half of #2617. CLAUDE.md's "Key commands" block is where
    /// every contributor and agent learns how to run this crate's gate, and
    /// the whole point of the build-script staging above is that the
    /// command printed there is TRUE as written — no unstated prerequisite
    /// step, nothing to hand-stub first.
    ///
    /// Asserted from this side of the fence deliberately: CLAUDE.md is not
    /// in any cargo workspace and nothing else in the repo checks that its
    /// desktop command still matches what this crate does.
    #[test]
    fn claude_md_documents_the_desktop_gate_this_crate_actually_runs() {
        let path = repo_root().join("CLAUDE.md");
        assert!(path.is_file(), "CLAUDE.md should exist at {path:?}");
        let claude_md = std::fs::read_to_string(&path).expect("just asserted CLAUDE.md exists");

        assert!(
            claude_md
                .lines()
                .any(|line| line.trim() == "cd packages/brink-desktop/src-tauri && cargo test"),
            "CLAUDE.md's \"Key commands\" should still document `cd \
             packages/brink-desktop/src-tauri && cargo test` verbatim as the desktop gate. \
             If that command has grown a prerequisite again, the fix is to make the \
             prerequisite unnecessary (build.rs stages the stub sidecar, #2617), not to \
             document a caveat — a doc describing a working command is worth more than one \
             describing a workaround"
        );
    }

    /// This crate's own `tauri.conf.json`.
    fn tauri_conf() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        assert!(path.is_file(), "tauri.conf.json should exist at {path:?}");
        std::fs::read_to_string(&path).expect("just asserted tauri.conf.json exists")
    }

    /// #2631: PR #2626's "a real bundle must ship the real `brink-cli`"
    /// invariant held for `cargo tauri build --debug` only through step
    /// ordering — `beforeBuildCommand` -> `pnpm build` happens to stage the
    /// real binary before `build.rs` ever runs, plus `bundle.active: false`
    /// making the whole question moot in practice. Nothing asserted it.
    ///
    /// `tauri.conf.json`'s `beforeBundleCommand` is the fix: tauri-cli runs
    /// it immediately before the bundling phase of `tauri build` — after the
    /// crate has compiled (so `build.rs` already ran) and right before
    /// tauri-bundler reads `binaries/brink-cli-<triple>` off disk to package
    /// it. `scripts/assert-real-sidecar.mjs` throws if that file's content
    /// is `STUB_SIDECAR` rather than a real binary.
    ///
    /// Deliberately inert by default, not a gap: `bundle.active` below must
    /// stay `false` (D3 scope, not this issue's — see
    /// `docs/desktop-shell-spec.md`). That is not the only thing standing
    /// between this hook and firing, though — tauri-cli's bundling phase
    /// also runs on an explicit `tauri build --bundles <target>` even with
    /// `bundle.active: false`, so "flip `bundle.active`" is not this hook's
    /// only door, just the one this crate's own config controls. No CI lane
    /// and no documented developer command invokes `tauri build` today — but
    /// an ad-hoc `--bundles` invocation does reach the hook, as #2687's
    /// observation (docs/desktop-shell-spec.md "Bundle-time sidecar
    /// assertion (#2631)") demonstrated. This test below pins only that
    /// `bundle.active` stays `false`; it does not and cannot pin the absence
    /// of a CI lane or developer command that calls `tauri build`.
    #[test]
    fn before_bundle_command_asserts_the_staged_sidecar_is_real() {
        let conf = tauri_conf();

        assert!(
            conf.contains("\"beforeBundleCommand\""),
            "tauri.conf.json's `build` block should set `beforeBundleCommand` so tauri-cli \
             runs a real-sidecar check right before the bundling phase of `tauri build` \
             (#2631) — PR #2626's \"a real bundle must ship the real brink-cli\" invariant \
             held for `--debug` bundles only via step ordering until this hook existed"
        );
        assert!(
            conf.contains("scripts/assert-real-sidecar.mjs"),
            "beforeBundleCommand should invoke packages/brink-desktop/scripts/assert-real-sidecar.mjs"
        );
        assert!(
            repo_root()
                .join("packages/brink-desktop/scripts/assert-real-sidecar.mjs")
                .is_file(),
            "packages/brink-desktop/scripts/assert-real-sidecar.mjs should exist — \
             tauri.conf.json hard-codes this path as a literal string, so a rename or move \
             must be caught here"
        );

        // #2626's review established that the stub payload, host-triple
        // detection and staged filename live in ensure-cli-sidecar.mjs
        // ALONE (`build_script_stages_the_dev_sidecar_the_way_ci_does`
        // above guards build.rs the same way) — the new script must import
        // `STUB_SIDECAR` from there rather than carry its own copy.
        let assert_script = std::fs::read_to_string(
            repo_root().join("packages/brink-desktop/scripts/assert-real-sidecar.mjs"),
        )
        .expect("just asserted assert-real-sidecar.mjs exists");
        assert!(
            assert_script.contains("STUB_SIDECAR")
                && assert_script.contains("ensure-cli-sidecar.mjs"),
            "assert-real-sidecar.mjs should import STUB_SIDECAR from ensure-cli-sidecar.mjs \
             rather than redefine what the stub looks like"
        );
        assert!(
            !assert_script.contains("#!/bin/sh"),
            "assert-real-sidecar.mjs should not carry its own copy of the stub payload — \
             detect it via the STUB_SIDECAR import instead, exactly as this guard requires \
             of build.rs"
        );

        // #2687: comparing against STUB_SIDECAR alone is a BLOCKLIST — it
        // refuses the one placeholder that exists today and passes an
        // empty, truncated or wrong-architecture file, because
        // `tauri_build`'s externalBin resolution only tests that the path
        // exists. The hook must also POSITIVELY identify the staged file as
        // a native executable for the target.
        assert!(
            assert_script.contains("looksLikeNativeExecutable"),
            "assert-real-sidecar.mjs should positively identify the staged sidecar as a \
             native executable (ELF/Mach-O/PE magic), not merely differ from STUB_SIDECAR \
             (#2687) — a blocklist fails open on every placeholder that is not \
             byte-identical to the one we happen to have"
        );
        assert!(
            assert_script.contains("executableFormatFor"),
            "assert-real-sidecar.mjs should ask ensure-cli-sidecar.mjs's \
             `executableFormatFor` which executable format the target triple expects, \
             rather than deciding that for itself (#2626's single-mechanism rule, #2687)"
        );
        assert!(
            !assert_script.contains("includes(\"windows\")"),
            "assert-real-sidecar.mjs should not re-derive platform facts from the triple \
             string — `ensure-cli-sidecar.mjs` owns triple detection and the `.exe`/PE rule \
             (#2481, #2626); import `executableFormatFor` instead of testing the triple here"
        );
        let ensure_script = std::fs::read_to_string(
            repo_root().join("packages/brink-desktop/scripts/ensure-cli-sidecar.mjs"),
        )
        .expect("ensure-cli-sidecar.mjs should exist");
        assert!(
            ensure_script.contains("export function executableFormatFor"),
            "`executableFormatFor` should be defined in ensure-cli-sidecar.mjs — the one \
             module #2626's review allows to hold triple-derived knowledge about the \
             staged sidecar (#2687)"
        );

        // #2699: the magic check above proves the staged file's FORMAT, not
        // that it IS brink-cli or that it runs — PR #2691's own passing
        // observation stood in GNU coreutils' `true` for a real brink-cli,
        // and that would satisfy the magic check exactly as a genuine
        // wrong-build binary would. A `--version` smoke check closes that
        // gap for the one case it is safe to attempt: the staged triple
        // matching the triple actually running the check.
        assert!(
            assert_script.contains("--version"),
            "assert-real-sidecar.mjs should run a `--version` smoke check against the \
             staged sidecar, in addition to the magic-bytes check (#2699) — the magic check \
             alone proves the file's FORMAT, not that it is brink-cli or that it runs"
        );
        assert!(
            assert_script.contains("looksLikeBrinkCliVersionOutput"),
            "assert-real-sidecar.mjs's --version smoke check should verify the PRINTED \
             OUTPUT identifies as brink-cli, not just the exit code (#2699) — GNU coreutils' \
             `true` (PR #2691's own stand-in for brink-cli) also exits 0 on `--version`, so \
             an exit-code-only check would catch nothing new"
        );

        // `bundle.active` turning this on is explicitly D3 scope (#2631's
        // own instruction), not this fix's — this assertion exists to keep
        // the two from getting conflated by a later, unrelated edit to this
        // file landing bundle.active: true without anyone noticing it also
        // silently made this hook load-bearing.
        assert!(
            conf.contains("\"active\": false"),
            "tauri.conf.json's bundle.active should still read false — turning bundling on \
             is D3 scope (docs/desktop-shell-spec.md), not #2631's; if this now legitimately \
             reads true, this assertion's job is done and it should be removed here rather \
             than edited to match"
        );
    }

    /// Every `.github/workflows/*.yml`/`*.yaml` file, sorted by name so the
    /// test's output order is stable. Enumerated from disk, not a
    /// hard-coded file list, so a brand-new workflow file is automatically
    /// in scope for `every_pnpm_install_lane_builds_wasm_first_in_the_same_job`
    /// below (#2504) — the same "don't hard-code the lane list" requirement
    /// its job enumeration meets too. Every workflow file in this repo
    /// uses `.yml` today (confirmed by directory listing), but GitHub
    /// Actions accepts `.yaml` too, so that extension is matched as well —
    /// otherwise a new workflow file saved as `.yaml` would enter this
    /// guard's blind spot by construction, silently, rather than by a
    /// deliberate exemption. Both compared case-insensitively per clippy's
    /// `case_sensitive_file_extension_comparisons`.
    fn workflow_files() -> Vec<String> {
        let dir = repo_root().join(".github/workflows");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("workflows dir should exist")
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| {
                Path::new(name).extension().is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml")
                })
            })
            .collect();
        names.sort();
        assert!(
            !names.is_empty(),
            "expected at least one workflow file in {dir:?}"
        );
        names
    }

    /// Split a workflow's `jobs:` block into `(job_id, body)` pairs, in
    /// declaration order. Manual line-based parsing, not a real YAML parser
    /// — same approach as `path_filter`/`steps_with_conditions` above.
    /// Every workflow in this repo formats a job id at exactly two spaces
    /// of indent directly under the top-level `jobs:` key, with the job's
    /// own body (permissions, steps, …) indented four spaces or deeper —
    /// so "a line with exactly two leading spaces, no third, ending in
    /// `:`" unambiguously marks a new job among CODE lines. It is not
    /// unambiguous against comments, though: a two-space-indented `#`
    /// comment line that happens to end in a colon (ci.yml:122's
    /// "# ... dependency-graph build:", ci.yml:138's "#    two steps below
    /// close it without reversing the ruling:") matches the same shape, so
    /// comment lines are skipped before the header check runs.
    fn jobs(workflow: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut in_jobs = false;
        for line in workflow.lines() {
            if line == "jobs:" {
                in_jobs = true;
                continue;
            }
            if !in_jobs {
                continue;
            }
            if line.trim_start().starts_with('#') {
                continue;
            }
            let is_job_header = line.starts_with("  ")
                && !line.starts_with("   ")
                && line.trim_end().ends_with(':');
            if is_job_header {
                let id = line.trim().trim_end_matches(':').to_owned();
                out.push((id, String::new()));
                continue;
            }
            if let Some((_, body)) = out.last_mut() {
                body.push_str(line);
                body.push('\n');
            }
        }
        out
    }

    /// `jobs()`'s doc comment claims two leading spaces with no third,
    /// ending in `:`, "unambiguously marks a new job" — but two real
    /// comment lines in `ci.yml` match that exact shape today: line 122's
    /// "  # ... dependency-graph build:" and line 138's "  #    two steps
    /// below close it without reversing the ruling:". A GitHub Actions job
    /// id can never contain `#` (`^[a-zA-Z_][a-zA-Z0-9_-]*$`), so any
    /// parsed id containing one is a phantom job born from a
    /// misinterpreted comment, not a symptom the enumeration walk should
    /// tolerate silently — a phantom job sitting between a real wasm build
    /// step and its `pnpm install` would otherwise split that job in two
    /// and hand `every_pnpm_install_lane_builds_wasm_first_in_the_same_job`
    /// a wrong diagnosis instead of the real one.
    #[test]
    fn jobs_skips_comment_lines_that_look_like_headers() {
        let ids: Vec<String> = jobs(&workflow("ci.yml"))
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(
            !ids.iter().any(|id| id.contains('#')),
            "jobs() returned a phantom job parsed from a comment line (job ids can never \
             contain '#'); ids seen: {ids:?}"
        );
        assert_eq!(
            ids,
            vec![
                "studio-changeset-guard".to_owned(),
                "check".to_owned(),
                "wasm-test".to_owned(),
                "test".to_owned(),
                "test-bench-counters".to_owned(),
                "test-all-features".to_owned(),
                "static-checks".to_owned(),
                "deny".to_owned(),
                "fuzz".to_owned(),
                "determinism-law".to_owned(),
                "book".to_owned(),
                "frontend".to_owned(),
                "e2e".to_owned(),
                "e2e-gate".to_owned(),
            ],
            "expected exactly ci.yml's real job ids, in declaration order, with no \
             comment-derived phantom entries interleaved"
        );
    }

    /// The `run:` command(s) of every step in a job body, in order. Matches
    /// the compact `- run: <cmd>` form (a step with no `name:`/`id:`), the
    /// `run: <cmd>` form on its own line under a preceding `- name:
    /// ...`/`- uses: ...`, and — unlike an earlier version of this
    /// function — a `run: |` block scalar's continuation lines, one
    /// "command" per line, for as long as those lines stay indented deeper
    /// than the `run: |` line itself. Without that, a lane written as a
    /// `run: |` script (several already exist, e.g. the `mdbook test`
    /// step above) would hide every command it contains from both
    /// `every_pnpm_install_lane_builds_wasm_first_in_the_same_job`'s
    /// ordering check and its four-lane pin.
    ///
    /// A line whose trimmed form starts with `#` is always skipped —
    /// inside a block scalar that is a shell comment, not a command, and
    /// at any other point it is a YAML comment; several of this file's own
    /// pointer comments contain the literal string `pnpm install
    /// --frozen-lockfile` in prose (desktop-smoke.yml:32/184), which must
    /// not be mistaken for an executed step. A blank line is skipped too,
    /// without ending an in-progress block scalar, since multi-line
    /// scripts routinely contain blank lines for readability.
    fn run_commands(job_body: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut block_indent: Option<usize> = None;
        for line in job_body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start().len();

            if let Some(run_indent) = block_indent {
                if indent > run_indent {
                    out.push(unquote(trimmed).to_owned());
                    continue;
                }
                block_indent = None;
            }

            if trimmed == "run: |" || trimmed == "- run: |" {
                block_indent = Some(indent);
                continue;
            }

            if let Some(cmd) = trimmed
                .strip_prefix("- run: ")
                .or_else(|| trimmed.strip_prefix("run: "))
            {
                out.push(unquote(cmd.trim()).to_owned());
            }
        }
        out
    }

    /// A `run: |` block scalar is common in this repo's own workflows
    /// (e.g. the `mdbook test (doctests)` step in `ci.yml`'s `book` job),
    /// so `run_commands` has to walk its continuation lines the same way
    /// it walks single-line `run:` steps — including staying inside the
    /// block across a blank line, and not mistaking a shell comment
    /// mentioning `pnpm install --frozen-lockfile` in prose for a real
    /// command.
    #[test]
    fn run_commands_reads_block_scalar_continuation_lines() {
        let job_body = concat!(
            "    - name: multi-line step\n",
            "      run: |\n",
            "        # pnpm install --frozen-lockfile is mentioned here only in prose\n",
            "        wasm-pack build crates/brink-web --target web --out-dir www/pkg\n",
            "\n",
            "        pnpm install --frozen-lockfile\n",
            "    - name: next step\n",
            "      run: echo done\n",
        );

        assert_eq!(
            run_commands(job_body),
            vec![
                "wasm-pack build crates/brink-web --target web --out-dir www/pkg".to_owned(),
                "pnpm install --frozen-lockfile".to_owned(),
                "echo done".to_owned(),
            ],
            "block-scalar continuation lines should be read as commands, in order, with the \
             shell comment line and the blank line both skipped without ending the block early"
        );
    }

    /// #2504 (follow-up to #2479/#2492): nothing made the
    /// wasm-build-before-install ordering self-enforcing. All four `pnpm
    /// install --frozen-lockfile` lanes (`ci.yml`'s `frontend` and `e2e`
    /// jobs, `desktop-smoke.yml`, `npm-release.yml`) are correctly ordered
    /// today, but `pnpm install --frozen-lockfile`'s exit code is not
    /// trustworthy evidence the install happened when the `file:` link
    /// from `@brink-lang/web` to `crates/brink-web/www/pkg` is missing:
    /// depending on the pnpm 10.x resolved on the machine, it either exits
    /// 0 with the link silently unresolved (#2479's original pnpm) or
    /// exits non-zero while still writing nothing to `node_modules` at all
    /// (#2593, reproduced on pnpm 10.34.5 — confirmed by direct
    /// reproduction, scripts/check-wasm-pkg.mjs's header comment) — so a
    /// future reorder, or a new lane that adds the install step without
    /// the wasm build first, would re-open #2479 with no lane catching it
    /// either way.
    ///
    /// Enumerated from the workflow files themselves — every workflow file
    /// under `.github/workflows` via `workflow_files()`, every job in each
    /// via `jobs()` above — not a hard-coded list of the four known lanes,
    /// so a NEW workflow file or job cannot opt out of this guard just by
    /// not being named here. The ordering check below runs against every
    /// job this walk finds, with no per-lane allowlist. Installs are
    /// matched by `starts_with("pnpm install")`, not exact equality
    /// against the full `pnpm install --frozen-lockfile` string: the only
    /// `run:` commands anywhere in this tree that start with that prefix
    /// are today's four lanes (enumerated by grep across every workflow
    /// file), so this stays exact in practice while also catching a future
    /// lane written with extra flags (e.g. `pnpm install --frozen-lockfile
    /// --filter …`) that an exact match would silently miss.
    ///
    /// Two `install`-shaped steps exist outside these four lanes, both out
    /// of scope because neither carries a `file:` dependency on the
    /// wasm-pack output:
    /// - `ci.yml`'s `book` job runs a plain `npm install --no-audit
    ///   --no-fund` against `docs/book/ts-check`'s own lockfile — a
    ///   declared exemption, pinned by
    ///   `book_job_install_is_a_plain_npm_install_not_a_pnpm_lane` below so
    ///   a rename to `pnpm install --frozen-lockfile` cannot silently
    ///   escape this guard.
    /// - `benchmarks-inkjs.yml`'s `inkjs-gate` job runs `npm ci` against
    ///   its own `benchmarks/drivers/inkjs/package-lock.json`. Only the
    ///   `book` job gets a dedicated pin test: it lives in `ci.yml`,
    ///   alongside two of the four guarded lanes, so its exemption is the
    ///   one most easily mistaken for — or accidentally turned into — one
    ///   of them during review. `benchmarks-inkjs.yml`'s `npm ci` needs no
    ///   separate pin to get the same protection: the prefix match above
    ///   would catch a rename to any `pnpm install`-prefixed command there
    ///   too, and the exact four-lane list below rejects it just the same
    ///   as an unpinned fifth lane appearing anywhere else.
    #[test]
    fn every_pnpm_install_lane_builds_wasm_first_in_the_same_job() {
        const PNPM_INSTALL_PREFIX: &str = "pnpm install";
        const WASM_BUILD_PREFIX: &str = "wasm-pack build crates/brink-web";

        let mut checked_jobs: Vec<String> = Vec::new();
        let mut pnpm_install_lanes: Vec<String> = Vec::new();

        for file in workflow_files() {
            let contents = workflow(&file);
            for (job_id, body) in jobs(&contents) {
                let lane = format!("{file}:{job_id}");
                checked_jobs.push(lane.clone());

                let commands = run_commands(&body);
                let Some(install_pos) = commands
                    .iter()
                    .position(|c| c.starts_with(PNPM_INSTALL_PREFIX))
                else {
                    continue;
                };
                pnpm_install_lanes.push(lane.clone());

                let wasm_pos = commands
                    .iter()
                    .position(|c| c.starts_with(WASM_BUILD_PREFIX));
                assert!(
                    wasm_pos.is_some_and(|w| w < install_pos),
                    "{lane}'s job runs a `{PNPM_INSTALL_PREFIX}` command without a preceding \
                     `{WASM_BUILD_PREFIX}` step in the SAME job — this re-opens #2479 (`pnpm \
                     install --frozen-lockfile` exits 0 even when the file: link to \
                     crates/brink-web/www/pkg silently failed to resolve); commands seen in \
                     order: {commands:?}"
                );
            }
        }

        // Enumeration transparency (house convention: state exactly which
        // workflow files and jobs were checked, not just the ones known in
        // advance): this must have walked more than just the four
        // pnpm-install lanes (proof the walk covers every job in every
        // workflow file, not only the ones that happen to install), and
        // the pnpm-install lanes found must be exactly today's known four
        // — so a fifth lane, correctly ordered or not, cannot join
        // silently: it has to be added here on purpose.
        assert!(
            checked_jobs.len() > pnpm_install_lanes.len(),
            "expected to see jobs beyond just the pnpm-install lanes, proving this walked \
             every job in every workflow file rather than only the ones with an install \
             step; jobs seen: {checked_jobs:?}"
        );
        assert_eq!(
            pnpm_install_lanes,
            vec![
                "ci.yml:frontend".to_owned(),
                "ci.yml:e2e".to_owned(),
                // #2709: desktop-bundle-smoke.yml is the non-required real
                // `tauri build --debug --bundles deb` lane. Sorted between
                // ci.yml and desktop-smoke.yml because `workflow_files()`
                // walks the directory alphabetically.
                "desktop-bundle-smoke.yml:desktop-bundle-smoke".to_owned(),
                // D4 (docs/desktop-shell-spec.md): the tag-triggered official
                // build. Added deliberately, per this assertion's contract.
                "desktop-release.yml:build".to_owned(),
                "desktop-smoke.yml:desktop-smoke".to_owned(),
                "npm-release.yml:release".to_owned(),
            ],
            "expected exactly these six jobs to run a `{PNPM_INSTALL_PREFIX}` command; a new \
             pnpm-install lane must both pass the ordering assertion above AND be added to \
             this list on purpose — that is what keeps a new lane from opting out of this \
             guard by simply existing"
        );
    }

    /// The exemption `every_pnpm_install_lane_builds_wasm_first_in_the_same_job`'s
    /// doc comment names: `ci.yml`'s `book` job's `npm install --no-audit
    /// --no-fund` is a different command, against a different lockfile
    /// (`docs/book/ts-check`, no `file:` dependency on the wasm-pack
    /// output), so it is correctly out of scope for the pnpm guard above.
    /// That guard already matches installs by `starts_with("pnpm
    /// install")`, so a rename of this step to any `pnpm install`-prefixed
    /// command would be caught there too (as an unpinned fifth lane,
    /// rejected by the exact four-lane list) — this test pins the fact
    /// directly anyway, as declared documentation of the exemption rather
    /// than this guard's only defence against it, matching the same
    /// prefix-matching strictness on its negative assertion below so
    /// neither side of the check is more lenient than the other.
    #[test]
    fn book_job_install_is_a_plain_npm_install_not_a_pnpm_lane() {
        let ci = workflow("ci.yml");
        let (_, body) = jobs(&ci)
            .into_iter()
            .find(|(id, _)| id == "book")
            .expect("ci.yml should still have a `book` job");
        let commands = run_commands(&body);

        assert!(
            commands
                .iter()
                .any(|c| c == "npm install --no-audit --no-fund"),
            "expected ci.yml's book job to run `npm install --no-audit --no-fund`; if this \
             changed to a `pnpm install`-prefixed command it now needs a `wasm-pack build \
             crates/brink-web` step before it and is no longer this guard's declared \
             exemption; commands seen: {commands:?}"
        );
        assert!(
            !commands.iter().any(|c| c.starts_with("pnpm install")),
            "ci.yml's book job now ALSO runs a `pnpm install`-prefixed command — it needs a \
             `wasm-pack build crates/brink-web` step before it like the other four lanes; \
             commands seen: {commands:?}"
        );
    }

    /// #2697 gap 1: GitHub Actions defaults a job's `timeout-minutes` to
    /// 360 when the workflow doesn't set one — a wedged fetch or a hung
    /// test then burns a full runner-hour before Actions itself kills it,
    /// silently: a red step with a six-hour-old timestamp, not the loud,
    /// fast failure a CI gate exists to give. Re-counted directly against
    /// the tree for this fix (not taken from the issue, which called the
    /// count "second-hand" and asked for a fresh one): **31 jobs** across
    /// the repo's 12 `.github/workflows/*.yml` files, of which only 4
    /// already set one before this PR (`ci.yml`'s `fuzz` and
    /// `determinism-law`, `desktop-smoke.yml`'s `desktop-smoke`,
    /// `fresh-environment.yml`'s `fresh-environment`) — the issue's "~4 of
    /// ~65" was in the right ballpark for the "~4 already set" half; the
    /// "~65" half over-counts because it is not this walk's unit (see
    /// below).
    ///
    /// Mirrors `every_pnpm_install_lane_builds_wasm_first_in_the_same_job`
    /// above: enumerated from disk via `workflow_files()`/`jobs()`, not a
    /// hard-coded lane list, so a NEW job is in scope for this guard the
    /// moment it lands. "Job" here means one YAML entry under a workflow's
    /// `jobs:` key — `e2e`'s 4-way `matrix.shard` fan-out and
    /// `determinism-law`'s 1-entry `matrix.seed` fan-out are each ONE job
    /// definition even though GitHub renders them as several runs; a
    /// `timeout-minutes` set on the job definition bounds every one of its
    /// matrix-expanded runs identically, so counting matrix legs
    /// separately would not change what this guard needs to assert.
    ///
    /// ONE narrow allowlist, not a repo-wide one: `release.yml`'s five jobs
    /// (`plan`, `build-local-artifacts`, `build-global-artifacts`, `host`,
    /// `announce`) are `cargo-dist`-AUTOGENERATED — see this file's own
    /// header ("This file was autogenerated by dist") and
    /// release-plz.yml's "release.yml is cargo-dist-generated, house rule
    /// 5" comment. Hand-editing an autogenerated file fights the next
    /// `cargo dist generate` regeneration, which would either silently
    /// drop a hand-added `timeout-minutes` or merge-conflict with it.
    /// Every OTHER job without a preexisting `timeout-minutes` (22 of them)
    /// got a real, judgement-based value directly in the workflow file in
    /// this same PR instead of an allowlist entry: an allowlist that only
    /// grows is a ratchet backwards, and nothing about any of those 22 jobs
    /// is structurally special enough to justify one, the way
    /// cargo-dist-ownership is for these five.
    ///
    /// This allowlist is removable the moment EITHER becomes true: (a)
    /// cargo-dist's own template gains `timeout-minutes` support and a
    /// `cargo dist generate` regeneration picks it up, or (b) a maintainer
    /// decides hand-editing release.yml despite house rule 5 is acceptable
    /// for this one field. Until then it names exactly the five jobs it
    /// covers, pinned by exact-list equality below, so a SIXTH job escaping
    /// this guard by simply existing next to them is impossible — it would
    /// fail the allowlist-contents assertion, not merely go unnoticed.
    ///
    /// #2710 gap 1, checked directly against upstream rather than assumed:
    /// the cargo-dist version this workspace pins (`cargo-dist-version =
    /// "0.32.0"`, root `Cargo.toml`'s `[workspace.metadata.dist]`) ships a
    /// `release.yml.j2` template (`cargo-dist/templates/ci/github/` in the
    /// `axodotdev/cargo-dist` repo, tag `v0.32.0`) with exactly ONE
    /// occurrence of the string "timeout" in the whole file: a conditional
    /// `timeout-minutes: {{{ step.timeout_minutes }}}` nested inside the
    /// `build-local-artifacts` job's per-step "custom build setup steps"
    /// loop, gated on `if step.timeout_minutes is not undefined`. That is
    /// STEP-level timeout support for a dist-config-supplied optional setup
    /// step — not a JOB-level `timeout-minutes:` on any of the five jobs
    /// this allowlist actually covers (`plan`, `build-local-artifacts`,
    /// `build-global-artifacts`, `host`, `announce` all still lack one in
    /// the upstream template itself). So as of this PR neither exit
    /// condition (a) nor (b) above holds: upstream does not yet support
    /// what would make this allowlist entry removable via regeneration.
    /// Re-check this comment (and the pinned version above) whenever
    /// `cargo-dist-version` bumps — this is a point-in-time finding, not a
    /// standing guarantee, and nothing here re-verifies it automatically.
    ///
    /// #2710 gap 2: an explicit ceiling, not just presence. A job could set
    /// `timeout-minutes: 4320` (3 days) and this guard would previously have
    /// passed it — the presence check alone caps nothing. `TIMEOUT_MINUTES_CEILING`
    /// below is 120 (2 hours), re-measured against this repo's real job
    /// durations for this PR (GitHub Actions API, 2026-08-17, `syynth/brink`)
    /// rather than trusted from the issue's own figures: `ci.yml`'s heaviest
    /// job ("Fuzz (smoke)", 30-minute cap) ran 7m32s in the most recent
    /// completed run (run 32002443794) — about 4x headroom under its own cap,
    /// consistent with the issue's "≥3.7x headroom" claim. The highest
    /// `timeout-minutes` actually set anywhere in the tree today is 60
    /// (`desktop-bundle-smoke.yml`, a real `cargo build -p brink-cli
    /// --release` + Tauri bundle) — and that job's last five completed runs
    /// took 5.8/6.1/9.8/7.0/8.2 minutes, i.e. ≥6x headroom under its own cap.
    /// 120 sits at 2x the highest cap this repo has ever needed, leaving room
    /// for a future legitimately-long lane without moving the ceiling, while
    /// still rejecting a multi-day value by more than an order of magnitude.
    ///
    /// SCOPE, read narrowly: this ceiling only rejects a `timeout-minutes`
    /// that is too HIGH. #2710's own motivating case for gap 2 was the
    /// opposite direction — a cap sitting BELOW typical runtime
    /// (release-plz's `release-pr` job capped at 15m while real runs took
    /// 17m04s/17m31s/13m34s, silently killing a job that was still doing
    /// real work) — and the issue's Ask #2 asked for a check that flags
    /// caps sitting below typical duration. That half is NOT implemented
    /// here: nothing in this guard reads any job's actual run history or
    /// compares it against that job's own `timeout-minutes`, so a lane
    /// whose cap sits below what it actually needs still passes this test
    /// silently. Building that would mean querying the Actions API for
    /// per-job run durations, which is a real design/cost call (a token,
    /// a data source, a staleness policy) left to the maintainer rather
    /// than guessed at here.
    #[test]
    fn every_workflow_job_sets_timeout_minutes() {
        const ALLOWLISTED_FILE: &str = "release.yml";
        const TIMEOUT_MINUTES_CEILING: u32 = 120;

        let mut checked_jobs: Vec<String> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        let mut allowlisted: Vec<String> = Vec::new();
        let mut too_high: Vec<String> = Vec::new();

        for file in workflow_files() {
            let contents = workflow(&file);
            for (job_id, body) in jobs(&contents) {
                let lane = format!("{file}:{job_id}");
                checked_jobs.push(lane.clone());

                // Job-level `timeout-minutes:` is indented exactly 4 spaces
                // in every workflow file in this repo (same convention
                // `is_job_header`/`run_commands` above rely on for
                // `permissions:`/`runs-on:`) — matched at that exact
                // indent so a `run: |` block scalar that happens to
                // contain the substring in prose is not mistaken for the
                // real key, the same discipline `run_commands` applies to
                // comment lines.
                let timeout_line = body.lines().find(|l| l.starts_with("    timeout-minutes:"));

                let Some(timeout_line) = timeout_line else {
                    if file == ALLOWLISTED_FILE {
                        allowlisted.push(lane);
                        continue;
                    }
                    missing.push(lane);
                    continue;
                };

                let value = timeout_line
                    .trim_start_matches("    timeout-minutes:")
                    .trim();
                // A value this guard cannot parse is exactly the kind of
                // thing it exists to catch, not something to skip past —
                // treat it as violating the ceiling rather than silently
                // passing an unrecognised format through. Keep the failure
                // message honest about which case it is: folding a parse
                // failure into `u32::MAX` would report a nonsense
                // "(4294967295m)" for something like `timeout-minutes: 30  #
                // comment` or a `${{ ... }}` expression, blaming the file
                // for a value it never set.
                match value.parse::<u32>() {
                    Ok(minutes) => {
                        if minutes > TIMEOUT_MINUTES_CEILING {
                            too_high.push(format!("{lane} ({minutes}m)"));
                        }
                    }
                    Err(_) => {
                        too_high.push(format!("{lane} (unparseable value {value:?})"));
                    }
                }
            }
        }

        assert!(
            missing.is_empty(),
            "these jobs inherit GitHub Actions' 360-minute default timeout instead of setting an \
             explicit `timeout-minutes` (#2697): {missing:?}. A wedged step there burns a full \
             runner-hour silently instead of failing loud and fast — give the job a real \
             `timeout-minutes` value based on what it actually does, or — ONLY if it is \
             release.yml — add it to this test's narrow cargo-dist allowlist with the same \
             justification as the other five entries."
        );

        assert!(
            too_high.is_empty(),
            "these jobs set a `timeout-minutes` above this guard's {TIMEOUT_MINUTES_CEILING}-minute \
             ceiling (#2710 gap 2): {too_high:?}. The ceiling exists so an absurd value (e.g. \
             4320 for 3 days) cannot pass this guard just by being present — every job's real \
             `timeout-minutes` in this tree today is 60 or less; if a job genuinely needs more \
             than {TIMEOUT_MINUTES_CEILING} minutes, raise the ceiling here with a fresh \
             measurement justifying it, don't just accept the job's own oversized value."
        );

        assert_eq!(
            allowlisted,
            vec![
                "release.yml:plan".to_owned(),
                "release.yml:build-local-artifacts".to_owned(),
                "release.yml:build-global-artifacts".to_owned(),
                "release.yml:host".to_owned(),
                "release.yml:announce".to_owned(),
            ],
            "expected exactly release.yml's five cargo-dist-autogenerated jobs to be the only \
             ones without an explicit timeout-minutes; a NEW unexplained entry here must not \
             slip in silently — either it belongs in this exact list with the same cargo-dist \
             justification, or it needs a real timeout-minutes value like every other job"
        );

        // Enumeration transparency (same house convention as the pnpm-install
        // guard above): this must have walked jobs beyond just the
        // allowlisted five, proving the walk covers every job in every
        // workflow file rather than only release.yml's.
        assert!(
            checked_jobs.len() > allowlisted.len(),
            "expected to see jobs beyond release.yml's allowlisted five, proving this walked \
             every job in every workflow file; jobs seen: {checked_jobs:?}"
        );
    }

    /// #2717: branch protection's required-checks list lives in GitHub
    /// SETTINGS, not in this repository — nothing under `.github/` or
    /// anywhere else in the tree contains it, and this crate's tests run
    /// with no GitHub API token wired in (a repo-admin-scoped token would be
    /// needed to read it, which is itself the open design/cost question the
    /// issue leaves to the maintainer). BE HONEST about what that means:
    /// **this test cannot see, and does not attempt to assert, GitHub's
    /// actual required-checks list stays free of these two lanes.** That
    /// half of the fence — a maintainer (or automation) adding
    /// `desktop-smoke` / `Desktop bundle smoke (tauri build --debug
    /// --bundles deb)` to required checks via the Settings UI — is
    /// UNTESTABLE from in-tree code today, exactly the class of gap #2610
    /// and #2613 warn against papering over with a guard whose docstring
    /// outruns its filter.
    ///
    /// What IS checkable in-tree, and what this test actually guards, is the
    /// documented invariant each lane already carries in its own workflow
    /// file: a comment naming its NON-REQUIRED standing and the ruling that
    /// backs it (#2346/#2402, "no required CI lane may grow a Tauri
    /// build"). #2714 added that comment to `desktop-bundle-smoke.yml`
    /// alongside `desktop-smoke.yml`'s preexisting one; this test is what
    /// stops either comment from being silently deleted or reworded past
    /// the point of naming the ruling, which is the smallest thing this
    /// tree CAN enforce until repo-admin API access lands.
    #[test]
    fn non_required_desktop_lanes_document_their_standing() {
        for file in ["desktop-smoke.yml", "desktop-bundle-smoke.yml"] {
            let contents = workflow(file);

            assert!(
                contents.contains("NON-REQUIRED"),
                "{file} should carry a comment naming its NON-REQUIRED standing (#2346/#2402) — \
                 this is the in-tree half of the fence keeping it out of branch protection's \
                 required-checks list; actual GitHub required-checks state is not readable from \
                 this tree (#2717) and is NOT what this assertion checks"
            );

            // Scoped to the SAME contiguous comment block the "NON-REQUIRED"
            // line lives in, not the whole file: a whole-file `contains`
            // check is satisfied by any unrelated later mention of both
            // issue numbers (e.g. desktop-bundle-smoke.yml's cargo-deny
            // licence-report comment also happens to say "#2402/#2346"), so
            // the header comment could be reworded to drop its citations
            // entirely and this test would still pass — exactly the
            // "docstring outruns its filter" class (#2610/#2613) this test
            // itself invokes.
            let block_opt = non_required_comment_block(&contents);
            assert!(
                block_opt.is_some(),
                "{file}: NON-REQUIRED line found but its comment block could not be extracted"
            );
            let block = block_opt.expect("just asserted above");
            assert!(
                block.contains("#2346") && block.contains("#2402"),
                "{file}'s NON-REQUIRED comment block should cite both rulings it stands on \
                 (#2346 and #2402) so the standing traces back to a real decision, not just an \
                 unattributed claim; block seen: {block:?}"
            );
        }
    }

    /// Returns the contiguous run of `#`-prefixed lines that contains the
    /// first line mentioning "NON-REQUIRED", extended upward and downward
    /// while lines keep starting with `#`. Used to scope the citation check
    /// above to the same comment block as the standing claim, rather than
    /// anywhere in the file.
    fn non_required_comment_block(contents: &str) -> Option<String> {
        let lines: Vec<&str> = contents.lines().collect();
        let anchor = lines.iter().position(|l| l.contains("NON-REQUIRED"))?;

        let mut start = anchor;
        while start > 0 && lines[start - 1].trim_start().starts_with('#') {
            start -= 1;
        }
        let mut end = anchor;
        while end + 1 < lines.len() && lines[end + 1].trim_start().starts_with('#') {
            end += 1;
        }

        Some(lines[start..=end].join("\n"))
    }

    #[test]
    fn resolve_rejects_escapes() {
        assert!(resolve("/tmp/proj", "../outside.ink").is_err());
        assert!(resolve("/tmp/proj", "/etc/passwd").is_err());
        assert!(resolve("/tmp/proj", "a/../../b.ink").is_err());
        assert!(resolve("/tmp/proj", "scenes/intro.brink").is_ok());
    }

    #[test]
    fn project_file_filter() {
        assert!(is_project_file(Path::new("a/story.brink")));
        assert!(is_project_file(Path::new("a/story.ink")));
        assert!(is_project_file(Path::new("brink.toml")));
        assert!(!is_project_file(Path::new("a/story.ink.json")));
        assert!(!is_project_file(Path::new("Cargo.toml")));
    }

    fn rest(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn cli_allowlist_accepts_every_documented_subcommand() {
        for sub in [
            "export-xliff",
            "compile-locale",
            "regenerate-xliff",
            "compile",
        ] {
            assert!(
                prepare_cli_invocation("/tmp/proj", "story.brink", sub, &[]).is_ok(),
                "expected {sub} to be allowed"
            );
        }
    }

    #[test]
    fn cli_allowlist_rejects_arbitrary_passthrough() {
        // The whole point of the allowlist: a subcommand `brink-cli` really
        // has (`play`) but that isn't fenced for the sidecar, and an
        // arbitrary non-brink-cli binary name/shell metacharacter, must
        // both be rejected before the sidecar is ever spawned.
        assert!(matches!(
            prepare_cli_invocation("/tmp/proj", "story.brink", "play", &[]),
            Err(ShellError::DisallowedCommand(_))
        ));
        assert!(matches!(
            prepare_cli_invocation("/tmp/proj", "story.brink", "--", &[]),
            Err(ShellError::DisallowedCommand(_))
        ));
    }

    #[test]
    fn cli_allowlist_rejects_empty_args() {
        assert!(matches!(
            prepare_cli_invocation("/tmp/proj", "story.brink", "", &[]),
            Err(ShellError::MissingSubcommand)
        ));
    }

    /// Regression test for the 2026-08 review finding: the old `run_cli`
    /// shape took a flat `Vec<String>` and forwarded it untouched, so a
    /// compromised webview could pass an absolute (or `..`-carrying) input
    /// path straight through to the sidecar — an arbitrary-file read/write
    /// primitive. `prepare_cli_invocation` must run the input through the
    /// same [`resolve`] guard every other filesystem command uses, exactly
    /// like this test asserts. Reverting to the old passthrough shape (skip
    /// `resolve` and just `args.extend([rel, ...rest])`) makes this fail.
    #[test]
    fn cli_invocation_rejects_path_escape_in_input() {
        assert!(matches!(
            prepare_cli_invocation("/tmp/proj", "../../etc/passwd", "export-xliff", &[]),
            Err(ShellError::PathEscape(_))
        ));
        assert!(matches!(
            prepare_cli_invocation("/tmp/proj", "/etc/passwd", "export-xliff", &[]),
            Err(ShellError::PathEscape(_))
        ));
    }

    /// The resolved input lands right after the subcommand, and trailing
    /// `rest` args (the dialog-chosen, possibly-absolute `--output <path>`)
    /// are forwarded verbatim after it.
    #[test]
    fn cli_invocation_resolves_input_and_keeps_rest_verbatim() {
        let args = prepare_cli_invocation(
            "/tmp/proj",
            "story.brink",
            "export-xliff",
            &rest(&["--output", "/abs/out.xlf"]),
        )
        .expect("valid invocation should build");
        assert_eq!(
            args,
            vec![
                "export-xliff".to_owned(),
                "/tmp/proj/story.brink".to_owned(),
                "--output".to_owned(),
                "/abs/out.xlf".to_owned(),
            ]
        );
    }

    /// `crates/brink-cli/src/main.rs`'s `enum Commands` body, as plain text.
    /// Read across the workspace fence the same way
    /// `lint_policy_matches_the_root_workspace`/
    /// `dependency_versions_track_the_root_workspace` above do — `src-tauri`
    /// cannot take a dev-dependency on `brink-cli` to introspect its `clap`
    /// surface without pulling the excluded crate back across the fence it
    /// was deliberately pushed out of (`docs/desktop-shell-spec.md`
    /// "Workspace placement"; #2402/#2346 rule out growing a required
    /// lane's Tauri build) — so this reads the source file as text instead.
    fn brink_cli_commands_enum_body() -> String {
        let main_rs = repo_root().join("crates/brink-cli/src/main.rs");
        assert!(
            main_rs.is_file(),
            "crates/brink-cli/src/main.rs should exist at {main_rs:?}"
        );
        let source = std::fs::read_to_string(&main_rs)
            .expect("just asserted crates/brink-cli/src/main.rs exists");
        source
            .split_once("enum Commands {")
            .map(|(_, body)| body.to_owned())
            .expect("crates/brink-cli/src/main.rs should still declare `enum Commands { ... }`")
    }

    /// clap derive's default `#[derive(Subcommand)]` rename rule:
    /// `PascalCase` variant name -> kebab-case subcommand. `enum Commands` in
    /// `crates/brink-cli/src/main.rs` carries no `#[command(name = ...)]` or
    /// `rename_all` override on any variant (checked by the caller below),
    /// so this default is the real rule in effect.
    fn to_kebab_case(pascal: &str) -> String {
        let mut out = String::with_capacity(pascal.len() + 4);
        for (i, c) in pascal.chars().enumerate() {
            if c.is_ascii_uppercase() {
                if i > 0 {
                    out.push('-');
                }
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Every top-level `Commands` variant name, converted to the kebab-case
    /// subcommand string `brink-cli`'s real `clap` surface accepts.
    /// Top-level variants sit at exactly 4-space indentation inside the
    /// enum body; struct-variant fields sit at 8, and the `Ide` variant's
    /// `#[command(long_about = "...")]` string continuations sit at column
    /// 0 — neither is mistaken for a variant name here.
    fn brink_cli_subcommand_surface() -> Vec<String> {
        let source = brink_cli_commands_enum_body();
        assert!(
            !source.contains("rename_all") && !source.contains("#[command(name"),
            "enum Commands now overrides clap's default kebab-case renaming; \
             brink_cli_subcommand_surface's parsing no longer matches the real rule"
        );
        let names: Vec<String> = source
            .lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("    ")?;
                if rest.starts_with(|c: char| c.is_whitespace()) {
                    return None; // nested field/attribute, indented further
                }
                let name: String = rest
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                let after = rest[name.len()..].trim_start();
                let is_variant_head = name.starts_with(|c: char| c.is_ascii_uppercase())
                    && (after.starts_with('{') || after.starts_with(','));
                is_variant_head.then(|| to_kebab_case(&name))
            })
            .collect();
        assert!(
            !names.is_empty(),
            "should have parsed at least one Commands variant out of \
             crates/brink-cli/src/main.rs's `enum Commands` body"
        );
        names
    }

    /// Fourth cost of the workspace fence (`docs/desktop-shell-spec.md`
    /// "Workspace placement", #2507): `ALLOWED_CLI_SUBCOMMANDS` hand-mirrors
    /// a subset of `brink-cli`'s real subcommand surface, and nothing tied
    /// the two together until this test — a subcommand rename or removal in
    /// `crates/brink-cli/src/main.rs` was invisible here until `run_cli`
    /// broke at runtime (issue #2507, follow-up from PR #2502's review).
    ///
    /// Deliberately a subset check, not an equality one: `brink-cli` has
    /// subcommands the sidecar never exposes (`play`, `fmt`, `convert`,
    /// `migrate-xliff`, `replay`, `ide` — see
    /// `cli_allowlist_rejects_arbitrary_passthrough` above), and `brink-cli`
    /// growing one of those is not drift this guard should fail on. A
    /// rename or removal of a subcommand `ALLOWED_CLI_SUBCOMMANDS` actually
    /// depends on is.
    #[test]
    fn cli_allowlist_subcommands_exist_in_brink_cli_surface() {
        let real = brink_cli_subcommand_surface();
        for sub in ALLOWED_CLI_SUBCOMMANDS {
            assert!(
                real.iter().any(|r| r == sub),
                "ALLOWED_CLI_SUBCOMMANDS contains {sub:?}, which crates/brink-cli/src/main.rs's \
                 `enum Commands` no longer declares (real surface: {real:?}) — a rename or \
                 removal on the brink-cli side has to be reflected in run_cli's allowlist here \
                 too (docs/desktop-shell-spec.md \"Workspace placement\", #2507)"
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    #[test]
    fn opened_url_to_path_decodes_file_urls() {
        let url = tauri::Url::parse("file:///Users/ben/story/scenes/intro.brink")
            .expect("valid file URL");
        assert_eq!(
            opened_url_to_path(&url).expect("file URL should convert"),
            "/Users/ben/story/scenes/intro.brink"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    #[test]
    fn opened_url_to_path_rejects_non_file_schemes() {
        let url = tauri::Url::parse("https://example.com/story.brink").expect("valid URL");
        assert!(opened_url_to_path(&url).is_none());
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    #[test]
    fn take_pending_opens_drains_once_then_switches_to_live() {
        let state = PendingOpens(std::sync::Mutex::new(Some(vec!["/a/b.ink".to_owned()])));
        let taken = {
            state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .unwrap_or_default()
        };
        assert_eq!(taken, vec!["/a/b.ink".to_owned()]);
        // A second take (mirrors `take_pending_opens` being invoked twice)
        // must come back empty, never re-deliver — the buffer is `None`
        // now, which is also the "frontend is ready, emit live" state.
        let second = state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or_default();
        assert!(second.is_empty());
    }

    /// A fresh, uniquely-named scratch file path under the OS temp dir —
    /// same precedent as `crates/brink-cli/tests/*_cli.rs`'s `project_dir`.
    fn scratch_recents_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "brink-desktop-recents-{}-{tag}.json",
            std::process::id()
        ))
    }

    #[test]
    fn recents_after_push_dedups_by_path_moving_it_to_front() {
        let existing = vec!["/a".to_owned(), "/b".to_owned(), "/c".to_owned()];
        let after = recents_after_push(existing, "/b".to_owned(), 10);
        assert_eq!(
            after,
            vec!["/b".to_owned(), "/a".to_owned(), "/c".to_owned()]
        );
    }

    #[test]
    fn recents_after_push_prepends_new_path() {
        let existing = vec!["/a".to_owned()];
        let after = recents_after_push(existing, "/new".to_owned(), 10);
        assert_eq!(after, vec!["/new".to_owned(), "/a".to_owned()]);
    }

    #[test]
    fn recents_after_push_caps_the_list() {
        let existing: Vec<String> = (0..10).map(|i| format!("/p{i}")).collect();
        let after = recents_after_push(existing, "/new".to_owned(), 10);
        assert_eq!(after.len(), 10);
        assert_eq!(after[0], "/new");
        // The oldest entry ("/p9", pushed last of the original 10 so it
        // sits at the back before the new push) falls off the cap.
        assert!(!after.contains(&"/p9".to_owned()));
    }

    #[test]
    fn recents_after_prune_removes_only_the_named_path() {
        let existing = vec!["/a".to_owned(), "/b".to_owned(), "/c".to_owned()];
        let after = recents_after_prune(existing, "/b");
        assert_eq!(after, vec!["/a".to_owned(), "/c".to_owned()]);
    }

    #[test]
    fn recents_after_prune_is_a_no_op_for_an_absent_path() {
        let existing = vec!["/a".to_owned()];
        let after = recents_after_prune(existing, "/not-there");
        assert_eq!(after, vec!["/a".to_owned()]);
    }

    #[test]
    fn load_recents_missing_file_is_empty_not_an_error() {
        let path = scratch_recents_file("missing");
        let _ = std::fs::remove_file(&path); // ensure absent
        let loaded = load_recents(&path).expect("missing file loads as empty, not Err");
        assert_eq!(loaded, Vec::<String>::new());
    }

    #[test]
    fn save_then_load_recents_roundtrips() {
        let path = scratch_recents_file("roundtrip");
        let list = vec!["/one".to_owned(), "/two".to_owned()];
        save_recents(&path, &list).expect("save_recents should succeed on a temp path");
        let loaded = load_recents(&path).expect("load_recents should succeed right after save");
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, list);
    }

    /// The `open-recent:{path}` menu-id contract (#2394 review): the
    /// producer (`build_menu`'s `format!`) and consumer
    /// (`on_menu_event`'s `strip_prefix`) must agree on the exact prefix,
    /// including for a Windows-drive-letter-shaped path whose own `:`
    /// could plausibly confuse a naive strip. This crate's cargo gates run
    /// only in the non-required `desktop-smoke.yml` lane
    /// (`docs/desktop-shell-spec.md` "Workspace placement"), so this test is
    /// the only thing guarding the two literals from drifting apart.
    #[test]
    fn open_recent_id_round_trips_posix_and_windows_paths() {
        for path in ["/Users/x/proj", r"C:\Users\x\proj"] {
            let id = format!("{OPEN_RECENT_ID_PREFIX}{path}");
            assert_eq!(id.strip_prefix(OPEN_RECENT_ID_PREFIX), Some(path));
        }
    }

    #[test]
    fn load_recents_self_heals_on_corrupt_json() {
        let path = scratch_recents_file("corrupt");
        std::fs::write(&path, "not json").expect("writing the fixture file should succeed");
        let loaded = load_recents(&path).expect("corrupt json self-heals rather than erroring");
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, Vec::<String>::new());
    }

    // ── atomic_write (#2445) ─────────────────────────────────────────

    /// A fresh, uniquely-named scratch DIRECTORY under the OS temp dir for
    /// `atomic_write` tests — same precedent as `scratch_recents_file`, but
    /// a directory rather than a single file path, since `atomic_write`'s
    /// temp file lives NEXT TO its target and these tests need to inspect
    /// that directory's full contents (to prove no temp file survives).
    fn scratch_atomic_write_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "brink-desktop-atomic-write-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir should be creatable");
        dir
    }

    /// Every entry directly inside `dir`, as file names (not full paths),
    /// sorted for a stable comparison — used to assert no stray temp file
    /// survives a completed (or failed) `atomic_write`.
    fn dir_entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("scratch dir should be readable")
            .filter_map(std::result::Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn temp_path_for_is_hidden_same_dir_and_unique_per_call() {
        let dir = scratch_atomic_write_dir("temp-path-unique");
        let target = dir.join("story.brink");
        let a = temp_path_for(&target);
        let b = temp_path_for(&target);
        assert_ne!(a, b, "two calls must never collide on the same temp path");
        for tmp in [&a, &b] {
            assert_eq!(
                tmp.parent(),
                Some(dir.as_path()),
                "temp file must live in the target's own directory, or the rename \
                 that follows would cross filesystems and lose atomicity"
            );
            let name = tmp
                .file_name()
                .and_then(|n| n.to_str())
                .expect("temp path should have a file name");
            assert!(
                name.starts_with('.'),
                "temp file {name} should be hidden by convention (leading dot)"
            );
            // The dot is cosmetic, not what excludes the temp file from the
            // project listing or the watcher — `is_project_file` matches only
            // `brink.toml`/`.brink`/`.ink`, and `is_skipped_dir`'s dot-rule
            // applies only to directory names. What actually excludes this
            // path is its extension (the trailing disambiguator), so assert
            // that property directly rather than the incidental leading dot.
            assert!(
                !is_project_file(tmp),
                "temp file {name} must not satisfy is_project_file, or `walk` would list it \
                 as a project file"
            );
            assert!(
                watch_key(&dir, tmp).is_none(),
                "temp file {name} must not produce a watch_key, or a write to it would fire a \
                 spurious fs:external-change into the #320 never-clobber machinery"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_creates_a_new_file() {
        let dir = scratch_atomic_write_dir("new-file");
        let target = dir.join("story.brink");
        atomic_write(&target, b"=== knot ===\n").expect("atomic_write should create a new file");
        assert_eq!(
            std::fs::read_to_string(&target).expect("target should be readable"),
            "=== knot ===\n"
        );
        // No temp file left behind after a successful write.
        assert_eq!(dir_entries(&dir), vec!["story.brink".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_replaces_an_existing_file() {
        let dir = scratch_atomic_write_dir("replace");
        let target = dir.join("story.brink");
        std::fs::write(&target, "OLD").expect("seeding the old file should succeed");
        atomic_write(&target, b"NEW").expect("atomic_write should replace the existing file");
        assert_eq!(
            std::fs::read_to_string(&target).expect("target should be readable"),
            "NEW"
        );
        assert_eq!(dir_entries(&dir), vec!["story.brink".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Rename-over swaps the inode, so without deliberately carrying the
    /// old permissions across, overwriting a read-only file would silently
    /// make it writable again (temp files are created with the process's
    /// default mode). POSIX-only: permission bits and rename-over-readonly
    /// semantics are the exact place Windows and POSIX diverge (see
    /// `atomic_write`'s doc comment).
    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_the_target_permissions_on_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch_atomic_write_dir("preserve-perms");
        let target = dir.join("story.brink");
        std::fs::write(&target, "OLD").expect("seeding the old file should succeed");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444))
            .expect("setting read-only permissions should succeed");

        atomic_write(&target, b"NEW").expect("atomic_write should replace a read-only file");

        let mode = std::fs::metadata(&target)
            .expect("target should be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o444,
            "overwriting a read-only file must not silently make it writable"
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("target should be readable"),
            "NEW"
        );

        // Restore write permission so the scratch dir can be cleaned up.
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The interruption-safety property #2445 asks for, proven directly
    /// rather than by actually killing a process mid-write (not
    /// reproducible in a unit test): replicate `atomic_write`'s own two
    /// steps by hand — write the temp file, THEN rename — and assert the
    /// target holds its old, COMPLETE content right up until the rename,
    /// and its new, COMPLETE content immediately after. There is no step
    /// in between where the target is anything else (partial/mixed), which
    /// is exactly the guarantee an interruption (quit killing the write)
    /// relies on: caught before the rename, the target is untouched;
    /// caught after, the rename (an atomic same-filesystem syscall) has
    /// already fully applied.
    #[test]
    fn atomic_write_protocol_never_exposes_a_partial_target() {
        let dir = scratch_atomic_write_dir("protocol");
        let target = dir.join("story.brink");
        std::fs::write(&target, "OLD-COMPLETE").expect("seeding the old file should succeed");

        let tmp = temp_path_for(&target);
        std::fs::write(&tmp, "NEW-COMPLETE").expect("temp write should succeed");
        // An interruption caught here (before the rename) leaves the
        // target exactly as it was — old, complete content, untouched.
        assert_eq!(
            std::fs::read_to_string(&target).expect("target should still be readable"),
            "OLD-COMPLETE",
            "target must be untouched while the temp write is still in flight"
        );

        std::fs::rename(&tmp, &target).expect("rename should succeed on the same filesystem");
        // An interruption caught here (after the rename) sees the target
        // fully replaced — new, complete content, nothing partial.
        assert_eq!(
            std::fs::read_to_string(&target).expect("target should be readable after rename"),
            "NEW-COMPLETE",
            "target must hold the complete new content once the rename lands"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_cleans_up_its_temp_file_on_a_failed_rename() {
        // Make the RENAME step fail (not the temp write): the target is a
        // pre-existing directory, so a file can never rename over it
        // (EISDIR) — but the temp file, a sibling regular file in the same
        // scratch dir, writes successfully first. Proves the cleanup path
        // removes the orphaned temp file rather than leaking it.
        let dir = scratch_atomic_write_dir("rename-failure");
        let target = dir.join("story.brink");
        std::fs::create_dir(&target).expect("target directory should be creatable");
        let result = atomic_write(&target, b"content");
        assert!(
            result.is_err(),
            "renaming a file over an existing directory should fail"
        );
        assert_eq!(
            dir_entries(&dir),
            vec!["story.brink".to_owned()],
            "a failed rename must not leave its temp file behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_concurrent_writes_to_the_same_path_never_interleave() {
        let dir = scratch_atomic_write_dir("concurrent");
        let target = dir.join("story.brink");
        std::fs::write(&target, "SEED").expect("seeding should succeed");

        // Each writer's payload is large and made of one repeated byte, so
        // any byte-level interleaving between two writers (or a torn read)
        // would produce content matching NEITHER candidate exactly.
        let candidates: Vec<Vec<u8>> = (0u8..8).map(|i| vec![b'A' + i; 200_000]).collect();
        let target = std::sync::Arc::new(target);
        let handles: Vec<_> = candidates
            .iter()
            .cloned()
            .map(|payload| {
                let target = std::sync::Arc::clone(&target);
                std::thread::spawn(move || {
                    atomic_write(&target, &payload)
                        .expect("concurrent atomic_write should succeed");
                })
            })
            .collect();
        for h in handles {
            h.join().expect("writer thread should not panic");
        }

        let final_content = std::fs::read(&*target).expect("target should be readable");
        assert!(
            candidates.iter().any(|c| c == &final_content),
            "final content must be exactly one writer's complete payload, never a mix"
        );
        assert_eq!(
            dir_entries(&dir),
            vec!["story.brink".to_owned()],
            "no writer's temp file should survive all the concurrent writes"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
