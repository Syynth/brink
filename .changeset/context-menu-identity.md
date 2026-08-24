---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Context-menu matrix, identity rows: right-clicking any identity-bearing token — divert targets, VAR/CONST/temp/param references, list items, labels, EXTERNAL calls, including refs inside `{interpolations}` — adds a Navigate/Rename group above the text group: Go to Definition (the ⌘-click path), Find References (the ⇧⌥F highlight), and Rename '<name>'… opening the inline-rename UI with its breakage report. The identity test is "goto-definition resolves here", so exactly the tokens with definitions get the group; the actions reuse the same callbacks as their keyboard/mouse counterparts (`navigateToLocation` and `showReferencesAt` extracted as shared entry points).
