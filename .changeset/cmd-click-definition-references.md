---
"@brink-lang/editor": minor
---

Cmd-clicking a symbol's *definition* now runs Find References instead of
a no-op self-navigation — you're already at the definition. Use sites
keep navigating to the definition; when references are unavailable or
empty the click falls back to selecting the declaration.
