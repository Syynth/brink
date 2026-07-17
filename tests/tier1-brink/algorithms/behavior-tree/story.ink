// ALGORITHMS CORPUS — AI-decision lane (issue #822)
// Behavior tree: sequence/selector/invert-decorator nodes composed over a
// small NPC blackboard (health/ammo/enemy_near), ticked across several
// simulated frames. This port exists specifically as the #521
// exotic-natives / lambda-friction detector the catalog names it: "if
// brink can't express 'a node is a thing that returns a status and can
// wrap another node' cleanly, that's the finding." See ERGONOMICS-FINDINGS
// below for the verdict.
//
// TYPES POLICY: gradual (default). `BTNode` is self-referential
// (`children: array<BTNode>`) and carries a `fn(): int` field — both are
// genuinely new shapes for this corpus, so gradual was chosen deliberately
// to isolate "does composition work at all" from "does it work under
// strict inference" as two separable questions. Whether `fn(...)` struct
// fields and self-referential structs even type-check under strict is
// noted below as an open question this file does not answer — a follow-up
// port is the right place to answer it, not a scope-creeping edit here.
//
// ERGONOMICS-FINDINGS:
//
// 1. SELF-REFERENTIAL STRUCTS WORK, CONTRADICTING THIS EPIC'S OWN
//    PREDICTION. The catalog's Quadtree entry predicts brink "doesn't have
//    pointers" so a recursive node type will "surface how brink wants to
//    represent tree nodes (arena-of-structs-by-index is the likely
//    answer, itself a findings note)." Tested directly before writing
//    this file's tree-building code: `STRUCT BTNode = #{ kind: int,
//    action: fn(): int, children: array<BTNode> }` compiles and runs
//    correctly under gradual mode, nested at least two levels deep, with
//    no arena/index workaround needed. `BTNode`'s `children: array<BTNode>`
//    is a direct value-semantics recursive struct — every level is copied
//    by value on construction/pass (per `docs/book/.../types.md`'s struct
//    value-semantics rule), which is fine for a tree built once and never
//    mutated after construction, as this one is. The predicted friction
//    did not materialize here; flagging the gap between the catalog's
//    prediction and this file's lived result so the Quadtree/HFSM ports
//    later in this epic don't reflexively reach for an arena when a plain
//    recursive struct already works.
//
// 2. THE HEADLINE FINDING — PARTIAL APPLICATION PARAMETERIZES LEAVES,
//    BUT DOES NOT COMPOSE NODES. This is the sharpest test #822 asked
//    for, and the answer is a clean split:
//    - `above`/`at_most`/`is_true` below are each ONE function definition
//      reused as many distinct leaf *conditions* purely by binding
//      different `ref` cells and thresholds at `#fn` creation time
//      (`#fn(above, ammo, 0)` vs. `#fn(at_most, health, 20)` vs.
//      `#fn(is_true, enemy_near)`). This is real, load-bearing reuse:
//      three leaf behaviors from one function body, zero duplication.
//      Partial application earns its keep completely here — this is
//      exactly the "considerations as scoring functions" idiom the
//      utility-ai port next door leans on too.
//    - But composing the STRUCTURE — "wrap these three leaves in a
//      sequence," "invert this leaf's result," "put these sequences under
//      a selector" — has nothing to do with function values at all. A
//      decorator in the classic OOP behavior-tree pattern is an object
//      that WRAPS another object and returns something with the same
//      interface; brink has no closures and no way for a function to
//      construct and return a new callable that closes over an argument
//      it was just handed (`bind`/`#fn` only ever curry a STATICALLY
//      NAMED function's own declared parameters — there is no
//      "wrap_in_invert(child_fn): fn(): int" you could write, because
//      the thing `wrap_in_invert` would need to return is an anonymous
//      closure over its `child_fn` argument, and brink's function values
//      are deliberately not that). The only way to express "this node
//      wraps that node" turned out to be exactly what plain composition
//      in a language without first-class functions has always looked
//      like: nodes as DATA (`BTNode`'s `kind` tag + `children` array) and
//      an explicit `tick()` function that recurses and dispatches on the
//      tag by hand. That is hand-rolled polymorphism, structurally
//      identical to what a C program forced through a `union` + `switch`
//      would write — not "the sharpest test of composition without
//      closures," in the sense that composition here is achieved by NOT
//      using function values for the composite/decorator layer at all.
//      The honest verdict: partial application sufficed for
//      parameterizing leaves; it did not, and structurally cannot,
//      suffice for composing nodes. That gap — needing tagged data plus
//      hand-written dispatch wherever real code would reach for a
//      closure returning a closure — is precisely the #521 evidence this
//      port was commissioned to produce.
//
// 3. SILENT MISCOMPILE ON A DIRECT CALL THROUGH A NON-BARE-NAME CALLEE —
//    FIXED as of #869 (was: flagged as a probable compiler bug, not a
//    design tradeoff). Before settling on `call(node.action)` below,
//    `node.action()` (calling a `fn(): int` struct field directly,
//    without `call()`) was tried first, since `function-values.md` only
//    said direct call syntax "isn't syntactically available" for a
//    callee "stored behind an index expression [or] a field" — read at
//    face value, that sounds like a parse rejection. At the time it was
//    not: `node.action()` (and the same shape over an array element,
//    `arr[0]()`) COMPILED with zero diagnostics and RAN, but silently did
//    not invoke the function — the trailing `(...)` was dropped and the
//    expression evaluated to the bare function value itself (`out =
//    b.action()` in the minimal repro that motivated this produced the
//    STRING `fn give_five()`, not the `5` the call should have
//    returned). Binding the same field to a `temp` first and calling
//    THAT (`temp f = b.action; f()`) worked correctly, confirming the
//    restriction was real but that its failure mode was a silent
//    wrong-value, not a diagnostic — exactly the kind of "calls the
//    wrong thing and says nothing" failure this project's own rules
//    treat as a bug, just surfacing in call position instead of a
//    collection mutator. #869 replaced the silent drop with a
//    compile-time `E104` diagnostic naming `call(f, args…)` as the fix.
//    Every function-value invocation in this file still goes through
//    `call(...)`, even in the two or three spots where the callee
//    happens to already be a bare name and wouldn't strictly need it —
//    consistency here is cheaper than remembering which shape is safe on
//    a case-by-case basis, and it's the ratified form for a computed
//    callee regardless (t1c-spec §3).
//
// 4. RUNNING STATUS AND `#@local` AS THE TREE'S OWN MEMORY. `do_reload`
//    below takes two ticks (`RUNNING` on the first, `SUCCESS` on the
//    second) — the catalog calls this "normal operation, not just a
//    save/load feature," and this port takes that literally: the whole
//    "world" runs eight ticks in a single flow, and `reload_ticks_left`
//    is the tree's own in-progress state persisting between them.
//    Marked `#@local` for the same reason `memoized-fibonacci`'s memo
//    map is: the honest annotation for "this is per-flow running state,"
//    even though a single-flow harness like this one can't observe the
//    difference from a plain `VAR` — see that file's header for the full
//    caveat, unchanged here.
//
// 5. NON-SHORT-CIRCUIT `and`/`or`, restated because it bites here too:
//    `bfs-grid-path`/`dijkstra-grid` already document that brink's
//    `and`/`or` always evaluate both sides. `tick()`'s dispatch below is
//    written as separate `if` statements with early `return`s specifically
//    to avoid ever writing a guard like `node.kind == NODE_SEQUENCE and
//    len(node.children) > 0` where a short-circuit assumption could hide
//    a fault on an empty-children edge case.

