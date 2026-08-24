# Desktop performance baseline (measure-first ruling, 2026-08-24)

The desktop studio's interactive performance was reported as extremely bad
— confirmed on a second, high-end machine running a packaged build:
visible delay on a single line break, and scrolling ahead of what CM6 has
rendered (blank viewport) in a large file. Per the ruling
(`docs/decision-log.md`, "Desktop performance: measure first…"), **no fix
lands before the badness is characterized numerically**. This document is
that characterization: the instruments, the recorded baseline, and the
hypothesis verdicts. Fix work is filed as issues referencing these rows.

## Instruments

| Instrument | Where | What it measures |
|---|---|---|
| Perf probe | `packages/ink-editor/src/perf/` (`@brink-lang/editor` exports) | Named spans in a ring buffer, mirrored to `performance.measure` so Chrome/Safari Performance recordings show them in the Timings track. Hot CM extension sites, `ProjectSession` phases, the wasm boundary (every session call, via Proxy: `wasm.<method>`). |
| Browser observers | `perf/observers.ts` | Long tasks, long animation frames, event-timing input latency (`input.keydown` duration = keypress→paint), long rAF frames. |
| Viewport probe | `perf/viewport-probe.ts` | `cm.viewportLag` (scroll event → CM viewport catch-up — the blank-scroll number) and per-viewport-update counts. |
| Store timing | `packages/studio-store` | Every zustand `set()` sweep, tagged `store.set.<field>` — each is a full mounted-selector re-run. |
| Wasm counters | `crates/brink-web/src/perf.rs` | Inside-the-boundary phases: `ide.updateSource` / `ide.analyze` (the incremental db pull; the pre-option-A `ide.snapshotClone`/`ide.applyAnalysis` rows exist only in baseline-era recorded runs), `ide.compile`, `ide.projectOutline`, `ide.storyGraph`, `ide.byteToUtf16` call count. `perf_compile_probe` = the #2885 two-compile experiment. |
| Perf HUD | studio "Performance" tool window (dev only) | Live aggregates + Copy JSON. |
| `ide_bench` | `crates/internal/brink-test-harness/src/bin/ide_bench.rs` | The same editor road natively: init curve, keystroke phases, compile-repeat, large-file variants. |
| Scenario runner | `packages/brink-studio/perf/` (`pnpm --filter @brink-lang/studio test:perf`) | Deterministic scenarios against `?fixture=perf`, each writing a run artifact under `perf-runs/` (probe.json, wasm-counters.json, CDP trace.json, meta.json). |
| Compare tool | `scripts/perf-compare.mjs` (`pnpm perf:compare -- <base> <cand>`) | Per-span delta tables between recorded runs — the instrument every future fix is judged with. |

Everything is dev-only/off-by-default: production builds neither collect
nor register the HUD (ruled 2026-08-24).

## Fixture

`?fixture=perf` (`packages/brink-studio/src/perf-fixture.ts`): deterministic
(fixed-seed LCG) — 50 files × 20 knots mirroring `compile_bench`'s shape,
plus `large.ink` (~5.9k lines), the large-file symptom reproducer.
`ide_bench` generates the same shape natively.

## Interactive tracing workflow

1. `wasm-pack build crates/brink-web --target web --out-dir www/pkg --profiling`
   — optimized wasm **with the names section**, so flame charts show real
   Rust symbols (the stock release build shows `wasm-function[N]`).
   Profiling-only; never the default build.
2. `pnpm --filter @brink-lang/studio dev` → open `/?fixture=perf` in Chrome.
3. DevTools → Performance → record → type/scroll → stop. The probe's named
   spans and marks appear in the Timings track over the flame chart; the
   Interactions track carries per-keypress input latency; the Frames track
   shows scroll jank. Save profiles into a `perf-runs/<ts>-<label>/` dir
   next to a HUD Copy-JSON export (`probe.json`) to make a comparable run.
4. Ground truth in the Tauri shell: right-click → Inspect → Safari Web
   Inspector → Timelines (same markers).

## Baseline — native (`ide_bench`, release, 10-run medians, 2026-08-24)

Machine: maintainer's dev box (Apple Silicon, macOS 25.5).

