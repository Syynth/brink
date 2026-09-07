//! Laying the story graph out: layers by reachability, rows within a
//! layer, and nothing that depends on a screen.
//!
//! **Layered, not force-directed.** A story reads forwards, and the
//! question an author asks a graph is "what leads where" — a layout that
//! puts distance-from-the-entry on one axis answers it directly, and one
//! that settles into a physical equilibrium does not. Layer = shortest
//! divert distance from the entry; nodes at the same distance share a
//! column, ordered by id so the same project always draws the same way.
//!
//! Nodes the entry never reaches still have to appear — an unreachable
//! knot is exactly the thing an author wants a graph to show — so after
//! the reachable ones are placed, whatever is left is walked from the
//! lowest-id root and laid out below.
//!
//! Pure geometry, so it is testable without a window: the panel supplies
//! the sizes and turns the result into pixels.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use brink_gpui_model::graph::StoryGraphReport;

/// One placed node, in layout units (a unit is a node's own box).
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub id: String,
    /// Distance from the entry, in whole layers.
    pub layer: usize,
    /// Position within the layer.
    pub row: usize,
}

/// Where every node sits, plus how wide and tall the whole graph is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub placed: Vec<Placed>,
    pub layers: usize,
    pub rows: usize,
}

impl Layout {
    /// The node at an id, if it was placed.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Placed> {
        self.placed.iter().find(|p| p.id == id)
    }
}

/// Breadth-first from whatever is in `queue`: a node's layer is its
/// shortest divert distance, so the column says how far into the story it
/// is. A node already placed keeps its first (shortest) distance, which is
/// also what terminates a cycle.
fn walk<'a>(
    out: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    queue: &mut VecDeque<&'a str>,
    layer_of: &mut BTreeMap<&'a str, usize>,
) {
    while let Some(id) = queue.pop_front() {
        let depth = layer_of.get(id).copied().unwrap_or(0);
        let Some(targets) = out.get(id) else { continue };
        for target in targets.iter().copied() {
            if !layer_of.contains_key(target) {
                layer_of.insert(target, depth + 1);
                queue.push_back(target);
            }
        }
    }
}

