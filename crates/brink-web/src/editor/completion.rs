use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::{CompletionItemJs, dedupe_out_of_scope, symbol_kind_str, typed_detail};

#[wasm_bindgen]
impl EditorSession {
    /// Compute completions for a document handle at the given offset. Returns JSON array.
    pub fn completions_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.completions_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute completions at the given byte offset. Returns JSON array.
    pub fn completions(&self, offset: u32) -> String {
        self.completions_impl(&self.active_path, self.view.as_ref(), offset)
    }
}

impl EditorSession {
    fn completions_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        let mut ctx = brink_ide::detect_completion_context(source, abs_offset as usize);
        let scope = brink_ide::cursor_scope(source, abs_offset as usize);

        // Cue-name completion (issue #2134, `docs/prose-dialect-spec.md`
        // §5): every `@NAME` cue harvested anywhere in the project — not
        // just this file — completes here, mirroring `brink-lsp`'s
        // `completion` handler. Reads the range-free completion projection
        // (`harvest_completion_names`), not the raw `harvest_index`, for
        // the same Eq-cutoff reason `resolution_index_query` exists for the
        // symbol index.
        //
        // `detect_completion_context` is dialect-agnostic (review finding on
        // #2134, minor): it classifies purely from source text, so a plain
        // ink prose line that happens to start with `@` is misread as the
        // same `CueName` position, even though ink's grammar has no cue
        // syntax at all (`cue_names_are_never_harvested_from_the_ink_frontend`,
        // `brink-analyzer`) — an ink file's own harvest contribution is
        // always empty, project-wide harvest from *other* native files
        // notwithstanding. Gate on the file's own language (native `.brink`
        // vs ink), not on whether the harvest happens to be empty right now:
        // a native file with zero declared cues anywhere in the project is
        // still a genuine (if currently empty) cue position and must keep
        // returning no items rather than falling back to ordinary symbols —
        // exactly what `cue_name_completion_offers_nothing_but_harvested_cues`
        // pins. Only an ink file — which can never mean a cue, regardless of
        // harvest state — downgrades `ctx` to `General` here.
        if matches!(ctx, brink_ide::CompletionContext::CueName) {
            if is_native_path(path) {
                let names = self.session.db().harvest_completion_names();
                let items: Vec<CompletionItemJs> = names
                    .cues
                    .iter()
                    .map(|name| CompletionItemJs {
                        name: name.clone(),
                        kind: "cue".to_owned(),
                        detail: None,
                        insert: None,
                        out_of_scope: false,
                        source_file: None,
                    })
                    .collect();
                return serde_json::to_string(&items).unwrap_or_default();
            }
            ctx = brink_ide::CompletionContext::General;
        }

        // Auto-import (#312 F): symbols declared in files NOT reachable from the
        // current file's INCLUDE graph are still offered, but tagged as
        // out-of-scope so the editor can render a "from <file>" affordance and
        // insert the INCLUDE on accept. Reachability includes the current file
        // itself; locals (params/temps) carry no owning importable file.
        let reachable = self
            .session
            .file_id(path)
            .map(|id| self.session.db().reachable_from(id));

        // T1e (docs/t1e-spec.md §2, issue #850): `ref` argument ROOT
        // position — completion right after `ref ` narrows to durable
        // cells only (`VAR`s, the E080 rule every `ref lvalue-path` root
        // must satisfy), instead of the full `FunctionArgs` set (which also
        // offers CONST/param/temp/ListItem — none of them a legal `ref`
        // root, so offering them there would suggest an argument that's
        // guaranteed to fail analysis). Path *continuations* (`ref npc.`,
        // `ref inventory[`) aren't narrowed here — see
        // `ref_arg_root_prefix`'s own doc for why that's out of scope for
        // "where cheap".
        let ref_root = brink_ide::ref_arg_root_prefix(source, abs_offset as usize);

        let symbol_items = analysis
            .index
            .symbols
            .values()
            .filter(|info| brink_ide::is_visible_in_context(&ctx, info, &scope))
            .filter(|info| ref_root.is_none() || info.kind == brink_ir::SymbolKind::Variable)
            .map(|info| {
                let is_local = matches!(
                    info.kind,
                    brink_ir::SymbolKind::Param | brink_ir::SymbolKind::Temp
                );
                // A symbol is out of scope when its declaring file is not
                // reachable from the current file. Locals are never imported.
                let out_of_scope = !is_local
                    && reachable
                        .as_ref()
                        .is_some_and(|set| !set.contains(&info.file));
                let source_file = if out_of_scope {
                    self.session.file_path(info.file).map(str::to_owned)
                } else {
                    None
                };
                CompletionItemJs {
                    name: info.name.clone(),
                    kind: symbol_kind_str(info.kind).to_owned(),
                    // Callables get a typed signature from /// docs or the host
                    // manifest, if any; otherwise the kind-derived detail.
                    detail: typed_detail(analysis, info).or_else(|| info.detail.clone()),
                    insert: None,
                    out_of_scope,
                    source_file,
                }
            });

        // Host value picker (#174): in an argument slot whose param has a value
        // source, offer its labelled values first (display the label, insert the
        // literal) — static items from the manifest, or `host` items from the
        // pushed cache.
        let mut items: Vec<CompletionItemJs> = Vec::new();
        // Host value-picker literals aren't legal `ref` roots either (#850) —
        // gate the same way the symbol-kind filter above does.
        if matches!(ctx, brink_ide::CompletionContext::FunctionArgs) && ref_root.is_none() {
            items.extend(
                brink_ide::signature::argument_value_completions(
                    analysis,
                    source,
                    abs_offset as usize,
                    Some(self.session.host_values()),
                )
                .into_iter()
                .map(|v| CompletionItemJs {
                    name: v.label,
                    kind: "value".to_owned(),
                    detail: v.detail,
                    insert: Some(v.value),
                    out_of_scope: false,
                    source_file: None,
                }),
            );
        }

        // Multiple definitions of one name (#312 F): when a name is declared in
        // several out-of-scope files, keep only the nearest by relative-path
        // distance so the auto-import targets a single deterministic file. In-
        // scope duplicates (already reachable) are left untouched — they insert
        // no INCLUDE. `dedupe_out_of_scope` sorts, so the result is stable.
        let symbol_items = dedupe_out_of_scope(path, symbol_items.collect());
        items.extend(symbol_items);

        // Stdlib slice 1 completion (docs/t1b-surface-spec.md §5, #589,
        // #600) — brink dialect only ("never offered in StrictInk"); an
        // author-defined symbol of the same name is already offered above
        // (shadowing, per §5), mirroring brink-lsp's `completion` handler.
        items.extend(
            brink_ide::stdlib_completions(&ctx, self.dialect)
                .iter()
                .map(|f| CompletionItemJs {
                    name: f.name.to_owned(),
                    kind: "stdlib".to_owned(),
                    detail: Some(f.signature_label()),
                    insert: None,
                    out_of_scope: false,
                    source_file: None,
                }),
        );

        serde_json::to_string(&items).unwrap_or_default()
    }
}

/// Whether `path` names a native `.brink` file — the only frontend whose
/// grammar has cue (`@NAME`) syntax at all
/// (`cue_names_are_never_harvested_from_the_ink_frontend`, `brink-analyzer`).
/// A deliberate, minimal duplicate of `brink-db`'s own `file_language`
/// extension check (crate-private there), used by
/// [`EditorSession::completions_impl`]'s cue-completion gate (review finding
/// on #2134, minor) to tell "an ink prose line that happens to start with
/// `@`" apart from a real native cue position.
fn is_native_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext == "brink")
}
