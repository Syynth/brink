// ALGORITHMS CORPUS — AI-decision lane (issue #822)
// Dialogue FSM driven by a map of `#fn` handlers — the "game-home-turf"
// case named directly in the epic's v1 pile (distinct from the catalog
// extension's separate Hierarchical FSM entry, which generalizes this
// flat version to a nested state stack; that generalization is future
// work, not attempted here). A vendor NPC's dialogue state (`idle` /
// `greet` / `trade`) advances by looking its CURRENT state up in a
// `map<string, fn(string): string>` and calling whatever handler comes
// back with the next scripted event — the dispatch-table idiom this
// corpus keeps returning to (behavior-tree's `kind`-tagged struct arena
// is the same idea with an `int` tag instead of a `string` key).
//
// TYPES POLICY: gradual (default). `handlers`'s value type is a function
// type (`fn(string): string`), and every handler really does share that
// exact signature — a strict-mode attempt is plausible future work, but
// this file stays gradual to keep the fn-map dispatch idiom itself (the
// thing #822 actually asked for) isolated from a second, separate
// strict-mode-fn-type experiment; minimax-tictactoe next door is this
// lane's dedicated strict-mode file.
//
// ERGONOMICS-FINDINGS:
// - `#fn(...)` is NOT a compile-time-constant expression, so `VAR
//   handlers = #{"idle": #fn(handle_idle), ...}` as a top-level `VAR`
//   default — the first thing tried here, and the natural reading of the
//   epic's "map of `#fn` handlers" phrase as a single declaration — is a
//   compile error: E077, "array element, map value, or #fn bound value
//   argument in a VAR/CONST declaration default is not a
//   compile-time-constant expression." `events` just below it, by
//   contrast, IS a legal `VAR` default (`#["approach", "trade", ...]` is
//   all string literals, which const-fold fine — this is the same
//   const-fold path `literals.md` documents for map-literal keys).  The
//   fix actually used: declare `handlers` as an empty `#{}` `VAR`, then
//   `insert()` each `#fn(...)` into it inside the opening `~ { }` block,
//   once, before the FSM runs. This is a real, previously-undocumented
//   split in this corpus between "collections of plain values can be
//   `VAR` defaults" and "collections of function values cannot" — worth
//   knowing before reaching for a `VAR = #{...#fn...}` one-liner on any
//   future map-of-handlers port.
// - Once past that, the map-of-`#fn` idiom is exactly as clean as the
//   epic's v1 framing promised: `state = call(handlers[state], event)`
//   below is the entire dispatch step — one line, no `if`/`else if`
//   chain on the state name anywhere in this file. Compare with
//   behavior-tree/story.ink, which COULDN'T do the analogous thing for
//   its node-kind dispatch (an `int` kind tag there still needs an
//   explicit `if`/`if`/`if` chain in `tick()`) only because a node's
//   "handler" isn't uniform across kinds (a leaf calls its `action`, a
//   sequence loops its `children`, an invert flips a status) — a flat
//   FSM's handlers ARE uniform (`fn(string): string`, always), which is
//   exactly what makes the map-of-fn dispatch table work here and not
//   there. That contrast is itself a finding: map-of-`#fn` dispatch is
//   the right idiom precisely when every branch has the same shape, and
//   stops being applicable the moment the branches' interfaces diverge.
// - `call(handlers[state], event)`, never `handlers[state](event)` — see
//   behavior-tree/story.ink finding #3: calling a function value reached
//   through a map/array index (or a struct field) via direct-call syntax
//   used to silently fail to invoke it (compiled clean, returned the bare
//   function value instead of calling it) rather than being rejected.
//   Confirmed again here on a map-indexed callee specifically (the
//   earlier repros covered a struct field and an array element), so the
//   restriction was confirmed to hold across all three non-bare-name
//   callee shapes brink's indexing/field-access surface has. As of #869
//   this is now a compile-time `E100` diagnostic naming `call(f, args…)`
//   as the fix, never a silent no-op — the workaround above stays the
//   right code to write either way, since `call(...)` is the ratified
//   form for a computed callee (t1c-spec §3), not a stopgap.
// - `state` is `#@local`: this is the FSM's own persistent turn-to-turn
//   memory (which dialogue state the NPC is currently in), the same
//   category of thing `memoized-fibonacci`'s memo map and
//   behavior-tree's `reload_ticks_left` both are — see either header for
//   the standing caveat that a single-flow harness like this one can't
//   observe `#@local`'s actual per-flow isolation, only document the
//   annotation as the honest one to reach for.
// - String literals have no working escape sequences (consistent with
//   behavior-tree/story.ink's `"\n"` finding, generalized here to `\"`):
//   an attempt to quote the NPC's line in `step`'s output (`"\"" + log +
//   "\""`) printed the literal two-character sequence `\"` rather than a
//   `"` character. Dropped the quoting entirely rather than fight it —
//   the golden transcript below reads fine without it.
// - An unmatched event (`"buy"` while `idle`) is handled by each handler's
//   own trailing `else` branch, not a shared "unknown event" fallback in
//   the dispatch loop — the map only ever answers "which function handles
//   this STATE," never "is this event valid in this state," so
//   event-validity has to live inside each handler. A state with no
//   entry in `handlers` at all (a typo'd state name written back by some
//   handler, say) would fault at `handlers[state]` with brink's ordinary
//   "map has no such key" fault (`docs/book/.../indexing.md`) — not
//   something this file's fixed, hand-verified state set can trigger,
//   but worth noting for anyone growing this table: adding a new state
//   name means adding its handler to `handlers` in the same edit, with
//   nothing in the language enforcing that the two stay in sync.

#@local
VAR state = "idle"

VAR log = ""
VAR transcript = #[]

VAR handlers = #{}

VAR events: array<string> = #["approach", "trade", "buy", "buy", "leave", "approach", "leave", "buy", "approach"]

~ {
    // `#fn(...)` isn't a compile-time-constant expression (E077 — see the
    // header note below), so the dispatch table can't be a `VAR`
    // initializer default the way `events` above is; it's built here
    // instead, once, before the FSM runs.
    insert(handlers, "idle", #fn(handle_idle))
    insert(handlers, "greet", #fn(handle_greet))
    insert(handlers, "trade", #fn(handle_trade))

    for event in events {
        push(transcript, step(event))
    }
}

{transcript[0]}
{transcript[1]}
{transcript[2]}
{transcript[3]}
{transcript[4]}
{transcript[5]}
{transcript[6]}
{transcript[7]}
{transcript[8]}
Final state: {state}.
-> END

=== function handle_idle(event: string): string ===
~ {
    if event == "approach" {
        log = log + "NPC: Oh, hello traveler!"
        return "greet"
    }
    log = log + "NPC ignores you."
    return "idle"
}

=== function handle_greet(event: string): string ===
~ {
    if event == "trade" {
        log = log + "NPC: Take a look at my wares."
        return "trade"
    }
    if event == "leave" {
        log = log + "NPC: Safe travels."
        return "idle"
    }
    log = log + "NPC: ..."
    return "greet"
}

=== function handle_trade(event: string): string ===
~ {
    if event == "buy" {
        log = log + "NPC: Pleasure doing business."
        return "trade"
    }
    if event == "leave" {
        log = log + "NPC: Come back soon."
        return "idle"
    }
    log = log + "NPC: Anything else?"
    return "trade"
}

=== function step(event: string): string ===
~ {
    temp prev_state = state
    log = ""
    temp handler = handlers[state]
    state = call(handler, event)
    return "[" + prev_state + "] event=" + event + " -> " + log + " next=[" + state + "]"
}
