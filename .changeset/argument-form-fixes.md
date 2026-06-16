---
"@brink-lang/studio": patch
"@brink-lang/web": patch
---

Argument-widget fixes.

- **Embedded host content theming/positioning** — widget popovers (the color
  picker, host pickers, the call Form) now mount inside the `.brink-studio` root
  and use `position: fixed`, so embedded host content inherits the theme tokens
  and positions correctly when the studio is embedded in a host page (rather than
  rendering unstyled or mis-placed against `document.body`).
- **Auto-open on completion-accept** — the completion kind map was keyed by the
  wrong casing, so every completion was typed `"text"`. This both mis-iconed
  completions and disabled "open the Form when accepting a function completion".
- **The call Form is driven by the signature metadata**, not the live call-site,
  so a partial or over-full call still renders its declared widgets (e.g. an
  arg-group picker) instead of degrading to plain text fields; Apply writes a
  well-formed call.