/// Lay the graph out. Deterministic: same report, same layout.
#[must_use]
pub fn layout(report: &StoryGraphReport) -> Layout {
    // Adjacency, sorted — a `BTreeMap` of `BTreeSet`s, so the walk order
    // never depends on a hash.
    let mut out: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for node in &report.nodes {
        out.entry(node.id.as_str()).or_default();
    }
    for edge in &report.edges {
        out.entry(edge.from.as_str())
            .or_default()
            .insert(edge.to.as_str());
        out.entry(edge.to.as_str()).or_default();
    }

    let mut layer_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    if let Some(entry) = report.entry.as_deref()
        && out.contains_key(entry)
    {
        layer_of.insert(entry, 0);
        queue.push_back(entry);
    }
    walk(&out, &mut queue, &mut layer_of);

    // Whatever the entry never reached: each island is walked from its
    // OWN root — an unplaced node nothing unplaced points at — so the
    // island reads forwards like the reachable part does, rather than
    // starting from whichever of its nodes sorts first. An unreachable
    // knot is exactly what an author opens a graph to find.
    loop {
        let unplaced: Vec<&str> = out
            .keys()
            .copied()
            .filter(|id| !layer_of.contains_key(id))
            .collect();
        if unplaced.is_empty() {
            break;
        }
        let has_unplaced_source = |target: &str| {
            out.iter().any(|(from, targets)| {
                !layer_of.contains_key(from) && *from != target && targets.contains(target)
            })
        };
        // A source if there is one; otherwise the lowest id, which is what
        // a cycle of unplaced nodes leaves.
        let root = unplaced
            .iter()
            .copied()
            .find(|id| !has_unplaced_source(id))
            .unwrap_or(unplaced[0]);
        layer_of.insert(root, 0);
        queue.push_back(root);
        walk(&out, &mut queue, &mut layer_of);
    }

    // Rows within a layer, by id — stable, so the picture does not move
    // between runs.
    let mut by_layer: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for (id, layer) in &layer_of {
        by_layer.entry(*layer).or_default().push(id);
    }
    let mut placed = Vec::with_capacity(layer_of.len());
    let mut rows = 0;
    for (layer, ids) in &by_layer {
        rows = rows.max(ids.len());
        for (row, id) in ids.iter().enumerate() {
            placed.push(Placed {
                id: (*id).to_owned(),
                layer: *layer,
                row,
            });
        }
    }
    placed.sort_by(|a, b| a.layer.cmp(&b.layer).then_with(|| a.row.cmp(&b.row)));
    Layout {
        layers: by_layer.len(),
        rows,
        placed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_gpui_model::graph::{GraphEdge, GraphNode};

    fn node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_owned(),
            name: id.to_owned(),
            kind: "knot".to_owned(),
            file: None,
            range: None,
            parent: None,
        }
    }

    fn edge(from: &str, to: &str) -> GraphEdge {
        GraphEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind: "divert".to_owned(),
            site: None,
        }
    }

    fn report(nodes: &[&str], edges: &[(&str, &str)], entry: Option<&str>) -> StoryGraphReport {
        StoryGraphReport {
            nodes: nodes.iter().map(|id| node(id)).collect(),
            edges: edges.iter().map(|(a, b)| edge(a, b)).collect(),
            entry: entry.map(str::to_owned),
        }
    }

    #[test]
    fn a_layer_is_the_shortest_distance_from_the_entry() {
        let r = report(
            &["start", "a", "b", "end"],
            &[("start", "a"), ("a", "end"), ("start", "b"), ("b", "end")],
            Some("start"),
        );
        let l = layout(&r);
        assert_eq!(l.find("start").map(|p| p.layer), Some(0));
        assert_eq!(l.find("a").map(|p| p.layer), Some(1));
        assert_eq!(l.find("b").map(|p| p.layer), Some(1));
        // `end` is two diverts away by either road — not three.
        assert_eq!(l.find("end").map(|p| p.layer), Some(2));
        assert_eq!(l.layers, 3);
        assert_eq!(l.rows, 2, "the widest layer decides the height");
    }

    #[test]
    fn an_unreachable_knot_is_still_placed() {
        // The thing an author opens a graph to find.
        let r = report(
            &["start", "orphan", "child"],
            &[("orphan", "child")],
            Some("start"),
        );
        let l = layout(&r);
        assert!(l.find("orphan").is_some(), "an island is still drawn");
        assert_eq!(l.find("orphan").map(|p| p.layer), Some(0), "its own root");
        assert_eq!(l.find("child").map(|p| p.layer), Some(1));
    }

    #[test]
    fn a_cycle_terminates_and_keeps_its_first_distance() {
        let r = report(
            &["a", "b", "c"],
            &[("a", "b"), ("b", "c"), ("c", "a"), ("c", "b")],
            Some("a"),
        );
        let l = layout(&r);
        assert_eq!(l.find("a").map(|p| p.layer), Some(0));
        assert_eq!(l.find("b").map(|p| p.layer), Some(1), "not revisited");
        assert_eq!(l.find("c").map(|p| p.layer), Some(2));
        assert_eq!(l.placed.len(), 3, "each node placed exactly once");
    }

    #[test]
    fn the_same_graph_lays_out_the_same_way_twice() {
        let r = report(
            &["start", "z", "y", "x"],
            &[("start", "z"), ("start", "y"), ("start", "x")],
            Some("start"),
        );
        assert_eq!(layout(&r), layout(&r));
        let l = layout(&r);
        // Rows within a layer are by id, so the picture is stable rather
        // than following whatever order the edges arrived in.
        let mut row_order: Vec<(&str, usize)> = l
            .placed
            .iter()
            .filter(|p| p.layer == 1)
            .map(|p| (p.id.as_str(), p.row))
            .collect();
        row_order.sort_by_key(|(_, row)| *row);
        assert_eq!(row_order, vec![("x", 0), ("y", 1), ("z", 2)]);
    }

    #[test]
    fn a_graph_with_no_entry_still_places_everything() {
        let r = report(&["b", "a"], &[("a", "b")], None);
        let l = layout(&r);
        assert_eq!(l.placed.len(), 2);
        // Lowest id first as the island root, so the fallback is stable.
        assert_eq!(l.find("a").map(|p| p.layer), Some(0));
        assert_eq!(l.find("b").map(|p| p.layer), Some(1));
    }

    #[test]
    fn nothing_to_lay_out_is_an_empty_layout() {
        let l = layout(&StoryGraphReport::default());
        assert!(l.placed.is_empty());
        assert_eq!(l.layers, 0);
        assert_eq!(l.rows, 0);
    }
}
