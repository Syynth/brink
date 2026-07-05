//! Whole-project story-graph extraction (studio-shell spec §4.1).
//!
//! Produces the structural graph of a project: knot/stitch nodes plus
//! `END`/`DONE` pseudo-nodes, and divert/choice/tunnel/thread edges between
//! them. Function knots and function-call edges are excluded.
//!
//! Divert targets are resolved through the analyzer's resolution map — the
//! same machinery goto-definition uses — so relative paths, labels, and
//! cross-file targets all land on the right node. Edges that target labels
//! are aggregated up to the label's owning knot/stitch; diverts inside a
//! choice's body are aggregated from the weave up to the owning knot/stitch
//! as `choice` edges. Unresolved targets and dynamic targets (diverts
//! through variables) produce no edge — the former are already diagnosed by
//! analysis, the latter are not statically resolvable.
//!
//! Diverts in a file's root content (before the first knot) have no owning
//! node and are not represented. A knot with no body content but stitches
//! carries the HIR-synthesized first-stitch auto-enter divert — that edge is
//! genuine control flow and is included.
//!
//! Every edge carries its source **occurrences** — the spans of the divert
//! sites that produced it. Aggregated edges (e.g. two choices targeting the
//! same knot) keep one occurrence per site. Path targets use the target
//! path's span; `-> DONE`/`-> END` use the divert statement's span. The only
//! edges without occurrences are HIR-synthesized `DONE`/`END` diverts that
//! have no syntax pointer (none are currently synthesized).
//!
//! Ordering is deterministic: nodes sort by id, edges by `(from, to, kind)`,
//! occurrences by `(file, range)` — independent of input order (the
//! HashMap-iteration rule).

use std::collections::{BTreeMap, BTreeSet};

use brink_analyzer::AnalysisResult;
use brink_ir::{
    Block, Conditional, Content, ContentPart, DivertPath, DivertTarget, FileId, HirFile, Sequence,
    Stmt, SymbolKind,
};
use brink_syntax::ast::SyntaxNodePtr;
use rowan::TextRange;

/// Node id of the `END` pseudo-node.
pub const END_NODE_ID: &str = "END";
/// Node id of the `DONE` pseudo-node.
pub const DONE_NODE_ID: &str = "DONE";

/// The kind of a story-graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StoryNodeKind {
    Knot,
    Stitch,
    /// The `END` pseudo-node (story permanently ends).
    End,
    /// The `DONE` pseudo-node (current flow completes).
    Done,
}

/// The kind of a story-graph edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StoryEdgeKind {
    /// Plain `-> target`.
    Divert,
    /// A choice's target divert, aggregated up to the owning knot/stitch.
    Choice,
    /// `-> target ->` — control returns to the caller.
    Tunnel,
    /// `<- target` — forked flow.
    Thread,
}

/// A node in the story graph: a knot, a stitch, or an `END`/`DONE`
/// pseudo-node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryGraphNode {
    /// Stable id — the qualified name (`knot`, `knot.stitch`), or
    /// [`END_NODE_ID`]/[`DONE_NODE_ID`] for pseudo-nodes.
    pub id: String,
    /// The qualified name (same as `id`; kept separate so ids can evolve
    /// without breaking display).
    pub name: String,
    pub kind: StoryNodeKind,
    /// Declaring file — `None` for pseudo-nodes.
    pub file: Option<FileId>,
    /// Byte span of the declaration name — `None` for pseudo-nodes.
    pub range: Option<TextRange>,
    /// For stitches: the owning knot's node id.
    pub parent: Option<String>,
}

/// A source site that produced an edge: the span of the divert's target
/// path, or the whole divert statement for `-> DONE`/`-> END`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeOccurrence {
    /// The file containing the divert site.
    pub file: FileId,
    /// Byte span of the site within `file`.
    pub range: TextRange,
}

/// A directed edge in the story graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryGraphEdge {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    pub kind: StoryEdgeKind,
    /// The divert sites that produced this edge, sorted by `(file, range)`
    /// and deduplicated. An aggregated edge keeps one entry per site.
    pub occurrences: Vec<EdgeOccurrence>,
}

/// The whole-project story graph. Nodes are sorted by id, edges by
/// `(from, to, kind)`; edges are deduplicated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoryGraph {
    pub nodes: Vec<StoryGraphNode>,
    pub edges: Vec<StoryGraphEdge>,
}

