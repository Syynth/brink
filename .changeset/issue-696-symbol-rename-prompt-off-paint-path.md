---
"@brink-lang/studio": patch
---

The knot/stitch rename prompt (`SymbolRenamePrompt`, the "Rename…" context-menu surface) no
longer runs its collision analysis synchronously on the paint path (issue #696). Previously a
confirmed rename (Enter) or a forced override ran `performSymbolRename`'s wasm call inline in the
same frame as the triggering event, with no yield point of its own — under CPU contention this
could block the whole page (including a real user's next interaction) for however long the
analysis took, with zero visual feedback until it finished.

#722 fixed this exact defect for the sibling inline (F2) rename widget by committing a pending
state synchronously (so it paints before the heavy call runs) and deferring the call itself to
the next idle slot; it never reached this modal prompt. The prompt now takes the same discipline
— a `.brink-rename-pending` indicator ("Checking for conflicts…") appears immediately on Enter or
Force, and the actual analysis runs afterward via the same `scheduleIdleWork` helper. This is also
what stabilizes the long-flaky `e2e/symbol-rename.spec.ts` "a colliding rename shows the breakage
report; Force overrides" test (PR #714's timeout-only fix reduced but never eliminated the
recurrence): the pending indicator gives that test — and `symbol-rename-prompt-pending.test.tsx`'s
deterministic, fake-timer-driven ordering check — a real signal to wait on instead of a race
against an unbounded synchronous call.
