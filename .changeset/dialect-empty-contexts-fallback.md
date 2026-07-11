---
"@brink-lang/editor": patch
---

Fixed a silent dialect-classification drop in `computeLineInfos` (#426):
when a mounted view's wasm document handle was present but not yet synced
(or a host's `line_contexts_doc` returned `[]`, as some test/mock sessions
do), every line was classified via the bare regex fallback and the TS
dialect interpreter (`applyDialectFallback`) was never run — character cues,
parentheticals, and chained dialogue lines silently rendered as plain
narrative with no diagnostic. The same TS dialect fallback the no-handle
path already ran is now also run over the regex-classified tail whenever the
handle yields fewer line contexts than the document has lines, so dialect
classification survives that path.
