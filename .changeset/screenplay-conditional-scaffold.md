---
"@brink-lang/editor": patch
---

Fix two screenplay-mode classification gaps (#413): a `~`-sigil logic line
immediately after a chained dialogue line was swallowed into the
cue→dialogue chain (rendered `brink-dialogue` instead of `brink-logic`),
and lines in/around conditional blocks (`{`, `- cond:`, `- else:`, `}`, and
cue/dialogue lines inside conditional arms) got no classification at all.

Sigil classification now always wins over chain continuation. Conditional
scaffold lines classify as logic; cue/dialogue lines written inside a
conditional or sequence arm classify normally (Character/Parenthetical/
Dialogue) and participate in the dialogue chain, matching top-level
narrative. Choice-body narrative is unaffected — it still classifies but
never chains, per the existing spec-mandated split.

Emitted classes for lines that already classified correctly are
byte-identical; only the previously-broken lines gain classes.
