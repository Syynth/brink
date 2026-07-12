//! Call-graph construction over inferable (knot/stitch) definitions, and the
//! strongly-connected-component decomposition that drives the "SCC fixpoint
//! for recursion" rule (typed-mode-spec §2).
//!
//! An edge `caller -> callee` means caller's body contains a resolved call
//! or divert-with-arguments target pointing at callee. [`topo_order`]
//! batches the graph's SCCs so [`super::infer_project`] can solve each batch
//! using only the already-finalized signatures of earlier batches, plus
//! (within a batch) the in-progress fixpoint estimates of its own members —
//! the signature firewall (`infer_body(A)` reads only `signature(B)`) holds
//! at the batch boundary; *within* a mutually-recursive batch, "signature"
//! means "this SCC's current fixpoint estimate", which is exactly what a
//! Haskell-style monomorphic binding-group solves.
//!
//! ## Why reachability sets, not Tarjan/Kosaraju
//!
//! A linear-time SCC algorithm earns its keep on graphs with thousands of
//! nodes. Ink call graphs (knots + stitches in a single project) are
//! reliably small — the corpus tops out in the hundreds — and this query is
//! not on any hot path yet (nothing calls it: see the module doc on
//! `infer_project`'s laziness). Correctness under review is worth far more
//! here than an asymptotic win nothing exercises: computing forward- and
//! backward-reachability per node via `BTreeSet` (`O(V * (V + E))`) is
//! straightforward to read, has no recursion-depth risk on deep call chains,
//! and is dead simple to verify against a handful of shape tests (linear
//! chain, mutual pair, self-loop, disconnected diamond).

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;

/// A directed call graph over inferable definitions.
#[derive(Debug, Clone, Default)]
pub struct CallGraph {
    pub nodes: BTreeSet<DefinitionId>,
    /// `caller -> { callees }`.
    pub edges: BTreeMap<DefinitionId, BTreeSet<DefinitionId>>,
}

impl CallGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, def: DefinitionId) {
        self.nodes.insert(def);
        self.edges.entry(def).or_default();
    }

    /// Record a resolved call/divert-target edge. Both endpoints are added
    /// as nodes if not already present (a call to a def with no
    /// inferable body of its own — e.g. an external — is simply never added
    /// as a node by the caller, so no edge is recorded for it; see
    /// `infer_project`'s node-selection pass).
    pub fn add_edge(&mut self, from: DefinitionId, to: DefinitionId) {
        self.add_node(from);
        self.add_node(to);
        self.edges.entry(from).or_default().insert(to);
    }

    /// Nodes reachable from `start` via forward edges, `start` included.
    fn reachable_forward(&self, start: DefinitionId) -> BTreeSet<DefinitionId> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            if seen.insert(n)
                && let Some(callees) = self.edges.get(&n)
            {
                stack.extend(callees.iter().copied());
            }
        }
        seen
    }

    /// Nodes that reach `target` via forward edges, `target` included
    /// (forward reachability on the transposed graph).
    fn reachable_backward(&self, target: DefinitionId) -> BTreeSet<DefinitionId> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![target];
        while let Some(n) = stack.pop() {
            if seen.insert(n) {
                for (caller, callees) in &self.edges {
                    if callees.contains(&n) {
                        stack.push(*caller);
                    }
                }
            }
        }
        seen
    }
}

/// Partition the graph into strongly-connected components: `u` and `v` share
/// a component iff each is reachable from the other (`u == v` always forms
/// its own singleton component, self-loop or not — direct recursion is a
/// component of size one with a self-edge, not folded into anything).
///
/// Deterministic: nodes are visited in `BTreeSet` order, every intermediate
/// collection is a `BTreeSet`, and the returned `Vec` is sorted by each
/// component's minimum member — the same partition, same order, regardless
/// of insertion history.
#[must_use]
pub fn strongly_connected_components(graph: &CallGraph) -> Vec<BTreeSet<DefinitionId>> {
    let mut assigned: BTreeSet<DefinitionId> = BTreeSet::new();
    let mut components: Vec<BTreeSet<DefinitionId>> = Vec::new();

    for &node in &graph.nodes {
        if assigned.contains(&node) {
            continue;
        }
        let fwd = graph.reachable_forward(node);
        let bwd = graph.reachable_backward(node);
        let component: BTreeSet<DefinitionId> = fwd.intersection(&bwd).copied().collect();
        assigned.extend(component.iter().copied());
        components.push(component);
    }

    components.sort_by_key(|c| c.iter().next().copied());
    components
}

