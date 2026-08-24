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
| Wasm counters | `crates/brink-web/src/perf.rs` | Inside-the-boundary phases: `ide.updateSource` / `ide.snapshotClone` / `ide.analyze` / `ide.applyAnalysis`, `ide.compile`, `ide.projectOutline`, `ide.storyGraph`, `ide.byteToUtf16` call count. `perf_compile_probe` = the #2885 two-compile experiment. |
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
