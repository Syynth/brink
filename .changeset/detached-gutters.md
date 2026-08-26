---
"@brink-lang/editor": patch
---

Detached gutters (#3119): in wrapping views the editor's gutters leave CodeMirror's scroller flex/sticky flow, with the horizontal space they vacate paid back as content padding. CodeMirror makes the gutter container a sticky flex child stretched to the full document height, which costs WebKit roughly 5x on every editor layout — a cost paid synchronously on each keystroke and once per frame while scrolling (Chromium is unaffected). Measured on a real ~1,100-line project under WebKit: forced layout 36-40ms → 17ms, felt keystroke latency 48ms → 24ms, long frames 55ms → 35ms. Self-gating: a non-wrapping view keeps CodeMirror's stock layout, since sticky gutters exist to survive horizontal scrolling.
