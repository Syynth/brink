---
"@brink-lang/studio": patch
"@brink-lang/web": patch
---

Argument-widget + editor polish.

- **Bundle the editor font** — the studio now self-hosts JetBrains Mono
  (Latin, regular/bold/italic), so embedders without it installed (e.g. RPG
  Maker MZ / NW.js) no longer fall back to the system monospace.
- **Typed widgets in the Host Functions panel** — composing a fresh call from
  the panel now uses the same value-list dropdowns, host widgets, and
  arg-group controls as the in-editor call Form, not plain text fields.
- **Host-sourced value-lists in the Form** — a slot whose semantic type
  declares `values: host` now surfaces its dropdown items from the pushed host
  cache, not just static manifest items.
