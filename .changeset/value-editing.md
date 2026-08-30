---
"@brink-lang/studio": patch
"@brink-lang/web": patch
"@brink-lang/editor": patch
---

Live value editing (W16, spec §F6 RULED): scalar globals and frame locals are click-to-edit in the Debugger panel while paused — inline mono input, Enter commits, Esc cancels, a parse/type-refused edit red-shakes with nothing written; edits can never change a value's type. Globals commit through the observed write path (`WebSession.debugEditGlobal`); locals through the new set-temp-in-frame debug seam (`debugEditTemp`), disabled at choice stops where choosing would restore the choice's captured thread over the edit. "Reveal in Program Explorer" now only appears in the editor's line menu while a session can actually resolve it (`canRevealInstructions` gate).
