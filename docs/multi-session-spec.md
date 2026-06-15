# Multi-session — flow / session picker spec

**Status:** Design (issue #182, Phase 8.4). Builds on the `SessionProvider` seam (#179,
`docs/live-inspector-spec.md` §3) and capability gating (#180). Spun out of the inspector spec
per the decision log (2026-06-14): #182 is the heaviest, least-blocking Phase 8 item and earns
its own document.

> Trust note: a studio/tooling surface. In the isolated-runner scope (§7) it makes **no** runtime,
> compiler, or oracle change — it is additive studio plumbing over the existing session model.

## 1. Purpose

Lift the single-session MVP assumption. A game — or the author, locally — can run multiple
**flows**; the inspector should let the user pick which session the session-bound views (Player /
transcript, State View, Story Graph overlay, status-bar story segment) follow. `studio-shell-spec`
§7.6 already keys those views to *the active session* rather than a global, so this is **additive,
not a rework** — no per-view change.

## 2. Background — what a "flow" is in brink

This shapes everything below, so it's stated up front (findings, file:line cited):

- **`FlowInstance`** (`crates/brink-runtime/src/story.rs:676`) is **host-orchestration**: an
  independent execution context (own call stack, output buffer, pending choices, status), created
  via `FlowInstance::new_at_root` / `new_at`. A `Story` owns a `default` flow plus a
  `HashMap<String, (FlowInstance, Context)>` of named flows; flows may **share** a `Context`
  (writes visible across flows, inklecate semantics) or hold isolated clones.
- **Brink does not compile ink's `FLOW` language feature.** Multiple flows exist only when a host
  *deliberately spawns* them. (`<-` threads are a different, intra-flow concept.) bevy-brink uses
  exactly this: one entity = one `FlowInstance` (`BrinkFlow<M>` + `BrinkContext<M>`).
- **The wasm `StoryRunner` is single-flow** — it exposes no flow concept today.

**Consequence:** a single compiled story played in the studio's wasm runner has exactly **one**
flow. Multiple sessions arise only from (a) the studio opening additional **independent runners**,
or (b) a **remote provider** reporting a game's N flows over a transport.

## 3. Model — a session registry

The unit stays the existing single-session **`SessionProvider`** (#179), unchanged. A thin registry
sits above it:

```ts
interface SessionEntry {
  id: SessionId;                 // stable per session ("local:default", a flow name, …)
  label: string;                 // picker label (e.g. "Main", "NPC: guard", knot name)
  provider: SessionProvider;     // the single-session unit (#179)
}
```

A new `sessions` store slice holds:

- `sessions: SessionEntry[]` — ordered (insertion order; deterministic).
- `activeSessionId: SessionId | null`.

The **active** entry's provider snapshot mirrors into today's reactive fields
(`sessionStatus`, `sessionText`, `sessionChoices`, `debugState`, `programChecksum`,
`programModel`, `programInkt`, `capabilities`) — reusing #179's mirror, keyed to the active entry.
**Views are unchanged.** Switching the active session: unsubscribe the prior provider, subscribe the
new one, re-mirror its snapshot; `prevDebugState` resets (a different execution timeline).
`_sessionBytes` / `compiledChecksum` stay compile-bound (shared across local sessions of the same
program). Capabilities follow the active provider (#180), so the drive commands gate on whatever
the active session permits.

This subsumes today's single-session store: the slice currently holds one `_provider`; #182
generalizes it to `sessions` + `activeSessionId`, with the single-session path being "exactly one
entry, always active."

## 4. Sources — who populates the registry

A **session source** owns registering / unregistering entries:

- **Local source** (built here): registers one entry `"local:default"` at startup — today's
  behavior exactly. Author actions add more entries, each a fresh `LocalSessionProvider` with **its
  own `StoryRunnerHandle`** started at an entry point (root, or a knot via `go_to_path` /
  `go_to_path_with_args`, #178). Globals are **isolated** per session (§7). Closing an entry
  disposes its runner.
- **Remote source** (future, Bevy / RMMZ — designed-for, not built): one transport connection
  registers **N** entries, one per `FlowInstance`; flows appearing / ending add / remove entries.
  These may be **shared-context** flows (§7), reported via the transport — the studio mirrors, it
  doesn't reproduce their semantics locally.

## 5. Picker UI

- A **session selector** lists entries and sets the active session. **Hidden when ≤1 entry** —
  local single-session behavior is identical to today, no picker noise (acceptance §9).
- **Placement (settled):** a status-bar segment (`status.sessions`) immediately after the
  story-status segment — session-scoped global state. The first extra session is opened via the
  `story.openSession` command (palette), so there is no affordance clutter at one session.
- Drive commands (`story.*`) act on the **active** provider, capability-gated (#180).
  `story.openSession` opens a local session; it gates only on a program existing (it always creates
  a *local* session, independent of the active provider's capabilities). Opening "from here" at a
  knot is wired (`openSession({ path })`), but a studio affordance that picks the knot is the #186
  *play-from-here* follow-on.

## 6. Lifecycle

- **Open** a local session: a command (e.g. `story.openSession`, or "Play from here as a new
  session") registers an entry and makes it active.
- A session that **ends** (status `ended`) stays in the list (still inspectable) until closed.
- **Removing the active** session falls back to the most-recently-active remaining entry, else
  `null` → placeholder (status `none`).
- **App teardown** disposes every provider.

## 7. Two kinds: independent runners *and* shared-context flows

*Decision 2026-06-14: "isolated now, shared later" — then shared pulled forward as a local
feature (#200).* The picker offers **both**, as distinct tools:

- **"+ New session" — independent runners.** A local session is its own `StoryRunnerHandle` with
  **isolated globals**. The right default for speculative play, compare-two-paths, and
  play-from-here without disturbing the main session. (#182.)
- **"+ New flow" — shared-context flows (#200).** A `FlowSessionProvider` drives a named
  `FlowInstance` spawned in the **primary session's** `Story`, sharing its `Context` (globals /
  visit counts / rng — one flow's writes visible to the others, true ink concurrent-flow
  semantics) while keeping its own call stack + temps. Realized end-to-end locally:
  `Story::spawn_flow_shared` + a `shared_instances` map (brink-runtime), the
  `spawn_flow`/`continue_flow`/`choose_flow`/`destroy_flow`/`flow_names`/`flow_debug_snapshot` wasm
  surface, and `LocalSessionProvider.spawnFlow` vending the provider on its shared runner. Flow
  sessions are dropped when the primary recompiles (its `Story` is replaced). The existing isolated
  `Story::spawn_flow` (bevy-brink's per-entity model) is unchanged, and single default-flow
  execution is untouched, so the oracle is unaffected.

Both kinds live in the **same registry** (§3) — the seam held: adding flows needed a new provider
+ source, **no view or registry change**.

## 8. Player tabs per session (#120) — follow-on

#120 (Player becomes an editor-area session document) is the natural second consumer: each session
can own a player **tab** (the Inky-style multi-up). **Not built here** — #182 is the single active
view-set + picker; per-session tabs layer on once #120's document model exists.

## 9. Acceptance

- A source exposing ≥2 sessions lists them; selecting one repoints Player / transcript, State View,
  and the graph overlay to that session.
- ≤1 session: **no picker**; behavior identical to today.
- Opening / closing local sessions works; closing the active session falls back cleanly.
- Drive commands gate on the active session's capabilities (#180).

## 10. Out of scope

The remote transport itself (the Bevy / RMMZ provider), shared-context flows (the §7 seam),
per-session player tabs (#120), and cross-session diffing.
