---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Prose checking: spelling and light grammar over a manuscript's prose.

The engine is Harper, in its own lazily-loaded wasm module — 6.5 MB gzipped,
larger than the entire compiler, so it is never in the main bundle and an
embedder that registers no checker pays nothing.

What makes it usable on fiction rather than hostile to it: the checker only
ever sees `content` spans with interpolations subtracted (never diverts,
tags, or logic), and its dictionary is seeded from the project's own names —
including the character cues, so writing the manuscript teaches it. Without
that, every invented name reports as a misspelling.

`@brink-lang/web` gains `getProseDictionary`, `getConfiguredProseDialect`
and `getConfiguredProseEnable`. `@brink-lang/editor` gains the `ProseChecker`
seam and a shared diagnostic-source registry, so the compile and the prose
check no longer overwrite each other's squiggles. `@brink-lang/studio` gains
the Prose settings section and registers the checker.
