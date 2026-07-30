// ALGORITHMS CORPUS — AI-decision lane (issue #822)
// GOAP (Goal-Oriented Action Planning): an NPC picks a goal ("enemy is
// dead") and A*-searches an action graph — five actions, each with a
// precondition set and an effect set over a small fact space — for the
// cheapest sequence that reaches it (Orkin, "Applying Goal-Oriented Action
// Planning to Games", AIWisdom2 2003 — F.E.A.R.'s AI). The plan is then
// EXECUTED against a live world (not just re-derived): the same
// precondition/effect data that drove the search also drives the
// step-by-step narration below, so "found" and "executed" are provably the
// same data, not two disconnected halves of the file.
//
// TYPES POLICY: gradual (default). Nothing here needed strict's static
// checking — the interesting questions this file answers are about VALUE
// REPRESENTATION and EQUALITY semantics (see findings below), which are
// runtime-behavioral, not type-shape questions; astar-grid/dijkstra-grid
// already own this corpus's dedicated strict-vs-gradual A/B, so this file
// doesn't re-litigate it.
//
// ERGONOMICS-FINDINGS — this is the file issue #822 commissioned
// specifically to answer "kind-tagged structs? maps of `#fn`? where does
// composition hurt vs work?", with an explicit compare-and-contrast against
// behavior-tree/story.ink's findings. Every claim below was verified by
// compiling and running a minimal repro with `brink compile --dialect
// brink` / `brink play` before being written down, not inferred from
// reading the book alone — several of them turned out to contradict the
// book text.
//
// 1. THE HEADLINE ANSWER: NEITHER. Actions are represented as pure DATA —
//    `STRUCT Action = #{name, cost, precond: Map<string,bool>, effect:
//    Map<string,bool>}` — with ZERO `#fn` fields anywhere in this file.
//    This is a third, previously-unseen outcome for this corpus's running
//    "closures vs composition" investigation:
//    - behavior-tree found partial application WORKS for parameterizing
//      LEAVES (one shared function body, three bound-differently leaf
//      conditions) but FAILS to compose composite NODES (a sequence/
//      selector/invert-decorator needs kind-tagged data + hand-written
//      `tick()` dispatch, because each node kind has a genuinely different
//      recursion shape).
//    - utility-ai found partial application unnecessary but function
//      VALUES essential: four differently-bodied scoring functions are
//      called uniformly through one `call(c.score_fn)` line, because each
//      consideration's score is arbitrary code (a formula over globals).
//    - GOAP needs neither. A precondition is "does fact F have value V in
//      this state" and an effect is "fact F now has value V" — that is
//      ALWAYS exactly a partial map from fact name to bool, for every
//      action, with no exceptions and no per-action logic. One generic
//      `meets(state, predicate)` and one generic `apply_effect(state,
//      effect)` (both below) interpret EVERY action in `actions` with zero
//      per-instance code — not "call a different function per action"
//      (utility-ai's answer) but "run the SAME interpreter over
//      declarative data with no function value in sight." The reason this
//      works where behavior-tree's composite nodes couldn't: a BT node's
//      MEANING differs by kind (a sequence loops-and-short-circuits, a
//      selector loops-the-other-way, an invert flips one child's result —
//      three different control shapes), whereas a GOAP action's meaning
//      is the SAME shape every time (compare N required facts, then write
//      M facts) — uniform enough to be pure data, not even needing the
//      "uniform interface, different bodies" trick utility-ai relied on.
//    - The honest caveat: this only holds because STRIPS-style
//      preconditions/effects happen to be flat fact-diffs. The moment an
//      action's effect needed arbitrary logic (e.g., "reload gives back an
//      amount of ammo proportional to a perk stat"), the plain-data model
//      breaks and this file's approach would need a `#fn` per affected
//      action, right back to utility-ai's answer. This file's domain
//      happens to sit at the "pure data suffices" end of that spectrum;
//      it is not evidence that GOAP-in-general never needs function
//      values, only that this classic STRIPS formulation doesn't.
//
// 2. NO NATIVE SET TYPE — `Map<string, bool>` STANDS IN, MOSTLY. The
//    catalog predicted the friction would be "designing action as data
//    ... without a real hash-set type." That prediction landed exactly:
//    there is no `Set<Fact>`, so a precondition/effect/state is a
//    `Map<string, bool>` used as a set of (fact, required-or-new-value)
//    pairs, exactly like npc-fsm's `Map<string, fn(...)>` and
//    weighted-loot-table's numeric maps use `map` as the corpus's
//    general-purpose "small keyed collection" answer. `contains(m, k)` and
//    `keys(m)` (stdlib.md) are sufficient for every read this file needs
//    (`meets()` below never needed a real set — checking one key's value
//    is all a precondition ever asks). Where it stopped being a clean
//    substitute is finding #3.
//
// 3. THE SHARP ONE: MAP EQUALITY IS INSERTION-ORDER-SENSITIVE, WITH NO
//    DIAGNOSTIC WHEN IT BITES. GOAP's open/closed-list bookkeeping needs
//    to ask "have I already reached this exact world state by some other
//    path?" — the natural check is comparing two `Map<string,bool>`
//    states for equality. Two things about this, confirmed by direct
//    repro (`brink compile --dialect brink` + `brink play` on each):
//    - `state_a == state_b` on two `map`s (or two `struct`s) is not
//      legal — it FAULTS at runtime (`type error: cannot apply Equal to
//      Map and Map`), gradual AND strict mode alike, not a diagnostic at
//      compile time. `contains(array_of_states, candidate_state)` DOES
//      work (its `Value`'s-`PartialEq`-based structural scan, per
//      `collection_ops.rs`, is a different code path from the `==`
//      binary op, which literally has no `Map`/`Map` arm — see
//      `value_ops.rs::binary_op`). This file's `closed`/dedup checks
//      route through `contains()` for exactly this reason; a first draft
//      that reached for `==` faulted immediately.
//    - Having switched to `contains()`: `brink-format`'s backing map type
//      (`OrderedMap`, `crates/internal/brink-format/src/value.rs`) is a
//      `Vec<(MapKey, Value)>` under a `#[derive(PartialEq)]` — which
//      compares entry-by-entry IN ORDER. Two maps holding the IDENTICAL
//      fact/value pairs, inserted in a DIFFERENT order, compare UNEQUAL.
//      Confirmed directly: `insert(a,"has_weapon",true);
//      insert(a,"at_enemy",true)` vs. `insert(b,"at_enemy",true);
//      insert(b,"has_weapon",true)` — same two facts, same two values,
//      opposite insertion order — `contains(#[a], b)` returns `false`.
//      This means a naive `apply_effect` that starts from a copy of the
//      old state and only overwrites the keys the effect names would
//      *usually* preserve order by accident (state's copy already has
//      every key positioned, an in-place overwrite doesn't move it) —
//      but that safety is a fragile, easy-to-violate invariant (one
//      stray `#{...}` literal built directly instead of going through the
//      shared constructor breaks it silently, with no error at the break
//      point — the two states just silently stop comparing equal). The
//      fix used throughout this file: `apply_effect` and `zero_state`
//      (below) NEVER build a state via a partial literal; every state is
//      rebuilt from scratch by iterating the single canonical
//      `fact_names` array in the SAME fixed order every time, so every
//      state value in this program has identical entry order by
//      construction, regardless of which action path produced it — the
//      one discipline that makes `contains()`-based state deduplication
//      sound. Flagging this as the sharpest, most transferable finding
//      in this file: **a map used as a value-equality key needs a
//      canonical insertion order maintained by convention; nothing in
//      the type system enforces it, and the failure mode is silent**
//      (two logically-identical states just fail to dedupe — not a
//      wrong answer here, since A* still finds the optimal path via the
//      open list regardless, only extra unmerged search nodes, but a
//      genuinely different algorithm relying on dedup for correctness
//      rather than efficiency could get this wrong with no signal at
//      all). This is the same family of concern as this project's
//      "silent data drops are always bugs" house rule, just surfacing
//      as an equality hazard instead of a lowering-pass drop.
//
// 4. DOCS/RUNTIME MISMATCH (not fixed here — flagged for its own
//    follow-up): `docs/book/src/toolchain/dialect/indexing.md` states "An
//    indexed map write never inserts. `m["new_key"] = v` on a key that
//    isn't already present faults, the same as reading it would." Direct
//    repro contradicts this under BOTH `types=gradual` and
//    `types=strict`: `temp d = #{}; d["x"] = 1` compiles and RUNS to
//    completion (`d["x"]` reads back `1`, `len(d) == 1`) — no fault, in
//    either mode. This file never relies on the behavior either way:
//    `apply_effect`/`zero_state` below always go through the `insert()`
//    stdlib mutator, which is unambiguously insert-or-overwrite by
//    contract (stdlib.md) regardless of which of the two documented/
//    actual indexed-write semantics is the real one. Not chasing this
//    further here — it's either a stale doc or a missing fault check in
//    the indexed-map-write lowering, and which one it is matters for a
//    fix but not for this port; recording it so whoever picks up the doc
//    or the lowering next doesn't have to re-discover it from scratch.
//
// 5. LAZY POP-TIME DEDUPLICATION, NOT EAGER — deliberately the same shape
//    as astar-grid's/dijkstra-grid's `if visited[...] == false { … }`:
//    this file's sorted-insertion `pq` (identical `pq_insert` to
//    astar-grid/story.ink, not re-derived) can and does carry multiple
//    stale entries for the same state (no native heap means no
//    `decrease-key`, so an already-queued node with a since-improved
//    priority is never edited in place — same finding those two files
//    already made). The `closed`-membership check happens only when an
//    entry is POPPED, not when it's pushed (beyond the immediate-effect
//    check next to the push, which is an optimization, not the
//    correctness backstop) — standard "lazy deletion" A*, and the reason
//    a correctness argument for this file never assumes the open list is
//    duplicate-free.
//
// 6. `call(...)` never needed. Worth noting by absence: this is the first
//    fn-value-adjacent file in the AI-decision lane (behavior-tree,
//    utility-ai) that ships with NO `#fn`, NO `call()`, and NO
//    `function-values.md` caveats to restate — direct evidence for
//    finding #1's claim that this domain's composition need is fully
//    satisfied by data and two generic interpreter functions.
//
// 7. Collection-literal-can't-span-lines and non-short-circuit `and`/`or`
//    (bfs-grid-path/dijkstra-grid's findings) both apply here too and
//    aren't re-derived: the `actions` array literal below is one long
//    line for the first reason; every multi-part guard below is written
//    as nested `if`s rather than `and`/`or` chains for the second.

