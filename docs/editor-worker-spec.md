# Editor worker architecture — the split model (option B)

**Status: IMPLEMENTED** (W1–W5 landed 2026-08-25; PRs #3102–#3109 plus
the W5c close-out). Ruled 2026-08-24 (decision log, "Editor background
architecture: split worker (option B), one architecture for web +
desktop, spec first"); the doc-road completeness ruling is decision log
2026-08-25. Sections carry **as-landed** notes where implementation
refined the plan; §14 records the endgame deltas.

## 1. Problem

The #3064 perf program made the keystroke path *usually* fast (~6–7 ms
instrumented on a 6k-line document) but the main thread is still where
every wasm query runs. What remains on the UI thread when it fires:

- the **deferred refresh** (~120 ms quiet): refined semantic tokens, HIR
  overlay, inlay hints, argument widgets, fold ranges, occurrences;
- **diagnostics**: a 500 ms-debounced `compileProject` in
  `packages/ink-editor/src/diagnostics.ts` — the single largest pull;
- **interactive queries**: completion, hover, signature help, code actions;
- **panel pulls**: outline, story graph, problems, binder queries;
- **structural ops**: rename/move/promote/demote and their breakage gates;
- **cold pulls**: the first analysis after boot or a config swap.

None of these land on keystrokes anymore, but any of them can eat frames
mid-scroll, mid-IME, or mid-animation whenever it fires. The goal of this
package is to make the main thread **structurally unable to block on
project analysis** — not just usually fast.

## 2. The ruled architecture

Two wasm instances with a hard capability boundary:

```
┌────────────────── main thread ──────────────────┐
│ CM6 views · decorations · panels                │
│                                                 │
│ ClassifierSession (NEW, capability-stripped)    │
│   open doc(s) only · segments · lex/parse/lower │
│   classifier tokens · line contexts             │
│   ← same-frame styling on every keystroke       │
│                                                 │
│ SessionClient (async RPC facade)                │
└───────────────┬─────────────────────────────────┘
                │ postMessage: edits ↓ / tagged results ↑
┌───────────────┴──────────── worker ─────────────┐
│ EditorSessionHandle (full project session)      │
│   salsa ProjectDb · analysis · diagnostics      │
│   compile · refactors · panels · everything     │
│ Scheduler: edits > interactive > background     │
└─────────────────────────────────────────────────┘
```

- The **worker owns the truth** for everything project-flavored. All of
  its results arrive asynchronously and land through the effect machinery
  the deferred-refresh work already built.
- The **main thread keeps a minimal classifier instance** so newly typed
  text is styled in the same frame by the real lexer (measured ~0.13 ms
  warm for the token walk). This is the moral equivalent of VS Code's
  synchronous TextMate layer — except it cannot be wrong about token
  boundaries, because it *is* the lexer.
- **One architecture everywhere**: browser playground, embeddable studio,
  and the Tauri desktop webview all run the same worker. No
  SharedArrayBuffer, no COOP/COEP requirement on embedders, no
  desktop-native fork.

### What was rejected (and why it stays rejected)

- **Pure worker (option A)** — with no synchronous layer underneath,
  fresh tokens render unstyled for a frame or two; a fast typist sees
  bare-then-styled flicker. VS Code tolerates async *semantic* tokens only
  because a synchronous grammar sits under them.
- **wasm threads / SharedArrayBuffer** — requires COOP/COEP headers on
  every page that embeds the studio (someone else's server config, per the
  embeddable-studio contract), and salsa-under-wasm-threads is unproven.
- **Desktop-native (Tauri IPC to a Rust-side session)** — forks the
  web and desktop architectures for marginal gain. May be layered later
  behind the same `SessionClient` interface if desktop ever wants process
  isolation.

## 3. What already exists (the protocol is mostly built)

The #3064/#3084 work delivered, deliberately message-shaped:

