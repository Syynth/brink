---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

The editor context menu is now always ours (docs/editor-context-menu-spec.md, phase 1): right-click anywhere in the editor suppresses the native menu — knot/stitch headers open the shared symbol menu (including **function** headers, whose clicks previously vanished: `headerName` treated the `function` keyword as part of the path), and everything else opens a text menu (Cut / Copy / Paste / Select All with shortcuts, Cut/Copy disabled without a selection) whose actions are bound to the raising view. New `onTextContextMenu` option threads through `brinkStudio`/`DocumentSessions`; the studio renders it via a new `EditorTextMenuHost` sharing the symbol menu's chrome and dismiss contract (`useContextMenuDismiss`, extracted).
