---
"@brink-lang/editor": patch
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

References in the hover card are now navigable. The cells an `effects` row
names, and the file in *Defined in*, are links to their declarations —
clicking one reveals it, the same route goto-definition already used.

The card named things without letting you reach them, which made it a
readout rather than a way to move.

- `HoverInfo` gains `links`, and content refers to them as `[text](#N)`. An
  index rather than a path inside the link target, deliberately: a path in
  markdown has to survive `)` and `:` inside it, and that escaping is a
  silent-corruption bug waiting on the first bracket in a filename.
- Atoms with nowhere to go stay plain text — `calls` atoms are raw external
  names with no symbol to point at, and the compiler-owned `rng` cell has no
  declaration. A link that navigates nowhere is worse than plain text.
- An embedder that passes no navigate hook gets plain text too, the same
  rule "Add to dictionary" follows.
- Effect atoms are now individually code-styled rather than the whole row
  being one code span, and clause labels and status words (`pure, silent,
  total`) read as prose.
