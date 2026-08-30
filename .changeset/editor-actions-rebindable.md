---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

The editor's named actions are rebindable, and listed in Settings ▸ Keymap

Rename Symbol (F2), Find References (Shift-Alt-F), Code Actions (Mod-.),
Edit Arguments (Mod-Shift-A) and Insert Element (Alt-Enter) existed only
as chords hardcoded inside their CodeMirror extensions — invisible to the
keymap surface and unrebindable.

Each extension now provides its behaviour through a runner registry while
the chords live in one rebindable keymap
(`@brink-lang/editor`'s `EDITOR_ACTIONS` / `setEditorActionKeys` /
`runEditorAction`). The studio registers the five as ordinary commands, so
they appear in Settings ▸ Keymap and the palette, and a rebind flows back
into every open editor live — one source of truth, so the table can never
show a chord the editor disagrees with. Embedders that never touch keys
get exactly the bindings that shipped.
