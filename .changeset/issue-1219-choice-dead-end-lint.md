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
"Flows end implicitly (native)"). Never blocks compilation. A choice
set whose continuation is non-empty (the dissolved-gather reconvergence
shape — `docs/native-surface-charter.md` §5) is never flagged, nor is a
choice set where every branch shares the same tail shape (all divert,
or all fall through). Configurable through the project's `[lints]`
table like every other `Warning`-base-severity diagnostic code.