STRUCT Action = #{
    name: string,
    cost: int,
    precond: Map<string, bool>,
    effect: Map<string, bool>,
}

STRUCT PlanNode = #{
    state: Map<string, bool>,
    g: int,
    parent: int,
    action_index: int,
}

STRUCT PQEntry = #{
    priority: int,
    node_index: int,
}

// The one canonical fact ordering every state in this file is built from —
// see finding #3: this is what keeps every `Map<string,bool>` state's
// entries in the same order regardless of which action path produced it.
VAR fact_names = #["has_weapon", "weapon_loaded", "at_enemy", "enemy_dead", "weapon_sharp"]

VAR found = false
VAR total_cost = -1
VAR plan_text = ""
VAR nodes_expanded = 0
VAR exec_lines = #[]

~ {
    // Five actions: four load-bearing (pickup a weapon, load it, close the
    // distance, attack) and one deliberate distractor (`sharpen_weapon` —
    // legal from the same state `pickup_weapon` opens up, cheap, but its
    // `weapon_sharp` effect is never in `goal` or any other action's
    // precondition, so it can be explored but never chosen as part of the
    // optimal plan).
    temp actions = #[Action#{name: "pickup_weapon", cost: 2, precond: #{"has_weapon": false}, effect: #{"has_weapon": true}}, Action#{name: "move_to_enemy", cost: 3, precond: #{"at_enemy": false}, effect: #{"at_enemy": true}}, Action#{name: "load_weapon", cost: 1, precond: #{"has_weapon": true, "weapon_loaded": false}, effect: #{"weapon_loaded": true}}, Action#{name: "sharpen_weapon", cost: 1, precond: #{"has_weapon": true, "weapon_sharp": false}, effect: #{"weapon_sharp": true}}, Action#{name: "attack_enemy", cost: 1, precond: #{"has_weapon": true, "weapon_loaded": true, "at_enemy": true}, effect: #{"enemy_dead": true}}]

    temp goal = #{"enemy_dead": true}
    temp start_state = zero_state()

    temp nodes = #[PlanNode#{state: start_state, g: 0, parent: -1, action_index: -1}]
    temp closed = #[]
    temp pq = #[PQEntry#{priority: heuristic(start_state, goal), node_index: 0}]

    temp goal_index = -1

    while len(pq) > 0 {
        temp top = pq[0]
        remove_at(pq, 0)
        temp node = nodes[top.node_index]

        if contains(closed, node.state) == false {
            push(closed, node.state)
            nodes_expanded = nodes_expanded + 1

            if meets(node.state, goal) {
                found = true
                goal_index = top.node_index
                break
            }

            temp i = 0
            while i < len(actions) {
                temp act = actions[i]
                if meets(node.state, act.precond) {
                    temp new_state = apply_effect(node.state, act.effect)
                    if contains(closed, new_state) == false {
                        temp new_g = node.g + act.cost
                        push(nodes, PlanNode#{state: new_state, g: new_g, parent: top.node_index, action_index: i})
                        temp new_index = len(nodes) - 1
                        temp priority = new_g + heuristic(new_state, goal)
                        pq_insert(pq, PQEntry#{priority: priority, node_index: new_index})
                    }
                }
                i = i + 1
            }
        }
    }

    if found {
        temp backward = #[]
        temp cursor = goal_index
        while cursor != -1 {
            temp n = nodes[cursor]
            if n.action_index != -1 {
                push(backward, n.action_index)
            }
            cursor = n.parent
        }
        temp forward = reverse_ints(backward)
        plan_text = plan_to_string(forward, actions)
        total_cost = nodes[goal_index].g

        // EXECUTE the plan against a fresh live world, step by step — the
        // same `apply_effect` the planner used to search hypothetically
        // now drives the real narration (finding #1's "one interpreter,
        // two uses" payoff).
        temp world = zero_state()
        temp k = 0
        while k < len(forward) {
            temp a = actions[forward[k]]
            world = apply_effect(world, a.effect)
            push(exec_lines, "Step " + string(k + 1) + ": " + a.name + " (cost " + string(a.cost) + ") -> enemy_dead=" + string(world["enemy_dead"]))
            k = k + 1
        }
    }
}

Plan found: {found}.
Plan: {plan_text}.
Total cost: {total_cost}.
Nodes expanded: {nodes_expanded}.
{exec_lines[0]}
{exec_lines[1]}
{exec_lines[2]}
{exec_lines[3]}
-> END

// Every state in this program is built exclusively through `zero_state`
// (once) and `apply_effect` (thereafter) — both iterate `fact_names` in
// the same fixed order, which is what keeps `contains()`-based dedup sound
// (finding #3). Never build a state via a bare partial `#{...}` literal.
=== function zero_state() ===
~ {
    temp s = #{}
    temp i = 0
    while i < len(fact_names) {
        insert(s, fact_names[i], false)
        i = i + 1
    }
    return s
}

// Generic precondition/goal check — reused for BOTH `act.precond` and the
// top-level `goal`, since both are the same shape (a partial fact
// predicate). No per-action code anywhere (finding #1).
=== function meets(state, predicate) ===
~ {
    temp ks = keys(predicate)
    temp i = 0
    while i < len(ks) {
        temp k = ks[i]
        temp required = predicate[k]
        temp actual = false
        if contains(state, k) {
            actual = state[k]
        }
        if actual != required {
            return false
        }
        i = i + 1
    }
    return true
}

// Generic effect application: rebuild a full state from scratch, in
// canonical `fact_names` order, taking each fact's new value from `effect`
// where present and carrying the old value forward otherwise. Never a
// partial in-place overwrite of `state` (see finding #3's discipline).
=== function apply_effect(state, effect) ===
~ {
    temp result = #{}
    temp i = 0
    while i < len(fact_names) {
        temp name = fact_names[i]
        temp value = state[name]
        if contains(effect, name) {
            value = effect[name]
        }
        insert(result, name, value)
        i = i + 1
    }
    return result
}

// Admissible AND consistent here: every action's effect touches exactly
// one fact (checked by inspection of `actions` above), so "count of unmet
// goal facts" never overestimates the true remaining action count, and
// never drops by more than 1 per action taken — the same properties
// astar-grid's Manhattan heuristic leans on, derived for this graph
// instead of a grid.
=== function heuristic(state, goal) ===
~ {
    temp ks = keys(goal)
    temp i = 0
    temp unmet = 0
    while i < len(ks) {
        temp k = ks[i]
        temp required = goal[k]
        temp actual = false
        if contains(state, k) {
            actual = state[k]
        }
        if actual != required {
            unmet = unmet + 1
        }
        i = i + 1
    }
    return unmet
}

// Identical shape to astar-grid/story.ink's `pq_insert` — same sorted-
// insertion no-native-heap pattern, not re-derived here (finding #5).
=== function pq_insert(ref pq, entry) ===
~ {
    temp idx = 0
    temp searching = true
    while searching {
        if idx >= len(pq) {
            searching = false
        } else {
            if pq[idx].priority <= entry.priority {
                idx = idx + 1
            } else {
                searching = false
            }
        }
    }
    insert(pq, idx, entry)
    return 0
}

=== function reverse_ints(xs) ===
~ {
    temp out = #[]
    temp i = len(xs) - 1
    while i >= 0 {
        push(out, xs[i])
        i = i - 1
    }
    return out
}

=== function plan_to_string(indices, actions) ===
~ {
    temp out = ""
    temp i = 0
    while i < len(indices) {
        temp a = actions[indices[i]]
        out = out + a.name
        if i < len(indices) - 1 {
            out = out + " -> "
        }
        i = i + 1
    }
    return out
}
