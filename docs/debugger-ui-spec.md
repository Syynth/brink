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
- **Break-on-write data breakpoints are IN scope** (revised 2026-08-29
  — F6/W18; D8's `debug_run_watching` supplies the runtime half) and
  list here with a distinct glyph. Conditional (expression) breakpoints
  remain out of scope (v2, runtime ticket #3222).

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
| Continue | Run until the next **content line** is delivered (or breakpoint/choices/terminal), then resume normal play — RULED 2026-08-30, revising this row's original free-run wording; the free-run remains `debug.run` (FF/auto's verb) | F5 |
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

### F6 — Variables (editing RULED 2026-08-29)

Locals-first: the selected frame's locals (by name, from D7's table) are
the top variables section, with the existing tri-state honesty ("no debug
info for this frame" / empty / table). Globals below, keeping the
changed-since-last-step diff highlight. Values render through the existing
`DebugValueView` union.

**Live editing** — the editing half of #57, pulled into this round:
click a value → inline mono input in place; Enter commits, Esc cancels;
parse/type-checked against the value's current type (red shake, no write
on failure); the changed-highlight lights the row on commit. **Scalars
only in v1** (int/float/bool/string; lists/structs read-only until their
own editor design). **Paused only** (RULED — chosen over
globals-anytime for the simpler model). Globals commit via the existing
`Story::set_variable` (wasm exposure needed); locals need a new
debug-seam set-temp-in-frame. Edits must take the dirty-marking write
path so watchpoints and parked wake checks observe them. No undo; fork
a save first (F17).

**Break-on-write**: variable-row context menu → "Break on write" — a
data breakpoint on that global, listed in the Breakpoints section with
a distinct glyph. Runtime half already shipped (D8's
`WatchpointObserver`/`debug_run_watching`); this is UI + bridge glue.

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

### F8 — Hot reload, with staleness as the fallback (REVISED 2026-08-29)

