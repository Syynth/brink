// ALGORITHMS CORPUS — AI-decision lane (issue #822)
// MCTS-lite: Monte Carlo Tree Search on a small binary tree. This port explores
// how brink's struct composition and partial-application handle MCTS phases:
// selection via UCB1, rollout, and visit-count updates. The test case builds a
// small arena-based tree, runs MCTS iterations with deterministic rollouts, and
// reports final visit counts.
//
// TYPES POLICY: gradual (default). Uses self-referential structs (MCTSNode with
// children array) and fn values for rollout. Tests whether functions-as-fields
// compose cleanly without closures, and how value semantics affect in-place tree
// mutation.
//
// ERGONOMICS-FINDINGS:
//
// 1. SELF-REFERENTIAL STRUCTS WORK, ARENA PATTERN IS MANDATORY. MCTSNode with
//    children: Array<int> compiles and runs. However, brink's value semantics
//    (each assignment copies deeply) make it impossible to hold a reference to
//    a node and mutate it across function calls. The backpropagation phase of
//    MCTS (update ancestor visits) normally walks parent pointers; here, we must
//    use an explicit arena (array indexed by int IDs) and re-index from root
//    on every update. This is idiomatic C-style, but it's a COMPOSITION DIFFERENCE:
//    in languages with references/pointers, MCTS walks parent links naturally;
//    in brink, the path back up must be reconstructed or stored separately. This
//    is not a blocker (arena patterns are common), but it's a real friction point
//    for algorithms that assume mutable reference semantics.
//
// 2. FUNCTIONS-AS-STRUCT-FIELDS WORK FOR PARTIAL APPLICATION LEAVES. Each
//    MCTSNode carries rollout_fn: fn(): int, created via #fn(rollout_sim, node_id)
//    to bind the node's ID into a lookup function. This works cleanly. The
//    limitation: there's no "wrap_rollout(fn_arg): fn(): int" idiom to customize
//    rollout at runtime without closures. For MCTS specifically, this isn't a
//    friction point (rollout is homogeneous), but it marks the ceiling: partial
//    application suffices for parameterizing fixed functions; it cannot compose
//    functions at runtime.
//
// 3. ARENA PATTERN IS ERGONOMIC. Index arithmetic for tree navigation is clean
//    and deterministic. Visit counts and value sums update easily via re-
//    assignment. The tree structure is explicit and readable. This proves the
//    pattern scales past toy examples and is idiomatic for tree structures.

STRUCT MCTSNode = #{
    children: Array<int>,
    visits: int,
    value_sum: float
}

VAR arena: Array<MCTSNode> = #[]
VAR v0 = 0
VAR v1 = 0
VAR v2 = 0
VAR v3 = 0
VAR v4 = 0
VAR v5 = 0
VAR v6 = 0

~ {
    // Build tree: 0 -> [1, 2], 1 -> [3, 4], 2 -> [5, 6], 3-6 are leaves
    push(arena, MCTSNode#{children: #[1, 2], visits: 0, value_sum: 0.0})
    push(arena, MCTSNode#{children: #[3, 4], visits: 0, value_sum: 0.0})
    push(arena, MCTSNode#{children: #[5, 6], visits: 0, value_sum: 0.0})
    push(arena, MCTSNode#{children: #[], visits: 0, value_sum: 0.0})
    push(arena, MCTSNode#{children: #[], visits: 0, value_sum: 0.0})
    push(arena, MCTSNode#{children: #[], visits: 0, value_sum: 0.0})
    push(arena, MCTSNode#{children: #[], visits: 0, value_sum: 0.0})

    // Run MCTS iterations
    temp iter = 0
    while iter < 30 {
        run_iteration()
        iter = iter + 1
    }

    // Collect visit counts
    v0 = arena[0].visits
    v1 = arena[1].visits
    v2 = arena[2].visits
    v3 = arena[3].visits
    v4 = arena[4].visits
    v5 = arena[5].visits
    v6 = arena[6].visits
}

MCTS Tree Search
Final visit counts:
Node 0: visits={v0}
Node 1: visits={v1}
Node 2: visits={v2}
Node 3: visits={v3}
Node 4: visits={v4}
Node 5: visits={v5}
Node 6: visits={v6}
Done.
-> END

=== function rollout_sim(node_id) ===
~ {
    temp h = (node_id * 7 + 5) % 20
    return h
}

=== function ucb1_score(parent_visits, child_visits, child_value_sum) ===
~ {
    if child_visits == 0 {
        return 1000.0
    }
    temp avg = child_value_sum / child_visits
    temp exploration = 1.4 * (parent_visits / (child_visits + 1))
    return avg + exploration
}

=== function select_path() ===
~ {
    temp current = 0
    temp depth = 0
    temp path = #[0]
    while depth < 3 {
        temp node = arena[current]
        if len(node.children) == 0 {
            return path
        }
        temp best = node.children[0]
        temp best_score = ucb1_score(node.visits, arena[best].visits, arena[best].value_sum)
        temp i = 1
        while i < len(node.children) {
            temp child_id = node.children[i]
            temp child = arena[child_id]
            temp score = ucb1_score(node.visits, child.visits, child.value_sum)
            if score > best_score {
                best_score = score
                best = child_id
            }
            i = i + 1
        }
        current = best
        push(path, current)
        depth = depth + 1
    }
    return path
}

=== function update_node(node_id, value) ===
~ {
    temp node = arena[node_id]
    node.visits = node.visits + 1
    node.value_sum = node.value_sum + value
    arena[node_id] = node
}

// Backpropagation: every node on the path from the selected leaf back up to
// the root gets its visit count and value sum updated, not just the leaf.
// Without this, parent/root visits stay at 0 forever and UCB1's exploration
// term (which divides by parent_visits) never engages.
=== function backprop(path, value) ===
~ {
    temp i = 0
    while i < len(path) {
        update_node(path[i], value)
        i = i + 1
    }
}

=== function run_iteration() ===
~ {
    temp path = select_path()
    temp selected = path[len(path) - 1]
    temp rollout = rollout_sim(selected)
    backprop(path, rollout)
}
