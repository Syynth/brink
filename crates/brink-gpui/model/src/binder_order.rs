//! The `.binder.json` order sidecar — the authored order of the Binder's
//! rows, kept beside the project rather than in it.
//!
//! Ported from `packages/studio-store/src/binder-order.ts` (#3038), whose
//! identity convention this keeps exactly, so one project opened in both
//! studios reads the same: child ids are project-relative paths, a FOLDER
//! id carries a trailing `/`, and the root container is `""`.
//!
//! **Placement is authorship** (ruled: the Continuous view reads the
//! manuscript in Binder order, `docs/decision-log.md`). So the order a
//! writer drags into IS content in the sense that matters — it decides
//! what the manuscript is — while the file itself is cached presentation:
//! a corrupt sidecar self-heals to the fallback rather than refusing to
//! open the project, because losing an arrangement is a smaller harm than
//! not opening at all.
//!
//! The sidecar never reaches the analysis session: `.json` is not a source
//! file, and the worker's loader only picks up `.ink`/`.brink`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The sidecar's project-relative path.
pub const PATH: &str = ".binder.json";

/// Per-container display order, plus the folders that exist without files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BinderOrder {
    /// Container id (`""` for the root, else a folder id) → child ids in
    /// order. A `BTreeMap` rather than a hash map so the written file is
    /// byte-stable: a sidecar that reshuffles itself on every write is a
    /// diff in every commit.
    pub order: BTreeMap<String, Vec<String>>,
    /// Folders made in the app with nothing in them yet — a file-derived
    /// tree cannot represent those otherwise.
    pub folders: Vec<String>,
}

/// Whether an id names a folder (the trailing-slash convention).
#[must_use]
pub fn is_folder_id(id: &str) -> bool {
    id.ends_with('/')
}

/// Parse sidecar text. Any malformed shape self-heals to the fallback.
#[must_use]
pub fn parse(text: &str) -> BinderOrder {
    let Ok(raw) = serde_json::from_str::<Value>(text) else {
        return BinderOrder::default();
    };
    let mut order = BTreeMap::new();
    if let Some(map) = raw.get("order").and_then(Value::as_object) {
        for (container, ids) in map {
            if let Some(ids) = ids.as_array() {
                order.insert(
                    container.clone(),
                    ids.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                );
            }
        }
    }
    let folders = raw
        .get("folders")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|f| is_folder_id(f))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    BinderOrder { order, folders }
}

/// The sidecar's text, with the trailing newline a text file wants.
#[must_use]
pub fn serialize(value: &BinderOrder) -> String {
    let order: serde_json::Map<String, Value> = value
        .order
        .iter()
        .map(|(k, ids)| (k.clone(), json!(ids)))
        .collect();
    let doc = json!({ "order": Value::Object(order), "folders": value.folders });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_owned())
    )
}

/// Record a reorder: the container's FULL child list in its new order,
/// never a delta, so an entry describes itself.
pub fn apply_reorder(value: &mut BinderOrder, container: &str, ordered: Vec<String>) {
    value.order.insert(container.to_owned(), ordered);
}

/// Re-key for a rename or move: every container key, ordered child id and
/// registered folder at or under `old` moves to `new`, so an arrangement
/// survives reorganisation.
pub fn rekey(value: &BinderOrder, old: &str, new: &str) -> BinderOrder {
    let rekey_one = |id: &str| -> String {
        if id == old {
            new.to_owned()
        } else if is_folder_id(old) && id.starts_with(old) {
            format!("{new}{}", &id[old.len()..])
        } else {
            id.to_owned()
        }
    };
    BinderOrder {
        order: value
            .order
            .iter()
            .map(|(container, ids)| {
                (
                    rekey_one(container),
                    ids.iter().map(|id| rekey_one(id)).collect(),
                )
            })
            .collect(),
        folders: value.folders.iter().map(|f| rekey_one(f)).collect(),
    }
}