| Piece | Where | Role in this spec |
|---|---|---|
| Edit-span ingress `applyEditsDocument` | `crates/brink-web/src/editor/doc_handles.rs`, wrapper in `packages/wasm/src/index.ts` | The worker-bound edit message body |
| Segment manifest (`index:generation` identity keys) | `ProjectDb::segment_manifest` → `getSegmentManifestDoc` | The version vector for doc-scoped results |
| Per-segment owned slices (contexts, tokens, classifier tokens) | `getSegment{LineContexts,SemanticTokens,SemanticTokensFast}Doc` | Bounded response payloads |
| Config epoch | `packages/wasm/src/index.ts` (`configEpoch()`), `DocHandle.sliceEpoch` | Invalidates caches across dialect/manifest swaps |
| Version-keyed slice cache + rebase assembly | `packages/ink-editor/src/document-handle.ts` | Becomes the main-thread materialized view of worker state |
| Deferred-refresh effects (`deferredRefresh`, per-consumer refresh effects) | `packages/ink-editor/src/deferred-refresh.ts` + consumers | The landing sites for async results |
| One choke point for all wasm calls (`withPerfTiming` proxy in `ProjectSession`) | `packages/ink-editor/src/project-session.ts` | Precedent: the RPC facade slots in at the same choke point |

What does *not* exist yet: the RPC layer, the worker host, the scheduler,
the classifier session type, and async-shaped consumers for the calls that
are synchronous today.

## 4. The classifier session (main thread)

### 4.1 Capability scope

`ClassifierSession` is a **new, separate wasm-exported type** (not a mode
flag on `EditorSessionHandle`). It holds:

- the open document(s)' text and segment substrate (`file_segments_query`
  → per-segment parse + lower — all project-independent by construction);
- the **config surface that affects classification only**: dialect
  (`setDialect`/`clearDialect` — at-cue presets change line classes),
  language dialect, and whatever `brink.toml` keys feed classification.
  Config is *pushed* to it by the same code path that pushes config to the
  worker; the config epoch covers both.

It exposes exactly:

- `applyEdits` / `updateDocument` (same delta ingress semantics);
- `getSegmentManifest`;
- `getSegmentLineContexts(key)` (per-segment lowered line contexts);
- `getSegmentSemanticTokensFast(key)` (classifier tokens).

Nothing else. No project files, no symbol index, no resolution, no
analysis, no compile, no refactors. If a keystroke-path consumer ever
needs more than this, that is a design smell to surface, not a method to
add.

### 4.2 Boundary enforcement is structural

Two guards, both required:

1. **Rust-side**: `ClassifierSession` wraps a `ProjectDb` (or a slimmer
   substrate DB) but its `#[wasm_bindgen]` surface simply does not export
   project methods. The type is the boundary.
2. **TS-side**: a repo check (same family as `check-no-nul-bytes` /
   `no-test-file-imports`) asserting that no file under
   `packages/ink-editor/src` or `packages/brink-studio/src` constructs or
   imports the *synchronous* `EditorSessionHandle` except the RPC layer
   and the explicitly allowlisted legacy shims during migration. This is
   the acceptance-gate-equivalent for the boundary: the split cannot
   silently erode back into "just call the session, it's right there."
   **Re-sequenced to W5 during W3**: mid-migration (W3–W4), nearly every
   consumer file legitimately touches the sync session, so the check's
   allowlist would be the whole tree — a guard that forbids nothing. It
   lands with the flip, when the allowlist collapses to the RPC layer +
   classifier plumbing and the check gets real teeth.

### 4.3 Cost accounting

The classifier instance duplicates the open document's text and segment
tree (not the project). Both wasm instances share one compiled module —
the module is already loaded on the main thread for the player
(`StoryRunnerHandle`, §10), so the marginal cost is one instantiation +
the open doc's substrate. The #3097 heap estimator should grow a
classifier-session line so the 2× accounting stays honest.

## 5. RPC protocol

### 5.1 Transport and framing

Plain `postMessage` with structured clone. Payloads are already bounded by
the delta protocol; no `SharedArrayBuffer`, no transferables (a §5.4
sufficiency requirement, not just an initial simplification), and no
structured-clone-only types (`Map`, `Set`, typed-array views as semantic
payload): **every protocol payload is JSON-serializable**, so postMessage
and a byte stream carry identical shapes.

Message shapes — **Rust is the source of truth** (serde structs, in
`brink-web` or a dedicated protocol module), with hand-maintained TS
mirrors in `wasm-types` per the existing house pattern. The TS below is
the mirror, not the definition (§5.4 explains why this direction is
load-bearing):

