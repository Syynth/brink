---
"@brink-lang/web": patch
---

Issue #2079 (`docs/decision-log.md` 2026-08-06 "Compact cue desugars to cue
+ content line"): a compact cue (`@NAME: dialogue`, `docs/prose-dialect-
spec.md` §8b.9) is now a claim candidate — `hir::lower_native::element::
candidate` widens with a `COMPACT_CUE` arm alongside the existing `CUE`/
`SCENE_HEADING`/`PARENTHETICAL` ones.

- `@[convention(claims = "…")]` matching is offered only the compact cue's
  **name segment** (its `CUE_NAME` sub-node) — exactly the same text a
  block cue's own `@NAME` line offers — never the fused dialogue. Before
  this, `COMPACT_CUE` was invisible to `candidate()` entirely and every
  compact cue fell to the loud `E129` default regardless of what any
  project or preset declared.
- The fused dialogue lowers as an **ordinary content line**, landing
  inside whatever run the matched handler's `attach`/`block` flavor
  captures (or, for a plain handler, right after its own call) — it keeps
  full interpolation rights, since literalness only ever applies to the
  name segment. It does not, however, get a free pass on structure: a
  dialogue carrying a fused `LABEL` (a leading `(word)`) or a fused
  `DIVERT_STMT`/`TUNNEL_CALL`/`CHOICE_POINT` (a trailing `->`/`->->`/`{?}`)
  declines the WHOLE claim (loud `E129`) rather than being silently folded
  into the captured run, matching what `capture_block`'s own terminator
  search already requires of an ordinary sibling line.
- Observable consequence for `std::conventions::screenplay` (mounted into
  every compiled project's `Environment` manifest since #2080): `cue`
  (attach mode, issue #2166) now claims `@NAME: dialogue` the same way it
  claims a bare `@NAME` line — the compact form's dialogue carries `cue`'s
  `speaker` attach data (`OutputLine.element.data`) exactly like a block
  cue's own following dialogue does.

No `use std::conventions::screenplay` import path exists yet (#2167/#2198),
so this preset is still only reachable by inlining its source — this
changeset is filed because the mounted preset's own lowering shape
changed, which `@brink-lang/web` re-exports through the `Environment`
manifest every compile mounts it into.
