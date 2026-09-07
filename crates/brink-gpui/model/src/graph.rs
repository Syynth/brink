//! The story graph, as the studio's canvas needs it.
//!
//! `brink_ide::story_graph` already computes the whole thing — knots and
//! stitches as nodes, diverts, choices, tunnels and threads as edges, with
//! `END`/`DONE` as pseudo-nodes and every edge carrying the sites that
//! produced it. This module is the plain-data crossing: `FileId`s become
//! the compiler's file keys, `TextRange`s become byte pairs, and the
//! result carries nothing of the engine, as everything the worker answers
//! must (`docs/gpui-studio-spec.md` §3).
//!
//! Occurrences are reduced to their FIRST site per edge. The panel draws
//! one line per edge and opens one place when you follow it; keeping all
//! of them would be data with nowhere to go.

use brink_ide::session::IdeSession;
use brink_ide::story_graph::{StoryEdgeKind, StoryNodeKind};

/// A node: a knot, a stitch, or an `END`/`DONE` pseudo-node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    /// `knot`, `stitch`, `end`, `done` — the panel's own vocabulary, kept
    /// as a string so the wire carries no engine enum.
    pub kind: String,
    /// Declaring file and the byte span of its name. Absent for a
    /// pseudo-node, which is nowhere in the text.
    pub file: Option<String>,
    pub range: Option<(u32, u32)>,
    /// For a stitch: the owning knot's node id.
    pub parent: Option<String>,
}

/// A directed edge, with the first site that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    /// `divert`, `choice`, `tunnel`, `thread`.
    pub kind: String,
    /// Where to go when the edge is followed: the first divert site.
    pub site: Option<(String, u32, u32)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoryGraphReport {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// The entry knot's node id, when the project names an entry file that
    /// declares one — the layout's root.
    pub entry: Option<String>,
}

#[must_use]
pub fn node_kind(kind: StoryNodeKind) -> &'static str {
    match kind {
        StoryNodeKind::Knot => "knot",
        StoryNodeKind::Stitch => "stitch",
        StoryNodeKind::End => "end",
        StoryNodeKind::Done => "done",
    }
}

#[must_use]
pub fn edge_kind(kind: StoryEdgeKind) -> &'static str {
    match kind {
        StoryEdgeKind::Divert => "divert",
        StoryEdgeKind::Choice => "choice",
        StoryEdgeKind::Tunnel => "tunnel",
        StoryEdgeKind::Thread => "thread",
    }
}

/// Build the report from the session's current analysis.
#[must_use]
pub fn report(session: &IdeSession, entry: Option<&str>, files: &[String]) -> StoryGraphReport {
    let Some(analysis) = session.analysis() else {
        return StoryGraphReport::default();
    };
    let ids: Vec<(brink_ir::FileId, &brink_ir::hir::HirFile)> = files
        .iter()
        .filter_map(|path| {
            let id = session.file_id(path)?;
            session.hir(id).map(|hir| (id, hir))
        })
        .collect();
    let graph = brink_ide::story_graph::story_graph(analysis, &ids);
    let path_of = |id: brink_ir::FileId| session.file_path(id).map(str::to_owned);

    let nodes: Vec<GraphNode> = graph
        .nodes
        .iter()
        .map(|n| GraphNode {
            id: n.id.clone(),
            name: n.name.clone(),
            kind: node_kind(n.kind).to_owned(),
            file: n.file.and_then(path_of),
            range: n.range.map(|r| (u32::from(r.start()), u32::from(r.end()))),
            parent: n.parent.clone(),
        })
        .collect();
    let edges: Vec<GraphEdge> = graph
        .edges
        .iter()
        .map(|e| GraphEdge {
            from: e.from.clone(),
            to: e.to.clone(),
            kind: edge_kind(e.kind).to_owned(),
            site: e.occurrences.first().and_then(|o| {
                path_of(o.file)
                    .map(|path| (path, u32::from(o.range.start()), u32::from(o.range.end())))
            }),
        })
        .collect();
    // The entry knot: the FIRST-DECLARED knot in the entry file, which is
    // where a reader starts. By declaration order, not by name — the node
    // list is sorted by id, so "first in the list" would pick whichever
    // knot sorts first alphabetically. A project with no entry has no
    // root, and the layout says so by starting from every unreached node.
    let entry_node = entry.and_then(|entry| {
        nodes
            .iter()
            .filter(|n| n.file.as_deref() == Some(entry) && n.kind == "knot")
            .min_by_key(|n| n.range.map_or(u32::MAX, |(start, _)| start))
            .map(|n| n.id.clone())
    });
    StoryGraphReport {
        nodes,
        edges,
        entry: entry_node,
    }
}
