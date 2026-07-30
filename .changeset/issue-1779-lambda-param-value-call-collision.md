---
"@brink-lang/web": patch
---

Issue #1779: fixed a soundness gap in effect-row narrowing where a value-call through a lambda's own parameter could resolve against an unrelated enclosing local's write summary if the two shared a bare name (lambda params are indexed in the same flat name keyspace an enclosing `~ temp` gets). Left unfixed, this would silently under-report the call's effect row instead of falling back to the pessimal floor — the direction docs/effects-spec.md §3 forbids.

Not observable through `@brink-lang/web` today: reproducing the collision requires combining an ink-only construct (`#fn(...)`, the only fn-value creation site) with a native-only one (`|...| ...` lambdas) in the same body, and no current frontend parses both together. This closes the gap in `InferPass` itself so it stays closed once that convergence happens, and is a pure classification-time restriction (never widens narrowing, only ever falls back to `Unknown` more often) — vanilla-ink stories and the oracle corpus are unaffected (episode count unchanged: 5,607).
