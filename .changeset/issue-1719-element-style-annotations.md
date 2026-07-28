---
"@brink-lang/web": patch
---

Compiler: `@[element]` / `@[style]` per-declaration annotation declaration
surface for the native `.brink` prose-dialect authoring surface (issue
#1719, `docs/prose-dialect-spec.md` §3.5b).

`@[element(args = "…")]` above a `flow`/`fn` declares the portable-regex
pattern the prose-dispatch `!name` sigil surface will eventually match a
content line against — this slice parses it, validates the pattern
compiles, and validates its named capture groups each bind a real
parameter on the declaration (the spec's "captures bind params by name,
compile-checked" contract). A companion `@[style(key = "value", …)]`
requires a paired `@[element]` on the same declaration, validates its keys
against the paired pattern's captures plus the two special keys `line`/
`dispatch`, and classifies each value against the closed built-in
presentation vocabulary (alignment, emphasis, case, conceal, raw hex
color) with any other name falling back to a custom `brink-*` CSS hook —
never a diagnostic, per the spec's own fallback rule.

Five new diagnostic codes (`E159`–`E163`) reach a project's compile
diagnostics, which is what makes this `@brink-lang/web`-observable even
though the feature itself is native-only: a malformed `@[element]` or
`@[style]` annotation that previously hard-failed with the generic `E111`
(unrecognized annotation name) now gets a targeted code.

**Declaration surface only** — the `!name` sigil dispatch rewrite itself
(matching a content line, binding captures, rewriting to a call) is not
implemented by this slice; neither is any editor-side consumption of
`@[style]` (that lands on the held editor track, issues #1131/#1350). See
`docs/prose-dialect-spec.md` §3.5b's Deferred list.
