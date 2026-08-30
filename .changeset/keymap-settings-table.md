---
"@brink-lang/studio": patch
---

Rebind keys by pressing them, in Settings ▸ Keymap

The keymap surface was a raw JSON textarea: it asked an author to know
both a command id and the `"Mod-Shift-P"` spelling, and offered no way to
discover either. It is now a searchable table of every registered command,
grouped by category, with its current bindings.

Recording a binding uses the same function the global key handler
dispatches through, so what you press is exactly what will fire — a typed
binding can be spelled correctly and still not be the chord your keyboard
produces. Commands keep all their bindings as chips, because several ship
two or three defaults to dodge browser-reserved chords and an override
replaces the whole set.

Taking a key that another command holds displaces that command and says
so before saving, naming what will lose the key. The resolution table is a
map from chord to command, so two commands holding one chord means one of
them silently does nothing — the editor will not let you build that state.

The JSON stays, below the table, for anything the table cannot express.
