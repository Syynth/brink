use wasm_bindgen::prelude::*;

use super::{DocState, EditorSession, byte_to_utf16, splice_fragment};
use crate::editor_dto::{ChangeSpecJs, ProjectFileJs};

#[wasm_bindgen]
impl EditorSession {
    /// Open a full-file document handle on `path`. Returns the handle id,
    /// or `0` (never a valid id) if the file is not loaded.
    pub fn open_document(&mut self, path: &str) -> u32 {
        if self.session.file_id(path).is_none() {
            return 0;
        }
        self.insert_doc(DocState {
            path: path.to_owned(),
            view: None,
        })
    }

    /// Open a fragment document handle scoping `path` to `[start, end)`
    /// (UTF-16 offsets, same convention as `set_view_context`). Returns the
    /// handle id, or `0` (never a valid id) if the file is not loaded.
    pub fn open_fragment(&mut self, path: &str, start: u32, end: u32) -> u32 {
        if self.session.file_id(path).is_none() {
            return 0;
        }
        let view = self.compute_view_context(path, start, end);
        self.insert_doc(DocState {
            path: path.to_owned(),
            view: Some(view),
        })
    }

    /// Close a document handle. Returns `false` if the handle was unknown.
    pub fn close_document(&mut self, doc: u32) -> bool {
        self.docs.remove(&doc).is_some()
    }

    /// Replace a document's content: full-file replace for file handles,
    /// fragment splice for fragment handles (the handle's own view range is
    /// updated to cover the new fragment). Reparses, lowers, and analyzes.
    ///
    /// Returns a change-spec JSON object `{path, start, end, text?}`
    /// describing what actually changed in the file, in UTF-16 **file**
    /// coordinates: `[start, end)` is the replaced range of the file's
    /// previous content. The inserted text is the `source` argument the
    /// caller already has — unless `text` is present, in which case the
    /// fragment splice appended a `\n` separator and `text` carries what was
    /// actually inserted (`source` + `"\n"`). Returns `"null"` for an
    /// unknown handle, **and for a handle whose file is currently a mounted
    /// stdlib copy** (issue #2306): `open_document`/`open_fragment` on a
    /// `std/` path succeed (a mounted file is legitimately browsable/openable
    /// — e.g. a goto-def into an inherited symbol), but writing through that
    /// handle would silently fork the library into the project, so it is
    /// refused with the same "did not apply" sentinel a caller already
    /// handles for an unknown handle. The content is left untouched, unlike
    /// an unknown handle there is no ambiguity to log — see `is_read_only`.
    pub fn update_document(&mut self, doc: u32, source: &str) -> String {
        let Some(state) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        let path = state.path.clone();
        if self.is_read_only(&path) {
            return "null".to_owned();
        }
        let view = state.view;
        let full = self
            .session
            .file_id(&path)
            .and_then(|id| self.session.source(id).map(str::to_owned))
            .unwrap_or_default();

        let spec = if let Some(view) = view {
            let outcome = splice_fragment(&full, &view, source);
            if let Some(v) = self.docs.get_mut(&doc).and_then(|s| s.view.as_mut()) {
                v.end = outcome.new_view_end;
            }
            let spec = ChangeSpecJs {
                path: path.clone(),
                start: byte_to_utf16(&full, outcome.replaced_start),
                end: byte_to_utf16(&full, outcome.replaced_end),
                text: outcome.inserted_separator.then(|| format!("{source}\n")),
            };
            self.session.update_and_analyze(&path, outcome.spliced);
            spec
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink files are always < 4GB"
            )]
            let full_len = full.len() as u32;
            let spec = ChangeSpecJs {
                path: path.clone(),
                start: 0,
                end: byte_to_utf16(&full, full_len),
                text: None,
            };
            self.session.update_and_analyze(&path, source.to_owned());
            spec
        };
        serde_json::to_string(&spec).unwrap_or_default()
    }

    // ── Document-handle query variants ──────────────────────────────
    //
    // Same offset conventions as the singleton queries above (UTF-16,
    // view-relative per handle) and same JSON response shapes. An unknown
    // handle returns the same empty sentinel as a missing file.

    /// Get the source text for a document handle's view (fragment or full
    /// file). Returns a JSON string, or `"null"` for an unknown handle.
    pub fn get_view_source_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.get_view_source_impl(&d.path, d.view.as_ref())
    }

    /// Get the current active file path.
    pub fn active_file(&self) -> String {
        self.active_path.clone()
    }

    /// List all loaded files. Returns JSON `[{path, mounted}]`.
    ///
    /// Lists mounted stdlib files alongside real project files, flagged
    /// `mounted: true` (issue #2306/#2343, "Mounted stdlib presents as a
    /// read-only library node"): #2231 originally excluded them entirely
    /// (a mount is not a file the project scan found or the user opened),
    /// but the ruling supersedes "hide" with "list, but mark read-only" so
    /// the Binder can render a distinct "Library" section. Consumers that
    /// must still exclude them (project-wide search —
    /// `packages/studio-store/src/slices/search.ts`; `file.saveAll` —
    /// `ProjectSession.markAllSaved`) filter on the flag themselves.
    pub fn list_files(&self) -> String {
        let db = self.session.db();
        let files: Vec<ProjectFileJs> = db
            .file_ids()
            .filter_map(|id| {
                db.file_path(id).map(|p| ProjectFileJs {
                    path: p.to_owned(),
                    mounted: self.mounted_std_ids.contains(&id),
                })
            })
            .collect();
        serde_json::to_string(&files).unwrap_or_default()
    }

    /// Get the source text for a file. Returns JSON string or `"null"`.
    pub fn get_file_source(&self, path: &str) -> String {
        let source = self
            .session
            .file_id(path)
            .and_then(|id| self.session.source(id));
        match source {
            Some(s) => serde_json::to_string(s).unwrap_or_default(),
            None => "null".to_owned(),
        }
    }

    /// Whether `path` currently resolves to the mounted stdlib copy — the
    /// session-level read-only fence (issue #2306, ruled 2026-08-06 "Mounted
    /// stdlib presents as a read-only library node"). `false` for an unknown
    /// path, an ordinary project file, and a path that *used to* be a mount
    /// but was shadowed by a real project file (`mounted_std_ids` no longer
    /// contains its id at that point — see the field's own doc). Consumed by
    /// `update_document` directly, and by the TS session/API layer
    /// (`EditorSessionHandle.isReadOnly`, `ProjectSession.applyEdit`) to
    /// refuse a bulk write (search/replace, results-buffer edits, binder
    /// undo) before it ever reaches `update_file`.
    pub fn is_read_only(&self, path: &str) -> bool {
        self.session
            .file_id(path)
            .is_some_and(|id| self.mounted_std_ids.contains(&id))
    }
}
