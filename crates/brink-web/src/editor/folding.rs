use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::{FoldRangeJs, fold_kind_str};

#[wasm_bindgen]
impl EditorSession {
    /// Compute folding ranges for a document handle. Returns JSON array.
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
        if self.fold_runs_enabled
            && let Some(root) = self.session.syntax_root(file_id)
        {
            let ctx = match self.session.dialect() {
                Some(dialect) => brink_ide::line_context::line_contexts_with_dialect(
                    source,
                    &root,
                    &projection,
                    dialect,
                ),
                None => brink_ide::line_context::line_contexts(source, &root, &projection),
            };
            ranges.extend(brink_ide::folding::machinery_and_narrative_folds(
                &projection,
                source,
                &ctx,
            ));
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
