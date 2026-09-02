---
"@brink-lang/studio": patch
"@brink-lang/editor": patch
---

App setting to hide inlay hints (#3350). Settings ▸ Editor gets a "Show
inlay hints" toggle, persisted app-scope alongside the other editor
preferences and broadcast live to every open editor via
`DocumentSessions.setInlayHints` (the same `_documents?.setXxx(...)`
broadcast shape `setFormGlyph`/`setAutoOpenForm` already use) — matching
editors opened later too. Default stays ON (current behavior).
