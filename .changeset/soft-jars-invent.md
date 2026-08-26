---
"@brink-lang/studio": patch
---

Add Single File view, the first of the three editor views. The editor root
area now holds one occupant: Code view (today's tabs and splits) or Single
File view, which shows one file with the player beside it and no tab strip at
all. Navigating — from the Binder, search, Problems, go-to-definition —
replaces what is on screen instead of accumulating tabs, and the player split
belongs to the view rather than being a document that happens to be open, so
it collapses and returns but never closes into an empty pane. The two views
share the active file, so switching keeps the document you were working on,
and the chosen view persists with the rest of the layout. Switch with the
"View: Single File" and "View: Code" commands.
