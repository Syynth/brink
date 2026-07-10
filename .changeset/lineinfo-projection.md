---
"@brink-lang/web": minor
"@brink-lang/editor": minor
---

LineInfo on one shared projection (#480). The HIR projection is now
computed once per edit and cached on the session — `getLineContextsDoc`,
`getFoldingRangesDoc`, and `getHirSpansDoc` all share it instead of each
re-projecting. `LineContext` gains two additive fields the editor now
consumes instead of deriving: `option_path` (option identity from real HIR
nesting — the TS weave re-walk only serves the pre-wasm regex fallback)
and `standalone` (structural divert-vs-tunnel/thread fact — no more text
sniffing in the editor or fold-run natures). Span kinds `tunnel_stmt` and
`thread_stmt` split out of `divert_stmt`, which now means a simple
`-> target` statement only.
Also fixes `has_tags`: it is now true for **any** line carrying an
author-written tag — tagged choice lines (`* Choice # tag`), tags inside
inline conditional/sequence branches, and standalone `#` lines — where the
legacy walk under-reported (decision 2026-07-10; verified against the C#
reference, whose runtime surfaces choice-line tags).
