---
"@brink-lang/web": patch
---

A breakpoint where a debug run starts now holds, unless the run is resuming from a stop at that same place (#3679). A breakpoint on a choice line holds when that choice is taken, as ruled on 2026-10-09, and a breakpoint on a story's very first instruction holds at the start. Before, a debug run always stepped past its starting position.
