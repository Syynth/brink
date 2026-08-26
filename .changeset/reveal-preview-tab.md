---
"@brink-lang/studio": patch
---

Revealing a location now opens the file as the editor group's **preview**
tab instead of a pinned one. `editor.reveal` is the shared destination of
every navigation surface — search results, Problems, TODOs, Find
References, cross-file go-to-definition — so each jump used to mint a
permanent tab and a few minutes of browsing buried the tab strip. The next
reveal now replaces the preview in place; editing it (or double-clicking
the tab) pins it, and revealing into a file that is already pinned leaves
it pinned.
