use brink_ir::FileId;

use crate::determinism::{LookupMap, LookupSet};

/// Tracks `INCLUDE` relationships between files.
///
/// Built by the `include_graph` tracked query in [`crate::queries`];
/// `PartialEq` (order-independent on the underlying maps) enables salsa
/// early-cutoff for dependents when edges are unchanged.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IncludeGraph {
    /// file → files it includes
    forward: LookupMap<FileId, Vec<FileId>>,
    /// file → files that include it
    reverse: LookupMap<FileId, Vec<FileId>>,
}

impl IncludeGraph {
    /// Files that `file` includes.
    pub fn includes(&self, file: FileId) -> &[FileId] {
        self.forward.get(&file).map_or(&[], Vec::as_slice)
    }

    /// All files reachable from `entry` via forward `INCLUDE` edges
    /// (transitive), `entry` included. Returned as a [`BTreeSet`] so iteration
    /// is deterministic regardless of internal graph storage.
    pub fn reachable_from(&self, entry: FileId) -> std::collections::BTreeSet<FileId> {
        let mut reachable = std::collections::BTreeSet::new();
        let mut stack = vec![entry];
        while let Some(node) = stack.pop() {
            if !reachable.insert(node) {
                continue;
            }
            for &child in self.includes(node) {
                if !reachable.contains(&child) {
                    stack.push(child);
                }
            }
        }
        reachable
    }
}

#[expect(dead_code, reason = "graph queries used by LSP")]
impl IncludeGraph {
    pub fn new() -> Self {
        Self {
            forward: LookupMap::new(),
            reverse: LookupMap::new(),
        }
    }

    /// Replace the include set for `file`. Removes old edges and inserts new ones.
    pub fn update(&mut self, file: FileId, includes: Vec<FileId>) {
        // Remove old reverse edges
        if let Some(old_includes) = self.forward.remove(&file) {
            for target in &old_includes {
                if let Some(rev) = self.reverse.get_mut(target) {
                    rev.retain(|&f| f != file);
                }
            }
        }

        // Insert new reverse edges
        for &target in &includes {
            self.reverse.entry(target).or_default().push(file);
        }

        self.forward.insert(file, includes);
    }

    /// Files that include `file`.
    pub fn included_by(&self, file: FileId) -> &[FileId] {
        self.reverse.get(&file).map_or(&[], Vec::as_slice)
    }

    /// Detect cycles in the include graph. Returns the first cycle found
    /// as an ordered path of file IDs (the last includes the first).
    pub fn find_cycle(&self) -> Option<Vec<FileId>> {
        let mut visited = LookupSet::new();
        let mut on_stack = LookupSet::new();

        // `forward` is a `HashMap`, so its key order is nondeterministic
        // across processes — the DFS start node picks which cycle (if the
        // graph has more than one) and which rotation of it comes back
        // first, and that path is rendered straight into
        // `DiscoverError::CircularInclude`'s user-facing message (issue
        // #801). Sort the start candidates so the result is stable.
        let mut starts: Vec<FileId> = self.forward.keys().copied().collect();
        starts.sort_by_key(|id| id.0);

        for start in starts {
            if visited.contains(&start) {
                continue;
            }
            // DFS with explicit stack: (node, iter_index)
            let mut stack: Vec<(FileId, usize)> = vec![(start, 0)];
            let mut path: Vec<FileId> = vec![start];
            on_stack.insert(start);

            while let Some((node, idx)) = stack.last_mut() {
                let children = self.includes(*node);
                if *idx < children.len() {
                    let child = children[*idx];
                    *idx += 1;
                    if on_stack.contains(&child) {
                        // Found a cycle — extract from child back to child
                        let cycle_start = path.iter().position(|&f| f == child);
                        if let Some(pos) = cycle_start {
                            let mut cycle: Vec<_> = path[pos..].to_vec();
                            cycle.push(child);
                            return Some(cycle);
                        }
                    } else if !visited.contains(&child) {
                        on_stack.insert(child);
                        path.push(child);
                        stack.push((child, 0));
                    }
                } else {
                    let finished = *node;
                    on_stack.remove(&finished);
                    visited.insert(finished);
                    path.pop();
                    stack.pop();
                }
            }
        }
        None
    }

    /// Return all file IDs reachable from `entry` in topological order
    /// (included files before the files that include them).
    ///
    /// Uses a post-order DFS: children (includes) are visited before their
    /// parent, giving the correct "paste-before" order for ink `INCLUDE`.
    pub fn topological_order(&self, entry: FileId, all_ids: &[FileId]) -> Vec<FileId> {
        fn dfs(
            node: FileId,
            graph: &IncludeGraph,
            visited: &mut LookupSet<FileId>,
            order: &mut Vec<FileId>,
        ) {
            if !visited.insert(node) {
                return;
            }
            for &child in graph.includes(node) {
                dfs(child, graph, visited, order);
            }
            order.push(node);
        }

        let mut visited = LookupSet::new();
        let mut order = Vec::new();

        dfs(entry, self, &mut visited, &mut order);

        // Include any remaining files not reachable from entry
        // (shouldn't happen in practice, but be safe).
        let mut all_sorted: Vec<_> = all_ids.to_vec();
        all_sorted.sort_by_key(|id| id.0);
        for &id in &all_sorted {
            if visited.insert(id) {
                order.push(id);
            }
        }

        order
    }

