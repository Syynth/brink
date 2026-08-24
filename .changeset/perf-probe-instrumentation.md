---
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

Performance probe + dev-only HUD (measure-first ruling, 2026-08-24).
`@brink-lang/editor` gains a perf module — `setPerfEnabled`/`perfSpan`/
`perfTime`/`perfReport` over a preallocated ring buffer, every span also
emitted as a `performance.measure` so DevTools recordings show named bars —
plus browser observers (long tasks, event-timing input latency, long
frames), a CM6 viewport/scroll probe (`cm.viewportLag`), a wasm-boundary
Proxy timing every session call (`wasm.<method>`), and spans at the hot
extension sites (element-type, highlight, HIR overlay + rails gutter,
inlay hints, folding, screenplay passes, argument widgets, hanging indent,
inline markup, the debounced compile cycle, project initialize). The studio
wires the dev edge (`import.meta.env.DEV`): store-write sweep timing
(`store.set.<field>`), compile fan-out spans, startup marks, a React
commit profiler, and a "Performance" tool window (aggregates, worst
events, marks, Copy JSON). Everything is inert single branches when
disabled — production builds neither collect nor register the HUD.
