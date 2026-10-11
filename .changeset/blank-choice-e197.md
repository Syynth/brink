---
"@brink-lang/web": patch
---

New warning E197: a choice with an empty `[]` and no text before it
(`* [] some text`) now gets inklecate's "Blank choice" warning (#3480). It
steps aside for E195 when the whole choice is empty and has no divert, and
it can be turned down through `[lints]`.