```ts
// main → worker
type Request =
  | { kind: "edit"; doc: DocumentId; docVersion: number; edits: EditSpan[] }
  | { kind: "push"; doc: DocumentId; docVersion: number; source: string }   // full-text fallback
  | { kind: "config"; configEpoch: number; op: ConfigOp }                   // dialect, lints, manifest, …
  | { kind: "files"; op: FileOp }                                           // add/remove/rename/external-change
  | { kind: "query"; id: RequestId; priority: "interactive" | "background";
      doc?: DocumentId; docVersion?: number; coalesceKey?: string;
      method: string; args: unknown[] }
  | { kind: "cancel"; id: RequestId };

// worker → main
type Response =
  | { kind: "ack"; doc: DocumentId; docVersion: number; applied: boolean }
  | { kind: "result"; id: RequestId; docVersion?: number; configEpoch: number; value: unknown }
  | { kind: "error"; id: RequestId; message: string }   // policy drops use a "dropped:" prefix
  | { kind: "event"; event: WorkerEvent };  // e.g. onFilesChanged egress, config warnings
```

Two W1 refinements (implemented in `crates/brink-web/src/protocol.rs`, the
source of truth): `ack.applied` is `false` for a refused mutation (a
malformed edit list, a read-only target) so the client can fall back to a
full push; and background supersession is keyed by an explicit,
client-chosen `coalesceKey` rather than `(method, doc)` — same-method
queries with different args (per-segment slice pulls) are distinct work
that a method-derived key would wrongly collapse.

- `docVersion` is a **main-thread monotonic counter per document**,
  incremented on every CM transaction that changes the doc. It is the
  staleness ruler: any `result` tagged with an older `docVersion` than the
  current one is stale.
- Segment keys (`index:generation`) remain the *content* versions inside
  a response; `docVersion` sequences the conversation.

### 5.2 The facade

`SessionClient` implements the async mirror of `EditorSessionHandle`'s
surface. It is a thin, mostly-generic proxy: `query` messages carry the
method name and args; the worker host dispatches to the real handle.
Hand-written wrappers exist only where semantics demand them (edits,
config, files — the ordered, versioned mutations).

The mutation stream (`edit`/`push`/`config`/`files`) is **strictly
ordered** and applied FIFO by the worker before anything else (§6).
Queries are unordered beyond their priority class.

### 5.3 Staleness policy (per consumer class)

- **Positional decorations** (tokens, contexts-derived line info): a stale
  result is still *landable* if the main thread maps it through the
  changes since its `docVersion` (the mapping machinery from C2 already
  does exactly this for existing decorations). Land-then-refresh; never
  block on freshness.
- **Point queries** (hover, signature, completion): stale results for a
  moved cursor are dropped — CM6's async completion sources already
  handle abandonment; hover/signature requests carry the position and are
  invalidated by cursor movement.
- **Whole-project results** (diagnostics, outline, story graph, problems):
  versionless per-doc; tagged with the *set* of doc versions the worker
  had applied. Landed as-is (these surfaces were already debounced-stale
  by design), superseded by the next result.
- **Structural ops** (rename, move): never stale-dropped — they are
  request/response commands. The UI awaits them (with the existing gated
  progress affordances) and applies returned edits through the normal
  transaction path.

### 5.4 Transport sufficiency: a hypothetical native session server

Explored 2026-08-25 (maintainer): could the Tauri app run the analysis
backend as an independent **native process** and share APIs with this
architecture? Not planned — but the boundary is required to be
*sufficient* for it, and three requirements above exist for that reason.

The finding: **the shareable thing is the session protocol and the Rust
core, not LSP-the-protocol.** `EditorSessionHandle` is already a thin
wasm binding over `brink-ide`/`ProjectDb`; a native server would wrap the
same Rust query surface and speak the same session protocol over a
sidecar's stdio (the natural home being a hypothetical `brink ide serve`
subcommand — the CLI already ships in the desktop bundle and already has
the scriptable `ide` family). brink-lsp stays what it is: a *different
wire language* (LSP, for foreign editors) over the same `ProjectDb`
internals. LSP's document sync maps cleanly onto our mutation stream
(both are versioned incremental UTF-16 edits), but most studio surfaces —
segment manifests with salsa identity versions, config epochs, line
contexts, argument widgets, story graph, compile-to-`StoryData`,
gated structural ops — have no LSP vocabulary; tunneling them as
`brink/*` custom methods would make LSP a framing tax around this
protocol with zero foreign-editor interop in return. Three consumers,
one core: wasm binding (web/worker), session-protocol server
(hypothetical native), LSP (foreign editors).

