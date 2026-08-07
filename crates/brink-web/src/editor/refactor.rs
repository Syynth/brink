use rowan::TextSize;
use wasm_bindgen::prelude::*;

use super::{EditorSession, byte_to_utf16, count_newlines, rebase_view, utf16_to_byte};
use crate::editor_refactor::{
    AutoImportJs, dir_error_json, dir_move_result_json, error_json, gated_move_json,
    move_result_json_simple, structural_result_json,
};

#[wasm_bindgen]
impl EditorSession {
    /// Reorder a stitch within its parent knot. Returns JSON `StructuralResult` or error string.
    ///
    /// `path`: file containing the knot.
    /// `direction`: 1 = down, -1 = up.
    pub fn reorder_stitch(&self, path: &str, knot: &str, stitch: &str, direction: i32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let dir = if direction >= 0 {
            brink_ide::structural_move::Direction::Down
        } else {
            brink_ide::structural_move::Direction::Up
        };

        match brink_ide::structural_move::reorder_stitch(source, knot, stitch, dir) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Move a stitch from one knot to another. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing both knots.
    pub fn move_stitch(&self, path: &str, src_knot: &str, stitch: &str, dest_knot: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::move_stitch(
            source, analysis, file_id, src_knot, stitch, dest_knot,
        ) {
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Rename or move a file, rewriting every `INCLUDE` that resolves to it
    /// (inbound) plus the moved file's own relative includes (outbound).
    /// Returns JSON `StructuralResult` or error: `new_source` is the moved file's
    /// content to write at `new`, `cross_file_edits` carry the referencing
    /// files' rewrites. The op computes edits only — the caller applies them
    /// (write `new`, remove `old`).
    pub fn rename_file(&self, old: &str, new: &str) -> String {
        match brink_ide::file_rename::rename_file(&self.session, old, new) {
            Ok(result) => structural_result_json(&self.session, &result, old),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Atomically rename or move a directory (#314): relocate every file under
    /// `old_prefix` to `new_prefix`, rewriting all affected `INCLUDE`s against a
    /// single pre-move snapshot (moved files' outbound includes, inbound includes
    /// from files outside the folder, and intra-folder sibling includes — all
    /// mutually consistent). Returns JSON `DirMoveResult`: `moved_files` are the
    /// relocated files (`old_path`, `new_path`, rewritten `new_source`),
    /// `cross_file_edits` carry the outside referrers' rewrites. `safe` +
    /// `introduced_diagnostics` are the shared safe-by-default breakage gate. The
    /// op computes edits only — the caller writes the new files, removes the old
    /// ones, and applies the inbound edits.
    pub fn rename_dir(&self, old_prefix: &str, new_prefix: &str) -> String {
        match brink_ide::dir_rename::rename_dir(&self.session, old_prefix, new_prefix) {
            Ok(result) => dir_move_result_json(&self.session, &result),
            Err(e) => dir_error_json(&e.to_string()),
        }
    }

    /// Ensure `current` `INCLUDE`s `target` (#312 F core).
    ///
    /// Returns JSON `{ ok, already_reachable, edit?: TextEdit, error? }`. When
    /// `target` is already reachable from `current`'s INCLUDE graph the op is a
    /// no-op (`already_reachable: true`, no `edit`). Otherwise `edit` is the
    /// byte-range insertion the caller applies to `current`'s source.
    pub fn auto_import_include(&self, current: &str, target: &str) -> String {
        let resp = match brink_ide::auto_import::ensure_include(&self.session, current, target) {
            Ok(result) => AutoImportJs {
                ok: true,
                already_reachable: result.already_reachable,
                edit: result.edit,
                error: None,
            },
            Err(e) => AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some(e.to_string()),
            },
        };
        serde_json::to_string(&resp).unwrap_or_default()
    }

    /// Auto-import `target` into the file backing document handle `doc` (#312 F,
    /// completion-accept path). Same `{ ok, already_reachable, edit?, error? }`
    /// shape as [`auto_import_include`], but the edit's `from`/`to` are
    /// **whole-file UTF-16** offsets (the INCLUDE block is a whole-file concept
    /// regardless of a fragment view), so the editor can apply it to the file
    /// source directly. Idempotent — no edit when `target` is already reachable.
    pub fn auto_import_include_doc(&self, doc: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("unknown document handle".to_owned()),
            })
            .unwrap_or_default();
        };
        let current = d.path.clone();
        let resp = match brink_ide::auto_import::ensure_include(&self.session, &current, target) {
            Ok(result) => {
                // Convert the byte-offset edit to whole-file UTF-16 so it can be
                // applied against the file source (or a whole-file view).
                let edit = result.edit.and_then(|e| {
                    let source = self.source_of(&current)?;
                    Some(brink_ide::line_convert::TextEdit {
                        from: byte_to_utf16(source, e.from),
                        to: byte_to_utf16(source, e.to),
                        insert: e.insert,
                    })
                });
                AutoImportJs {
                    ok: true,
                    already_reachable: result.already_reachable,
                    edit,
                    error: None,
                }
            }
            Err(e) => AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some(e.to_string()),
            },
        };
        serde_json::to_string(&resp).unwrap_or_default()
    }

    /// Auto-import `target` into the file backing document handle `doc` **and
    /// apply the INCLUDE edit out-of-band**, rebasing every open fragment view
    /// on that file (#312 F, fragment-view completion-accept path).
    ///
    /// A fragment (symbol-tab / "play from here") view cannot dispatch the
    /// whole-file INCLUDE edit into its own CM document — the INCLUDE lives
    /// above the fragment. So the caller applies it here. A raw whole-file
    /// replace ([`update_file`]) would prepend the INCLUDE but leave every open
    /// fragment handle's stored `ViewContext` pointing at pre-shift byte
    /// offsets, so the next fragment splice would clobber the INCLUDE line and
    /// surrounding content. This method inserts the INCLUDE *and* shifts the
    /// byte range (and start line) of every fragment view on the file that
    /// begins at/after the insertion point, keeping them consistent.
    ///
    /// Returns the same `{ ok, already_reachable, edit?, error? }` shape as
    /// [`auto_import_include_doc`]. On success the `edit` (whole-file UTF-16)
    /// **describes the shift that was already applied** — the caller must NOT
    /// re-apply it; it exists only so the caller can rebase its own TS-side
    /// fragment-range mirror by the UTF-16 delta before inserting the symbol
    /// text into the fragment view. When `target` is already reachable this is
    /// a no-op (`already_reachable: true`, no `edit`).
    pub fn auto_import_apply_include_doc(&mut self, doc: u32, target: &str) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("unknown document handle".to_owned()),
            })
            .unwrap_or_default();
        };
        let current = d.path.clone();
        if self.is_read_only(&current) {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("document handle is read-only (mounted stdlib file)".to_owned()),
            })
            .unwrap_or_default();
        }
        let result = match brink_ide::auto_import::ensure_include(&self.session, &current, target) {
            Ok(result) => result,
            Err(e) => {
                return serde_json::to_string(&AutoImportJs {
                    ok: false,
                    already_reachable: false,
                    edit: None,
                    error: Some(e.to_string()),
                })
                .unwrap_or_default();
            }
        };

        // Already reachable, or no edit produced: nothing to apply.
        let Some(edit) = result.edit.filter(|_| !result.already_reachable) else {
            return serde_json::to_string(&AutoImportJs {
                ok: true,
                already_reachable: result.already_reachable,
                edit: None,
                error: None,
            })
            .unwrap_or_default();
        };

        // `ensure_include` returns byte offsets for `from`/`to` into the current
        // file source. Apply the insertion to the whole-file source.
        let Some(source) = self.source_of(&current).map(str::to_owned) else {
            return serde_json::to_string(&AutoImportJs {
                ok: false,
                already_reachable: false,
                edit: None,
                error: Some("current file source unavailable".to_owned()),
            })
            .unwrap_or_default();
        };
        let from = (edit.from as usize).min(source.len());
        let to = (edit.to as usize).clamp(from, source.len());
        let mut merged = String::with_capacity(source.len() + edit.insert.len());
        merged.push_str(&source[..from]);
        merged.push_str(&edit.insert);
        merged.push_str(&source[to..]);

        // Rebase every open fragment view on this file whose range starts at or
        // after the insertion point. The edit removes `to - from` bytes and
        // inserts `edit.insert`, so downstream offsets shift by the net delta;
        // start lines shift by (inserted newlines − removed newlines).
        #[expect(
            clippy::cast_possible_wrap,
            reason = "ink files are always < 4GB, so byte counts fit i64"
        )]
        let byte_delta = edit.insert.len() as i64 - (to - from) as i64;
        let removed_newlines = count_newlines(&source[from..to]);
        let inserted_newlines = count_newlines(&edit.insert);
        let line_delta = i64::from(inserted_newlines) - i64::from(removed_newlines);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "ink files are always < 4GB"
        )]
        let insert_at = from as u32;
        for state in self.docs.values_mut() {
            if state.path != current {
                continue;
            }
            let Some(view) = state.view.as_mut() else {
                continue;
            };
            rebase_view(view, insert_at, byte_delta, line_delta);
        }

        // The whole-file UTF-16 edit that was applied, so the caller can rebase
        // its own TS-side fragment range mirror by the UTF-16 delta. This edit
        // is NOT for the caller to re-apply (it is already applied) — it merely
        // describes the shift.
        let applied_edit = brink_ide::line_convert::TextEdit {
            from: byte_to_utf16(&source, edit.from),
            to: byte_to_utf16(&source, edit.to),
            insert: edit.insert,
        };

        self.session.update_and_analyze(&current, merged);

        serde_json::to_string(&AutoImportJs {
            ok: true,
            already_reachable: false,
            edit: Some(applied_edit),
            error: None,
        })
        .unwrap_or_default()
    }

    /// Promote a stitch to a top-level knot. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing the knot.
    pub fn promote_stitch(&self, path: &str, knot: &str, stitch: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::promote_stitch_to_knot(
            source, analysis, file_id, knot, stitch,
        ) {
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder a knot within the top-level knot list. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing the knot.
    /// `direction`: 1 = down, -1 = up.
    pub fn reorder_knot(&self, path: &str, knot: &str, direction: i32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        let dir = if direction >= 0 {
            brink_ide::structural_move::Direction::Down
        } else {
            brink_ide::structural_move::Direction::Up
        };

        match brink_ide::structural_move::reorder_knot(source, knot, dir) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder all stitches in a knot to match `order` (a permutation of the
    /// knot's stitch names). Used by drag-and-drop and multi-select moves,
    /// which know the full destination order. Returns JSON `StructuralResult` or error.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen requires owned Vec<String> across the boundary"
    )]
    pub fn reorder_stitches(&self, path: &str, knot: &str, order: Vec<String>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        match brink_ide::structural_move::reorder_stitches(source, knot, &order) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Reorder all top-level knots to match `order` (a permutation of the knot
    /// names). Returns JSON `StructuralResult` or error.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen requires owned Vec<String> across the boundary"
    )]
    pub fn reorder_knots(&self, path: &str, order: Vec<String>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(source) = self.session.source(file_id) else {
            return error_json("no source");
        };

        match brink_ide::structural_move::reorder_knots(source, &order) {
            Ok(new_source) => move_result_json_simple(new_source, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Demote a top-level knot to a stitch inside another knot. Returns JSON `StructuralResult` or error.
    ///
    /// `path`: file containing both knots.
    pub fn demote_knot(&self, path: &str, knot: &str, dest_knot: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let (Some(source), Some(analysis)) =
            (self.session.source(file_id), self.session.analysis())
        else {
            return error_json("no source or analysis");
        };

        match brink_ide::structural_move::demote_knot_to_stitch(
            source, analysis, file_id, knot, dest_knot,
        ) {
            Ok(result) => gated_move_json(&self.session, result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Delete a knot (`stitch` empty) or a stitch, safe-by-default (#316).
    ///
    /// Removes the knot's whole region (header, body, nested stitches) or the
    /// named stitch's region, then runs the breakage gate: every divert /
    /// thread / tunnel / call that targeted the removed symbol now dangles, and
    /// those introduced diagnostics travel out so the caller can show a breakage
    /// report and apply the delete only on an explicit force. Returns the
    /// unified `StructuralResult` JSON (`new_source` for `path`, `safe`,
    /// `introduced_diagnostics`) or an error.
    pub fn delete_symbol(&self, path: &str, knot: &str, stitch: &str) -> String {
        let stitch = (!stitch.is_empty()).then_some(stitch);
        match brink_ide::structural_delete::delete_symbol(&self.session, path, knot, stitch) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Extract the selected lines into a new top-level `=== name ===` knot,
    /// replacing the selection with a tunnel call `-> name ->` (#315 H).
    ///
    /// `start_offset`/`end_offset` are whole-file UTF-16 offsets into `path`'s
    /// source (converted to bytes here). The selection is snapped to whole lines;
    /// the new knot is appended at end of file and ends with a `->->` return.
    /// Returns the unified `StructuralResult` JSON — `safe` is false and
    /// `introduced_diagnostics` is populated when the extraction pulls a
    /// weave/gather label or a local/temp reference out of scope. On failure a
    /// `StructuralResult`-shaped error is returned.
    pub fn extract_to_knot(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        name: &str,
    ) -> String {
        let Some(source) = self.source_of(path) else {
            return error_json("file not loaded");
        };
        let start = utf16_to_byte(source, start_offset) as usize;
        let end = utf16_to_byte(source, end_offset) as usize;
        match brink_ide::extract::extract_to_knot(&self.session, path, start, end, name) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Extract the selected lines into a new `=== function name() ===`, replacing
    /// the selection with the call — `{name()}` for a single value expression,
    /// `~ name()` for a statement (#315 H). Same offset/gate semantics as
    /// [`extract_to_knot`](Self::extract_to_knot).
    pub fn extract_to_function(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        name: &str,
    ) -> String {
        let Some(source) = self.source_of(path) else {
            return error_json("file not loaded");
        };
        let start = utf16_to_byte(source, start_offset) as usize;
        let end = utf16_to_byte(source, end_offset) as usize;
        match brink_ide::extract::extract_to_function(&self.session, path, start, end, name) {
            Ok(result) => structural_result_json(&self.session, &result, path),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Rename a knot or stitch by name, safe-by-default. Returns a
    /// `StructuralResult`-shaped JSON payload (`new_source` for `path`,
    /// `cross_file_edits` for referencing files) extended with
    /// `introduced_diagnostics` and a `safe` flag. When `safe` is false the
    /// rename would introduce the listed diagnostics — the caller shows a
    /// breakage report and applies the (already-computed) edits only on an
    /// explicit force. An empty `stitch` renames the knot itself.
    pub fn rename_symbol(&self, path: &str, knot: &str, stitch: &str, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let Some(hir) = self.session.hir(file_id) else {
            return error_json("no analysis");
        };
        let stitch = (!stitch.is_empty()).then_some(stitch);
        let Some(offset) = brink_ide::rename::declaration_offset(hir, knot, stitch) else {
            return error_json("symbol not found");
        };
        match brink_ide::rename::rename_safe(&self.session, file_id, offset, new_name) {
            Some(result) => structural_result_json(&self.session, &result, path),
            None => error_json("cannot rename this symbol"),
        }
    }

    /// Rename the symbol at a UTF-16 **file** offset, safe-by-default — the
    /// offset-based sibling of `rename_symbol`, used by the editor's F2 (which
    /// resolves any symbol under the cursor, not just knots/stitches). Returns
    /// the same `RenameResultJs` payload. The offset is a whole-file UTF-16
    /// offset (the caller folds any fragment-view origin in); it is converted
    /// to a byte offset here.
    pub fn rename_symbol_at(&self, path: &str, offset: u32, new_name: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return error_json("file not loaded");
        };
        let abs_offset = self.to_absolute(path, None, offset);
        match brink_ide::rename::rename_safe(
            &self.session,
            file_id,
            TextSize::new(abs_offset),
            new_name,
        ) {
            Some(result) => structural_result_json(&self.session, &result, path),
            None => error_json("cannot rename this symbol"),
        }
    }
}
