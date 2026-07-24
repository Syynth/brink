use wasm_bindgen::prelude::*;

use super::{EditorSession, byte_to_utf16};
use crate::editor_dto::{
    StoryGraphEdgeJs, StoryGraphEdgeOccurrenceJs, StoryGraphJs, StoryGraphNodeJs,
    story_edge_kind_str, story_node_kind_str,
};

#[wasm_bindgen]
impl EditorSession {
    /// Whole-project story graph (studio-shell spec §4.1): knot/stitch nodes
    /// plus `END`/`DONE` pseudo-nodes, and divert/choice/tunnel/thread edges.
    /// Function knots and function-call edges are excluded. Node spans and
    /// edge-occurrence spans are UTF-16 offsets in their own file; each edge
    /// lists the divert sites that produced it (#371). Deterministically
    /// ordered (nodes by id, edges by from/to/kind, occurrences by
    /// file/span). Returns JSON `StoryGraph`, or `"null"` when no analysis
    /// is available.
    pub fn story_graph(&self) -> String {
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };
        let db = self.session.db();
        let files: Vec<(brink_ir::FileId, &brink_ir::HirFile)> = db
            .file_ids()
            .filter_map(|id| db.hir(id).map(|hir| (id, hir)))
            .collect();
        let graph = brink_ide::story_graph::story_graph(analysis, &files);

        let nodes: Vec<StoryGraphNodeJs> = graph
            .nodes
            .into_iter()
            .map(|n| {
                let (file, start, end) = match (n.file, n.range) {
                    (Some(f), Some(r)) => {
                        let src = db.source(f).unwrap_or("");
                        (
                            db.file_path(f).map(str::to_owned),
                            Some(byte_to_utf16(src, r.start().into())),
                            Some(byte_to_utf16(src, r.end().into())),
                        )
                    }
                    _ => (None, None, None),
                };
                StoryGraphNodeJs {
                    id: n.id,
                    name: n.name,
                    kind: story_node_kind_str(n.kind),
                    file,
                    start,
                    end,
                    parent: n.parent,
                }
            })
            .collect();
        let edges: Vec<StoryGraphEdgeJs> = graph
            .edges
            .into_iter()
            .map(|e| StoryGraphEdgeJs {
                from: e.from,
                to: e.to,
                kind: story_edge_kind_str(e.kind),
                occurrences: e
                    .occurrences
                    .iter()
                    .filter_map(|o| {
                        let file = db.file_path(o.file)?.to_owned();
                        let src = db.source(o.file).unwrap_or("");
                        Some(StoryGraphEdgeOccurrenceJs {
                            file,
                            start: byte_to_utf16(src, o.range.start().into()),
                            end: byte_to_utf16(src, o.range.end().into()),
                        })
                    })
                    .collect(),
            })
            .collect();

        serde_json::to_string(&StoryGraphJs { nodes, edges }).unwrap_or_default()
    }
}