What this section binds the rest of the spec to:

1. **Rust source of truth for protocol shapes** (§5.1) — a TS-only
   protocol would force a native server to reimplement shapes by hand.
2. **JSON-serializable payloads only** (§5.1) — postMessage and a byte
   stream must carry identical shapes.
3. **The scheduler is server-side *policy*, not shared code** (§6) — the
   worker implements it as admission control because single-threaded wasm
   must; a native host may satisfy the same observable contract with real
   threads, where salsa's cancellation-on-write actually works and a
   running pull *can* be interrupted. The client must not depend on
   admission-control timing, only on the ordering and staleness contracts
   of §5.3.

A native transport, if ever built, is a third `SessionClient` transport
next to `LocalTransport` and `WorkerTransport` — consumers unchanged.

## 6. Worker-side scheduler

The worker is single-threaded and a running salsa query cannot be
interrupted, so scheduling is admission control:

1. **Drain all pending mutations first.** Before starting *any* query,
   apply every queued `edit`/`push`/`config`/`files` message. Mutations
   are cheap by construction (`update_source` is a splice; no eager
   analysis — the #3093/#3100 shape).
2. **Interactive queries next** (completion, hover, signature, code
   actions): FIFO among themselves.
3. **Background pulls last** (refined tokens, contexts refresh, overlay,
   hints, folds, diagnostics/compile, panels), and only when the mutation
   and interactive queues are empty.
4. **Coalescing**: before executing a background query, drop it if a newer
   request with the same `coalesceKey` (§5.1) is queued behind it, and
   drop it if its `docVersion` is already stale. Dropped queries are
   *answered* (`dropped:superseded` / `dropped:stale`), never silently
   swallowed. Salsa memoization makes any redundant execution cheap, but
   not executing is cheaper. Interactive queries and mutations never drop.
5. **Compile coalesces to one in flight**: a `compileProject` request that
   arrives while one is queued replaces it (the existing 500 ms debounce
   moves main-side or stays — either way at most one compile runs behind
   the freshest text).

Worst case: a cold pull or compile occupies the worker for tens-to-hundreds
of ms. During that window edits queue in the worker's inbox (the main
thread is untouched — the classifier instance keeps styling), and drain in
order when the pull returns. That is the accepted cost of single-threaded
wasm; it bounds *result latency*, never input latency.

`cancel` is best-effort: it removes queued work; it cannot interrupt a
running query. (If a genuinely interruptible pull is ever needed, salsa's
cancellation-on-write applies only across threads and is out of scope
here — noted so nobody rediscovers it mid-implementation.)

## 7. Consumer migration map

Every wasm consumer, its current shape, and its target shape. "Effect
landing" means results dispatch as the consumer's existing refresh effect.

| Consumer | Today | Target |
|---|---|---|
| Element line info (`element-type.ts`) | sync per-segment slices | **classifier instance**, unchanged shape |
| Highlight fast path (`highlight.ts`) | sync classifier slices | **classifier instance**, unchanged shape |
| Highlight refined | deferred sync pull | worker query → effect landing |
| Line contexts (assembled consumers) | sync | classifier instance (keystroke) + worker refinement where analysis-flavored |
| HIR overlay, occurrences | deferred sync pull | worker query → effect landing |
| Inlay hints, argument widgets, color hints | deferred sync pull | worker query → effect landing |
| Fold ranges | deferred sync pull | worker query → effect landing |
| Diagnostics (`diagnostics.ts`) | 500 ms debounce → sync `compileProject` | debounce → worker compile → `setDiagnostics` (shape barely changes: the callback becomes `await`) |
| Completion | sync source | CM6 **async** completion source (natively supported) |
| Hover / signature help | sync on demand | async tooltip sources with cursor-move invalidation |
| Code actions | sync on demand | async request |
| Panels (outline, story graph, problems, binder) | sync pulls from store | worker queries; stores already consume push-shaped updates |
| Structural ops (rename/move/promote/demote + gates) | sync command handlers (some idle-deferred via `pendingIdleWork`) | awaited worker commands; same gated-progress UX |
| `ProjectSession` file lifecycle (add/remove/rename/external/config discovery) | sync | ordered `files`/`config` mutations; egress events (`onFilesChanged`) flow back as worker events. **Sequenced with the W4 flip, not W2** (decided during W2e): `ProjectSession`'s lifecycle couples sync mutations to immediately-following sync reads (`updateFile` → `listFiles`/`getFileSource` in the same turn), so mutations cannot go async-ordered while reads stay sync — both sides move together when the transport flips. The W2e slice that DID land: the structural computes (`renameFile`/`renameDir`/`moveStitch`/`promoteStitch`/`demoteKnot`/`renameSymbol`/`renameSymbolAt`) ride `ProjectSession.structuralQuery` — they are compute-only (`gate_with_source`: new sources + breakage report, no session mutation; TS applies afterwards), so interactive-query semantics are exactly right and no protocol change was needed. Still direct-sync alongside the lifecycle: the reorder ops (cheap, ungated) and the inline-rename live badge query (`renameSymbolAt` via `rename-query-cache`). |
| Player / `StoryRunnerHandle` | main thread | **unchanged** (§10) |

