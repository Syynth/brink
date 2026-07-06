# Editor Round 2 — celeris feedback → 0.8.0

Plan of record for the second `@brink-lang/*` release round, triaged 2026-07-05 from the
celeris-filed issue batch (#362–#371) plus earlier embedder asks (#343, #347, #276).
Companion to `docs/editor-epic-plan.md` (round 1 / 0.7.0) and the 2026-07-05
decision-log entry "Editor round 2 (celeris feedback): design-first, single 0.8.0 release".

## Rulings (locked)

- **One release: 0.8.0.** Everything in this round ships together; design-epic
  *implementations* are in scope, not deferred.
- **Design-first gating.** Any build item whose public shape a design round could affect
  is held until that design is approved. Design-independent items build immediately.
- **Snapshots are session methods.** #371's state-snapshot + semantic-diff exposure is
  designed as part of the #370 Story Session API (`session.snapshot()`, `diff(a, b)`),
  not exposed off current internals.
- **Open class taxonomy.** #363's class/attribute contract documents the naming *scheme*
  (string-keyed element kinds with a documented core set), not a closed enum list —
  keeps #368 dialect-added element types non-breaking.
- **#362 (line-fit metrics) is parked** — needs #366, the #368 manifest-trajectory
  answers, and celeris-side display metrics. Not in 0.8.0.
- **#351 closed** — decomposed into #369/#370/#371 (binder undo-stack not re-filed).

## Design track (critical path)

Two parallel design fan-outs, 3 competing API proposals each + adversarial critique,
distilled into a comparison doc for user rulings. Implementations start only after approval.

| Epic | Subject | Key questions the fan-out must settle |
|---|---|---|
| #370 | Public Story Session primitive (`@brink-lang/web`) | Replay-divergence as a first-class result (replayed / diverged-at-step / failed); externals during replay (record-and-replay vs live re-invoke — lean record, mirror `FlowInstance::advance()`/`AwaitingExternal`); snapshot/diff methods (#371 half); studio's `LocalSessionProvider` migrates onto it |
| #368 | Dialogue-dialect system | Custom element types (string-keyed extension vs closed enum); data-first transition table; V1 = mount-time config, validate (don't build) manifest trajectory; what the runtime/plugin side consumes; review the shapes #364/#365 emit so the dialect design doesn't immediately rename them |

## Build track

### Wave A — design-independent, builds now

| Issue | Work | Area |
|---|---|---|
| #369 | Re-export `CompileResult`; publish `sortDiagnostics` (file → offset → errors-first) + `lineColAt` | TS (editor/web) |
| #363 | Theme opt-out on extensions builder; inline-style sweep (~21 uses / 7 widget files); class taxonomy documented as **open scheme** contract | TS (editor) |
| #364 | `data-option-path` (+ `data-option`) on Choice/ChoiceBody lines, nested weaves first-class | TS (editor, element post-pass) |
| #367 | `inlineMarkup(rules)` extension — content-region-scoped, host-registered patterns, RMMZ angle-tag preset; zero rules by default | TS (editor) |
| #343 | Host gutter-marker contribution hook (`getGutterMarkers`/`onGutterMarkerClick`) in `BrinkStudioOptions`, coordinated with built-in gutters | TS (editor) |
| #347 | `viewState(docKey, groupId?)` getter + `restoreViewState(...)` on `DocumentSessions` — works for unmounted cached slots | TS (editor) |
| #276 | Replace `color-mix()` (~48 uses) with Chromium-88-safe fallbacks (precomputed rgba / opacity layers) | TS/CSS (editor+studio theme) |
| #371-graph | Story-graph model as HIR query (`brink-ide`), wasm-exposed via `brink-web`: knots/stitches as nodes, typed edges with source spans; deterministic ordering | Rust + wasm + TS |

### Wave B — held for #368 (dialect) design approval

| Issue | Why held |
|---|---|
| #365 | Fold kinds' machinery/narrative classification and pill cast-names sit on the line classification the dialect generalizes |
| #366 | Lines-table exposure is neutral, but `characterName()` is "the dialect's extractor" — ships dialect-shaped |

### Wave C — held for #370 (session) design approval

| Issue | Why held |
|---|---|
| #370-impl | The session primitive itself |
| #371-snapshots | Snapshot/diff are session methods per ruling |

## Process

Same machinery as round 1: per-issue worktree build agents → 3-lens adversarial review
(correctness / API-contract-consumer / regression) → fix → PR with auto-merge on green CI,
serial merge-train under branch protection. Changeset per npm-affecting PR (minor bumps
collapse to 0.7.0 → 0.8.0). Design fan-outs run concurrently with Wave A.

## Status

- [x] Triage + rulings (2026-07-05)
- [x] #351 closed (decomposed), #362 parked
- [x] Design fan-outs #370 + #368 — comparison docs in `docs/design/`
- [x] Design rulings (2026-07-05): session journal **Rust-canonical** (bevy first-class);
      dialect **tooling-only, never runtime-delivered** — both logged in decision-log
- [x] Implementation specs: `docs/story-session-spec.md`, `docs/dialect-spec.md`
- [x] Wave A: 6/9 merged (#379 deny fix, #381/#374/#376/#375/#372); #373/#377/#378 green-pending.
      Workflow was killed mid-review (usage-limit recalibration — see
      `feedback_lean_agent_usage` memory); 13 completed reviews' findings recovered from
      transcripts and fixed inline; #377/#378 reviewed inline by hand
- [x] Intl × dialect-affixes follow-up filed (#383); quick-xml 0.41 = Dependabot #356 (kicks
      #380 when green)
- [x] Wave A complete (9/9 merged)
- [x] Waves B & C complete via the autonomous-pump workflow (sonnet builds, opus adversarial
      reviews, serialized train): #386 dialect core, #394 dialect editor + enum→string cut,
      #385 session core, #389 StorySessionHandle, #393 onJournalDirty, #392+#396 session docs,
      #398 custom-dialect completeness (#395), #399 lines table + DialectParser (#366),
      #400 fold kinds (#365), #401 studio session migration (#388)
- [x] Scope-reconciliation follow-ups filed: #390/#391 (rolled in + shipped), #395 (rolled in +
      shipped), #383 intl, #397 scripting epic (parked), #402 CI wall-clock, #403–#409 cleanup
- [x] **npm 0.8.0 published** (2026-07-06): @brink-lang/editor, @brink-lang/web,
      @brink-lang/studio — via #382 (note the bot-push CI gotcha now in docs/publishing.md)
- [ ] Crates release: blocked on a fresh `RELEASE_PLZ_TOKEN` (PAT expired 2026-07-01);
      then release-plz refreshes #349 → audit → merge → crates.io publish
