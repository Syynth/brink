---
"@brink-lang/editor": patch
---

Fixes stuck/unescapable menus and popovers (#279).

The global Escape safety net added alongside the capture-phase dismiss fixes
(`dismiss-registry.ts`) previously attached its own listener on `document` in
the capture phase — the same phase every individual surface uses for its own
dismiss listener. Because the net installs once, on the very first surface
that ever registers, it ended up running *before* a surface's own listener on
every subsequent open: on the code-actions menu (Ctrl-./Cmd-.) this stripped
focus return and let Escape leak to CodeMirror's keymap; on the argument-
widget popover/modal chrome it defeated their own `preventDefault()`/
`stopPropagation()` outright. The net now attaches on `window` in the bubble
phase, so it only ever runs after every capture-phase listener already had a
chance to handle the event — restoring each surface's own dismiss behavior
while keeping the net's resilience against an orphaned listener intact.

Also: `InlineNameInput` (the shared F2-rename / extract-to-knot inline
prompt) is now wired into the same safety net — its own Escape handling was
scoped to the `<input>` element, so Escape did nothing while the breakage
report's force-override button (a sibling subtree) held focus; and the
inline element-type picker's (`keybindings.ts`, Alt+Enter) outside-dismiss
listener moved from a bubble-phase `mousedown` to a capture-phase
`pointerdown`, matching the dismiss contract everywhere else.
