---
"@brink-lang/web": patch
---

New diagnostic tier, **compat-deny**: "inklecate rejects this; brink can run
it; you must opt in." Its first member, `E194`, catches a knot's `~ temp`
(native `~ let`) read from one of that knot's stitches — brink shares one
call frame across a knot and its stitches and plays such a program
correctly, but the official ink compiler rejects it outright
(`Unresolved variable`). Default severity is `Error`, matching inklecate's
own rejection — the Problems panel, and a plain `brink compile`, now refuse
this construct by default where they used to accept it silently (as a
warning-level `E193`, or not at all).

Unlike other `Error`-default codes, this tier is `[lints]`-overridable all
the way to `allow`, not just `warn` — a project that leans on the pattern
deliberately can turn it off (`[lints] E194 = "allow"` in `brink.toml`, or
`--allow E194` on the CLI); the diagnostic then disappears from the Problems
panel entirely, exactly like any other suppressed code, and the story still
plays.
