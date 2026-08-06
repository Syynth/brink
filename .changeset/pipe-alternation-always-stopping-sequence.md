---
"@brink-lang/web": patch
---

Native parser: `{|…}` is always a stopping-sequence alternation (ruled 2026-07-22, correcting the earlier "malformed lambda" clause). `{|x| x}`, `{|heads|tails}`, and `{|heads| tails}` are all valid two-branch stopping-sequences; the fragile space-after-separator "malformed lambda" heuristic is removed. A lambda in content position is spelled `{(|x| x)}`. Observable through the web editor's diagnostics for `.brink` files (a `{|x| x}` that previously errored no longer does).
