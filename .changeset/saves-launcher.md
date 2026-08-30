---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Runtime save/load — the idle-Player launcher (W14/#3307, RULED;
re-scopes #57's save half). The wasm session's `loadState` now RETURNS
the runtime's `LoadReport` (the session layer used to discard it) — a
stale load's drops surface inline, never silently. The idle Player body
becomes the launcher: "Run from the start" beside a typeahead over
knots/stitches (KNOT/STITCH chips + file context; plays from there via
the play-from-here start path), then the checkpoint stores as PROJECT
and THIS COMPUTER sections in the landing Recent-list style — TURN-count
chips, amber OLD for saves against an older compile, and hover
Load/Fork/delete. Load ATTACHES the session to the slot ("Save state" —
the new toolbar button — writes back); Fork starts from a copy and the
next save picks a new slot. The payload is the runtime's existing
`SaveState` boundary (no execution position — loading diverts to the
slot's recorded knot). Both stores are localStorage on the web;
`mountStudio`'s new `saveStores` option is the seam for desktop's
file-backed stores. Settings → Player picks the default target for new
saves.