**Edits during play reach the running Player.** On every successful
compile, the live session migrates to the new program automatically:
snapshot durable state (F17's `SaveState` boundary) → swap programs →
reload → re-anchor breakpoints (F2 already re-binds) → surface the
`LoadReport` inline when anything dropped ("reloaded — 2 anonymous
visit states dropped"). Migration lands at turn boundaries — waiting on
a choice, paused, parked, between paced reveals. UI is deliberately
minimal: a brief "reloaded" affirmation in the status chip, the report
banner only when lossy. This supersedes live-inspector-spec §5's
every-edit-degrades posture (that section needs the supersession
recorded, per the D8 precedent).

**Degraded mode remains, as the fallback**: a failing compile keeps the
old program running with the error surfaced, and a migration that
cannot preserve the current position drops to the existing
"out of sync" state — highlights suppressed, gutter dots hollow,
restart re-syncs. Suppressed-never-stale still governs every source
mapping in the window between edit and successful migration.

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

### F11 — Parked flows (RULED 2026-08-29 — #3225 resolved)

The UI treatment: disabled stepping + "parked — resumes here" frame/flow
label + condition-park vocabulary per debugger-spec §4 — never "waiting
for a value". The position semantics are now **ruled**
(`docs/debugger-spec.md` §4.1): while parked, `debug_position` reports
the **resume point** (`(continuation container, 0)`, just past the park
statement) with an explicit parked tag; while awaiting a deferred
external, the **call site** with a distinct awaiting-external tag. The
tags are API-level, so no consumer can render either as "currently at".
Implementation rides the #3215 `#[non_exhaustive]` fix and sequences
against FS-3r (#980). W5's park presentation is unblocked.

### F12 — Multi-flow (selection surface ruled; runtime deferred)

The runtime seam is default-flow-only (#3223). **The Debugger panel's
Flows section is the selection surface (RULED 2026-08-29)**: the list of
open flows/sessions lives in the panel above Frames — the status bar's
`SessionPicker` retires, the status bar keeps only the one-line story
status. Selecting a flow scopes Frames/Variables and the transport to
it; a parked flow shows "parked — resumes here" in this list (its
Frames view shows the resume frame), never as a pseudo-frame in another
flow's stack. Per-flow breakpoint filtering and cross-flow stepping wait
for #3223's runtime work; nothing in this design forecloses it.

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

### F14 — Choice-point visualization (RULED 2026-08-29)

When the session reaches `Step::Choices`, the status chip reads
**"Waiting on choice"** and the editor lights the choice point:

- Every **presented** choice's line gets the success-tint band (they are
  the live frontier — the possible next lines).
- Authored choices that were **not added to the block** render dimmed
  with the reason beside them: the failing condition (`gold > 20 =
  false`) or `once-only · used`.
- **No new runtime seam (REVISED 2026-08-29, same session)** — the
  reasons derive by elimination from surfaces that already exist:
  the presented set (`DebugState.choices`) + per-container visit counts
  (`DebugState.visits`, since choosing increments the body container's
  count — once-only exhaustion IS a visit count) + the overlay
  projection's container ids for anonymous choice bodies (#3234, which
  is exactly the identity join needed). Not presented + once-only +
  body count ≥ 1 → "used"; not presented otherwise → condition failed.
  Two honesty notes: the condition-failed bucket is by-elimination (a
  catch-all — thread-gathered and fallback edge cases belong in W11's
  proof list, not behind an always-exact label), and W11 must **verify
  `DebugState.visits` carries anonymous choice-body containers** — if
  the snapshot filters to named containers, widening that filter is the
  only bridge change needed.

### F15 — Runtime-value hover (RULED 2026-08-29)

While a session is live and in-sync, hovering a variable in the editor
adds its **current runtime value** to the existing hover: globals
always, frame locals while paused in that frame's scope. Studio-side
merge (existing hover identifies the symbol; `DebugState`
globals/locals supply the value) — no new wasm surface expected.
Suppressed under `sessionDegraded` like every position feature.

### F16 — Player appearance settings (RULED 2026-08-29)

The Player gets its own appearance section in App settings, starting
with **font size** — its own knob on the `--bs-editor-font-size`
precedent (the reading surface's size is not the UI's size), separate
from the app type scale. Room to grow (line spacing, face) without
re-ruling.

### F17 — Runtime save/load for testing (RULED 2026-08-29)

Game-style checkpoints so the author can get somewhere in the story and
keep testing from there:

- **Payload**: the runtime's existing `SaveState` boundary — visit
  counts, globals, position: durable story state, never
  internal/ephemeral runtime state. No new format; the Rust↔TS DTO
  parity tripwire already covers `SaveState`.
- **Surface**: the Player's **idle body** is the launcher (RULED
  2026-08-29, canvas round): "Run from the start" beside a
  **combobox/typeahead over knots and stitches** (file locations as
  context — KNOT/STITCH kind chips in the landing recents-row
  vocabulary) that plays from there, reusing the play-from-here start
  path (#186) and the symbol query; below, **two stacked sections in
  the landing screen's Recent-list style** (uppercase cap over the
  bordered recents row list — `.landing-cap`/`.recent-projects`,
  maintainer screenshot): **PROJECT** (project saves) and **THIS
  COMPUTER** (machine saves). Rows follow the recents anatomy — small
  mono chip (turn count; amber `OLD` for an older compile), save name,
  right-aligned muted context (knot path · age). While a session runs,
  "Save state" captures the current point into the default location.
- **Load vs Fork (RULED 2026-08-29)**: every save offers both. *Load*
  attaches the session to the slot — "Save state" writes back to it,
  like continuing a save file. *Fork* starts from a copy — the session
  is unattached and the next save picks a new slot, leaving the
  checkpoint untouched (branch experiments without clobbering it).
- **Location**: both stores are first-class and always visible as the
  two doors; the App setting picks the **default target for new saves**
  — *local* (private app-data folder, per project) vs *project* (inside
  the project tree, e.g. `.brink/saves/`, shareable/committable);
  extensible. Desktop-first via the Tauri host callbacks; the web
  embed's fallback store is a W14 build question.
- **Compat honesty**: loading against a newer compile surfaces the
  runtime's `LoadReport` inline ("loaded — 3 anonymous visit states
  dropped"), never a silent load. The #3283/#3234 block-local identity
  work is what makes saves survive ordinary editing; a save whose
  program checksum matches loads clean.
- Re-scopes **#57**: the save/restore half lands here; editable runtime
  state stays out of this round.

### F18 — Watch: the full mini-REPL (RULED 2026-08-29)

A Watch section in the Debugger panel — **the full mini-REPL, not
expressions-only**, because the engine is already wired end-to-end
(verified this round; the scratch-eval-spec's "never landed" provenance
note is stale for everything this needs): F4.1–F4.3 shipped
`Speculation`/`KindTieredHandler` and the web `speculate()` surface,
and F5.1 shipped tier-1 fragment evaluation (`compile_fragment`,
mechanism B — synthetic-symbol wrap + cached recompile per
`(checksum, fragment, kind)` + live-state seed + sandboxed run), proven
by wasm tests through the real `evaluate()` export.

- **Entries**: an arbitrary typed expression (`gold >= pour(2)` →
  `false`) or a divert/content fragment (`-> market.haggle` → an
  expandable transcript preview of what it *would* produce from the
  current state). Every evaluation is side-effect-proof
  (discard-on-drop sandbox, budgeted).
- **Cadence**: re-evaluated at every stop/turn boundary; the fragment
  compile is paid once per distinct entry per program version (hot
  reload re-keys the cache once per watch).
- **Failure**: a fragment that doesn't compile or errors shows its
  message inline on the row; degraded suppresses re-evaluation like
  every position feature.
- Externals follow the shipped `@kind` tiering (queries live; effects
  fallback-or-stop in watch context).

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
- **Idle body**: the saves launcher (F17) — Run from the start, or load
  a checkpoint and continue testing from there.
- Absorbed Player follow-ups: #3165 (Hide/Show persistence), #2795
  (narrow-tier route back to a closed player), #2796 (closed-tab layout
  memory for the other singletons — decide during build whether it
  generalizes).

