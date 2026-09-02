---
"@brink-lang/web": patch
---

Auto-fix `Safe` tier: the obligation behind `Fix.applicability === "safe"` is
now an executable check rather than a label.
`brink_test_harness::fix::assert_safe_fix` compiles a fixer's pre-fix and
post-fix sources, replays the pre-fix program's explored run set on the
post-fix program, and diffs the exported line tables — observable equivalence
plus translation identity.

No API or behavior change in this release: nothing declares
`applicability: "safe"` yet. The four fixers that reach the wasm surface
(`E025` add-import, `E063` call/bind trim, `E080`/`E081` creation-site) all
discharge diagnostics that prevent compilation, so there is no pre-fix
program to preserve and none of them can be promoted to `safe` — they stay
`"suggested"`, one explicit click each.
