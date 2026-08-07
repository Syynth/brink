//! The Tauri shell for brink-desktop (docs/desktop-shell-spec.md, D1).
//!
//! Custom commands instead of the fs plugin: the shell owns filesystem
//! policy, and the stay-inside-the-project-root guard lives here, next to
//! the I/O it guards. The frontend passes the project root (obtained from
//! our own `pick_project_folder` dialog) plus project-relative paths; this
//! module rejects anything absolute or `..`-carrying, so the webview can
//! never reach outside the folder the user picked.

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
// filesystem reach — this allowlist is the real security boundary, not
// the `shell:allow-execute` capability scope (which only pins *which
// binary*, not *which arguments*).
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

/// The allowlist check, pulled out of [`run_cli`] so it's testable without
/// an `AppHandle`/sidecar: `args[0]` must be one of
/// [`ALLOWED_CLI_SUBCOMMANDS`], checked before the sidecar is ever spawned.
fn validate_cli_args(args: &[String]) -> Result<(), ShellError> {
    let subcommand = args.first().ok_or(ShellError::MissingSubcommand)?;
    if !ALLOWED_CLI_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Err(ShellError::DisallowedCommand(subcommand.clone()));
    }
    Ok(())
}

/// Run an allowlisted `brink-cli` subcommand as a Tauri sidecar, streaming
/// its stdout/stderr to the webview as `cli:output` events and resolving
/// to the process exit code once it terminates. `args[0]` must be one of
/// [`ALLOWED_CLI_SUBCOMMANDS`]; anything else is rejected before the
/// sidecar is ever spawned.
#[tauri::command]
async fn run_cli(app: tauri::AppHandle, args: Vec<String>) -> Result<i32, ShellError> {
    use tauri::Emitter;
    use tauri_plugin_shell::process::CommandEvent;
    use tauri_plugin_shell::ShellExt;

    validate_cli_args(&args)?;

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
fn build_menu(
    handle: &tauri::AppHandle,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

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
    // Proves the sidecar path end-to-end (D3, #2392); the fuller intl UI
    // (locale picker, progress, batch ops beyond xliff export) is future
    // work — this item exists to exercise one real path, not to be it.
    let export_xliff =
        MenuItem::with_id(handle, "export-xliff", "Export XLIFF…", true, None::<&str>)?;
    let file_menu = Submenu::with_items(
        handle,
        "File",
        true,
        &[
            &open,
            &close,
            &PredefinedMenuItem::separator(handle)?,
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
        .setup(|app| {
            let menu = build_menu(app.handle())?;
            app.set_menu(menu)?;
            // Menu events forward to the webview as plain events; the
            // frontend owns what "open" and "close" mean (it holds the
            // StudioHandle). The shell stays policy-free.
            app.on_menu_event(|app, event| {
                let forwarded = match event.id().as_ref() {
                    "open-project" => Some("menu:open-project"),
                    "close-project" => Some("menu:close-project"),
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
            run_cli,
        ])
        .run(tauri::generate_context!())
        .expect("error while running brink-desktop");
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

    fn args(strs: &[&str]) -> Vec<String> {
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
                validate_cli_args(&args(&[sub, "story.brink"])).is_ok(),
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
            validate_cli_args(&args(&["play", "story.brink"])),
            Err(ShellError::DisallowedCommand(_))
        ));
        assert!(matches!(
            validate_cli_args(&args(&["--", "/bin/sh"])),
            Err(ShellError::DisallowedCommand(_))
        ));
    }

    #[test]
    fn cli_allowlist_rejects_empty_args() {
        assert!(matches!(
            validate_cli_args(&args(&[])),
            Err(ShellError::MissingSubcommand)
        ));
    }
}
