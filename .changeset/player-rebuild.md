---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

The rebuilt Player (W7/#3300). Every delivered line now carries its
source (`Line.source` / `DebugOutputLine.source` — file + byte range,
from the line table's own locations), and the Player makes each
transcript row a provenance handle: full-width line rows with a subtle
alternating tint, hover shows `file:line`, click (or ⌘-click the row)
reveals the source in the editor. A tags toggle renders per-line tags as
muted mono chips (off by default, persisted). The status chip is the
single home of stop reasons — ready / playing / paused at file:line /
waiting on choice / ended / error / out-of-sync — and clicking it
reveals the current line. The story no longer auto-starts (RULED): the
Player opens idle with the toolbar live; Run compiles and starts.
Auto-reveal is paced by default (RULED, ~150 ms per line, Settings →
Player to switch to all-at-once); pausing or a breakpoint stops the run
instantly. Auto-scroll suspends while reading back. Narrow-tier layouts
regain the hamburger route to a closed player, and the reopen split
honors "when there is room" (#2795).
