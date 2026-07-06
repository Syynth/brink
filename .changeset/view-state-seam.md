---
"@brink-lang/editor": minor
---

DocumentSessions: per-view cursor+scroll save/restore seam (#347). New `viewState(docKey, groupId?)` reads `{ anchor, head, scrollTop }` from the live view when mounted or from the cached slot (EditorState selection + unmount scroll snapshot) for backgrounded tabs, so hosts can persist every open tab, not just the focused one. New `restoreViewState(docKey, state)` re-applies a saved snapshot on the next mount (or immediately when mounted) via the pending-reveal mechanism — full selection + pixel scroll, no focus steal. Scroll now also survives in-session background/remount cycles.