| Row | Median | Reading |
|---|---:|---|
| `ide_init.analyze_each.files_50` | 81.2 ms | The `initialize()` shape: full analysis per file while loading |
| `ide_init.analyze_once.files_50` | 33.0 ms | Counterfactual: batch-load, analyze once — 2.5× cheaper at 50 files, gap grows superlinearly |
| `ide_keystroke.update_and_analyze` | 2.5 ms | Small-file keystroke at studio scale — NOT the problem natively |
| `ide_keystroke.phase.snapshot` | 1.3 ms | — |
| `ide_compile.first` | 308 ms | Cold compile (88% effect inference, per compile-time-profile-findings) |
| `ide_compile.repeat_no_edit` | 3.8 ms | **#2885 refuted natively**: memos survive `compile()`; repeats are warm |
| `ide_large.update_and_analyze` | **32.6 ms** | One keystroke with a 5,863-line file in the project |
| `ide_large.phase.snapshot` | **29.7 ms (91%)** | `IdeSnapshot` clones every `HirFile`/manifest — the big file's HIR cloned per keystroke |
| `ide_large.phase.update_source` | 0.8 ms | Re-lower the big file itself — cheap by comparison |
| `ide_large.phase.analyze` | 2.0 ms | The analysis itself is NOT the cost — the clone before it is |
| `ide_large.compile.first` | 1,613 ms | Cold compile with the big file |
| `ide_large.compile.repeat_no_edit` | 7.6 ms | Warm |

**Native headline: the per-keystroke cost scales with total project HIR
size because `snapshot()` deep-clones it every time, and at large-file
scale the clone is >90% of the whole `update_and_analyze`.** The analysis
pass itself is 2 ms.

## Baseline — browser (scenario runner, first recorded runs, 2026-08-24)

Same machine; Chromium via Playwright, vite dev server, **release wasm**
(so the wasm numbers are production-representative; the JS around them is
dev-served and unminified — re-measure against a packaged build before
trusting the JS-side absolute values to the millisecond). Runs:
`perf-runs/2026-08-24T15-43-*` (probe.json / wasm-counters.json /
trace.json each).

### typing-burst — the "delay on a line break" symptom, quantified

229 keystrokes typed into `large.ink` (5,866 lines):

| Metric | p50 | p95 | max |
|---|---:|---:|---:|
| `input.keydown` (keypress → next paint) | **96 ms** | **104 ms** | 264 ms |
| `cm.elementType.computeLineInfos` (per keystroke) | 53 ms | 57 ms | 59 ms |
| — of which `wasm.updateDocument` | 35 ms | 37 ms | 39 ms |
| —— of which `ide.snapshotClone` (wasm counter) | ≈28 ms mean | — | 33 ms |
| — plus `wasm.getLineContextsDoc` | 16 ms | 20 ms | 21 ms |
| `cm.folding.computeRanges` (whole doc) | 12 ms | 13 ms | 13 ms |
| `cm.hirOverlay.buildState` (whole doc) | 10.5 ms | 11 ms | 12 ms |

**Every keystroke blocks the main thread ~100 ms** (23.2 s of longtask
across the burst — effectively the entire typing session inside long
tasks). Composition: ~53 ms classification (dominated by the wasm
snapshot clone + line contexts), ~23 ms folding+HIR overlay, plus
semantic-token/inlay/screenplay passes and the store/React tail. The
browser p50 for the snapshot clone (~28 ms) matches `ide_large.phase.snapshot`
natively (29.7 ms) — wasm is *not* multiplying the Rust here; the cost is
the clone itself, paid identically everywhere.

### compile-cycles — the debounced-compile freeze

Five isolated single-character edits, each left to compile (500 ms
debounce): `cm.diagnostics.compileCycle` p50 **340 ms** on top of the
keystroke itself — `ide.projectOutline` **241 ms** of it (the outline
pull inside the fan-out), warm `ide.compile` only 8–9 ms.
`ide.byteToUtf16` count: **17,744 calls per cycle** — the outline/story-
graph cost IS the O(offset²) conversion scans.
⚠ Two caveats: (1) the **first** edit-triggered compile after load is far
worse — measured interactively at **1,114 ms** inside `ide.compile`
(subsequent ones warm to ~9 ms); (2) `perfCompileProbe` = **[5.7, 3.8] ms**
— the #2885 revision-stamp hypothesis is refuted in-browser as well.

