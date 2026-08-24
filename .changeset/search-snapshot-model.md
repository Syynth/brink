---
"@brink-lang/studio": minor
"@brink-lang/editor": minor
---

Frozen search snapshot model (search-result cards, PR B). The Search
panel's result set is now a snapshot: edits never remove or re-filter
rows. Match spans are edit-mapped through document changes (driven by
the compile seam), flagging rows `edited`/`stale` instead of dropping
them; only a new search or the explicit refresh replaces the set. The
store gains the context-lines setting (default 1 above / 2 below), the
per-card collapse map, and `refreshSearchSnapshot()` — query snapshots
re-run their frozen query, references snapshots re-resolve from the
edit-mapped declaration anchor. The editor's Find References surfaces
(`onShowReferences`) now pass the symbol's declaration location as an
anchor when goto-definition resolves one.
