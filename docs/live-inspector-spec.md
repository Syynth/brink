# Live Inspector — `SessionProvider` spec

**Status:** Design (issue #127). This is the keystone spec for the live-inspector feature. It
defines the **host session channel** (Shell Phase 7) and the contract the **live inspector
views** (Shell Phase 8, issues #179–#182) consume. Implementation does not begin until this
spec is approved.

> Trust note: this spec describes a *studio/tooling* surface. It never changes runtime or
> compiler semantics, the compiled program, or the oracle. It is additive plumbing on top of
> the existing session model.

## 1. Purpose

The studio's session-bound surfaces — Player/transcript, State View, the Story Graph live
overlay, and the status-bar story segment (studio-shell-spec §4, §7.6) — render against a
**story session** that is, today, backed by the studio's own wasm `StoryRunner`. This spec
promotes that backing into a **`SessionProvider`** interface so a session can instead be a VM
running *inside a game* — RPG Maker MZ or Bevy — turning the same views into a **live
inspector** with zero per-view work.

Two consumer surfaces stack on the one channel (hence the layered milestones):

- **Run-time observation** (this spec + Phase 8 views): watch a game's VM — transcript,
  variables, current location, graph overlay.
- **Author-time host vocabulary** (Phase 9, #174/#175/#176): the same live bridge answers
  `enumerate`/`resolveLabel` for the argument picker. Out of scope here; noted for transport
  reuse.

## 2. Background — the current session model

The session model is already well-factored for this change. (Findings from the current tree;
file:line cited so the refactor in #179 has exact targets.)

- **Studio session state** lives in `SessionSlice`
  (`packages/studio-store/src/slices/session.ts`). It splits cleanly into:
  - **Reactive, view-consumed data:** `sessionStatus`, `sessionText` (append-only
    transcript), `sessionChoices`, `debugState`, `prevDebugState`, `programModel`.
  - **Private, never-consumed refs:** `_runner` (the wasm `StoryRunnerHandle`),
    `_sessionBytes` (program identity), `_choiceLog` (replay history).
- **Views consume DATA only.** `PlayerPane.tsx`, `StateView.tsx`, `StoryGraphDocument.tsx`,
  and the status bar all read the reactive fields and **never touch `_runner`** — the
  standing constraint (#127) already holds today. This is the load-bearing fact: the
  abstraction can be introduced entirely inside the store, with no view changes.
- **Commands own the lifecycle.** `story.start`/`restart`/`stop`/`choose`/`continue`
  (`packages/brink-studio/src/story-commands.ts`) are the only session mutators, each gated by
  a `when` predicate over `sessionStatus` (studio-shell-spec §7.6).
- **Program identity** exists as `ProgramModel.checksum` — `"0x{:08x}"` derived from
  `StoryData.source_checksum` (`crates/brink-web/src/program_model.rs`). Captured per
  `startSession`. *Gap:* no lightweight checksum before a runner is created.
- **Multi-flow** (`FlowInstance`, `crates/brink-runtime`) exists in the runtime and is used by
  bevy-brink per-flow, but is **not surfaced to the studio** — single session today.

Shared types already cross the wasm boundary as JSON (`packages/wasm-types/src/index.ts`):
`SessionStatus`, `Choice`, `Line`, `DebugState`, `ProgramModel`. The protocol messages
essentially exist already — serializable by construction.

## 3. The `SessionProvider` interface

Designed from the consumer's side: the store binds to **one** `SessionProvider`; the provider
is the *source of truth* for the reactive session data and the *only* thing that can drive the
session — and only to the extent its capabilities allow. The wasm `StoryRunnerHandle` stops
being a store field and becomes an implementation detail of the **local provider**.

```ts
/** What the session-bound views select from. Push- or pull-sourced, normalized to a snapshot. */
interface SessionSnapshot {
  status: SessionStatus;            // "none" | "running" | "awaiting-choice" | "done" | "ended" | "error"
  transcript: string[];             // append-only text (today's sessionText)
  choices: Choice[];                // pending offers (today's sessionChoices)
  debugState: DebugState | null;    // name-resolved location / globals / call stack / visits / rng
  programChecksum: string | null;   // identity of the RUNNING program (§5)
  auto: boolean;                    // reveal mode (#3011): false = one line per reveal
}

**Reveal granularity (#3011, ruled 2026-08-23).** Every reveal — initial load,
after a choice, and `continue` — advances a SINGLE line. `setAuto(true)` switches
all of them to run-to-next-pause. One line is the default because the Player is
an authoring tool before it is a preview: revealing a whole run at once makes it
impossible to see where a line lands or which convention fired on it. A provider
advertises `auto` only if it can actually switch — both current providers can
(`continueToPause` for the primary session, `continueFlowMaximally` for a flow).

/** Drive verbs. A provider advertises only those it supports (§3.2). */
type SessionCapability = "start" | "restart" | "stop" | "choose" | "continue" | "auto";

interface SessionProvider {
  readonly kind: "local" | "remote";
  readonly capabilities: ReadonlySet<SessionCapability>;

  /** Current data + subscription. The store mirrors snapshots into the reactive slice fields;
   *  views are unchanged. Returns an unsubscribe. */
  getSnapshot(): SessionSnapshot;
  subscribe(listener: (snapshot: SessionSnapshot) => void): () => void;

  /** Drive operations — each callable ONLY if the matching capability is present.
   *  Calling an unadvertised verb is a programming error (the command layer gates first, §4),
   *  so these need not be defensive. `start` takes program bytes for the local provider;
   *  remote providers that can (re)start ignore bytes and act on their own program. */
  start?(bytes?: Uint8Array): void;
  restart?(): void;
  stop?(): void;
  choose?(index: number): void;
  continue?(): void;
  /** Set the reveal mode (#3011). Takes effect on the NEXT reveal; it does not
   *  retroactively expand or collapse the existing transcript. */
  setAuto?(auto: boolean): void;

  dispose(): void;
}
```

### 3.1 Why a snapshot + subscription (not a handle)

The local provider is **pull**-based (`revealNext` steps the runner synchronously); a remote
provider is **push**-based (the game advances on its own loop and emits events). Normalizing
both to *"keep this snapshot current; notify on change"* lets the store mirror provider data
into the existing reactive fields with one adapter, and keeps the push/pull difference out of
the views entirely. `prevDebugState` (diff highlighting) stays a store concern — derived from
successive snapshots, not the provider's problem.

### 3.2 Capabilities

The capability set is the whole of the observe-vs-drive distinction:

| Provider | Capabilities |
|---|---|
| **Local** (wasm runner) | `start`, `restart`, `stop`, `choose`, `continue` (full — unchanged behavior) |
| **Remote, interactive** | `choose` (and maybe `restart`/`continue`) — the game lets the studio steer |
| **Remote, observe-only** | *(empty)* — inspect only; the game drives itself |

Capabilities are static per provider instance (a provider may be re-created on reconnect with a
different set). They flow from the transport (§6): the game declares what it permits.

## 4. Capability-gated commands

No new UI model. The existing `story.*` commands already gate on session state via `when`
predicates (studio-shell-spec §6, §7.6); capabilities plug into the **same mechanism**:

```
story.choose   when: status == "awaiting-choice"  AND  provider.capabilities has "choose"
story.continue when: sessionCanContinue(status)    AND  provider.capabilities has "continue"
story.start    when: status == "none"              AND  provider.capabilities has "start"  AND program exists
story.restart  when: status != "none"              AND  provider.capabilities has "restart"
story.stop     when: status != "none"              AND  provider.capabilities has "stop"
```

An **observe-only** provider therefore makes every drive command fail its `when` predicate, so
they vanish from the palette, strips, and view headers automatically — the views render fully
populated but read-only, with no per-view branching. This is the "capabilities plug into the
same predicate machinery" claim from #127, made concrete.

## 5. Program identity & degraded mode

Source mapping — reveal-from-stack-frame, graph current-location highlighting, visit-count →
node badges — is valid **only when the studio's local compile is the program running in the
game.** The author editing `.ink` while the game runs is the *normal* case, so skew is expected,
not an error.

- The provider reports `programChecksum` (the running program's `StoryData.source_checksum`).
- The studio compares it to its own latest compile's `ProgramModel.checksum`.
- **Match → full fidelity:** reveal, graph location, visit badges all live.
- **Mismatch → first-class degraded mode:** transcript + variables only. Source-position
  features disable; the State View still renders (its values are name-keyed and
  program-independent); the graph overlay drops the current-location highlight and visit
  badges. Surfaced as a **status** (status-bar segment: "inspecting — source out of sync"),
  never a notification-worthy failure.
- **Recovery is live:** when a later compile's checksum matches again (or the game reloads to
  match), full fidelity returns with no view teardown.

*Closed (#181):* the running program's checksum is on the provider snapshot
(`SessionSnapshot.programChecksum`, mirrored to the slice). The studio's latest-compile identity
is `compiledChecksum` on the compile slice, computed by the `program_checksum(bytes)` wasm util
(`@brink-lang/web`) — which avoids constructing a throwaway runner. `sessionDegraded(running,
compiled)` is the derived predicate; the graph overlay and the status-bar segment consume it.
Remote providers will carry their checksum in the transport status/handshake.

## 6. Transports

### 6.1 Local provider (default, unchanged behavior)

Wraps today's `StoryRunner`. `getSnapshot`/`subscribe` are driven by the existing
step/reveal/choose path; full capabilities; choice-replay-on-recompile (the silent
`_choiceLog` replay with divergence truncation) stays a local-provider concern. This is the
`kind: "local"` provider and the studio's behavior with it is byte-for-byte what it is today.

### 6.2 RPG Maker MZ — mount-time extension / `postMessage`

The game is a browser app and the studio embeds via the §8 mount-time extension API. The host
passes a provider at mount — `sessions?: SessionProvider[]` alongside `toolWindows` — or
exchanges snapshot/command messages over `postMessage` when the studio runs in a separate
window. Synergy: the same host already registers a capability manifest (Phase 9) for
author-time checking; this adds run-time inspection of the same game. `@codetta/brink-host`
(celeris) is the reference consumer.

### 6.3 Bevy — dev-only websocket plugin

A dev-only debug plugin in `bevy-brink` exposes a websocket that streams serialized session
events. bevy-brink's per-flow observer events are most of the event stream already; the
snapshot types are JSON by construction. **Per-flow maps onto multi-session** (§7): each
`FlowInstance` becomes a session.

## 7. Multi-session / per-flow (deferred — #182)

Single session at MVP. The runtime supports multiple flows, so the contract keys session-bound
views to *the active session* rather than a global (studio-shell-spec §7.6), making a session
selector additive. When it lands (#182): per-`FlowInstance` = per-session; the deferred session
selector becomes the flow picker; #120's player-tabs-per-session gets a second consumer.
Designed-for here, not built here.

## 8. Implementation map

This spec is the shared design for two milestones:

- **Phase 7 — Host session channel** (this issue, #127): the `SessionProvider` interface (§3),
  the capability/predicate wiring contract (§4), the identity protocol (§5), and the transport
  definitions (§6). Spawns the channel/transport implementation issues.
- **Phase 8 — Live inspector views** (#179–#182): the studio-side consumption —
  - **#179 (8.1):** extract the session slice behind `SessionProvider`; local provider conforms (§3, §6.1).
  - **#180 (8.2):** verify views stay data-only; wire capability-gated commands (§4).
  - **#181 (8.3):** program identity + degraded mode (§5).
  - **#182 (8.4):** multi-session / flow picker (§7).

Related but separate: **#178** (host-directed parameterized-knot entry) shares the host-bridge
*transport* concept but is a runtime/web feature with its own spec — not part of the inspector
channel.

## 9. Out of scope

Pause/step execution control, breakpoints, hot-reload of the running game's story, and
multi-game connections (per #127). Author-time host vocabulary (Phase 9). These may reuse the
channel later but are not designed here.
