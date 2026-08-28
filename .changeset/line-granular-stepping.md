---
"@brink-lang/web": patch
---

Line-granular stepping (#3264), alongside the existing instruction stepping.

`Story::debug_step_line(mode, …)` advances to the next **source line** — the
granularity every GDB-style debugger means by `step`/`next`/`finish`. The
existing `debug_step` remains and is unchanged: both granularities are
first-class, because the studio presents the `.inkt` disassembly beside the
source and drives each directly.

`DebugStopReason` gains `noLineInfo`, reported when a line-granular step is
asked for on an artifact that cannot say which line execution is on — no
debug info, or a file compiled without source text. It is reported rather
than quietly behaving like instruction stepping, which would turn a missing
line index into "why does step take four presses" instead of a legible
"this build has no line info".
