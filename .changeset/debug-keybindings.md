---
"@brink-lang/studio": patch
---

Debug transport keybindings + status bar (W10/#3303). The spec's F-row
lands on the command descriptors (user-remappable via keymap overrides):
F5 continue · F6 pause · F9 toggle breakpoint at the focused editor's
cursor line (a new `debug.toggleBreakpoint` command, gated on a focused
ink file rather than debug capability — anchors exist without a session)
· F10 step over · F11 step into · Shift-F11 step out · Shift-F5 restart.
Function keys fire globally, including from the editor. The status bar's
story segment shows the paused state (warning dot + "paused"), and the
retired multi-session picker is fully removed (its behaviors — switch
active flow, primary not closable — live in the Debugger panel's Flows
section).