CONST SUCCESS = 0
CONST FAILURE = 1
CONST RUNNING = 2

CONST NODE_LEAF = 0
CONST NODE_SEQUENCE = 1
CONST NODE_SELECTOR = 2
CONST NODE_INVERT = 3

STRUCT BTNode = #{
    kind: int,
    action: fn(): int,
    children: array<BTNode>,
}

VAR health = 40
VAR ammo = 2
VAR enemy_near = true
VAR log = ""

#@local
VAR reload_ticks_left = 0

VAR tick_lines = #[]

~ {
    // Leaves. `above`/`at_most`/`is_true` are each bound with different
    // cells/thresholds — one function body, three distinct conditions
    // (finding #2 above).
    temp cond_enemy_near = BTNode#{kind: NODE_LEAF, action: #fn(is_true, enemy_near), children: #[]}
    temp cond_has_ammo = BTNode#{kind: NODE_LEAF, action: #fn(above, ammo, 0), children: #[]}
    temp cond_health_low = BTNode#{kind: NODE_LEAF, action: #fn(at_most, health, 20), children: #[]}
    temp act_attack = BTNode#{kind: NODE_LEAF, action: #fn(do_attack), children: #[]}
    temp act_reload = BTNode#{kind: NODE_LEAF, action: #fn(do_reload), children: #[]}
    temp act_flee = BTNode#{kind: NODE_LEAF, action: #fn(do_flee), children: #[]}
    temp act_patrol = BTNode#{kind: NODE_LEAF, action: #fn(do_patrol), children: #[]}

    // Decorator: NOT has_ammo, built by wrapping `cond_has_ammo` in an
    // invert node — data composition, not a function returning a function
    // (finding #2).
    temp cond_no_ammo = BTNode#{kind: NODE_INVERT, action: #fn(noop), children: #[cond_has_ammo]}

    temp attack_branch = BTNode#{kind: NODE_SEQUENCE, action: #fn(noop), children: #[cond_enemy_near, cond_has_ammo, act_attack]}
    temp reload_branch = BTNode#{kind: NODE_SEQUENCE, action: #fn(noop), children: #[cond_enemy_near, cond_no_ammo, act_reload]}
    temp flee_branch = BTNode#{kind: NODE_SEQUENCE, action: #fn(noop), children: #[cond_health_low, act_flee]}

    temp root = BTNode#{kind: NODE_SELECTOR, action: #fn(noop), children: #[attack_branch, reload_branch, flee_branch, act_patrol]}

    // Eight scripted frames — deterministic world-state changes drive the
    // tree through every branch, including a two-tick RUNNING reload.
    // Each `run_tick` result is pushed into `tick_lines` rather than
    // concatenated with an embedded newline: ink prose lines are the
    // narrative model's own line-break unit (one physical `.ink` source
    // line per output line), not something a computed string can splice
    // in — `"\n"` inside a value has no special meaning here and prints
    // as the two literal characters, so each tick gets its own indexed
    // prose line below instead.
    push(tick_lines, run_tick(root, 1))
    push(tick_lines, run_tick(root, 2))
    push(tick_lines, run_tick(root, 3))
    push(tick_lines, run_tick(root, 4))
    push(tick_lines, run_tick(root, 5))
    health = 15
    enemy_near = false
    push(tick_lines, run_tick(root, 6))
    push(tick_lines, run_tick(root, 7))
    health = 25
    push(tick_lines, run_tick(root, 8))
}