/// Order SCCs so every component appears after every *other* component it
/// calls into (a component's own self-edges/internal edges never block it).
/// Ties (independent components, or components with no cross-component
/// calls) break on the component's minimum member for determinism.
///
/// This is the processing order [`super::infer_project`] uses: by the time a
/// component is solved, every component it depends on already has a
/// finalized signature — the cross-SCC half of the firewall. Within one
/// returned component, callers may be mutually recursive with callees
/// (that's what makes it one SCC); the caller solves those together via
/// fixpoint, not via this ordering.
#[must_use]
pub fn topo_order(graph: &CallGraph) -> Vec<BTreeSet<DefinitionId>> {
    let components = strongly_connected_components(graph);
    if components.is_empty() {
        return components;
    }

    // Map each node to the index of its component in `components`.
    let mut owner: BTreeMap<DefinitionId, usize> = BTreeMap::new();
    for (idx, comp) in components.iter().enumerate() {
        for &n in comp {
            owner.insert(n, idx);
        }
    }

    // Condensation adjacency: component -> { other components it calls }.
    let mut depends_on: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut dependents: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for idx in 0..components.len() {
        depends_on.insert(idx, BTreeSet::new());
        dependents.insert(idx, BTreeSet::new());
    }
    for (caller, callees) in &graph.edges {
        let Some(&from) = owner.get(caller) else {
            continue;
        };
        for callee in callees {
            let Some(&to) = owner.get(callee) else {
                continue;
            };
            if from != to {
                depends_on.entry(from).or_default().insert(to);
                dependents.entry(to).or_default().insert(from);
            }
        }
    }

    // Kahn's algorithm: a component is ready once every component it calls
    // has already been placed in `order`.
    let mut remaining = depends_on.clone();
    let mut order: Vec<usize> = Vec::new();
    let component_key = |idx: usize| components[idx].iter().next().copied();

    loop {
        let mut ready: Vec<usize> = remaining
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(&idx, _)| idx)
            .collect();
        if ready.is_empty() {
            break;
        }
        ready.sort_by_key(|&idx| component_key(idx));
        for idx in ready {
            if remaining.remove(&idx).is_none() {
                // Already processed via an earlier `idx` in this same batch
                // sharing dependents — cannot happen since `remaining` is
                // the source of truth we just filtered from, but stay
                // defensive rather than double-count.
                continue;
            }
            order.push(idx);
            if let Some(deps) = dependents.get(&idx) {
                for &dep in deps {
                    if let Some(set) = remaining.get_mut(&dep) {
                        set.remove(&idx);
                    }
                }
            }
        }
    }

    // `remaining` is only non-empty here if the condensation had a cycle,
    // which cannot happen (SCCs are maximal by construction — any cycle
    // would have been folded into one component). Guard against silently
    // dropping components anyway (house rule: never silently drop data) by
    // appending whatever is left, deterministically ordered, rather than
    // assuming the impossible-in-theory case away.
    let mut leftover: Vec<usize> = remaining.keys().copied().collect();
    leftover.sort_by_key(|&idx| component_key(idx));
    order.extend(leftover);

    order
        .into_iter()
        .map(|idx| components[idx].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::DefinitionTag;

    fn def(n: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, n)
    }

    #[test]
    fn empty_graph_has_no_components() {
        let g = CallGraph::new();
        assert!(topo_order(&g).is_empty());
    }

    #[test]
    fn isolated_node_is_its_own_component() {
        let mut g = CallGraph::new();
        g.add_node(def(1));
        let sccs = strongly_connected_components(&g);
        assert_eq!(sccs, vec![BTreeSet::from([def(1)])]);
    }

    #[test]
    fn direct_recursion_is_a_singleton_component() {
        let mut g = CallGraph::new();
        g.add_edge(def(1), def(1));
        let sccs = strongly_connected_components(&g);
        assert_eq!(sccs, vec![BTreeSet::from([def(1)])]);
    }

    #[test]
    fn linear_chain_orders_callee_before_caller() {
        // a -> b -> c (a calls b, b calls c).
        let mut g = CallGraph::new();
        g.add_edge(def(1), def(2));
        g.add_edge(def(2), def(3));
        let order = topo_order(&g);
        let flat: Vec<DefinitionId> = order.into_iter().flatten().collect();
        let pos = |d: DefinitionId| flat.iter().position(|&x| x == d).expect("present");
        assert!(pos(def(3)) < pos(def(2)), "callee c before caller b");
        assert!(pos(def(2)) < pos(def(1)), "callee b before caller a");
    }

    #[test]
    fn mutual_recursion_is_one_component() {
        // a <-> b mutually recursive; c calls a (external caller).
        let mut g = CallGraph::new();
        g.add_edge(def(1), def(2));
        g.add_edge(def(2), def(1));
        g.add_edge(def(3), def(1));
        let sccs = strongly_connected_components(&g);
        let ab = sccs
            .iter()
            .find(|c| c.contains(&def(1)))
            .expect("component containing a");
        assert_eq!(
            ab,
            &BTreeSet::from([def(1), def(2)]),
            "a and b fold into one SCC"
        );

        let order = topo_order(&g);
        let ab_idx = order
            .iter()
            .position(|c| c.contains(&def(1)))
            .expect("ab component present");
        let c_idx = order
            .iter()
            .position(|c| c.contains(&def(3)))
            .expect("c component present");
        assert!(
            ab_idx < c_idx,
            "the mutually-recursive pair solves before its caller"
        );
    }

    #[test]
    fn disconnected_components_both_appear() {
        let mut g = CallGraph::new();
        g.add_edge(def(1), def(2));
        g.add_edge(def(10), def(20));
        let order = topo_order(&g);
        let flat: BTreeSet<DefinitionId> = order.into_iter().flatten().collect();
        assert_eq!(flat, BTreeSet::from([def(1), def(2), def(10), def(20)]));
    }
}
