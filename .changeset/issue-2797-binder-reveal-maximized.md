---
"@brink-lang/studio": patch
---

Clicking a Binder entry for a file that is already open in another editor
group, while a different group is maximized, now un-maximizes so the
revealed group actually paints (issue #2797). Previously the reveal moved
focus to a group the editor area was not rendering, so the click appeared
to do nothing.
