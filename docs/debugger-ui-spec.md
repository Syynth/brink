# Debugger UI spec — the Player rebuild + D9 view layer

Status: **DESIGNED 2026-08-29** (this document; epic #452 D9/#3187, absorbing
the Player half of #3199). Rulings recorded in `docs/decision-log.md`
(2026-08-29, three entries: "debug info on by default", "session-only debug
state", "Debugger UI round = the Player half of #3199"). Visual designs:
`docs/design/debugger-ui/` (design canvas). Wire/runtime contract:
`docs/debugger-spec.md` (D1) — this document is the *consumer* design and
does not restate it.

Everything below was verified against code on 2026-08-29, not against specs
alone. The load-bearing discovery of the survey: **the store and command
layer for the debugger already exist unconsumed**
(`packages/studio-store/src/slices/debug.ts`,
`packages/brink-studio/src/debug-commands.ts` — both carry "no view consumes
this yet" headers), StateView already renders call stack + locals (#3140),
and the Program Explorer already has the current-instruction highlight. This
round is view-layer + three wiring seams, not new architecture.

## 1. Rulings this design rests on

| Ruling | Where |
|---|---|
| Debug info **on by default** for all studio compiles; App-settings opt-out; release export still omits the section | decision-log 2026-08-29; supersedes #3229's "not on by default" (mechanism unchanged) |
| **No debug mode.** No enter/leave lifecycle, no restart-to-debug. Breakpoints bind mid-play; stepping starts from wherever the story is | same entry; makes #3249's original scope moot |
| **Pause/resume is a first-class Player verb**; interleaved play↔debug coherence is required proof | same entry |
| Session-only debug state; **breakpoints persist per project** | decision-log 2026-08-29 |
| This round **is** the Player rebuild (#3199's Player half); Story Graph is a later round. StateView is **replaced**, not extended | decision-log 2026-08-29 |
| Both surfaces (`.ink` and `.brink`) get every feature | debugger-spec §0 (RULED 2026-08-28) |
| Line stepping and instruction stepping are both first-class; the studio presents `.inkt` disassembly beside source | `brink-runtime/src/debug_session.rs` module doc (RULED 2026-08-28) |
| Every source-position feature gates on `sessionDegraded` — suppressed, never stale | live-inspector-spec §5; already enforced in `ProgramView`/`StoryGraphDocument` |
| Thread step-out and park stepping have **no honest analogue** — distinct operations, never mislabeled | debugger-spec §4 |

## 2. End-user feature catalog

Each feature states: what the author sees, how it functions, and how it
degrades. "Panel" below means the Debugger panel (§4, the StateView
replacement); "Player" means the rebuilt Player document (§3).

### F1 — Always-ready debugging

There is no debug switch. Every studio compile carries `DebugInfo`; the
Player is always one click from being a debugger. The only related setting
is the App-settings opt-out ("Emit debug info in studio compiles", default
on) for authors who measure a compile cost they don't want to pay. With it
off, the debugger degrades exactly like a prebuilt artifact without the
section (debugger-spec §1.2): breakpoints refuse to bind (hollow markers,
tooltip explains), stepping reports no source position.

### F2 — Breakpoints

- **Set/clear**: click the editor gutter margin on any line, either surface.
  Solid dot = bound to a program address in the current compile; hollow dot
  = unbound (no statement on that line, or the compile is stale/absent).
  Clicking a line with no statement binds to the nearest following
  `IS_STMT` entry (DAP convention) and the dot renders on the line it
  actually bound to.
- **Placement (RULED 2026-08-29)**: the breakpoint glyph shares the
  **play gutter's column** — no separate host-gutter column. ▶ appears
  only on hovered header lines and breakpoints live on statement lines,
  so conflicts are rare; on a header line the hover glyph stays ▶ and the
  gutter context menu carries "Set breakpoint here". The paused-here
  arrow (F4) overlays the same column.
- **Model**: the store keeps breakpoints **source-anchored** (`file` +
  range), per D1's range-keyed v1 ruling. On every compile they re-bind via
  the source→program resolver; the `(container_idx, offset)` handed to the
  runtime is derived state, never the stored identity. Edits above a
  breakpoint move it (CM6 change mapping); a breakpoint whose anchor text
  vanishes goes hollow rather than silently rebinding elsewhere.
- **Enable/disable** without removing (checkbox in the panel's Breakpoints
  section; dimmed dot in the gutter).
- **Persistence**: per project, like layout state.
- **Breakpoints section** in the panel: every breakpoint as
  `file:line — knot.path`, checkbox, click-to-reveal, remove ×; header
  actions: disable all, clear all.
- Conditional breakpoints and watchpoint UI are **out of scope** (v2;
  runtime tickets #3222 and `debug_run_watching` exist, the UI does not).

### F3 — Pause, resume, and stepping (the transport)

**The granularity ladder (RULED 2026-08-29).** Three stepping tiers,
coarse to fine, matching who is holding the controls:

1. **Story line** — the delivered `OutputLine`, the author's primary
   debugging unit. The existing reveal-next verb IS the first-class
   step; auto mode is it self-advancing (F4/F13). Most story-and-logic
   debugging happens here without touching the debug cluster at all.
2. **Source statement** — step into/over/out, the deeper
   logic-investigation tier when line-level advance isn't enough.
3. **Instruction** — `stepi`, the programmer-assist tier ("when the
   programmer needs to step in to help"); lives in the Program
   Explorer, never the Player toolbar.

The Player toolbar carries the transport (§3). Verbs, with proposed
keybindings (declared on the `debug.*`/`story.*` command descriptors;
user-overridable through the existing keymap service):

| Verb | Behavior | Key |
|---|---|---|
| Pause | Suspend at the next statement boundary | F6 |
| Continue | Resume normal play (to next breakpoint, choice, or terminal) | F5 |
| Step over | One source line, calls run to completion | F10 |
| Step into | One source line, descending into calls | F11 |
| Step out | Run until the current frame returns | Shift-F11 |
| Toggle breakpoint (current line) | | F9 |
| Restart | Existing `story.restart` | Shift-F5 |

Step controls enable only while paused. Step-out follows debugger-spec §4's
frame table: disabled in `Root`, and for `Thread` frames the control
relabels to **"Finish thread"** (distinct operation, never "step out"). At
a condition park, stepping controls disable and the status reads "parked —
resumes when its condition next re-evaluates true" (§F11).

Instruction stepping (`stepi`) lives in the **Program Explorer's** header
actions, not the Player toolbar — the audience watching instructions is
already looking at the disassembly. Same enablement rules.

### F4 — Execution highlight (RULED 2026-08-29: live during play, not paused-only)

**Play is stepping.** The story never auto-starts (the Player opens
idle; Run begins a session), and from the moment a session runs, the
editor carries the per-line treatment *continuously*: the highlight
follows each delivered line as it is revealed — paced auto, manual
Continue, or a debug step are all the same visualization advancing.
Pause and breakpoints stop the advance; they do not switch anything on.

- **Editor, while playing**: the line being revealed carries a
  success-tint band (no gutter glyph), moving with the reveal cadence
  (F13). The editor never auto-scrolls to follow playback — clicking the
  status chip reveals the current line (follow-toggle: open knob, §7).
- **Editor, while paused**: warning-tint band + gutter arrow, auto-open
  + reveal on stop like any IDE. Selecting a non-top stack frame shows
  that frame's line in an accent-tint band with a hollow arrow.
- Color language across all execution states: **live = success tint ·
  paused = warning tint + arrow · selected frame = accent tint + hollow
  arrow · parked = info dashed · breakpoints = error dots**.
- **Program Explorer**: the existing current-instruction highlight, plus
  it follows frame selection.
- Both are suppressed under `sessionDegraded` (already the Program
  Explorer's behavior; the editor inherits the same predicate).

### F5 — Call stack with frame selection

The panel's Frames section becomes interactive: click a frame → it becomes
the selected frame; the editor reveals its source line (F4), Locals shows
that frame's temps, the Program Explorer highlights its instruction. Frame
rows keep the kind badges (`function`/`tunnel`/`thread`/`external`…) and
gain the resolved `file:line` (clickable) beside the container path. A
parked flow's resume frame is labeled **"parked — resumes here"**
(debugger-spec §2.6), never presented as a live position.

### F6 — Variables

Locals-first: the selected frame's locals (by name, from D7's table) are
the top variables section, with the existing tri-state honesty ("no debug
info for this frame" / empty / table). Globals below, keeping the
changed-since-last-step diff highlight. Values render through the existing
`DebugValueView` union. Editing values is out of scope (that's #57's
re-scope, noted in §7).

### F7 — Stop reasons and status

Every `DebugRunOutcome` reason surfaces in exactly one place — the Player's
status chip (§3) — and the panel header mirrors it:

- `breakpoint` → "Paused at breakpoint — main.ink:12" (breakpoint's name if
  set)
- step boundary → "Paused — main.ink:14"
- `choices` → normal choice presentation in the Player (play and debug
  share the choice UI; picking a choice while paused stays paused)
- `terminal` → existing ended status
- `DebugBudgetExceeded` → "Stopped — step budget exceeded (possible
  runaway loop)" with a Continue affordance (resumes with a fresh budget)

The status bar's `StorySegment` gains a paused state (dot color + label)
alongside the existing degraded state.

### F8 — Staleness

Unchanged machinery, wider application: editing source while paused flips
`sessionDegraded`; the editor highlight and gutter binding indicators go
stale-suppressed (hollow), the panel header shows the existing "inspecting
— source out of sync" language, and the transport stays usable (the story
still runs — only *source mapping* is suppressed, matching the per-file
degradation posture of debugger-spec §2.3). Restart re-syncs.

### F9 — Transcript provenance jump (Player rebuild)

Every transcript line the Player renders came from a real `OutputLine`
carrying `block_id`; the line tables map it to source. The rebuilt Player
makes each transcript line a provenance handle: hover shows `file:line`,
click/⌘-click reveals it in the editor (same `editor.reveal` verb).
This is the Player-side twin of the breakpoint gutter — navigation from
*output* back to source, where the gutter navigates from source forward.
(Build-time verification required: confirm `block_id` → line-table →
source resolution is exposed or cheaply exposable through the wasm bridge.)

### F10 — Disassembly beside source

Already ruled and mostly built: the Program Explorer shows offset-carrying
disassembly with the current-instruction highlight. This round adds: `stepi`
controls in its header (F3), frame-selection following (F5), and a
"Reveal in Program Explorer" action on the editor's current-line context
(jump from a source line to its instructions — the inverse of the existing
`.inkt` open).

### F11 — Parked flows (blocked on a ruling)

The UI treatment is designed (F3/F5: disabled stepping + "parked — resumes
here" frame label + condition-park vocabulary per debugger-spec §4 — never
"waiting for a value"), but what `Story::debug_position()` *reports* while
parked is the open ruling #3225. This round proposes: report the
continuation container's offset-0 position tagged `parked: true` (and the
call site tagged for `AwaitingExternal`), which is exactly what the
"resumes here" label needs; the ruling belongs to #3225, not here.

### F12 — Multi-flow (deferred)

The runtime seam is default-flow-only (#3223). The UI reserves the concept:
the debugger drives the **active session** (the existing session picker is
the selection surface), and the panel header names the flow being
inspected. Per-flow breakpoint filtering and cross-flow stepping wait for
#3223's runtime work; nothing in this design forecloses it.

### F13 — Transcript line presentation (RULED 2026-08-29)

Three refinements from the canvas review, all Player-side:

- **Paced auto-reveal.** Auto mode is a transport *toggle button*
  (fast-forward icon, pressed = on), not a checkbox. An App setting
  ("Auto reveal: paced / all at once") controls playback pacing: when the
  runtime delivers a turn's lines as a chunk, paced mode reveals them one
  line at a time in rapid succession (proposed default: paced, ~150 ms
  cadence — a playback timer in the Player, no runtime change).
  Debugger interaction rules: a breakpoint/pause **flushes the reveal
  queue instantly** (the paused marker must never lag reality), and
  stepping output is never paced — pacing applies to free-running play
  only.
- **Tags toggle.** A Player toolbar toggle shows each line's delivered
  tags (`OutputLine.tags`) as muted mono chips after the line text.
  Off by default; persisted UI state.
- **Line-row boundaries.** Every transcript row carries a subtle
  always-on treatment (alternating ~2.5% row tint) so the boundary of
  each delivered line — the runtime's delivery unit and the debugger's
  stepping unit — is visible at a glance; hover strengthens the band
  full-width and carries F9's provenance affordance.

## 3. The rebuilt Player

Still an editor-area document (two-up with the source, per the Inky
lineage) — the rebuild changes its anatomy, not its address:

- **Toolbar** (left→right): Run ▶(compile+start) · Restart ⟳ · transport
  cluster (Pause ⏸ / Continue ▶ when paused · Step Over · Step Into · Step
  Out — the cluster renders only when the provider has the `debug`
  capability, disabled-not-hidden while running) · **Auto toggle**
  (fast-forward icon, F13) · **tags toggle** (F13) · spacer · **status
  chip** (playing / paused at `file:line` / parked / ended / out-of-sync —
  the single home of F7's stop reasons) · Maximize. The chip's state set
  starts at **ready** — the story never plays by default (RULED
  2026-08-29); Run compiles and starts the session, and clicking the
  chip reveals the current line in the editor.
- **Transcript**: the existing screenplay rendering, plus F13's line-row
  boundaries and tag chips, F9's provenance affordance, an unobtrusive
  marker on the line where execution is paused, and auto-scroll that
  suspends when the author scrolls up (rebuild housekeeping).
- **Choices**: unchanged presentation; shared between play and debug.
- Absorbed Player follow-ups: #3165 (Hide/Show persistence), #2795
  (narrow-tier route back to a closed player), #2796 (closed-tab layout
  memory for the other singletons — decide during build whether it
  generalizes).

## 4. The Debugger panel (StateView replacement)

Replaces StateView in its strip slot (right dock, `state` id retired or
reused — keep the Mod-N slot stable). Sections, top to bottom:

1. **Header**: status line mirroring the Player chip + flow name;
   header-actions: transport mirror (small pause/step icons) so stepping
   works with the Player hidden.
2. **Frames** — interactive call stack (F5).
3. **Variables** — Locals (selected frame) then Globals (F6).
4. **Breakpoints** (F2).
5. **Story** (collapsed group): Pending choices · Visit counts (with the
   existing filter) · RNG — the old StateView's inspection content,
   retained but demoted.

Placeholders keep today's honesty: no session → "No story session" +
start; no debug info → names the App setting.

## 5. Component-change inventory

| Change | Where | Nature |
|---|---|---|
| Flip debug-info default on; App-settings toggle; acceptance-gate re-baseline; perf-HUD measurement | `crates/brink-web/src/editor/mod.rs`, `brink-lsp`, `brink-ide`, `acceptance_gate.rs`, settings UI | modify |
| Export source→program: `definition_id_for_path`, `resolve_address`, #3246's inverse resolver; add a `resolveSourceLine(file, line)` binding for breakpoint binding | `crates/brink-web/src/session.rs` + `story_runner.rs`, `packages/wasm`, `wasm-types` | add |
| Register `program` + `session` location resolvers with the `sessionDegraded` gate | `packages/brink-studio/src/mount.tsx` (~10 lines; resolvers already exported) | wire |
| Breakpoint gutter plumbing: markers merge into the play gutter's column (extend `play-from-here.ts` or point host-marker rendering at its slot — ruled 2026-08-29, no parallel column); new `DocumentSessionsOptions` callback + refresh fan-out on store change; "Set breakpoint here" in the gutter context menu | `packages/ink-editor/src/play-from-here.ts`, `document-sessions.ts`, `packages/brink-studio` | wire |
| New execution-highlight CM6 extension (effect-driven StateField, `hir-overlay.ts` pattern) | `packages/ink-editor/src/` (new file) | add |
| Breakpoint source-anchor model: range-keyed store state, rebind-on-compile, CM6 change mapping, per-project persistence, bound/hollow status | `packages/studio-store/src/slices/debug.ts` | extend |
| Drive-loop unification: Player advance routes through `debugRun` when breakpoints armed/paused; `DebugRunOutcome` ↔ `Step` coherence; pause verb | `packages/studio-store/src/session/local-provider.ts`, session slice | modify (load-bearing) |
| Player rebuild: toolbar transport, status chip, transcript provenance, auto-scroll | `packages/studio-ui/src/PlayerPane.tsx` (rewrite) | replace |
| Debugger panel | `packages/studio-ui/src/StateView.tsx` → new component (keep `DebugValueView`, `FrameLocals` internals where they fit) | replace |
| Frame-selection state (`selectedFrameIdx`), reveal-on-stop, highlight publication | session/debug slices + `mount.tsx` | add |
| Keybindings on `debug.*`/`story.*` descriptors (F3's table) | `packages/brink-studio/src/debug-commands.ts`, `story-commands.ts` | modify |
| Status bar paused state | `packages/studio-ui/src/StatusBar.tsx` | modify |
| `stepi` + frame-follow in Program Explorer | `packages/studio-ui/src/ProgramView.tsx` | modify |

Changesets: every one of these lands under the `@brink-lang/studio`
changeset rule; the wasm exports additionally need `@brink-lang/web`
changesets.

## 6. Work breakdown (proposed tickets, in dependency order)

1. **W1 — Debug info on by default** (#3230 becomes this): flip the three
   hardcoded sites, App setting, acceptance-gate re-baseline in lockstep,
   perf-HUD before/after numbers in the PR. *Proof:* real
   `EditorSession`-built session resolves a position non-null; gate green
   with zero diagnostic drift.
2. **W2 — Source→program wasm exports** (unblocks breakpoint binding).
   *Proof:* Rust test over `WebSession` binds `file:line` → address on
   both surfaces.
3. **W3 — Resolver registration + degraded gate** (tiny; unblocks all
   reveals). *Proof:* vitest — program location reveals source; degraded
   suppresses.
4. **W4 — Breakpoint model + gutter** (#3233's first half): store
   anchoring/rebinding/persistence + editor gutter on both surfaces.
   *Proof:* vitest incl. edit-above-moves-marker, no-statement-line
   snapping, degraded hollow state.
5. **W5 — Drive-loop unification + pause** (the load-bearing one):
   interleaved play→breakpoint→step→continue→choice→terminal with a
   coherent transcript. *Proof:* store-level vitest against a real
   compiled story; mirrors a `.dbg` golden where possible.
6. **W6 — Execution highlight** (#3233's second half): editor extension
   with live-line tracking during play (follows F13's reveal cadence) +
   paused arrow/band + reveal-on-stop + frame tint. *Proof:* both
   surfaces + degraded suppression + the highlight advancing across a
   multi-line reveal, the `program-view-current-position.test.tsx`
   standard.
7. **W7 — Player rebuild**: toolbar/transport/status chip/transcript
   provenance/auto-scroll + F13 (auto toggle button, paced reveal + its
   App setting, tags toggle, line-row boundaries) + absorbed follow-ups.
   *Proof:* vitest over commands/capabilities; provenance jump test;
   paced-reveal queue flushes instantly on a breakpoint/pause.
8. **W8 — Debugger panel**: the StateView replacement. *Proof:* frame
   selection drives locals + reveal; placeholder states.
9. **W9 — Program Explorer additions**: `stepi`, frame-follow, reveal-in-
   explorer.
10. **W10 — Keybindings + status bar + palette polish.**

Rulings/tickets referenced but *not* absorbed: #3225 (parked position —
needed before W5 can present parks; F11 carries the proposed answer),
#3223 (multi-flow runtime seam), #3222 (conditional breakpoints), #57
(value editing), Story Graph rebuild (the other half of #3199).

## 7. Open questions

1. **#3225** — parked `debug_position` semantics. Proposed answer in F11;
   needs the maintainer ruling recorded in `docs/debugger-spec.md` §4
   before W5 ships park presentation.
2. **Transcript provenance plumbing** (F9): `block_id` → source is believed
   cheap via the line tables; verify the wasm surface during W7 and demote
   F9 to a follow-up ticket if it isn't.
3. **Keybinding set**: F-row proposed (desktop-first per the ruled
   consumer); web embeds may need Mod-based alternates — the keymap
   override service already covers per-user remapping.
4. **Follow-execution scrolling**: the live highlight (F4) never scrolls
   the editor by default; whether a "follow execution" toggle (viewport
   tracks the live line) is wanted, and where it lives, is an open knob
   for W6/W7 — the chip-click reveal is the committed baseline.
