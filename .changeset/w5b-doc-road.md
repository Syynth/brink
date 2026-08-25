---
"@brink-lang/editor": patch
---

The doc-scoped road rides the worker (editor worker architecture W5b, decision log 2026-08-25): the W4 query-time flush becomes a **continuous replica** — every session mutation (file writes, config ops, doc lifecycle, per-doc edits as protocol edit/push messages) forwards to the worker session the moment it happens through the mirror choke point, spawned eagerly at construction so the replica tracks from t0. Doc ids stay aligned by determinism (both sessions mint ids monotonically from the same replayed open/close sequence), guarded by a runtime tripwire: a forwarded open whose replica id mismatches drops the worker and every road falls back in-process. Interactive queries (completion, hover, signature help, code actions), deferred-refresh warm-ups, and structural computes now route to the replica via `ProjectSession.docClient()`. The main session stays fully written — its sync reads remain valid until the W5c delete.