### fast-scroll — the blank-viewport symptom

Wheel top-to-bottom through `large.ink`: `cm.hirRails.lineMarkers`
(the rails gutter's per-visible-line span-map rebuild) ran 91 batches at
**19.7 ms p50 per batch — 1.5 s total during one scroll pass**, the
single largest measured scroll cost. Long frames p50 33 ms, max 667 ms.
Mouse-move events over the editor cost 32–40 ms each (hover machinery).
(The `cm.viewportLag` signal recorded nothing — the scroll listener sits
on CM's content DOM but scrolling happens on `.cm-scroller`; noted as an
instrumentation fix, the frames track in trace.json covers the gap
meanwhile.)

### project-open

In-memory provider (no Tauri IPC in this harness): `project.initialize`
222 ms for 52 files; first compile **2,173 ms** (`ide.compile` 2,076 ms);
outline + story graph pulled twice (~235 ms each per pull); worst long
task **2.9 s**. The desktop shell adds the 2×N serial IPC read on top
(recon hypothesis 6) — measure in the shell before attributing.

## Hypothesis verdicts

| # | Hypothesis (from the recon) | Verdict | Evidence |
|---|---|---|---|
| 1 | Per-keystroke full-project analysis dominates | **Reframed → CONFIRMED as the snapshot clone**: the *analysis* is cheap (≤6 ms); `IdeSnapshot`'s deep clone of every `HirFile`/manifest is ~28–33 ms per keystroke at large-file scale, native and wasm alike | `ide_large.phase.*`; typing-burst `ide.snapshotClone` |
| 2 | `compile()`'s unconditional options write cold-prices every 500 ms compile (#2885) | **Refuted**, natively (repeat 3.8 ms) and in-browser (`perfCompileProbe` [5.7, 3.8] ms). New finding instead: the FIRST edit-triggered compile after load costs ~1.1 s in wasm | `ide_compile.repeat_no_edit`; compile-cycles meta |
| 3 | Six whole-doc wasm queries + JSON per keystroke | **Confirmed and ranked**: line contexts 16 ms + folding 12 ms + HIR projection 10.5 ms per keystroke on the large file; together with the clone they compose the ~100 ms keystroke | typing-burst rows |
| 4 | Per-compile fan-out (outline, story graph, `.inkt` capture, store sweeps) | **Confirmed, dominated by `ide.projectOutline` (241 ms/cycle) + `ide.storyGraph` (~230 ms)** — i.e. by the 17,744 `byte_to_utf16` linear scans per cycle | compile-cycles; `ide.byteToUtf16` count |
| 5 | Scroll path: rails gutter per-line maps, viewportChanged rebuilds, hover machinery | **Confirmed, led by the rails gutter**: 19.7 ms per rebuild batch, 1.5 s per full scroll pass; hover/mouse-move 32–40 ms per event | fast-scroll rows |
| 6 | Startup O(N²): double IPC read + per-file analysis | **Confirmed shape natively** (analyze-each 81 ms vs analyze-once 33 ms at 50 files, gap superlinear); first compile 2.2 s in-browser; desktop IPC half still to be timed in the shell | `ide_init.*`; project-open |

## Budgets (ruled 2026-08-24)

The frame budget is **8 ms** (120 fps — ProMotion macOS is the target
hardware). "Done" is falsifiable against these, measured on the perf
fixture via the scenario runner; whatever the optimization wave cannot
meet is the quantified case for the async-architecture phase (the
standing-pressure mechanism from the optimization-first ruling):

| Scenario | Budget | Baseline today |
|---|---|---|
| Keystroke-to-paint p95 (`input.keydown`, large file) | ≤ 8 ms | 104 ms |
| High-speed scroll: every frame, text ahead of the scroll | no frame > 8 ms | long frames p50 33 ms, max 667 ms |
| Compile cycle (on its new save/interaction triggers) | ≤ 100 ms | 340 ms p50, 1.1–2.2 s worst |
| Project open → first paint | ≤ 1 s | ~3.0 s |

Note the keystroke budget is ~3.5× smaller than today's snapshot clone
alone — it is deliberately not reachable by trimming the synchronous
pipeline, only by removing whole-document work from the keystroke path
entirely (viewport-scoping / memoized deltas / async).

## Deep dive: the keystroke, fully attributed

The typing-burst spans sum to ≈94.6 ms per keystroke against the observed
96 ms `input.keydown` p50 — the budget closes; nothing material is
unmeasured. Top-level composition (nested spans indented, per keystroke,
5.9k-line file):

| Pass | ms | Share |
|---|---:|---:|
| `cm.elementType.computeLineInfos` | 53.6 | 56% |
| — `wasm.updateDocument` (snapshot clone ≈28, analyze ≈4, relower ≈1.4) | 35.3 | |
| — `wasm.getLineContextsDoc` (whole-doc JSON round trip) | 17.3 | |
| `cm.folding.computeRanges` (whole-doc) | 12.3 | 13% |
| `cm.hirOverlay.buildState` (incl. `getHirSpansDoc` 7.4) | 10.8 | 11% |
| `cm.highlight.decorations` (incl. `getSemanticTokensDoc` 5.5) | 7.1 | 7% |
| `cm.hirRails.lineMarkers` | 4.1 | 4% |
| everything else (occurrences, screenplay, argument widgets, inlay hints, inline markup, React commits, store sweeps) | ≈6.7 | 7% |

**Acquitted by measurement** (recon suspects that turned out immaterial at
this scale): the zustand fan-out (`store.set.*` + React commits ≈0.7 ms
per keystroke), argument-widgets/hanging-indent viewport rebuilds
(≈1.3 ms; 124 ms across a whole scroll pass), screenplay passes (≈2 ms).

Startup marks add one more headline: `studio.projectInitialized` at
792 ms but `studio.renderStart` at 3,535 ms — **first paint waits ~2.7 s
behind the first compile**, which `mountStudio` runs to completion before
rendering anything.

## Optimize vs. architecture (the discussion split)

**Local optimizations — no design change, each judged by its scenario row:**

1. `byte_to_utf16` → `LineIndex` (#3065): removes most of the ~485 ms
   outline+story-graph share of every compile cycle. Byte-identical
   output required; purely local.
2. Rails gutter map hoist (#3067): 19.7 ms/batch → sub-ms; one function.
3. Desktop double IPC read (#3068, shell half): stop discarding the
   pre-read, or stop pre-reading.
4. Memoize per-generation whole-doc query results wasm-side (folding
   12 ms + line contexts 17 ms + semantic tokens 5.5 ms are recomputed
   even when only serialized output is wanted again); incremental but
   local per query.

**Architectural shortcomings — need a design discussion before touching:**

1. **The snapshot clone / off-db analyze road** (#3063): ~28–33 ms per
   keystroke, scaling with total project HIR. Options range from
   Arc-sharing `HirFile`s (data-model change in brink-ide) to retiring
   the off-db road for the editor in favor of the salsa road — a
   both-roads-doctrine question the maintainer owns.
2. **What must be synchronous with a keystroke at all** (#3064): even
   with the clone fixed, ~60 ms of whole-doc work rides every
   transaction. Viewport-scoping, deferring passes a frame, or a worker
   are different architectures with different correctness stories.
3. **Compile-before-first-paint at startup**: `mountStudio` blocks
   render on the initial compile (2.2 s on the fixture). Rendering
   first and compiling after is an ordering/UX decision.
4. **Effect inference on the interactive path** (#3069 + the 2.2 s first
   compile): cold compile is 88% effect inference for rows the runtime
   does not read (compile-time-profile-findings), and the decision log
   (2026-08-01, #1511 close) already ruled full recompile "a build/on-
   save operation, not an interactive path" — yet the studio runs
   compile-to-bytes on a 500 ms debounce while typing. Whether the
   interactive loop should compile at all (vs. analyze-only until play),
   and whether effect inference belongs in editor compiles, are ruling-
   level questions.

## Toolchain delta — web dep sweep (vite 6.4 → 8.2, same day)

The sweep (vite 8, plugin-react 6, Playwright 1.62, CM6/zustand/react
minors; TypeScript 7 and changesets 3 deliberately held) was landed as a
discrete commit and the four scenarios re-recorded
(`perf-runs/2026-08-24T15-55-*` vs `15-43-*`, via `perf-compare`):

- **typing-burst: neutral within ±3% on every span** — keystroke p95
  stays 104 ms, `cm.elementType` p95 57.1 ms both sides. The toolchain is
  acquitted; the costs are architectural (#3063–#3067).
- fast-scroll shows `frame.long` 83× → 3× — **a measurement artifact,
  not a speedup**: Playwright 1.62 bundles a newer Chromium whose frame
  scheduling under synthetic wheel events differs; the actual gutter work
  (`cm.hirRails.lineMarkers`) is unchanged (−7%, within noise). Compare
  scroll runs only within one Chromium version.

## Filed issues (the fix-wave backlog)

One issue per confirmed hot path, each carrying its numbers and its judge
scenario — filed 2026-08-24, on the Brink board:

- #3063 — `IdeSnapshot` deep-clones the whole project's HIR per keystroke (needs-design)
- #3064 — per-keystroke whole-document query stack (96 ms p50 keystroke)
- #3065 — `byte_to_utf16` linear scans (17,744 calls/compile cycle)
- #3066 — unconditional compile fan-out (outline/story-graph/disassembly)
- #3067 — rails gutter per-visible-line map rebuild (1.5 s per scroll pass)
- #3068 — superlinear project open (per-file analysis + desktop double IPC read)
- #3069 — one-time ~1.1 s first post-edit compile (root-cause)
- #2885 — commented with the refutation numbers (perf half answered)

## Recording + comparing runs

```sh
pnpm --filter @brink-lang/studio test:perf          # all scenarios
pnpm --filter @brink-lang/studio test:perf -- -g fast-scroll
node scripts/perf-compare.mjs perf-runs/<base> perf-runs/<candidate>
```

Runs are per-machine measurements and are gitignored; judge fixes by
running the same scenario N times before and after on the same machine and
comparing. The compare tool marks regressions `▲` / improvements `▼`
beyond a 10% noise threshold — informational, never a CI gate (ruled).

## Option A delta (2026-08-24, same day — the db-road migration landed)

Recorded runs: `perf-runs/2026-08-24T19-*` vs the `16-5*` baselines
(measured on an idle machine — an earlier contaminated bench that ran
concurrently with a wasm build produced a phantom 3× "regression" whose
triage is itself a method lesson: **never bench under load**; a salsa
event-count probe then showed the db pull executes each query exactly
once per edit, textbook incrementality).

Native (`ide_bench`, 10-run medians):

| Row | Before | After |
|---|---:|---:|
| small-file keystroke | 2.5 ms | **1.1 ms** |
| large-file keystroke | 32.6 ms | **30.6 ms** |
| — snapshot clone | 29.7 ms | **deleted** |
| — db analysis pull | — | 30.0 ms |
| init analyze-each @50 | 81 ms | **61 ms** |
| repeat compile (no edit) | 3.8 ms | **0.0 ms** |

Browser (typing-burst / compile-cycles):

| Metric | Before | After |
|---|---:|---:|
| keystroke-to-paint p95 (large file) | 104 ms | **96 ms** |
| `wasm.updateDocument` p95 | 37.2 ms | 34.6 ms |
| compile cycle p95 | 44 ms | **35 ms** |
| `wasm.compileProject` p95 | 9.6 ms | **2.9 ms** |
| `perfCompileProbe` | [5.7, 3.8] ms | **[0.1, 0.0] ms** |

**Honest verdict on #3063:** the clone is deleted, but large-file typing
improved only ~6–8%, not the hoped ~28 ms — the old road's "cheap
analysis" was cheap only because the clone had already paid the big
file's re-lowering; the db pull now carries that same irreducible
single-file pipeline (parse + lower + resolve + per-file diagnostics +
index rebuild over a 5.9k-line file, ~30 ms native and wasm alike).
What the migration DID buy: the divergence class closed structurally,
#2885 closed, the LSP's background pass memoized (unchanged roots
validate instead of re-analyzing), compile-side costs collapsed, and
small-file/startup improved. The keystroke budget's remaining owners are
now sharply named: the #3064 whole-doc query stack (~60 ms) and the
single-big-file re-analysis (~30 ms) — the latter needing per-knot
incremental lowering or the async-architecture phase. **The 8 ms budget's
pressure mechanism is functioning exactly as ruled: the residual is the
architecture phase's quantified case.**