## 4. The Debugger panel (StateView replacement)

Replaces StateView in its strip slot (right dock, `state` id retired or
reused — keep the Mod-N slot stable). Sections, top to bottom:

1. **Header**: status line mirroring the Player chip; header-actions:
   transport mirror (small pause/step icons) so stepping works with the
   Player hidden.
2. **Flows** — every open flow/session, active one highlighted; parked
   flows carry their "resumes here" state here (F12, ruled 2026-08-29 —
   replaces the status bar's `SessionPicker`). Selection scopes
   everything below plus the transport.
3. **Frames** — the selected flow's interactive call stack (F5).
4. **Variables** — Locals (selected frame) then Globals, inline-editable
   while paused (F6).
5. **Watch** — the mini-REPL entries with live values / transcript
   previews (F18); "+" in the section header adds one.
6. **Breakpoints** (F2) — program-wide, not per-flow; also lists
   break-on-write data breakpoints (F6).
7. **Story** (collapsed group): Pending choices · Visit counts (with the
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
| Status bar paused state; `SessionPicker` retires (flow list moves into the Debugger panel, ruled 2026-08-29) | `packages/studio-ui/src/StatusBar.tsx` | modify |
| `stepi` + frame-follow in Program Explorer | `packages/studio-ui/src/ProgramView.tsx` | modify |

Changesets: every one of these lands under the `@brink-lang/studio`
changeset rule; the wasm exports additionally need `@brink-lang/web`
changesets.

## 6. Work breakdown (proposed tickets, in dependency order)

1. **W1 (#3294) — Debug info on by default** (#3230 becomes this): flip the three
   hardcoded sites, App setting, acceptance-gate re-baseline in lockstep,
   perf-HUD before/after numbers in the PR. *Proof:* real
   `EditorSession`-built session resolves a position non-null; gate green
   with zero diagnostic drift.
2. **W2 (#3295) — Source→program wasm exports** (unblocks breakpoint binding).
   *Proof:* Rust test over `WebSession` binds `file:line` → address on
   both surfaces.
3. **W3 (#3296) — Resolver registration + degraded gate** (tiny; unblocks all
   reveals). *Proof:* vitest — program location reveals source; degraded
   suppresses.
4. **W4 (#3297) — Breakpoint model + gutter** (#3233's first half): store
   anchoring/rebinding/persistence + editor gutter on both surfaces.
   *Proof:* vitest incl. edit-above-moves-marker, no-statement-line
   snapping, degraded hollow state.
5. **W5 (#3298) — Drive-loop unification + pause** (the load-bearing one):
   interleaved play→breakpoint→step→continue→choice→terminal with a
   coherent transcript. *Proof:* store-level vitest against a real
   compiled story; mirrors a `.dbg` golden where possible.
6. **W6 (#3299) — Execution highlight** (#3233's second half): editor extension
   with live-line tracking during play (follows F13's reveal cadence) +
   paused arrow/band + reveal-on-stop + frame tint. *Proof:* both
   surfaces + degraded suppression + the highlight advancing across a
   multi-line reveal, the `program-view-current-position.test.tsx`
   standard.
7. **W7 (#3300) — Player rebuild**: toolbar/transport/status chip/transcript
   provenance/auto-scroll + F13 (auto toggle button, paced reveal + its
   App setting, tags toggle, line-row boundaries) + absorbed follow-ups.
   *Proof:* vitest over commands/capabilities; provenance jump test;
   paced-reveal queue flushes instantly on a breakpoint/pause.
8. **W8 (#3301) — Debugger panel**: the StateView replacement. *Proof:* frame
   selection drives locals + reveal; placeholder states.
9. **W9 (#3302) — Program Explorer additions**: `stepi`, frame-follow, reveal-in-
   explorer.
10. **W10 (#3303) — Keybindings + status bar + palette polish.**
11. **W11 (#3304) — Choice-point visualization** (F14): presented-line highlight
    from `DebugState.choices`; rejection reasons derived by elimination
    from visits + presented set + #3234's container ids (no new runtime
    seam — verify anonymous choice bodies appear in `DebugState.visits`
    first). *Proof:* a choice point with a false-condition choice and an
    exhausted once-only, both dimmed with the right reason;
    thread-gathered and fallback edge cases; degraded suppression.
12. **W12 (#3305) — Runtime-value hover** (F15): studio-side hover merge.
    *Proof:* global + frame-local hover values, gone when degraded or
    no session.
13. **W13 (#3306) — Player appearance settings** (F16): font-size knob in the
    App settings Player section, wired to the Player's prose styles.
14. **W14 (#3307) — Runtime save/load** (F17): saves store (machine app-data /
    project tree per the App setting), idle-Player launcher UI, save
    while running, `LoadReport` surfacing. Decide the web embed's
    store during build. *Proof:* save → load → identical
    `DebugState`; load after a recompile surfaces the report; the
    location setting actually moves where files land.
15. **W15 (#3308) — Hot reload** (F8): auto-migrate the live session on every
    successful compile (save → swap → load → re-anchor, at turn
    boundaries); degraded mode demoted to fallback; record the
    live-inspector-spec §5 supersession. Builds on W14's machinery —
    sequence after it. *Proof:* edit a line mid-play → the next reveal
    shows the new text with no restart; visit counts/globals survive;
    a lossy migration surfaces the report; a failed compile keeps the
    old program running with the error shown; an unmigratable position
    falls back to out-of-sync instead of guessing.
16. **W16 (#3309) — Value editing** (F6): expose `set_variable` through wasm;
    new debug-seam set-temp-in-frame (+ its wasm binding); inline edit
    UI, paused-only gating, dirty-path verification (a parked condition
    reading an edited global re-evaluates; a watchpoint on it fires).
    *Proof:* edit → continue → story reflects it; type-mismatch
    refused; locals edit visible in the frame.
17. **W17 (#3310) — Watch section** (F18): UI over the shipped `evaluate()` —
    entry management, re-eval on stop, expandable fragment previews,
    inline errors. *Proof:* expression + fragment entries against a
    live session; side-effect-proofness (watch eval leaves
    `DebugState` untouched); degraded suppression.
18. **W18 (#3311) — Break-on-write UI** (F6): variable-row context menu, bridge
    `debug_run_watching` into the drive loop, Breakpoints-section rows
    with the data-breakpoint glyph. *Proof:* write to a watched global
    pauses at the writing line.

Rulings/tickets referenced but *not* absorbed: #3225 (parked position —
needed before W5 can present parks; F11 carries the proposed answer),
#3223 (multi-flow runtime seam), #3222 (conditional breakpoints), #57
(value editing), Story Graph rebuild (the other half of #3199).

## 7. Open questions

1. ~~**#3225** — parked `debug_position` semantics~~ — **RULED
   2026-08-29** (resume point / call site, tagged; recorded in
   `docs/debugger-spec.md` §4.1 and the decision log). No longer open.
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
