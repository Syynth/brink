use wasm_bindgen::prelude::*;

use super::EditorSession;
use crate::editor_dto::{DocumentSymbolJs, FileOutlineJs, IncludeInfoJs, convert_document_symbol};

#[wasm_bindgen]
impl EditorSession {
    /// Compute document symbols (outline) for a document handle. Returns JSON array.
    ///
    /// This family has no `is_native` branch of its own: `document_symbols_impl`
    /// reads `hir`/`manifest`, whose native dispatch happens a layer down in
    /// `brink_db::queries::raw_lowered_query`. That a `.brink` file opened
    /// through a handle still reaches native lowering is guarded by
    /// `native_outline_and_navigation_doc_entry_points_reach_native_lowering`
    /// (#2501) in `super::tests`; see `docs/brink-ide-spec.md`,
    /// "Document-handle (`*_doc`) entry points: two standing invariants".
    pub fn document_symbols_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.document_symbols_impl(&d.path)
    }

    /// Compute document symbols (outline). Returns JSON array.
    pub fn document_symbols(&self) -> String {
        self.document_symbols_impl(&self.active_path)
    }

    /// Get document symbols for a specific file. Returns JSON `DocumentSymbol[]`.
    pub fn file_symbols(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(manifest)) =
            (self.session.hir(file_id), self.session.manifest(file_id))
        else {
            return "[]".to_owned();
        };

        let source = self.session.source(file_id).unwrap_or("");
        let syms = brink_ide::document::document_symbols(hir, manifest, source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();
        serde_json::to_string(&items).unwrap_or_default()
    }

    /// Get project outline — all files with their symbols. Returns JSON
    /// `[{path, symbols, mounted}]`.
    ///
    /// Lists mounted stdlib files alongside real project files, flagged
    /// `mounted: true` (issue #2306/#2343, "Mounted stdlib presents as a
    /// read-only library node"): #2231 originally excluded them entirely
    /// (a mount is not a file the project scan found or the user opened),
    /// but the ruling supersedes "hide" with "list, but mark read-only" so
    /// the Binder (`packages/brink-studio/src/mount.tsx` feeds the Binder
    /// from this on every compile) can render a distinct "Library" section.
    pub fn project_outline(&self) -> String {
        crate::perf::time("ide.projectOutline", || self.project_outline_inner())
    }

    /// Project-relative paths of the current compile closure (#3017) —
    /// the exact file set codegen builds from, keyed by the entry the most
    /// recent `compile_project` set. Returns a JSON string array; empty
    /// (`[]`) before any compile. A file `project_outline` lists that is
    /// absent here is on disk but **not in the story** — the out-of-scope
    /// editor banner and the Binder's "not included" marks read exactly
    /// this difference. Read-only: never perturbs the entry or any salsa
    /// input, so calling it after a compile recomputes nothing.
    pub fn compilation_closure(&self) -> String {
        serde_json::to_string(&self.session.compilation_closure_paths()).unwrap_or_default()
    }

    /// Get resolved INCLUDE paths for a file. Returns JSON `[{path, resolved, loaded}]`.
    pub fn file_includes(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let Some(hir) = self.session.hir(file_id) else {
            return "[]".to_owned();
        };

        let db = self.session.db();
        let items: Vec<IncludeInfoJs> = hir
            .includes
            .iter()
            .map(|inc| {
                let resolved = brink_db::resolve_include_path(path, &inc.file_path);
                let loaded = db.file_id(&resolved).is_some();
                IncludeInfoJs {
                    path: inc.file_path.clone(),
                    resolved,
                    loaded,
                }
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }
}

impl EditorSession {
    fn project_outline_inner(&self) -> String {
        let db = self.session.db();
        let mut outline: Vec<FileOutlineJs> = Vec::new();

        for id in db.file_ids() {
            let mounted = self.mounted_std_ids.contains(&id);
            let Some(path) = db.file_path(id) else {
                continue;
            };
            let (Some(hir), Some(manifest)) = (db.hir(id), db.manifest(id)) else {
                outline.push(FileOutlineJs {
                    path: path.to_owned(),
                    symbols: Vec::new(),
                    mounted,
                });
                continue;
            };

            let source = db.source(id).unwrap_or("");
            let syms = brink_ide::document::document_symbols(hir, manifest, source);
            let items: Vec<DocumentSymbolJs> = syms
                .into_iter()
                .map(|s| convert_document_symbol(s, source))
                .collect();
            outline.push(FileOutlineJs {
                path: path.to_owned(),
                symbols: items,
                mounted,
            });
        }

        // Sort by path for deterministic output
        outline.sort_by(|a, b| a.path.cmp(&b.path));
        serde_json::to_string(&outline).unwrap_or_default()
    }

    fn document_symbols_impl(&self, path: &str) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(manifest)) =
            (self.session.hir(file_id), self.session.manifest(file_id))
        else {
            return "[]".to_owned();
        };

        let source = self.session.source(file_id).unwrap_or("");
        let syms = brink_ide::document::document_symbols(hir, manifest, source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, source))
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }
}
