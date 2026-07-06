---
"@brink-lang/editor": minor
"@brink-lang/studio": patch
---

Headless-ready editor (#363): the `brinkTheme` skin is now opt-out — pass `theme: false` to `brinkStudio(...)` (or `DocumentSessions`'s new options bag) for a fully headless editor, or pass your own CM `Extension` to substitute it; the default is unchanged and brink-studio now opts into `brinkTheme` explicitly. All presentational inline styles on editor-owned popups and widgets (code-actions menu, inline element picker, widget popover, argument-form anchor, inlay hints, color swatch + picker) moved onto classes; dynamic values (popup coordinates, swatch colors) ride on CSS custom properties consumed by a new always-on, zero-specificity (`:where(...)`) structural stylesheet (`ensureStructuralStyles`, exported), so hosts can restyle the full class taxonomy directly. The taxonomy — element line classes (an open `brink-<kind>` scheme), structural decoration classes, floating-surface classes, and custom properties — is now documented as a semi-stable contract in docs/editor-consumer-guide.md.
