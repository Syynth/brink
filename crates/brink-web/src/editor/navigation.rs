use rowan::TextSize;
use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext, byte_to_utf16};
use crate::editor_dto::{LocationJs, LocationWithKindJs};

#[wasm_bindgen]
impl EditorSession {
    /// Compute goto-definition for a document handle at the given offset. Returns JSON or "null".
    ///
    /// Like the outline family, none of this module's `*_doc` entry points
    /// branches on `is_native`: they read `analysis`, whose native dispatch
    /// happens a layer down in `brink_db::queries::raw_lowered_query`. That
    /// `goto_definition_doc`, `find_references_doc` and `prepare_rename_doc`
    /// all still reach native lowering for a `.brink` file opened through a
    /// handle is guarded by
    /// `native_outline_and_navigation_doc_entry_points_reach_native_lowering`
    /// (#2501) in `super::tests`; see `docs/brink-ide-spec.md`,
    /// "Document-handle (`*_doc`) entry points: two standing invariants".
    pub fn goto_definition_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        crate::perf::time("ide.gotoDefinition", || {
            self.goto_definition_impl(&d.path, d.view.as_ref(), offset)
        })
    }

    /// Compute goto-definition at the given byte offset. Returns JSON or "null".
    pub fn goto_definition(&self, offset: u32) -> String {
        crate::perf::time("ide.gotoDefinition", || {
            self.goto_definition_impl(&self.active_path, self.view.as_ref(), offset)
        })
    }

    /// Find all references for a document handle. Returns JSON array.
    pub fn find_references_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        crate::perf::time("ide.findReferences", || {
            self.find_references_impl(&d.path, d.view.as_ref(), offset, true)
        })
    }

    /// Find all references. Returns JSON array.
    pub fn find_references(&self, offset: u32) -> String {
        crate::perf::time("ide.findReferences", || {
            self.find_references_impl(&self.active_path, self.view.as_ref(), offset, true)
        })
    }

    /// Find all references at an explicit file path + offset, with control over
    /// whether the declaration itself is included. Document-agnostic: resolves
    /// the file by `path` against the session, not the active document. Returns
    /// a JSON `Location[]` array (`"[]"` if the path or analysis is unavailable).
    pub fn find_references_at(&self, path: &str, offset: u32, include_declaration: bool) -> String {
        crate::perf::time("ide.findReferences", || {
            self.find_references_impl(path, None, offset, include_declaration)
        })
    }

    /// [`Self::find_references_at`], with each site classified by how it
    /// uses the symbol (the Search panel's per-card badges — card spec
    /// PR E). Returns a JSON array of `{file, start, end, kind}` where
    /// `kind` ∈ `"decl" | "call" | "divert" | "read" | "write"`.
    /// Document-agnostic and file-absolute like `find_references_at`.
    pub fn find_references_with_kinds_at(
        &self,
        path: &str,
        offset: u32,
        include_declaration: bool,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, None, offset);
        let db = self.session.db();
        let refs = brink_ide::navigation::find_references_with_kinds(
            db,
            analysis,
            file_id,
            TextSize::new(abs_offset),
            include_declaration,
        );

        let items: Vec<LocationWithKindJs> = refs
            .iter()
            .map(|r| {
                let src = self.session.source(r.file).unwrap_or("");
                LocationWithKindJs {
                    file: db.file_path(r.file).unwrap_or_default().to_owned(),
                    start: byte_to_utf16(src, r.range.start().into()),
                    end: byte_to_utf16(src, r.range.end().into()),
                    kind: match r.kind {
                        brink_ide::navigation::ReferenceKind::Decl => "decl",
                        brink_ide::navigation::ReferenceKind::Call => "call",
                        brink_ide::navigation::ReferenceKind::Divert => "divert",
                        brink_ide::navigation::ReferenceKind::Read => "read",
                        brink_ide::navigation::ReferenceKind::Write => "write",
                    },
                }
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Find all references to a symbol identified by its canonical name. Resolves
    /// the symbol via the analysis index; returns `"[]"` (fail-safe, deterministic)
    /// if the name is unknown or ambiguous (more than one matching definition).
    /// Otherwise locates the symbol's declaration (file + range start) and returns
    /// its references as a JSON `Location[]` array.
    pub fn references_to_symbol(&self, symbol_name: &str, include_declaration: bool) -> String {
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };
        // Resolve the symbol name to a single definition. Unknown or ambiguous
        // names fail safe to an empty result rather than guessing.
        let ids = match analysis.index.by_name.get(symbol_name) {
            Some(ids) if ids.len() == 1 => ids,
            _ => return "[]".to_owned(),
        };
        let Some(info) = analysis.index.symbols.get(&ids[0]) else {
            return "[]".to_owned();
        };
        let Some(path) = self.session.file_path(info.file) else {
            return "[]".to_owned();
        };
        let Some(source) = self.session.source(info.file) else {
            return "[]".to_owned();
        };
        // The impl expects a UTF-16, view-relative offset; with no view that is
        // the file-absolute UTF-16 offset of the declaration's name start.
        let offset = byte_to_utf16(source, info.range.start().into());
        let path = path.to_owned();
        crate::perf::time("ide.findReferences", || {
            self.find_references_impl(&path, None, offset, include_declaration)
        })
    }

    /// Check if rename is possible for a document handle. Returns JSON or "null".
    pub fn prepare_rename_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        crate::perf::time("ide.prepareRename", || {
            self.prepare_rename_impl(&d.path, d.view.as_ref(), offset)
        })
    }

    /// Check if rename is possible. Returns JSON or "null".
    pub fn prepare_rename(&self, offset: u32) -> String {
        crate::perf::time("ide.prepareRename", || {
            self.prepare_rename_impl(&self.active_path, self.view.as_ref(), offset)
        })
    }
}

