---
"@brink-lang/web": patch
---

Runner-free lines table, and per-scope byte sizes on the program model

Two thin surfaces for the Program Explorer redesign (#3339):

- `linesTableOf(storyBytes)` — the static mirror of
  `StoryRunner.linesTable`, off raw `.inkb` bytes. The Line tables view
  shows compiled output, which exists the moment a compile lands, with or
  without a running story.
- `KnotNode.byte_size` / `container_count` — each scope's total bytecode
  bytes and container count, anonymous children (gathers, choice targets)
  included. Those children are deliberately not tree nodes, so this
  rollup is the only place their bytes are visible to size accounting.
