---
"@brink-lang/studio": patch
---

Player: tabbing away from the Player tab and back no longer replays the arrival
animation for the whole transcript or throws away the scroll position.

The shell mounts only a group's active tab, so switching tabs unmounts the
Player and switching back rebuilds every row as a new DOM node. The
`player-row-in` entrance animation therefore replayed across the entire
transcript — blanking every already-read line for its 240ms delay before
fading it back in — and the fresh scroll container dropped an author who had
scrolled up to read back, smooth-scrolling them to the bottom again.

Lines already delivered when a pane instance mounts now render settled, with
the entrance off; lines that arrive afterwards animate exactly as before, and a
restart still plays the entrance from the first line. Scroll offset is
remembered per view, so a split-duplicated Player keeps two independent places.
The session itself was never involved: a remount touches no story state.

Opening a file from the Player also no longer buries it. `openDocument`
defaults to the focused group and a click inside the Player focuses the
Player's group, so "open in the editor" put the file straight over the
transcript — the one pair you want side by side. A new file now goes to
another split when one exists; with a single group, or for a file already open
somewhere (which the reveal policy focuses in place), nothing changes.
