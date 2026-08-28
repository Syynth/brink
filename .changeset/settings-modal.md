---
"@brink-lang/studio": minor
---

Settings is now a modal with a searchable section rail — Project,
Diagnostics, Editor, Appearance, Keymap — showing one section at a time,
rather than a takeover of the editor area with everything in one scrolling
column.

Whatever you were reading stays on screen behind it. Search matches what a
section is about as well as its name, so "todo" finds Diagnostics.

`registerSettingsCommand` now takes an open-callback rather than the shell
layout store, and the `settings` document type is gone.