/// Extract the story graph for a project from its analysis result and
/// per-file HIR.
pub fn story_graph(analysis: &AnalysisResult, files: &[(FileId, &HirFile)]) -> StoryGraph {
    let mut builder = Builder {
        analysis,
        nodes: BTreeMap::new(),
        edges: BTreeMap::new(),
    };
    // Walk files in FileId order so duplicate-definition ties (already a
    // diagnostic) break deterministically.
    let mut files: Vec<(FileId, &HirFile)> = files.to_vec();
    files.sort_by_key(|&(id, _)| id.0);
    for &(file, hir) in &files {
        builder.walk_file(file, hir);
    }
    builder.finish()
}

/// Edge identity: `(from, to, kind)`. `BTreeMap` keys keep edges sorted.
type EdgeKey = (String, String, StoryEdgeKind);
/// Occurrence as an orderable tuple: `(file, start, end)` — `TextRange`
/// itself isn't `Ord`, so occurrences sort/dedupe in this form.
type OccKey = (FileId, u32, u32);

struct Builder<'a> {
    analysis: &'a AnalysisResult,
    /// Keyed by node id — `BTreeMap` so the final node list is sorted.
    nodes: BTreeMap<String, StoryGraphNode>,
    /// Aggregated edges: the inner `BTreeSet` dedupes and sorts each edge's
    /// source occurrences.
    edges: BTreeMap<EdgeKey, BTreeSet<OccKey>>,
}