/// Drop a removed id — a file, or a folder and everything under it.
pub fn remove(value: &BinderOrder, id: &str) -> BinderOrder {
    let gone = |candidate: &str| candidate == id || (is_folder_id(id) && candidate.starts_with(id));
    BinderOrder {
        order: value
            .order
            .iter()
            .filter(|(container, _)| !gone(container))
            .map(|(container, ids)| {
                (
                    container.clone(),
                    ids.iter().filter(|id| !gone(id)).cloned().collect(),
                )
            })
            .collect(),
        folders: value.folders.iter().filter(|f| !gone(f)).cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_of(pairs: &[(&str, &[&str])]) -> BinderOrder {
        BinderOrder {
            order: pairs
                .iter()
                .map(|(k, ids)| {
                    (
                        (*k).to_owned(),
                        ids.iter().map(|s| (*s).to_owned()).collect(),
                    )
                })
                .collect(),
            folders: Vec::new(),
        }
    }

    #[test]
    fn a_sidecar_round_trips() {
        let mut value = order_of(&[("", &["b.ink", "acts/"]), ("acts/", &["acts/two.ink"])]);
        value.folders = vec!["empty/".to_owned()];
        assert_eq!(parse(&serialize(&value)), value);
    }

    #[test]
    fn a_corrupt_sidecar_self_heals_rather_than_refusing_to_open() {
        // Cached presentation: losing an arrangement is a smaller harm
        // than a project that will not open.
        assert_eq!(parse("not json at all"), BinderOrder::default());
        assert_eq!(parse("[]"), BinderOrder::default());
        assert_eq!(parse("{}"), BinderOrder::default());
        // Entries of the wrong shape are dropped, the rest survives.
        let mixed = parse(r#"{"order":{"":["a.ink",3,null],"x":5},"folders":["f/","no-slash"]}"#);
        assert_eq!(mixed.order[""], vec!["a.ink".to_owned()]);
        assert!(!mixed.order.contains_key("x"));
        assert_eq!(mixed.folders, vec!["f/".to_owned()]);
    }

    #[test]
    fn a_reorder_is_the_whole_list_not_a_delta() {
        let mut value = order_of(&[("", &["a.ink", "b.ink"])]);
        apply_reorder(&mut value, "", vec!["b.ink".to_owned(), "a.ink".to_owned()]);
        assert_eq!(
            value.order[""],
            vec!["b.ink".to_owned(), "a.ink".to_owned()]
        );
    }

    #[test]
    fn a_rename_carries_the_arrangement_with_it() {
        let value = order_of(&[
            ("", &["one.ink", "acts/"]),
            ("acts/", &["acts/two.ink", "acts/three.ink"]),
        ]);
        let moved = rekey(&value, "acts/two.ink", "acts/act-two.ink");
        assert_eq!(
            moved.order["acts/"],
            vec!["acts/act-two.ink".to_owned(), "acts/three.ink".to_owned()],
            "the file keeps its place in its folder"
        );

        // A FOLDER rename moves the container key and everything under it.
        let moved = rekey(&value, "acts/", "parts/");
        assert!(moved.order.contains_key("parts/"), "the container moved");
        assert!(!moved.order.contains_key("acts/"));
        assert_eq!(
            moved.order["parts/"],
            vec!["parts/two.ink".to_owned(), "parts/three.ink".to_owned()]
        );
        assert_eq!(
            moved.order[""],
            vec!["one.ink".to_owned(), "parts/".to_owned()]
        );
    }

    #[test]
    fn a_delete_drops_the_id_and_a_folder_takes_its_subtree() {
        let value = order_of(&[
            ("", &["one.ink", "acts/"]),
            ("acts/", &["acts/two.ink", "acts/three.ink"]),
        ]);
        let gone = remove(&value, "acts/two.ink");
        assert_eq!(gone.order["acts/"], vec!["acts/three.ink".to_owned()]);

        let gone = remove(&value, "acts/");
        assert!(!gone.order.contains_key("acts/"), "the container went too");
        assert_eq!(
            gone.order[""],
            vec!["one.ink".to_owned()],
            "and the folder left its parent's order"
        );
    }
}