    /// Discover independent projects from include relationships.
    ///
    /// A "project" is a root `.ink` file plus everything it transitively INCLUDEs.
    /// Roots are files in `all_ids` that are not included by any other file.
    /// Returns `(root, members)` pairs sorted by root `FileId`.
    pub fn compute_projects(&self, all_ids: &[FileId]) -> Vec<(FileId, Vec<FileId>)> {
        let all_set: LookupSet<FileId> = all_ids.iter().copied().collect();

        // Roots: files not included by any other file in the set
        let mut roots: Vec<FileId> = all_ids
            .iter()
            .copied()
            .filter(|&id| {
                self.included_by(id)
                    .iter()
                    .all(|parent| !all_set.contains(parent))
            })
            .collect();
        roots.sort_by_key(|id| id.0);

        // For each root, DFS forward to collect members
        let mut claimed: LookupSet<FileId> = LookupSet::new();
        let mut projects: Vec<(FileId, Vec<FileId>)> = Vec::new();

        for &root in &roots {
            let mut members = Vec::new();
            let mut stack = vec![root];
            let mut visited = LookupSet::new();

            while let Some(node) = stack.pop() {
                if !visited.insert(node) || !all_set.contains(&node) {
                    continue;
                }
                members.push(node);
                claimed.insert(node);
                for &child in self.includes(node) {
                    stack.push(child);
                }
            }
            members.sort_by_key(|id| id.0);
            projects.push((root, members));
        }

        // Any files not claimed by a root become single-file projects
        let mut orphans: Vec<FileId> = all_ids
            .iter()
            .copied()
            .filter(|id| !claimed.contains(id))
            .collect();
        orphans.sort_by_key(|id| id.0);
        for orphan in orphans {
            projects.push((orphan, vec![orphan]));
        }

        projects.sort_by_key(|(root, _)| root.0);
        projects
    }

    /// Return root file IDs (files not included by any other file in `all_ids`).
    pub fn roots(&self, all_ids: &[FileId]) -> Vec<FileId> {
        let all_set: LookupSet<FileId> = all_ids.iter().copied().collect();
        let mut roots: Vec<FileId> = all_ids
            .iter()
            .copied()
            .filter(|&id| {
                self.included_by(id)
                    .iter()
                    .all(|parent| !all_set.contains(parent))
            })
            .collect();
        roots.sort_by_key(|id| id.0);
        roots
    }

    /// Remove a file from the graph entirely.
    pub fn remove(&mut self, file: FileId) {
        // Remove forward edges and their reverse entries
        if let Some(includes) = self.forward.remove(&file) {
            for target in &includes {
                if let Some(rev) = self.reverse.get_mut(target) {
                    rev.retain(|&f| f != file);
                }
            }
        }

        // Remove reverse edges pointing to this file
        if let Some(included_by) = self.reverse.remove(&file) {
            for source in &included_by {
                if let Some(fwd) = self.forward.get_mut(source) {
                    fwd.retain(|&f| f != file);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #801: `find_cycle`'s DFS start node used to come straight from
    /// `self.forward.keys()` — a `HashMap` — so which rotation of a cycle it
    /// returned depended on that map's per-instance `RandomState` seed.
    /// Fresh-instance repetition (the #795 regression test's shape,
    /// generalized): build a fresh `IncludeGraph` with a 3-file cycle
    /// (`a -> b -> c -> a`) on every iteration — a fresh `HashMap` picks a
    /// fresh seed each time — and assert the reported cycle never varies.
    #[test]
    fn find_cycle_start_node_is_stable_across_fresh_graphs() {
        let a = FileId(0);
        let b = FileId(1);
        let c = FileId(2);

        let mut first: Option<Vec<FileId>> = None;
        for _ in 0..64 {
            let mut graph = IncludeGraph::new();
            graph.update(a, vec![b]);
            graph.update(b, vec![c]);
            graph.update(c, vec![a]);

            let cycle = graph.find_cycle();
            match &first {
                None => first = Some(cycle.clone().expect("a -> b -> c -> a is a cycle")),
                Some(expected) => {
                    assert_eq!(
                        cycle.as_ref(),
                        Some(expected),
                        "find_cycle's reported rotation diverged across fresh IncludeGraph \
                         instances — a HashMap iteration order is leaking into the result"
                    );
                }
            }
        }
    }
}
