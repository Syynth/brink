---
"@brink-lang/web": patch
---

A `temp` read before its declaration runs now warns instead of breaking the story.

A `~ temp` lives in its knot's call frame, so a read from a sibling choice
branch, a gather, a stitch, or a line written above the declaration names the
same slot — but the declaring statement may not have run yet. The C# runtime
prints the line and warns; brink either faulted (`cannot apply Add to Null and
Int`) or, for a read written ahead of the declaration, emitted a program that
died at its first step with `unresolved global`.

- **E193**, a new warning-level, `[lints]`-overridable diagnostic, names the
  read and the declaration in the Problems panel — before the story is played.
  It covers three shapes: a sibling choice branch, a gather reached from a
  branch that did not declare it, and a read textually ahead of the
  declaration. (A fourth shape — a stitch reading a temp declared at its
  knot's root — turned out to be a different, stricter question: see the
  companion compat-deny changeset.)
- Reads, writes, and `ref` arguments that precede the declaration now resolve
  to the frame's own slot instead of a phantom global that could not link.
- An unset slot reads as ink's missing-variable default (`0`) with a runtime
  warning, matching the reference runtime, so the story keeps playing.
  `WebSession.takeRuntimeWarnings()` drains them.
