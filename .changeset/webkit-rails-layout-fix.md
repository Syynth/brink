---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Rails-gutter WebKit layout fix: the percent-height inline-flex rail marker made every forced layout cost ~1 ms per visible marker in WebKit (~110 ms per keystroke-burst refresh on a real project — the dominant slice of desktop typing latency; Chromium was unaffected). Markers now use an in-flow fixed-width spacer plus an absolutely-positioned bar layer — same visuals, measured 120 ms → 36 ms full-layout and ~3x lower felt keystroke latency under WebKit. Also: `cm.dispatch`/`cm.dispatch.state`/`cm.dispatch.view` perf spans on the main editor view, `__brinkPerf.report(worstCount)`, and the playground's `?fixtureUrl=` loader for measuring real-project shapes without baking content into the repo.
