---
"@brink-lang/web": patch
---

Analyzer: native-only lint for an asymmetric choice-branch dead-end
(`E151`, issue #1219).

A native `{? … }` choice branch that falls through (no `->`/`return`)
while a sibling branch diverts onward, at a genuine dead end (nothing
follows the choice point to reconverge into), now emits a `Warning`-
severity `E151` diagnostic — the relocated residual value of ink's
retired "ran out of content" runtime error (decision-log 2026-07-22,
"Flows end implicitly (native)"). On by default (not opt-in): fires on
every compile, like any other `Warning`-base-severity code. Never blocks
compilation on its own. A choice set whose continuation is non-empty
(the dissolved-gather reconvergence shape —
`docs/native-surface-charter.md` §5) is never flagged, nor is a choice
set where every branch shares the same tail shape (all divert, or all
fall through). Re-levelable and suppressible through the project's
`[lints]` table / `//brink-disable` like every other `Warning`-base-
severity diagnostic code — including promotion to a hard error via
`[lints] E151 = "deny"` or a project-wide `[lints] deny-warnings = true`.
