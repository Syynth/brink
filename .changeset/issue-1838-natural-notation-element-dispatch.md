---
"@brink-lang/web": patch
---

Compiler: natural-notation `@[element(claims = "…")]` handlers now dispatch
prose lines (issue #1838, `docs/decision-log.md` 2026-07-31 "Conventions are
annotated handlers").

Issue #1715 landed the native prose grammar — scene headings, cues,
parentheticals — and nothing lowered any of it, so a writer could type a
scene heading and the compiler would only report it as not-yet-lowered. This
slice makes the first of those shapes mean something.

`@[element(claims = "…")]` is the new spelling beside `args = "…"`: a
pattern that claims a prose line carrying no `!name` sigil. A claimed line —
a wholly literal content line, or a scene heading — is matched, its named
captures bind the handler's parameters by name, and the line lowers to
**exactly one call** on the handler, whose value is the line. `args` (the
`!name`-dispatched remainder pattern) is unchanged and still does not
dispatch.

Web-observable through the compile-diagnostics surface and through compiled
output for `.brink` sources:

- a new diagnostic `E167` — a claiming handler declaring a parameter its
  pattern never captures (the converse of `E159`'s existing capture check,
  needed because every argument of the rewrite comes from a capture);
- `E159`'s message widened to name both clause spellings, and an
  `@[element]` carrying both `args` and `claims` now raises it;
- `E112` (misplaced annotation) for a claim anywhere but a top-level `fn` —
  only a `fn` is callable as the expression the rewrite produces;
- a scene-heading-shaped line that a handler claims now compiles and
  produces output instead of reporting `E129`. An *unclaimed* heading still
  reports `E129`, unchanged.

Every claimed line is recorded on the lowered file (matched kind, handler
name and declaration range, the claiming annotation's range, captures as
source spans, disposition), so nothing the compiler rewrote is invisible to
tooling.

Block capture and `fn conventions()` registration are the ruling's other two
build slices (issues #1839/#1840) and are not in this one.
