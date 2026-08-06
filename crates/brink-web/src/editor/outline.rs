use wasm_bindgen::prelude::*;

use super::EditorSession;
use crate::editor_dto::{DocumentSymbolJs, FileOutlineJs, IncludeInfoJs, convert_document_symbol};

#[wasm_bindgen]
impl EditorSession {
    /// Compute document symbols (outline) for a document handle. Returns JSON array.
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

    /// Get project outline — all files with their symbols. Returns JSON `[{path, symbols}]`.
    ///
    /// Excludes mounted stdlib files (issue #2231 review finding): a mount
    /// is not a file the project scan found or the user opened, so it must
    /// not appear in the Binder (`packages/brink-studio/src/mount.tsx`
    /// feeds the Binder from this on every compile).
    pub fn project_outline(&self) -> String {
        let db = self.session.db();
        let mut outline: Vec<FileOutlineJs> = Vec::new();

        for id in db.file_ids() {
            if self.mounted_std_ids.contains(&id) {
                continue;
            }
            let Some(path) = db.file_path(id) else {
                continue;
            };
            let (Some(hir), Some(manifest)) = (db.hir(id), db.manifest(id)) else {
                outline.push(FileOutlineJs {
                    path: path.to_owned(),
                    symbols: Vec::new(),
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
            });
        }

        // Sort by path for deterministic output
        outline.sort_by(|a, b| a.path.cmp(&b.path));
        serde_json::to_string(&outline).unwrap_or_default()
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
