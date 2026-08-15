use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::{FoldRangeJs, fold_kind_str};

#[wasm_bindgen]
impl EditorSession {
    /// Compute folding ranges for a document handle. Returns JSON array.
    ///
    /// The handle's own `view` reaches `Self::to_relative_line` below, which
    /// rebases surviving folds onto the fragment and drops those starting
    /// before it — guarded by `native_folding_ranges_doc_uses_the_handles_own_fragment_view`
    /// (#2500) and `native_folding_ranges_doc_entry_point` (#2458) in
    /// `super::tests`; see `docs/brink-ide-spec.md`, "Document-handle
    /// (`*_doc`) entry points: two standing invariants".
    pub fn folding_ranges_doc(&self, doc: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.folding_ranges_impl(&d.path, d.view.as_ref())
    }

    /// Compute folding ranges. Returns JSON array.
    pub fn folding_ranges(&self) -> String {
        self.folding_ranges_impl(&self.active_path, self.view.as_ref())
    }
}

impl EditorSession {
    fn folding_ranges_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(hir), Some(source)) = (self.session.hir(file_id), self.session.source(file_id))
        else {
            return "[]".to_owned();
        };

        // One cached projection feeds both fold families (#476, #480).
        let Some(projection) = self.session.projection(file_id) else {
            return "[]".to_owned();
        };

        // Structural folds (#313 G, #476 weave folds) — never auto-collapsed
        // by a host.
        let mut ranges = brink_ide::folding::folding_ranges(hir, source, &projection);

        // `~ { … }` logic-block + nested if/while/for folds (#589, #600).
        // No dialect gate: brink-syntax always parses the superset grammar
        // and brink-ir always lowers it to this HIR shape regardless of
        // dialect (docs/t1b-surface-spec.md §1) — a logic block folds
        // identically in a strict-ink file (flagged E051) as in a brink one.
        ranges.extend(brink_ide::folding::block_folds(hir, source));

        // Machinery/narrative fold runs (#365): computed from the same
        // per-line classification `line_contexts_impl` exposes, so a
        // registered dialect's declared `nature` (#368) flows into the fold
        // computation exactly as it flows into `line_contexts`. Gated (#479):
        // only hosts that opt in via `set_fold_runs_enabled` pay for it.
        //
        // Dialect-gated (#2291): a native (`.brink`) file's real CST is
        // `syntax_root_native`, not `syntax_root` (which always runs the
        // ink parser over the source text regardless of extension —
        // `IdeSession::syntax_root`'s doc comment, #2280's failure mode).
        // Feeding a `.brink` file's text to ink's grammar would compute
        // fold runs from a garbled tree; route to the native-CST-aware
        // `line_context` siblings instead.
        if self.fold_runs_enabled {
            let ctx = if self.session.is_native(file_id) {
                self.session
                    .syntax_root_native(file_id)
                    .map(|root| match self.session.dialect() {
                        Some(dialect) => {
                            brink_ide::line_context::line_contexts_with_dialect_native(
                                source,
                                &root,
                                &projection,
                                dialect,
                            )
                        }
                        None => brink_ide::line_context::line_contexts_native(
                            source,
                            &root,
                            &projection,
                        ),
                    })
            } else {
                self.session
                    .syntax_root(file_id)
                    .map(|root| match self.session.dialect() {
                        Some(dialect) => brink_ide::line_context::line_contexts_with_dialect(
                            source,
                            &root,
                            &projection,
                            dialect,
                        ),
                        None => brink_ide::line_context::line_contexts(source, &root, &projection),
                    })
            };
            if let Some(ctx) = ctx {
                ranges.extend(brink_ide::folding::machinery_and_narrative_folds(
                    &projection,
                    source,
                    &ctx,
                ));
            }
        }

        let items: Vec<FoldRangeJs> = ranges
            .iter()
            .filter_map(|r| {
                let start_line = Self::to_relative_line(view, r.start_line)?;
                let end_line = Self::to_relative_line(view, r.end_line)?;
                Some(FoldRangeJs {
                    start_line,
                    end_line,
                    collapsed_text: r.collapsed_text.clone(),
                    from_line_start: r.from_line_start,
                    kind: fold_kind_str(r.kind),
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }
}