impl EditorSession {
    fn goto_definition_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let db = self.session.db();
        match brink_ide::navigation::goto_definition(
            db,
            analysis,
            file_id,
            TextSize::new(abs_offset),
        ) {
            Some(loc) => {
                let file_path = db.file_path(loc.file).unwrap_or_default().to_owned();
                let (start, end) = if loc.file == file_id {
                    // Same file: adjust to view-relative UTF-16 offsets
                    (
                        self.to_relative(path, view, loc.range.start().into())
                            .unwrap_or(loc.range.start().into()),
                        self.to_relative(path, view, loc.range.end().into())
                            .unwrap_or(loc.range.end().into()),
                    )
                } else {
                    // Cross-file: convert bytes to UTF-16 in the target file
                    let src = self.session.source(loc.file).unwrap_or("");
                    (
                        byte_to_utf16(src, loc.range.start().into()),
                        byte_to_utf16(src, loc.range.end().into()),
                    )
                };
                let js = LocationJs {
                    file: file_path,
                    start,
                    end,
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }

    fn find_references_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        offset: u32,
        include_declaration: bool,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let db = self.session.db();
        let refs = brink_ide::navigation::find_references(
            db,
            analysis,
            file_id,
            TextSize::new(abs_offset),
            include_declaration,
        );

        let items: Vec<LocationJs> = refs
            .iter()
            .filter_map(|loc| {
                if loc.file == file_id {
                    // Same file: adjust offsets, filter out-of-view
                    let start = self.to_relative(path, view, loc.range.start().into())?;
                    let end = self.to_relative(path, view, loc.range.end().into())?;
                    Some(LocationJs {
                        file: db.file_path(loc.file).unwrap_or_default().to_owned(),
                        start,
                        end,
                    })
                } else {
                    // Cross-file: convert bytes to UTF-16 in the target file
                    let src = self.session.source(loc.file).unwrap_or("");
                    Some(LocationJs {
                        file: db.file_path(loc.file).unwrap_or_default().to_owned(),
                        start: byte_to_utf16(src, loc.range.start().into()),
                        end: byte_to_utf16(src, loc.range.end().into()),
                    })
                }
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn prepare_rename_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let db = self.session.db();
        match brink_ide::rename::prepare_rename(db, analysis, file_id, TextSize::new(abs_offset)) {
            Some(range) => {
                let start = self.to_relative(path, view, range.start().into());
                let end = self.to_relative(path, view, range.end().into());
                match (start, end) {
                    (Some(s), Some(e)) => {
                        let js = LocationJs {
                            file: path.to_owned(),
                            start: s,
                            end: e,
                        };
                        serde_json::to_string(&js).unwrap_or_default()
                    }
                    _ => "null".to_owned(),
                }
            }
            None => "null".to_owned(),
        }
    }
}
