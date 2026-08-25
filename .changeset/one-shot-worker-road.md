---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

The one-shot analysis family rides the worker road (#3110, closing the last main-thread analysis paths outside documented fallbacks): goto-definition and find-references become async sources (the cmd-click gesture is claimed immediately and lands on resolution — with CM's multi-cursor emulated when nothing resolves), the inline-rename family resolves through the client (`startInlineRename` pre-resolves the target via a resolver facet and dispatches on landing; the live breakage badge lands through `InlineNameInput`'s existing pending machinery; the context menu's identity/rename gating resolves before the menu opens), symbol-tab ranges resolve hint-first with an async worker verify that restores a degraded fragment at its fresh offsets, and search-card highlighting fetches asynchronously (cards render unhighlighted and colorize on landing). The main-thread analysis boundary guard's allowlist shrinks to the choke-point fallbacks only.
