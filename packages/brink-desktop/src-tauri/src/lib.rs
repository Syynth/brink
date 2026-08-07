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
    std::fs::write(&path, content).map_err(|e| io_err(&path, e))
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
/// same skip rules as `list_files` (dotdirs, node_modules, target, dist)
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
    *state.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(watcher);

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
                        let _ = app.emit("fs:external-change", ExternalChangeOut {
                            path: key,
                            content,
                        });
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
    *state.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
/// bounds per the OverlayPersistence contract). Working defaults from the
/// 2026-08-07 ruling; a Settings surface can adjust later.
const RING_MAX_ENTRIES: usize = 25;
const RING_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// A filesystem-safe, deterministic key for one project's ring directory.
/// The full sanitized path (not a hash): stable forever, debuggable by eye.
fn project_ring_key(root: &str) -> String {
    root.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
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
        .filter_map(|e| e.ok())
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
fn take_pending_opens(state: tauri::State<'_, PendingOpens>) -> Vec<String> {
    state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .unwrap_or_default()
}

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
/// the two can never drift out of sync — this crate has no CI at all
/// (`docs/desktop-shell-spec.md`: "CI in v1: none required"), so a typo'd
/// duplicate literal at either site would ship a silently inert menu behind
/// a fully green required gate. See `open_recent_id_round_trips_posix_and_windows_paths`
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
/// pattern; the plugin dispatches the actual NSOpenPanel to the main
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
    let recent_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = recent_items
        .iter()
        .map(|item| item as &dyn IsMenuItem<tauri::Wry>)
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
        .build(tauri::generate_context!())
        .expect("error while building brink-desktop")
        .run(move |app_handle, event| {
            // File associations (#2393) only exist on macOS/iOS/Android —
            // `RunEvent::Opened` itself is cfg-gated out of the enum on
            // other platforms, so the arm is compiled out there too rather
            // than matching a variant that doesn't exist.
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
            if let tauri::RunEvent::Opened { urls } = &event {
                handle_opened(app_handle, urls);
            }
            let _ = (app_handle, &event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

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
    /// could plausibly confuse a naive strip. This crate has zero CI
    /// (`docs/desktop-shell-spec.md`: "CI in v1: none required"), so this
    /// is the only thing guarding the two literals from drifting apart.
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
}
