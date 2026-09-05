use wasm_bindgen::prelude::*;

use super::EditorSession;
use super::utf16_index::Utf16Index;
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
        let index = Utf16Index::new(source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, &index))
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
    /// The draft path set behind [`EditorSession::draft_paths`].
    ///
    /// The rule itself lives at the session layer
    /// (`brink_ide::drafts`) so both studio surfaces get the same answer —
    /// see the decision log, "Both studio consumers sit on the same layer".
    pub(crate) fn draft_path_list(&self) -> Vec<String> {
        self.session.draft_paths()
    }

    /// JSON behind [`EditorSession::draft_glob_report`] —
    /// `{ compiled, globs: [{ glob, drafts, in_story }] }`.
    ///
    /// [`Self::draft_paths`] answers "which files are drafts"; this answers
    /// "what is each glob I wrote actually doing", which is the question the
    /// settings list has to answer and cannot derive from the first. Two
    /// things are invisible in a bare list of glob strings and both are
    /// ordinary author mistakes:
    ///
    /// - a glob that matches **nothing** (a typo, or a folder since renamed)
    ///   is indistinguishable from one that is working;
    /// - a glob that matches a file the entry still reaches marks it in
    ///   `in_story` rather than `drafts`, because of the "reachability wins"
    ///   ruling (2026-08-27). The author wrote a glob and the file did not
    ///   become a draft; without this split the settings view would show that
    ///   glob as if it had taken effect.
    ///
    /// `compiled` is false before the first compile, when the closure is
    /// empty and nothing is known to be unreachable yet — the same window
    /// [`Self::draft_paths`] reports as empty. Every list is then empty too,
    /// and a caller should say "not known yet" rather than "matches nothing":
    /// those look identical in the data and mean opposite things.
    ///
    /// Globs come back in the order the author wrote them, and a file
    /// matching two globs is listed under both — this is attribution, not a
    /// partition.
    pub(crate) fn draft_glob_report_json(&self) -> String {
        serde_json::to_string(&self.draft_glob_report_inner()).unwrap_or_default()
    }

    fn draft_glob_report_inner(&self) -> DraftGlobReportJs {
        // Marshalling only: the attribution is computed at the session layer.
        let report = self.session.draft_glob_report();
        DraftGlobReportJs {
            compiled: report.compiled,
            globs: report
                .globs
                .into_iter()
                .map(|glob| DraftGlobJs {
                    glob: glob.glob,
                    drafts: glob.drafts,
                    in_story: glob.in_story,
                })
                .collect(),
        }
    }

    fn project_outline_inner(&self) -> String {
        let db = self.session.db();
        let mut outline: Vec<FileOutlineJs> = Vec::new();

        for id in db.file_ids() {
            let mounted = self.session.is_mounted_std(id);
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
            let index = Utf16Index::new(source);
            let items: Vec<DocumentSymbolJs> = syms
                .into_iter()
                .map(|s| convert_document_symbol(s, &index))
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
        let index = Utf16Index::new(source);
        let items: Vec<DocumentSymbolJs> = syms
            .into_iter()
            .map(|s| convert_document_symbol(s, &index))
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }
}

/// One configured `[project] drafts` glob and what it currently matches.
#[derive(serde::Serialize)]
struct DraftGlobJs {
    glob: String,
    /// Files this glob makes drafts — matched and outside the compile closure.
    drafts: Vec<String>,
    /// Files it matches that the entry still reaches, so they are NOT drafts.
    in_story: Vec<String>,
}

/// The report behind [`EditorSession::draft_glob_report`].
#[derive(serde::Serialize)]
struct DraftGlobReportJs {
    /// False before the first compile — every list is empty because draft
    /// status is not yet knowable, not because nothing matched.
    compiled: bool,
    globs: Vec<DraftGlobJs>,
}
