---
"@brink-lang/web": patch
---

Issue #2166: the built-in screenplay preset (`std/conventions/
screenplay.brink`, mounted into every compiled project's `Environment`
manifest since #2080) migrates `cue`/`parenthetical` off the interim
single-file `block`-capturing shape onto **attach mode**
(`@[convention(…, attach = StructName, order = N)]`, issue #2178).

- `cue`/`parenthetical` now each claim and consume **only their own
  matched line** — neither declares `block` anymore, so neither
  wrap-and-re-emits the dialogue that follows. Each declares a plain
  `struct` (`Cue { speaker: string }` / `Parenthetical { delivery: string
  }`) via `attach`, matching its own return type (`E180`-checked).
- Observable consequence: a claimed `@NAME` cue line or chain-gated
  `(delivery)` parenthetical line now renders as its declared struct's
  structural-default text (e.g. `Cue { speaker: VENDOR }`) instead of the
  old block-wrapped `NAME` / dialogue splice; the dialogue line(s) that
  follow are ordinary, unclaimed prose, rendered verbatim on their own —
  neither handler receives them anymore.
- `heading`/`transition` are unchanged (plain, non-`block`,
  non-`attach` `@[convention]` handlers).
- `order` values (`heading = 10`, `transition = 20`, `cue = 30`,
  `parenthetical = 40`) are unchanged in value but now deliberately
  justified in the preset's own module doc (none of the four patterns
  actually overlap today; the ordering anticipates future narrowing).

No consumer reads `attach` as attachment metadata yet (no `use
std::conventions::screenplay` import path exists — #2167/#2198 — so this
preset is still only reachable by inlining its source, as `tests/tier1-
native/conventions-screenplay-preset/` does); this changeset is filed
because the mounted preset's own source and lowering shape changed, which
`@brink-lang/web` re-exports through the `Environment` manifest every
compile mounts it into.
