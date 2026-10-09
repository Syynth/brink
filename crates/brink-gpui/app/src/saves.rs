//! Save slots — the Player's checkpoints (W14, decision log 2026-10-09
//! "Native Save state"): the web studio's model, on disk.
//!
//! Two stores, both always listed: the PROJECT store, `<project>/.brink/
//! saves/` — shareable through the repo, out of the story files' way — and
//! the LOCAL store, this computer's app data, one folder per project. One
//! JSON file per slot, in the web's `SavePayload` shape (`meta`, `state`,
//! `transcript`) so a person can read one and the two studios can share
//! them. The payload is the runtime's durable `SaveState` plus where the
//! story was; never an execution position (a load resumes at the knot).

use std::path::{Path, PathBuf};

use brink_gpui_model::play::SavedState;
use serde::{Deserialize, Serialize};

/// Which store a slot lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Project,
    Local,
}

impl Store {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Store::Project => "project",
            Store::Local => "this computer",
        }
    }
}

/// One slot's listing row — the web's `SaveSlotMeta`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotMeta {
    pub id: String,
    pub name: String,
    pub turn: u32,
    pub knot_path: Option<String>,
    /// The program it was saved against (hex), so a list can mark a save
    /// made before the last edit.
    pub checksum: Option<String>,
    /// Unix milliseconds.
    pub saved_at: u64,
}

/// A slot's file — the web's `SavePayload`.
#[derive(Serialize, Deserialize)]
struct Payload {
    meta: SlotMeta,
    state: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transcript: Option<serde_json::Value>,
}

/// What a load needs back from a slot.
pub struct Loaded {
    pub meta: SlotMeta,
    pub state: String,
    pub transcript: Option<String>,
}

/// Where `store`'s slots for the project at `root` live.
#[must_use]
pub fn dir(store: Store, root: &Path) -> Option<PathBuf> {
    match store {
        Store::Project => Some(root.join(".brink").join("saves")),
        Store::Local => {
            // One folder per project, named by a stable hash of its path
            // (FNV-1a: the same across runs and builds).
            let key = root
                .to_string_lossy()
                .bytes()
                .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
                });
            brink_gpui_shell::settings::settings_dir()
                .map(|d| d.join("saves").join(format!("{key:016x}")))
        }
    }
}

/// Every slot in `dir`, newest first. A file that does not read as a slot
/// is skipped — a save store is not the place to fail loudly over a stray
/// file.
#[must_use]
pub fn list(dir: &Path) -> Vec<SlotMeta> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut slots: Vec<SlotMeta> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<Payload>(&text).ok())
        .map(|p| p.meta)
        .collect();
    slots.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then_with(|| a.id.cmp(&b.id)));
    slots
}

/// Read slot `id` for a load.
#[must_use]
pub fn read(dir: &Path, id: &str) -> Option<Loaded> {
    let text = std::fs::read_to_string(dir.join(format!("{id}.json"))).ok()?;
    let payload: Payload = serde_json::from_str(&text).ok()?;
    Some(Loaded {
        meta: payload.meta,
        state: payload.state.to_string(),
        transcript: payload.transcript.map(|t| t.to_string()),
    })
}

/// Write `saved` to slot `id` — over it, keeping its name (a loaded
/// slot's write-back) — or to a new slot when `id` is `None`.
///
/// # Errors
/// The folder or the file could not be written, or the checkpoint's JSON
/// did not parse.
pub fn write(dir: &Path, id: Option<&str>, saved: &SavedState) -> std::io::Result<SlotMeta> {
    std::fs::create_dir_all(dir)?;
    let existing = list(dir);
    let (id, name) = match id.and_then(|id| existing.iter().find(|m| m.id == id)) {
        Some(meta) => (meta.id.clone(), meta.name.clone()),
        None => {
            let n = existing
                .iter()
                .filter_map(|m| m.id.strip_prefix("save-")?.parse::<u32>().ok())
                .max()
                .unwrap_or(0)
                + 1;
            (format!("save-{n}"), format!("Save {n}"))
        }
    };
    let invalid = |e: serde_json::Error| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
    let meta = SlotMeta {
        id: id.clone(),
        name,
        turn: saved.turn,
        knot_path: saved.knot.clone(),
        checksum: Some(format!("{:08x}", saved.checksum)),
        saved_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
    };
    let payload = Payload {
        meta: meta.clone(),
        state: serde_json::from_str(&saved.state).map_err(invalid)?,
        transcript: Some(serde_json::from_str(&saved.transcript).map_err(invalid)?),
    };
    let text = serde_json::to_string_pretty(&payload).map_err(invalid)?;
    std::fs::write(dir.join(format!("{id}.json")), text)?;
    Ok(meta)
}

/// Remove slot `id`.
///
/// # Errors
/// The file could not be removed.
pub fn remove(dir: &Path, id: &str) -> std::io::Result<()> {
    std::fs::remove_file(dir.join(format!("{id}.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint(turn: u32) -> SavedState {
        SavedState {
            state: r#"{"version":1,"globals":{}}"#.to_owned(),
            transcript: r#"{"version":1,"checksum":7,"parts":[]}"#.to_owned(),
            turn,
            knot: Some("cove".to_owned()),
            checksum: 7,
        }
    }

    #[test]
    fn slots_are_numbered_written_back_listed_and_removed() {
        let dir = crate::harness::scratch_dir("saves");
        let first = write(&dir, None, &checkpoint(1)).expect("a new slot");
        assert_eq!(
            (first.id.as_str(), first.name.as_str()),
            ("save-1", "Save 1")
        );
        let second = write(&dir, None, &checkpoint(2)).expect("another");
        assert_eq!(second.id, "save-2");

        // Writing back to a loaded slot keeps it, and its name.
        let back = write(&dir, Some("save-1"), &checkpoint(5)).expect("write-back");
        assert_eq!(
            (back.id.as_str(), back.name.as_str(), back.turn),
            ("save-1", "Save 1", 5)
        );
        assert_eq!(list(&dir).len(), 2, "no new slot for a write-back");

        let loaded = read(&dir, "save-1").expect("reads");
        assert_eq!(loaded.meta.knot_path.as_deref(), Some("cove"));
        assert!(loaded.transcript.is_some());

        remove(&dir, "save-2").expect("removes");
        assert_eq!(list(&dir).len(), 1);
    }
}
