---
"@brink-lang/studio": patch
---

Clicking a Binder entry for a file that is not yet open anywhere, while a
different editor group is maximized, now un-maximizes so the newly opened
tab actually paints (issue #2826). Previously this new-tab case moved focus
to a group the editor area was not rendering, so the click appeared to do
nothing — PR #2817 fixed the same symptom for the already-open-file reveal
case only.
