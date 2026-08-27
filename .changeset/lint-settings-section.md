---
"@brink-lang/studio": minor
---

Settings gains a **Diagnostics** section for `[lints]`: two lists, where
which list a code is in *is* whether it is in `brink.toml`. "Configure"
moves a code up — writing the key at its current default, so the first
click changes nothing about the build — and the down arrow moves it back
out, removing the key.

Both lists group by category, rows carry the Problems panel's own severity
glyphs showing each code's *effective* level, and a written explanation
expands in place.

What is listed comes from the compiler, not from the studio: only
overridable codes appear (30 of 189), and a project is only offered codes
its own source surfaces can produce — so a `.ink`-only project sees no
settings for `.brink` markup spans.

The previous "Diagnostics" section is now titled **External functions**,
which is what it configures. Unlike the lints, it is a studio preference
rather than a `brink.toml` setting.
