---
"@brink-lang/editor": patch
---

Interactive queries ride the async session facade (editor worker architecture W2c, `docs/editor-worker-spec.md`): completion, hover, signature help, and code actions accept sync-or-async sources, and the studio wiring routes them through the `SessionClient` at interactive priority (after queued mutations, before background pulls; never coalesced or dropped). Completion and hover lean on CM6's native promise handling; signature help lands under sequence + doc-held-still guards (an out-of-order or stale landing is discarded); the code-actions menu opens on landing only if the document and cursor held still. Hosts passing synchronous sources are unaffected.