impl Builder<'_> {
    fn finish(self) -> StoryGraph {
        let nodes_by_id = self.nodes;
        // Drop edges whose target didn't materialize as a node (e.g. a
        // stitch of an excluded function knot). Sources always exist —
        // edges are only emitted while walking their owning node.
        let edges: Vec<StoryGraphEdge> = self
            .edges
            .into_iter()
            .filter(|((_, to, _), _)| nodes_by_id.contains_key(to))
            .map(|((from, to, kind), occs)| StoryGraphEdge {
                from,
                to,
                kind,
                occurrences: occs
                    .into_iter()
                    .map(|(file, start, end)| EdgeOccurrence {
                        file,
                        range: TextRange::new(start.into(), end.into()),
                    })
                    .collect(),
            })
            .collect();
        StoryGraph {
            nodes: nodes_by_id.into_values().collect(),
            edges,
        }
    }

    fn walk_file(&mut self, file: FileId, hir: &HirFile) {
        for knot in &hir.knots {
            if knot.is_function {
                continue;
            }
            let id = knot.name.text.clone();
            self.add_node(StoryGraphNode {
                id: id.clone(),
                name: id.clone(),
                kind: StoryNodeKind::Knot,
                file: Some(file),
                range: Some(knot.name.range),
                parent: None,
            });
            self.walk_block(file, &id, &knot.body, false);
            for stitch in &knot.stitches {
                let sid = format!("{id}.{}", stitch.name.text);
                self.add_node(StoryGraphNode {
                    id: sid.clone(),
                    name: sid.clone(),
                    kind: StoryNodeKind::Stitch,
                    file: Some(file),
                    range: Some(stitch.name.range),
                    parent: Some(id.clone()),
                });
                self.walk_block(file, &sid, &stitch.body, false);
            }
        }
    }

    fn add_node(&mut self, node: StoryGraphNode) {
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    /// Walk a block's statements, emitting edges owned by `owner`.
    /// `in_choice` is true inside a choice's body — plain diverts there are
    /// `choice` edges (weave aggregation).
    fn walk_block(&mut self, file: FileId, owner: &str, block: &Block, in_choice: bool) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Content(c) => self.walk_content(file, owner, c, in_choice),
                Stmt::Divert(d) => {
                    let kind = if in_choice {
                        StoryEdgeKind::Choice
                    } else {
                        StoryEdgeKind::Divert
                    };
                    let stmt_range = d.ptr.as_ref().map(SyntaxNodePtr::text_range);
                    self.add_edge(file, owner, &d.target, kind, stmt_range);
                }
                Stmt::TunnelCall(tc) => {
                    let stmt_range = Some(tc.ptr.text_range());
                    for target in &tc.targets {
                        self.add_edge(file, owner, target, StoryEdgeKind::Tunnel, stmt_range);
                    }
                }
                Stmt::ThreadStart(ts) => {
                    let stmt_range = Some(ts.ptr.text_range());
                    self.add_edge(file, owner, &ts.target, StoryEdgeKind::Thread, stmt_range);
                }
                Stmt::ChoiceSet(cs) => {
                    for choice in &cs.choices {
                        for content in [
                            choice.start_content.as_ref(),
                            choice.bracket_content.as_ref(),
                            choice.inner_content.as_ref(),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            self.walk_content(file, owner, content, true);
                        }
                        self.walk_block(file, owner, &choice.body, true);
                    }
                    // The gather continuation runs after the choices
                    // converge — it keeps the enclosing context.
                    self.walk_block(file, owner, &cs.continuation, in_choice);
                }
                Stmt::LabeledBlock(b) => self.walk_block(file, owner, b, in_choice),
                Stmt::Conditional(c) => self.walk_conditional(file, owner, c, in_choice),
                Stmt::Sequence(s) => self.walk_sequence(file, owner, s, in_choice),
                // Expression-bearing statements: function calls are
                // excluded from the graph by design.
                Stmt::TempDecl(_)
                | Stmt::Assignment(_)
                | Stmt::Return(_)
                | Stmt::ExprStmt(_)
                | Stmt::EndOfLine => {}
            }
        }
    }

    fn walk_content(&mut self, file: FileId, owner: &str, content: &Content, in_choice: bool) {
        for part in &content.parts {
            match part {
                ContentPart::InlineConditional(c) => {
                    self.walk_conditional(file, owner, c, in_choice);
                }
                ContentPart::InlineSequence(s) => self.walk_sequence(file, owner, s, in_choice),
                ContentPart::Text(_)
                | ContentPart::Glue
                | ContentPart::Spring
                | ContentPart::Interpolation(_) => {}
            }
        }
    }

    fn walk_conditional(&mut self, file: FileId, owner: &str, cond: &Conditional, in_choice: bool) {
        for branch in &cond.branches {
            self.walk_block(file, owner, &branch.body, in_choice);
        }
    }

    fn walk_sequence(&mut self, file: FileId, owner: &str, seq: &Sequence, in_choice: bool) {
        for branch in &seq.branches {
            self.walk_block(file, owner, branch, in_choice);
        }
    }

    /// Record an edge and its source occurrence. Path targets anchor the
    /// occurrence on the target path's span; `DONE`/`END` fall back to the
    /// divert statement's span (`stmt_range`), which is absent only on
    /// HIR-synthesized diverts.
    fn add_edge(
        &mut self,
        file: FileId,
        from: &str,
        target: &DivertTarget,
        kind: StoryEdgeKind,
        stmt_range: Option<TextRange>,
    ) {
        let (to, span) = match &target.path {
            DivertPath::Done => {
                self.ensure_pseudo(DONE_NODE_ID, StoryNodeKind::Done);
                (DONE_NODE_ID.to_owned(), stmt_range)
            }
            DivertPath::End => {
                self.ensure_pseudo(END_NODE_ID, StoryNodeKind::End);
                (END_NODE_ID.to_owned(), stmt_range)
            }
            DivertPath::Path(path) => {
                let Some(to) = self.resolve_target(file, path.range) else {
                    return;
                };
                (to, Some(path.range))
            }
        };
        let occurrences = self.edges.entry((from.to_owned(), to, kind)).or_default();
        if let Some(range) = span {
            occurrences.insert((file, range.start().into(), range.end().into()));
        }
    }

    fn ensure_pseudo(&mut self, id: &str, kind: StoryNodeKind) {
        self.nodes
            .entry(id.to_owned())
            .or_insert_with(|| StoryGraphNode {
                id: id.to_owned(),
                name: id.to_owned(),
                kind,
                file: None,
                range: None,
                parent: None,
            });
    }

    /// Resolve a divert path to a graph node id via the analyzer's
    /// resolution map — the same resolution goto-definition uses (the
    /// recorded reference range is the path's source range). Labels
    /// aggregate up to their owning knot/stitch (their qualified name minus
    /// the label segment). Function knots and non-address targets
    /// (variables) yield no edge.
    fn resolve_target(&self, file: FileId, range: TextRange) -> Option<String> {
        let target = self
            .analysis
            .resolutions
            .iter()
            .find(|r| r.file == file && r.range == range)?
            .target;
        let info = self.analysis.index.symbols.get(&target)?;
        match info.kind {
            SymbolKind::Knot | SymbolKind::Stitch => {
                (info.detail.as_deref() != Some("function")).then(|| info.name.clone())
            }
            SymbolKind::Label => info
                .name
                .rsplit_once('.')
                .map(|(owner, _)| owner.to_owned()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DONE_NODE_ID, END_NODE_ID, StoryEdgeKind, StoryGraph, StoryNodeKind, story_graph};
    use crate::session::IdeSession;

    /// Build a graph from `(path, source)` pairs through a full analyze pass.
    fn graph_for(files: &[(&str, &str)]) -> StoryGraph {
        let mut session = IdeSession::new();
        for (path, source) in files {
            session.update_and_analyze(path, (*source).to_string());
        }
        let analysis = session.analysis().expect("analysis");
        let db = session.db();
        let hirs: Vec<_> = db
            .file_ids()
            .filter_map(|id| db.hir(id).map(|h| (id, h)))
            .collect();
        story_graph(analysis, &hirs)
    }

    const MAIN: &str = "\
-> start
=== start ===
Hello.
* [Go east] -> east.gate
* [Stay] -> stay_here
- -> hub
= stay_here
You stay.
-> DONE

=== hub ===
The hub.
<- ambient
-> trinket ->
{helper() > 0:
    -> start
}
~ temp x = helper()
- (mark) A mark.
-> END

=== trinket ===
A trinket.
->->

=== ambient ===
Birdsong.
-> DONE

=== function helper ===
~ return 1
";

    const EAST: &str = "\
=== east ===
= gate
The east gate.
-> hub.mark
";

    fn ids(graph: &StoryGraph) -> Vec<&str> {
        graph.nodes.iter().map(|n| n.id.as_str()).collect()
    }

    fn edge_triples(graph: &StoryGraph) -> Vec<(&str, &str, StoryEdgeKind)> {
        graph
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str(), e.kind))
            .collect()
    }

    #[test]
    fn multi_file_graph_nodes_and_edges() {
        let graph = graph_for(&[("main.ink", MAIN), ("east.ink", EAST)]);

        // Nodes: sorted by id; functions excluded; END/DONE pseudo-nodes
        // present because they're referenced.
        assert_eq!(
            ids(&graph),
            vec![
                DONE_NODE_ID,
                END_NODE_ID,
                "ambient",
                "east",
                "east.gate",
                "hub",
                "start",
                "start.stay_here",
                "trinket",
            ]
        );

        // Edges: sorted by (from, to, kind); deduplicated.
        assert_eq!(
            edge_triples(&graph),
            vec![
                ("ambient", DONE_NODE_ID, StoryEdgeKind::Divert),
                // `east` has no body content — HIR synthesizes the
                // first-stitch auto-enter divert, real control flow.
                ("east", "east.gate", StoryEdgeKind::Divert),
                // Cross-file divert to a label aggregates to the owning knot.
                ("east.gate", "hub", StoryEdgeKind::Divert),
                ("hub", END_NODE_ID, StoryEdgeKind::Divert),
                ("hub", "ambient", StoryEdgeKind::Thread),
                // Divert inside a conditional branch — plain divert.
                ("hub", "start", StoryEdgeKind::Divert),
                ("hub", "trinket", StoryEdgeKind::Tunnel),
                // Choice targets aggregate to the owning knot. The relative
                // `-> stay_here` resolves to the qualified stitch.
                ("start", "east.gate", StoryEdgeKind::Choice),
                ("start", "hub", StoryEdgeKind::Divert),
                ("start", "start.stay_here", StoryEdgeKind::Choice),
                ("start.stay_here", DONE_NODE_ID, StoryEdgeKind::Divert),
            ]
        );

        // No function-call edges, no edges from the file root (`-> start`
        // before the first knot has no owning node).
        assert!(!graph.edges.iter().any(|e| e.to == "helper"));
        assert!(graph.nodes.iter().all(|n| n.id != "helper"));
    }

    #[test]
    fn node_metadata_kinds_files_spans_parents() {
        let graph = graph_for(&[("main.ink", MAIN), ("east.ink", EAST)]);

        let node = |id: &str| {
            graph
                .nodes
                .iter()
                .find(|n| n.id == id)
                .expect("missing node")
        };

        let start = node("start");
        assert_eq!(start.kind, StoryNodeKind::Knot);
        assert_eq!(start.parent, None);
        assert!(start.file.is_some());
        let range = start.range.expect("knot range");
        let main_id = start.file.expect("knot file");

        // The span is the declaration *name* — "start" inside `=== start ===`.
        assert_eq!(u32::from(range.end() - range.start()), 5);

        let stay = node("start.stay_here");
        assert_eq!(stay.kind, StoryNodeKind::Stitch);
        assert_eq!(stay.parent.as_deref(), Some("start"));
        assert_eq!(stay.file, Some(main_id), "stitch declared in main.ink");

        let gate = node("east.gate");
        assert_eq!(gate.kind, StoryNodeKind::Stitch);
        assert_eq!(gate.parent.as_deref(), Some("east"));
        assert_ne!(gate.file, Some(main_id), "east.gate declared in east.ink");

        let end = node(END_NODE_ID);
        assert_eq!(end.kind, StoryNodeKind::End);
        assert_eq!(end.file, None);
        assert_eq!(end.range, None);
        let done = node(DONE_NODE_ID);
        assert_eq!(done.kind, StoryNodeKind::Done);
    }

    #[test]
    fn weave_aggregation_nested_choices_and_gathers() {
        // Nested weave: inner choices aggregate to the knot too; the
        // top-level gather divert stays a plain divert.
        let src = "\
=== top ===
* [A]
  * * [A1] -> alpha
  * * [A2] -> beta
* [B] -> beta
- -> alpha

=== alpha ===
-> DONE

=== beta ===
-> DONE
";
        let graph = graph_for(&[("w.ink", src)]);
        assert_eq!(
            edge_triples(&graph),
            vec![
                ("alpha", DONE_NODE_ID, StoryEdgeKind::Divert),
                ("beta", DONE_NODE_ID, StoryEdgeKind::Divert),
                ("top", "alpha", StoryEdgeKind::Divert),
                ("top", "alpha", StoryEdgeKind::Choice),
                // Two choices target beta — deduplicated to one edge.
                ("top", "beta", StoryEdgeKind::Choice),
            ]
        );
    }

    #[test]
    fn unresolved_and_dynamic_targets_produce_no_edges() {
        let src = "\
VAR somewhere = -> alpha
=== top ===
-> somewhere
-> missing_knot

=== alpha ===
-> END
";
        let graph = graph_for(&[("v.ink", src)]);
        // Only alpha's `-> END` survives: the divert through a variable and
        // the unresolved target are skipped.
        assert_eq!(
            edge_triples(&graph),
            vec![("alpha", END_NODE_ID, StoryEdgeKind::Divert)]
        );
    }

    #[test]
    fn edge_occurrences_point_at_divert_sites() {
        let graph = graph_for(&[("main.ink", MAIN), ("east.ink", EAST)]);
        let edge = |from: &str, to: &str, kind: StoryEdgeKind| {
            graph
                .edges
                .iter()
                .find(|e| e.from == from && e.to == to && e.kind == kind)
                .expect("missing edge")
        };
        let occ_texts = |from, to, kind, src: &str| -> Vec<String> {
            edge(from, to, kind)
                .occurrences
                .iter()
                .map(|o| src[std::ops::Range::<usize>::from(o.range)].to_owned())
                .collect()
        };

        // Path targets anchor on the target path's span — cross-file too.
        assert_eq!(
            occ_texts("east.gate", "hub", StoryEdgeKind::Divert, EAST),
            vec!["hub.mark"]
        );
        assert_eq!(
            occ_texts("start", "east.gate", StoryEdgeKind::Choice, MAIN),
            vec!["east.gate"]
        );
        // Thread and tunnel sites.
        assert_eq!(
            occ_texts("hub", "ambient", StoryEdgeKind::Thread, MAIN),
            vec!["ambient"]
        );
        assert_eq!(
            occ_texts("hub", "trinket", StoryEdgeKind::Tunnel, MAIN),
            vec!["trinket"]
        );
        // DONE/END edges fall back to the divert statement's span.
        assert_eq!(
            occ_texts("hub", END_NODE_ID, StoryEdgeKind::Divert, MAIN),
            vec!["-> END"]
        );

        // Occurrences carry the file of the divert site, not the target.
        let cross = edge("east.gate", "hub", StoryEdgeKind::Divert);
        let east_file = graph
            .nodes
            .iter()
            .find(|n| n.id == "east")
            .and_then(|n| n.file)
            .expect("east file");
        assert_eq!(cross.occurrences[0].file, east_file);
    }

    #[test]
    fn aggregated_edges_keep_one_occurrence_per_site() {
        let src = "\
=== top ===
* [A] -> beta
* [B] -> beta
* [C] -> beta

=== beta ===
-> DONE
";
        let graph = graph_for(&[("w.ink", src)]);
        let edge = graph
            .edges
            .iter()
            .find(|e| e.from == "top" && e.to == "beta")
            .expect("choice edge");
        assert_eq!(edge.kind, StoryEdgeKind::Choice);
        // One deduplicated edge, but all three sites preserved, in order.
        let starts: Vec<u32> = edge
            .occurrences
            .iter()
            .map(|o| o.range.start().into())
            .collect();
        assert_eq!(starts.len(), 3);
        assert!(starts.windows(2).all(|w| w[0] < w[1]), "sorted: {starts:?}");
        for o in &edge.occurrences {
            assert_eq!(&src[std::ops::Range::<usize>::from(o.range)], "beta");
        }
    }

    #[test]
    fn deterministic_across_repeated_builds() {
        let a = graph_for(&[("main.ink", MAIN), ("east.ink", EAST)]);
        let b = graph_for(&[("main.ink", MAIN), ("east.ink", EAST)]);
        assert_eq!(a, b);
    }
}
