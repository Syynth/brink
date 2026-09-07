//! Watching the project on disk.
//!
//! Without this the studio reads a file when it opens it and writes it
//! when it saves, and anything that happened in between is lost: an edit
//! made in another editor, a file added by a script, a `git checkout`.
//! Nothing said so — the next save simply overwrote it.
//!
//! What a change MEANS is [`classify`], a pure function, and the policy
//! it encodes is the web studio's (`conflict-view.ts`, #320): a change
//! under a CLEAN buffer is adopted, a change under a DIRTY one keeps the
//! buffer and says so. The merge surface the web puts on top of that is
//! not built here; what is built is the half that stops work being lost
//! silently.
//!
//! The watcher itself runs on its own thread and reports paths, not
//! events: the same file being written twice is one thing to look at, and
//! whether anything actually changed is decided by reading it and
//! comparing — which also makes the studio's OWN writes free to ignore,
//! with no bookkeeping of what it just wrote.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{App, Entity};
use notify::{RecursiveMode, Watcher};

use crate::project::Project;

/// How long the watcher gathers paths before handing them over. An editor
/// saving a file often writes it more than once; a `git checkout` touches
/// many at once. Both are one look from here.
const SETTLE: Duration = Duration::from_millis(200);

/// What a file on disk has done, relative to what the studio holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskChange {
    /// Nothing the studio has to do: the disk agrees with what it saved,
    /// which is also what its own writes look like from here.
    Ignore,
    /// The buffer is clean, so the disk wins and the text is adopted.
    Adopt(String),
    /// The buffer is dirty and the disk moved under it. The buffer is
    /// kept, and the author is told — losing their edits to a background
    /// event is the one outcome that must not happen quietly. Carries the
    /// disk's text, which is what makes "the same conflict again" a thing
    /// the caller can recognise: one write often reaches a watcher as
    /// several events.
    Conflict(String),
    /// A file the studio holds is gone from disk.
    Vanished,
    /// A file the studio does not hold has appeared.
    Appeared(String),
}

/// Decide what a path's disk state means.
///
/// `saved` is the text as of the load or the last save, `buffer` the text
/// the editors hold, `disk` what is there now — `None` for a file that is
/// not there at all, on either side.
#[must_use]
pub fn classify(saved: Option<&str>, buffer: Option<&str>, disk: Option<&str>) -> DiskChange {
    match (saved, buffer, disk) {
        // Not ours, and there: something else made it.
        (None, None, Some(text)) => DiskChange::Appeared(text.to_owned()),
        // Not ours and not there either.
        (None, None, None) => DiskChange::Ignore,
        // Ours, and gone.
        (_, Some(_), None) => DiskChange::Vanished,
        (Some(saved), Some(buffer), Some(disk)) => {
            if saved == disk {
                // Includes every write the studio made itself.
                DiskChange::Ignore
            } else if saved == buffer {
                DiskChange::Adopt(disk.to_owned())
            } else {
                DiskChange::Conflict(disk.to_owned())
            }
        }
        // A file with no buffer but a saved text, or the other way round,
        // is a state the mirror does not produce; treat it as nothing to
        // do rather than guess.
        _ => DiskChange::Ignore,
    }
}

/// Whether a path is one the project would hold — the sources and the
/// config, not build output or a sidecar the studio writes itself.
#[must_use]
pub fn is_watched(path: &str) -> bool {
    // `.binder.json` and a dialect artifact are the studio's own writes and
    // are read through the config road, not the mirror.
    if path.starts_with('.') || path.contains("/.") {
        return false;
    }
    matches!(
        path.rsplit('.').next(),
        Some("ink" | "brink" | "toml") if !path.ends_with(".ink.json")
    )
}

/// Start watching `root`, and apply what changes there to `project`.
///
/// The returned task owns the watch: dropping it stops the pump, and the
/// watcher thread ends with the channel.
pub fn start(project: Entity<Project>, root: PathBuf, cx: &mut App) -> gpui::Task<()> {
    let (tx, rx) = async_channel::unbounded::<PathBuf>();
    let mut watcher =
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            for path in event.paths {
                let _ = tx.send_blocking(path);
            }
        }) {
            Ok(watcher) => watcher,
            // A studio that cannot watch still works; it just cannot notice.
            Err(_) => return gpui::Task::ready(()),
        };
    if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
        return gpui::Task::ready(());
    }
    cx.spawn(async move |cx| {
        // Held for the life of the task: dropping the watcher stops the
        // events, and nothing else owns it.
        let _watcher = watcher;
        while let Ok(first) = rx.recv().await {
            let mut paths = vec![first];
            // Let a burst settle: an editor writes a file more than once,
            // and a checkout touches many at a time.
            cx.background_executor().timer(SETTLE).await;
            while let Ok(more) = rx.try_recv() {
                paths.push(more);
            }
            paths.sort();
            paths.dedup();
            let relative: Vec<String> = paths
                .iter()
                .filter_map(|path| relative_to(&root, path))
                .filter(|path| is_watched(path))
                .collect();
            if relative.is_empty() {
                continue;
            }
            project.update(cx, |project, cx| project.disk_changed(&relative, cx));
        }
    })
}

/// `path` as a root-relative, forward-slashed key, or `None` when it is
/// not under `root`.
fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let key = rest
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    (!key.is_empty()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::{DiskChange, classify, is_watched};

    #[test]
    fn the_studios_own_write_looks_like_nothing_happened() {
        // After a save, `saved` and the disk agree — which is exactly what
        // the watcher sees when the studio itself wrote the file, and why
        // no bookkeeping of its own writes is needed.
        assert_eq!(
            classify(Some("hello"), Some("hello"), Some("hello")),
            DiskChange::Ignore
        );
    }

    #[test]
    fn a_clean_buffer_adopts_the_disk_and_a_dirty_one_is_kept() {
        assert_eq!(
            classify(Some("old"), Some("old"), Some("new")),
            DiskChange::Adopt("new".to_owned()),
            "nothing unsaved to lose, so the disk wins"
        );
        assert_eq!(
            classify(Some("old"), Some("mine"), Some("theirs")),
            DiskChange::Conflict("theirs".to_owned()),
            "unsaved work is never dropped for a background event"
        );
        // A dirty buffer whose disk has NOT moved is still nothing to do.
        assert_eq!(
            classify(Some("old"), Some("mine"), Some("old")),
            DiskChange::Ignore
        );
    }

    #[test]
    fn appearing_and_vanishing_are_their_own_answers() {
        assert_eq!(
            classify(None, None, Some("text")),
            DiskChange::Appeared("text".to_owned())
        );
        assert_eq!(
            classify(Some("old"), Some("old"), None),
            DiskChange::Vanished
        );
        assert_eq!(classify(None, None, None), DiskChange::Ignore);
    }

    #[test]
    fn only_source_and_config_paths_are_watched() {
        assert!(is_watched("story.ink"));
        assert!(is_watched("acts/two.brink"));
        assert!(is_watched("brink.toml"));
        // Not the studio's own sidecars, not build output, not an inklecate
        // dump that happens to sit beside the sources.
        assert!(!is_watched(".binder.json"));
        assert!(!is_watched("acts/.hidden.ink"));
        assert!(!is_watched("story.ink.json"));
        assert!(!is_watched("notes.md"));
    }
}
