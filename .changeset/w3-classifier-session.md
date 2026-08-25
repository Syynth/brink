---
"@brink-lang/web": patch
"@brink-lang/editor": patch
---

`ClassifierSession` — the capability-stripped main-thread session (editor worker architecture W3, `docs/editor-worker-spec.md` §4). The wasm module exports a new single-document session whose surface is exactly the keystroke path's needs — delta/full-text ingress, segment manifest, per-segment line contexts and classifier tokens, dialect config — with no project method exported and write paths that never trigger an analysis pull (parity with the full session's slices is pinned Rust-side). `@brink-lang/web` wraps it as `ClassifierSessionHandle` (feature-detected: `available` is false on older builds and mocks). In the editor, full-file document handles attach a `ClassifierMirror`: the keystroke path's line contexts and fast tokens serve from the classifier's own analysis-free instance (with its own version-keyed slice cache), and the fast-token road blends positionally — cached refined slices keep their colors while uncached (edited) segments serve from the classifier. Mocks and older wasm keep the previous session-road behavior exactly.