## 8. Boot sequence

1. Main thread: `initWasm()` (already async, already required for the
   player) → construct `ClassifierSession`.
2. Spawn the worker; worker runs its own `initWasm()` (same module URL;
   the browser caches the fetch/compile) → constructs
   `EditorSessionHandle`.
3. `ProjectSession.initialize()` streams provider files as `files`
   mutations to the worker (contents cross once; the provider read stays
   main-side), then `config` (brink.toml discovery result), then opens
   documents.
4. Until the worker reports ready, consumers show the same "no data yet"
   states they already have for a fresh session (empty decorations,
   pending diagnostics) while the classifier instance styles the open doc
   immediately — first-paint styling does not wait for the project.
5. Worker crash/restart: the main thread holds the file truth (provider +
   open buffers), so recovery is re-running step 3. Non-goal to make this
   invisible in v1; surface it like a session error today.

## 9. Cross-view mirror

The mirror (`DocumentSessions`, `syncAnnotation`) stays a main-thread
concern: sibling views of one file reconcile through CM transactions as
today. Only the *canonical* per-file edit stream is forwarded to the
worker (and classifier) — mirrored transactions do not re-send (the C1
double-apply guard generalizes: anything carrying `syncAnnotation` never
egresses).

## 10. Out of scope

- **The player** (`StoryRunnerHandle`, speculation, journal): stays on the
  main thread in this package. Its call pattern is bursty request/response
  on user action, not per-keystroke. Moving it can be its own later
  package behind the same client pattern if profiling ever demands it.
- **Prose checking** was out of scope for this package because it did not
  exist when it was written; it has since been moved to a worker of its
  own — see §15.
- **brink-lsp / desktop-native transports**: unaffected; the LSP already
  runs out of process.
- **Multi-project / multi-worker**: one worker per `ProjectSession`.

## 11. Testing

1. **Protocol unit tests** (vitest, fake transport): ordering (mutations
   before queries), coalescing, staleness drops, config-epoch
   invalidation, cancel semantics. The transport is an interface so tests
   drive it synchronously and deterministically.
