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

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, Entity, Subscription};
use notify::{RecursiveMode, Watcher};

use crate::project::{Project, ProjectEvent};

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

/// The watch on a project: dropping it stops it.
pub struct Watching {
    _pump: gpui::Task<()>,
    _config: Subscription,
}

/// Files the config reads from outside the root (#3687), by where they are
/// on disk: their config key, as `Project::reads_config` knows it.
type Outside = Rc<RefCell<BTreeMap<PathBuf, String>>>;

/// Start watching `root`, and apply what changes there to `project` —
/// and, besides the root, the folder of every file the config reads from
/// outside it (a host manifest in a sibling build folder, #3687), kept in
/// step with the config as it is applied again.
pub fn start(project: Entity<Project>, root: PathBuf, cx: &mut App) -> Watching {
    let (tx, rx) = async_channel::unbounded::<PathBuf>();
    let outside: Outside = Rc::default();
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        for path in event.paths {
            let _ = tx.send_blocking(path);
        }
    })
    .ok();
    let Some(mut watcher) = watcher else {
        // A studio that cannot watch still works; it just cannot notice.
        return Watching {
            _pump: gpui::Task::ready(()),
            _config: cx.observe(&project, |_, _| {}),
        };
    };
    let watching_root = watcher.watch(&root, RecursiveMode::Recursive).is_ok();
    let watcher = Rc::new(RefCell::new(watcher));

    // The outside folders follow the config: opened, and applied again on
    // every analysis (an edit to `brink.toml` can name a new manifest).
    let config = {
        let (watcher, outside, root) = (watcher.clone(), outside.clone(), root.clone());
        let dirs: Rc<RefCell<BTreeSet<PathBuf>>> = Rc::default();
        cx.subscribe(&project, move |project, event, cx| {
            if !matches!(event, ProjectEvent::Opened { .. } | ProjectEvent::Analyzed) {
                return;
            }
            let reads = project.read(cx).config_reads().cloned().collect::<Vec<_>>();
            let wanted = outside_reads(&root, &reads);
            let want_dirs: BTreeSet<PathBuf> = wanted
                .keys()
                .filter_map(|abs| abs.parent().map(Path::to_path_buf))
                .filter(|dir| dir.is_dir())
                .collect();
            let mut dirs = dirs.borrow_mut();
            let mut watcher = watcher.borrow_mut();
            for gone in dirs.difference(&want_dirs) {
                let _ = watcher.unwatch(gone);
            }
            for new in want_dirs.difference(&dirs) {
                let _ = watcher.watch(new, RecursiveMode::NonRecursive);
            }
            *dirs = want_dirs;
            *outside.borrow_mut() = wanted;
        })
    };

    let pump = cx.spawn(async move |cx| {
        // Held for the life of the task: dropping the watcher stops the
        // events, and nothing else owns it.
        let _watcher = watcher;
        if !watching_root {
            return;
        }
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
            let keys: Vec<String> = paths
                .iter()
                .filter_map(|path| key_for(&root, &outside.borrow(), path))
                .collect();
            // Sources and the config, plus the files the config reads —
            // which only the project knows (#3666: a host regenerating its
            // manifest is the point of reading one from disk).
            project.update(cx, |project, cx| {
                let watched: Vec<String> = keys
                    .into_iter()
                    .filter(|path| is_watched(path) || project.reads_config(path))
                    .collect();
                if !watched.is_empty() {
                    project.disk_changed(&watched, cx);
                }
            });
        }
    });
    Watching {
        _pump: pump,
        _config: config,
    }
}

/// The config reads that leave `root` (`../build/host.json`, or an
/// absolute path), by where they are on disk.
fn outside_reads(root: &Path, reads: &[String]) -> BTreeMap<PathBuf, String> {
    reads
        .iter()
        .filter_map(|key| {
            let abs = normalize(&root.join(key));
            (!abs.starts_with(root)).then(|| (abs, key.clone()))
        })
        .collect()
}

/// What a path the watcher reported is to the project: its root-relative
/// key, or the config key of a file read from outside the root.
fn key_for(root: &Path, outside: &BTreeMap<PathBuf, String>, path: &Path) -> Option<String> {
    relative_to(root, path).or_else(|| outside.get(&normalize(path)).cloned())
}

/// `path` with `.` and `..` resolved lexically — the way the config's own
/// keys resolve (`host_manifest_path`), so `root/../build/x` and the
/// watcher's `build/x` meet.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
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
    use super::{DiskChange, classify, is_watched, key_for, outside_reads};
    use std::path::{Path, PathBuf};

    /// A config read that leaves the root is followed by where it is on
    /// disk, and an event there maps back to the config's own key (#3687);
    /// one inside the root needs nothing extra.
    #[test]
    fn a_config_read_outside_the_root_maps_back_to_its_key() {
        let root = Path::new("/work/game/ink");
        let reads = [
            "../build/host-manifest.json".to_owned(),
            "dialect.json".to_owned(),
        ];
        let outside = outside_reads(root, &reads);
        assert_eq!(
            outside.keys().collect::<Vec<_>>(),
            [&PathBuf::from("/work/game/build/host-manifest.json")],
            "only the read that leaves the root"
        );
        assert_eq!(
            key_for(
                root,
                &outside,
                Path::new("/work/game/build/host-manifest.json")
            )
            .as_deref(),
            Some("../build/host-manifest.json")
        );
        assert_eq!(
            key_for(root, &outside, Path::new("/work/game/ink/story.ink")).as_deref(),
            Some("story.ink"),
            "inside the root, its relative key as ever"
        );
        assert_eq!(
            key_for(root, &outside, Path::new("/work/game/build/story.ink.json")),
            None,
            "a neighbour of the manifest is nothing to the project"
        );
    }

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
