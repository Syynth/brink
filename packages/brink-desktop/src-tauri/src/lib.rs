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
fn build_menu(
    handle: &tauri::AppHandle,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let app_menu = Submenu::with_items(
        handle,
        "Brink Studio",
        true,
        &[
            &PredefinedMenuItem::about(handle, None, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::quit(handle, None)?,
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
    let file_menu = Submenu::with_items(
        handle,
        "File",
        true,
        &[
            &open,
            &close,
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
            pick_project_folder,
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
}
