---
"@brink-lang/web": patch
---

Issue #1738 — a consistency audit of the escape/markup layer (§8d.6,
`docs/prose-dialect-spec.md` §4.6) across every native prose scanner found
one clear bug and fixed it: `\#` inside a `#tag`'s own text (or an `@NAME`
cue's own name) is now recognized as escaping `#`'s tag/name-terminating
role, matching the ruled, final four-character inline escape set (`\< \{
\# \\`) that already worked everywhere else. Before this fix, `\#` inside a
tag body still split the tag in two at the `#`, leaving a dangling
backslash — e.g. `Bell tolls #sound \#not a new tag` compiled to *two*
runtime tags (`sound` and `\#not a new tag`) instead of one (`sound
\#not a new tag`). Runtime tags surface through `brink_runtime::Line`'s
`tags` field, which the wasm package re-exports, so this is
wasm-observable through story playback. The backslash itself is not
stripped from the tag's own literal text — matching the pre-existing `\{`
precedent in the same two scanners, not a new "strip the backslash"
behavior.
