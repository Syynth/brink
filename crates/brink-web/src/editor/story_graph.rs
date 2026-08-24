use wasm_bindgen::prelude::*;

use super::EditorSession;
use super::utf16_index::Utf16Index;
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
    /// file/span). Returns JSON `StoryGraph` — since option A (2026-08-24)
    /// analysis is always available from the db, so the former pre-analysis
    /// `"null"` sentinel no longer occurs (a fresh session yields an empty
    /// graph); the wrapper's `StoryGraph | null` type is kept for wire
    /// compatibility.
    ///
    /// Lists mounted stdlib files' knots/stitches alongside real project
    /// files' (issue #2306/#2343, "Mounted stdlib presents as a read-only
    /// library node"): #2231 originally excluded them entirely (a mount is
    /// not a file the project scan found or the user opened), but the
    /// ruling supersedes "hide" with "list, but mark read-only" so the
    /// Binder's Library section can render its own story-graph nodes. Each
    /// node carries `mounted` (see [`StoryGraphNodeJs::mounted`]).
    pub fn story_graph(&self) -> String {
        crate::perf::time("ide.storyGraph", || self.story_graph_inner())
    }
}

// Private helpers — outside the `#[wasm_bindgen]` block, per this crate's
// convention (see `navigation.rs`).
impl EditorSession {
    fn story_graph_inner(&self) -> String {
        let Some(analysis) = self.session.analysis() else {
            return "null".to_owned();
        };
        let db = self.session.db();
        let files: Vec<(brink_ir::FileId, &brink_ir::HirFile)> = db
            .file_ids()
            .filter_map(|id| db.hir(id).map(|hir| (id, hir)))
            .collect();
        let graph = brink_ide::story_graph::story_graph(analysis, &files);

        // One Utf16Index per file (#3065): the naive per-offset scan made
        // node/edge-occurrence conversion O(occurrences × file size).
        let indexes: std::collections::BTreeMap<brink_ir::FileId, Utf16Index<'_>> = files
            .iter()
            .map(|(id, _)| (*id, Utf16Index::new(db.source(*id).unwrap_or(""))))
            .collect();

        let nodes: Vec<StoryGraphNodeJs> = graph
            .nodes
            .into_iter()
            .map(|n| {
                let mounted = n.file.is_some_and(|f| self.mounted_std_ids.contains(&f));
                let (file, start, end) = match (n.file, n.range) {
                    (Some(f), Some(r)) => {
                        let (start, end) = indexes.get(&f).map_or((0, 0), |ix| {
                            (
                                ix.byte_to_utf16(r.start().into()),
                                ix.byte_to_utf16(r.end().into()),
                            )
                        });
                        (db.file_path(f).map(str::to_owned), Some(start), Some(end))
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
                    mounted,
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
                        let (start, end) = indexes.get(&o.file).map_or((0, 0), |ix| {
                            (
                                ix.byte_to_utf16(o.range.start().into()),
                                ix.byte_to_utf16(o.range.end().into()),
                            )
                        });
                        Some(StoryGraphEdgeOccurrenceJs { file, start, end })
                    })
                    .collect(),
            })
            .collect();

        serde_json::to_string(&StoryGraphJs { nodes, edges }).unwrap_or_default()
    }
}