{tick_lines[0]}
{tick_lines[1]}
{tick_lines[2]}
{tick_lines[3]}
{tick_lines[4]}
{tick_lines[5]}
{tick_lines[6]}
{tick_lines[7]}
Final: health={health}, ammo={ammo}, enemy_near={enemy_near}.
-> END

=== function above(ref stat, threshold): int ===
~ {
    if stat > threshold {
        return SUCCESS
    }
    return FAILURE
}

=== function at_most(ref stat, threshold): int ===
~ {
    if stat <= threshold {
        return SUCCESS
    }
    return FAILURE
}

=== function is_true(ref flag): int ===
~ {
    if flag == true {
        return SUCCESS
    }
    return FAILURE
}

=== function noop(): int ===
~ return SUCCESS

=== function do_attack(): int ===
~ {
    log = log + "attack;"
    ammo = ammo - 1
    return SUCCESS
}

=== function do_reload(): int ===
~ {
    if reload_ticks_left <= 0 {
        reload_ticks_left = 2
    }
    reload_ticks_left = reload_ticks_left - 1
    if reload_ticks_left > 0 {
        log = log + "reload(running);"
        return RUNNING
    }
    log = log + "reload(done);"
    ammo = ammo + 3
    return SUCCESS
}

=== function do_flee(): int ===
~ {
    log = log + "flee;"
    health = health + 5
    return SUCCESS
}

=== function do_patrol(): int ===
~ {
    log = log + "patrol;"
    return SUCCESS
}

=== function status_name(s: int): string ===
~ {
    if s == SUCCESS {
        return "SUCCESS"
    }
    if s == FAILURE {
        return "FAILURE"
    }
    return "RUNNING"
}

// Explicit tag-dispatch recursion — see finding #2: this is the "hand
// rolled polymorphism" this file's header discusses, not a fn-values
// composition mechanism.
=== function tick(node: BTNode): int ===
~ {
    if node.kind == NODE_LEAF {
        return call(node.action)
    }
    if node.kind == NODE_SEQUENCE {
        temp i = 0
        temp status = SUCCESS
        while i < len(node.children) {
            status = tick(node.children[i])
            if status != SUCCESS {
                return status
            }
            i = i + 1
        }
        return SUCCESS
    }
    if node.kind == NODE_SELECTOR {
        temp i = 0
        temp status = FAILURE
        while i < len(node.children) {
            status = tick(node.children[i])
            if status != FAILURE {
                return status
            }
            i = i + 1
        }
        return FAILURE
    }
    // NODE_INVERT — exactly one child by construction convention above.
    temp inner = tick(node.children[0])
    if inner == SUCCESS {
        return FAILURE
    }
    if inner == FAILURE {
        return SUCCESS
    }
    return RUNNING
}

=== function run_tick(node: BTNode, frame: int): string ===
~ {
    log = ""
    temp status = tick(node)
    temp line = "Tick " + string(frame) + ": " + status_name(status) + " [" + log + "]"
    return line
}