2. **Parity gate**: the async road must return byte-identical results to
   the direct session for the same inputs — a harness that runs every
   migrated query both ways over the corpus fixture (the same discipline
   as the segment road's `assert_roads_agree`).
3. **Real-wasm e2e**: the existing real-wasm e2e harness (the one that
   caught the C1 mirror double-apply that every mock hid) grows a
   worker-backed variant. jsdom has no workers — this must run in the
   browser harness, and it is the *only* place the actual
   postMessage/structured-clone path is exercised. Budget for it.
4. **Boundary guard**: the §4.2 TS-side check, wired into
   `pnpm test:scripts` / CI like its siblings.
5. **Perf judge**: the typing-burst scenario must not regress; a new
   long-task assertion (zero long tasks attributable to wasm on the main
   thread during a burst-with-background-refresh scenario) becomes the
   headline number.

## 12. Work items (strangler order — each lands green on main)

- **W1 — protocol + facade over a local transport.** `SessionClient`,
  message types, scheduler, with the "worker" running *in-process* on the
  main thread (a `LocalTransport` that dispatches to the real handle in a
  microtask). All the semantics, none of the worker. Parity gate lands
  here.
- **W2 — consumer migration to async, in waves** against the local
  transport: (a) diagnostics/compile; (b) deferred-refresh consumers;
  (c) interactive queries; (d) panels; (e) structural ops +
  `ProjectSession` lifecycle. Each wave is its own PR with its suite.
- **W3 — `ClassifierSession`** (Rust + wasm export + wrapper), keystroke
  consumers rewired to it, heap-estimator line, boundary check script.
- **W4 — the actual worker.** `WorkerTransport`, worker host module,
  boot sequence, crash surface. Real-wasm e2e worker variant.
  **As landed:** the worker runs the PROJECT-LEVEL road first — compile,
  outline, story graph, closure, the pulls that carry all the residual
  main-thread cost — against its own wasm session, kept current by an
  ordered file/config mutation stream (`ProjectSession.projectQuery`:
  changed-file contents + the un-replayed config-log suffix flush before
  every worker query; the scheduler's mutations-before-queries ordering
  makes the flush safe by construction). Doc-scoped queries stay
  in-process until the W5 flip, which owns doc-id replication /
  DocHandle-async. The host semantics are `SessionHostCore`, extracted
  from and shared with `LocalTransport` — the two transports cannot
  drift. Feature-detected end to end: no `Worker` global, a bundler
  that cannot process the `new URL` worker pattern, boot failure, or a
  crash all fall back to the in-process road (crash = reject in-flight,
  never retry the worker this session). Enabled by
  `MountStudioOptions.workerSession` / the playground's `?worker=1`;
  default stays off until W5. The e2e discriminator: the main thread's
  wasm perf registry records ZERO `ide.compile` in worker mode while
  the story still runs from landed compile bytes — with a control test
  pinning the same counter nonzero without the flag.
- **W5 — flip + delete.** Worker transport becomes the default in studio
  and desktop; the synchronous session on the main thread is no longer
  constructed outside the classifier + player; the migration allowlist in
  the boundary check shrinks to its final form.
- **W6 — measure + docs.** Typing-burst + background-refresh scenarios,
  long-task headline, baseline appendix, CLAUDE.md/architecture-doc
  updates, changesets.

W1/W2 are pure-TS and independently shippable with zero behavior change
(async-over-sync in a microtask). W3 is Rust-side and parallelizable with
W2. W4+ depend on all of it.

## 13. Open questions for the maintainer

1. **Fragment handles** (`open_fragment` — symbol-scoped views): served by
   the classifier instance (they are single-document by nature) or
   routed to the worker? Proposal: classifier for styling, worker for
   intelligence, same as full docs — but fragments' view-context
   choreography deserves a check during W3.
2. **Diagnostics cadence**: keep the 500 ms compile debounce main-side, or
   let the worker's coalescing replace it (compile whenever idle behind
   freshest text)? Proposal: keep the debounce initially — identical UX,
   one variable at a time.
3. **Desktop `brink-cli` sidecar**: none of this touches it, but D3 export
   flows that currently reuse the in-webview session should be audited in
   W2e for accidental sync-session dependencies.

## 14. As landed (W5c close-out, 2026-08-25)

Two deliberate deltas from the §12 plan, both recorded in the decision
log:

**The "delete" became a demotion.** §12 W5 planned to stop constructing
the synchronous session on the main thread. As landed, it SURVIVES with
two demoted jobs: the CONTENT store + mutation source that feeds the
worker replica (every mutation dual-writes through the mirror choke
point), and the complete in-process fallback road — which the fully
feature-detected worker architecture REQUIRES (no-Worker environments,
non-vite bundlers, boot failures, crashes all land on it). What the plan
actually wanted — a main thread that cannot pull analysis on recurring
paths — is delivered structurally instead: worker-fed stashes serve every
deferred rebuild (`DocHandle` stashes + the refined-token worker plane,
dirty-bit guarded so a stash is never served across an edit it predates),
and the **main-thread analysis boundary guard**
(`packages/brink-studio/src/__tests__/main-thread-analysis-guard.test.ts`)
pins every surviving analysis-flavored call site to a documented
allowlist. Memory-wise the main session's derived state stays
lowering-grade (nothing pulls its analysis), so the residency is inputs +
substrate — the class the classifier ruling already accepted.

**One-shots stay main-side, tracked.** Goto-definition, the rename
family, symbol-tab mount ranges, and search-card highlighting still hit
the main session — each user-triggered, each INCREMENTAL analysis against
delta-fed inputs (never compile-class cost), each enrolled in the guard
with a reason. Migration: issue #3110.

