---
"@brink-lang/web": patch
---

Wasm-internal perf counters (measure-first ruling, 2026-08-24).
`EditorSessionHandle` gains `setPerfEnabled`/`getPerfCounters`/
`resetPerfCounters` over a new counter store inside the wasm: the
per-keystroke reanalysis decomposed by phase (`ide.updateSource`,
`ide.snapshotClone`, `ide.analyze`, `ide.applyAnalysis`), the editor
compile (`ide.compile`), the per-compile outline/story-graph builds, and
an `ide.byteToUtf16` call counter. `perfCompileProbe(entry)` runs the
#2885 revision-stamp experiment directly — two back-to-back compiles with
no edits, returning `[firstMs, secondMs]`. Counters are off by default and
cost one branch per site while disabled; behavior of every existing call
is unchanged.
