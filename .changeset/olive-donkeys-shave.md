---
"@brink-lang/studio": patch
"@brink-lang/editor": patch
---

Remember the editor across a reload: open tabs and their order, pin/preview
state, the active tab per group, the split structure and its sizes, and each
open document's cursor and scroll. State is scoped per project — the host
names the scope (`mountStudio`'s `sessionScope`; the desktop passes the
project root) — so two projects keep their own layouts instead of
overwriting one another, with a least-recently-used cap on how many are
remembered. A project with nothing remembered still opens as the default
two-up, and tabs naming files that no longer exist are dropped on restore.