Small documents (< the 1000-line deferral threshold) keep the legacy
synchronous rebuild road — bounded incremental cost, zero behavior
change for the mock/test surface; large documents are fully worker-fed.

## 15. The prose worker (#3491, 2026-09-03)

A **second, independent** worker, not a capability of the session worker
above. Prose checking landed in PR #3177, after W5 closed, and ran the
`brink-prose` wasm module synchronously on the main thread behind a
700 ms debounce. Measured on main before this section: one check took
**651 ms** on a real 1,125-line `.ink` file and **4.8 s** (p95) on the
8k-line `?fixture=perf` document, each landing as a co-located
`browser.longtask` of the same size. §1's goal — a main thread
structurally unable to block on analysis — held for the compiler and was
simply not true for prose.

**Separate rather than merged into the session worker**, for the reason
the crate itself is separate (`crates/brink-prose`, module docs): the
module is 6.5 MB gzipped against `brink-web`'s 2.6 MB, and it is loaded
only if someone actually checks prose. Putting it behind the session
worker would tie the 6.5 MB download to boot, or make the session
worker's boot conditional on a feature most consumers never use.

As landed:

- `packages/brink-studio/src/prose-worker.ts` — the worker entry. The
  wasm module is `import()`ed **inside the worker**, on the first
  request; a load failure is remembered, never retried.
- `packages/brink-studio/src/prose-checker.ts` — the client.
  `ProseChecker.check` is unchanged (it was already async, for exactly
  this), so `@brink-lang/editor` needed no interface change.
- **Coalescing**: at most one request is in flight and at most one is
  waiting. A waiting request that a newer edit supersedes is rejected
  with `ProseCheckSuperseded` before it is ever posted — the editor
  treats a rejection as "leave the previous squiggles standing", which
  is the honest answer for a check that never ran. An in-flight wasm
  call cannot be interrupted; it can only be the last one that runs.
- **Fallback, same posture as §8's**: no `Worker` global (jsdom, node), a
  bundler that left `new URL(..., import.meta.url)` alone, or a worker
  crash all route to the in-process checker. Checking degrades in speed,
  never into silence. A failure to load the wasm module *inside* a
  healthy worker deliberately does NOT fall back — the in-process road
  resolves the same specifier and would fail the same way, for another
  6.5 MB of fetch. It rejects, which the editor reads as "no new
  squiggles", the same thing an unregistered checker gives.
- **The cost is now visible**: `prose.check` is a permanent perf span in
  `packages/ink-editor/src/prose.ts`, annotated with the document
  length. Its absence for four days after PR #3177 is why the regression
  survived a perf baseline (§11.5's "perf judge" measures what has a
  name).

**Measured, one machine, same session, `?fixture=perf`** — the arms
differ only in this section's change, with the `prose.check` span present
in both so the two are directly comparable:

| scenario | arm | `prose.check` | `browser.longtask` p95 / max |
|---|---|---|---|
| fast-scroll | before | 6,382 ms | 6,465 / 6,465 ms |
| fast-scroll | after | 5,812 ms | **184 / 184 ms** |
| typing-burst | before | 6,608 ms | 543 / 6,689 ms |
| typing-burst | after | 5,850 ms (p95 of 2) | 460 / **760 ms** |

Read it as: the *check* did not get faster — it got off the thread that
owns input. `fast-scroll` is the clean isolation, one check with little
else running: the co-located long task collapses from 6,465 ms to
184 ms.

`typing-burst`'s **p95 barely moves, and that is not this change
failing**. Its p95 is set by ~238 per-keystroke long tasks whose p50 is
already 431–476 ms in both arms — the separate per-keystroke cost
tracked elsewhere, which prose never contributed to. What this change
owns there is the *max*: 6,689 ms → 760 ms. Anyone re-running these must
compare max, or use `fast-scroll`, rather than reading `typing-burst`'s
p95 as a prose number.

Also in that change, and independent of the worker: `brink-prose`'s
`check` caches the merged dictionary and the curated `LintGroup` across
calls (thread-local, invalidated when the project's words or the dialect
change), so several hundred rules are constructed once per configuration
rather than once per keystroke pause. The `LintGroup`'s own per-chunk
LRU then survives between calls too, which is a first, partial answer to
the incremental checking the issue lists as its item 3.
