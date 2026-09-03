---
"@brink-lang/studio": minor
"@brink-lang/editor": patch
---

The editor follows the Player (#3437, ruled 2026-09-02): a **Follow**
toggle in the Player toolbar (on by default, persisted with the Player
settings, also in Settings → Player). While the story plays, each
revealed line scrolls the editor to its source — opening the file as a
preview if needed, never taking focus — and bands it (accent, full
width). Editing the document pauses follow until Run/Restart or the
toggle. Hovering a transcript row bands its source line in the editor
with a neutral hover band. `@brink-lang/editor` gains
`DocumentSessions.scrollTo` (scroll without focus or selection) and the
`follow` / `hover` execution-highlight kinds.
