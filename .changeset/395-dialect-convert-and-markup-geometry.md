---
"@brink-lang/editor": minor
---

Generalize the last two at-cue-hardcoded sites so custom `DialogueDialect`s are functionally complete (#395, a follow-up from #368):

- **Dialect `convert` transition rows** now extract a line's content via the resolved dialect's OWN declared shapes (`ResolvedDialect.convertibleShapes()`), not the hardcoded `@name:<>`/`(text)<>` regexes. A custom dialect's non-at-cue wrapping kinds (e.g. `<<name>>`) now convert correctly via a `transitions` row's `convert` action.
- **`contentRegions`** (the inline-markup content-region scoping core) now accepts an optional third `geometry` argument (a line's cached `LineInfo.dialect`); when given, a Character/Parenthetical-shaped line's content bounds derive from `geometry.contentSpan` instead of the fixed at-cue affix-length constants, so a dialect overriding those kinds with different affix widths scopes markup correctly.
- `extractLineContent` gains an optional `shapes: ConvertibleShape[]` parameter (tried before the built-in at-cue shapes); `ConvertibleShape` is now exported.

Both changes are additive — omitting the new optional parameters reproduces the exact pre-#395 behavior for the default (at-cue) preset.
