---
"@brink-lang/web": patch
---

Host semantic-access enforcement for `#@private` definitions (M-2b,
docs/modules-spec.md §4 boundary rules 2/3), building on M-2's compile-time
visibility surface.

- **Per-definition visibility compiled into `StoryData`** — a new optional
  `.inkb`/`.inkt` `Visibility` section (tag `0x0E`) enumerates every
  `#@private` definition's `DefinitionId`. Omitted entirely for all-public
  stories, so the entire pre-modules corpus stays byte-identical and no
  format version bump is needed. Writer + reader + round-trip land together
  for both codecs.
- **Runtime refuses host semantic access to private defs.** With visibility
  enforcement on (the default), `getVar`/`setVar` on a `#@private` variable
  no-op (`undefined`/`false`), and `goToPath`/`goToPathWithArgs`/`runKnot`/
  `callFunction` into a `#@private` knot or function error. The host is
  outside every module.
- **Persistence is unaffected.** Save/load/journal/replay serialize the whole
  state, including private cells — persistence routes through `DefinitionId`,
  never the enforced name-based host surface, so pause/resume still holds.
- **Documented dev-tooling override (play-from-here).** A new
  `setDevVisibilityOverride(allow)` on the story runner and session runs the
  story with enforcement off so editors and debug hosts can start flows at
  private knots and inspect private state; the studio's "play from here"
  sessions enable it automatically. Production hosts leave it off. A host
  capability, not a language switch — the compiled program is identical
  either way.
