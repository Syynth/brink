---
"@brink-lang/web": patch
---

Issue #1720: the built-in screenplay preset (`std/conventions/
screenplay.brink`), Track 1 step 8 of #1351. Widens `@[element(claims =
"…")]`/`@[element(args = "…", block)]` natural-notation dispatch (issues
#1838/#1839) to two grammar shapes it did not reach before: a real
`@NAME` cue's name and a chain-gated `(delivery)` parenthetical's text
are now claim candidates, the same way a wholly-literal `CONTENT_LINE` or
slug/tag-free `SCENE_HEADING` already were — matched line, captures bound
to params by name, exactly one call. Adds `ElementKind::Cue` and
`ElementKind::Parenthetical`.

The shipped preset covers `heading` (bare `INT.`/`EXT.` headings, no
explicit slug), `transition` (a bare all-caps line ending in `:`), `cue`
(block-capturing directly-following dialogue), and `parenthetical`
(block-capturing directly-following dialogue). A cue directly followed by
a parenthetical (the common screenplay shape) is two independent claims,
not one joined attachment: the cue's own block capture sees zero lines
(the ruled block-capture terminator ends a run at any element-level line,
and a parenthetical is one), and the parenthetical claims the dialogue on
its own next iteration.

Not covered: compact cues (`@NAME: text`), any cue/heading carrying a tag
extension, and a heading carrying an explicit `[slug]` (every worked-page
heading in the spec uses one) — `candidate`'s literalness rule declines
all three. Promoting a heading to a real HIR stitch (a genuine divert
target) is not built anywhere in the compiler; a project wanting that
still needs an ordinary `flow name() { … }`. Not reachable via `use
std::conventions::screenplay` yet either — no `std::`-namespaced module
resolution exists in the compiler, and `fn conventions()` registration/
comptime (issue #1840) hasn't landed — so this ships as authored source
only, proven end to end via a project that inlines the same handler
declarations directly.
