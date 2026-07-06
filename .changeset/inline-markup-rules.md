---
"@brink-lang/editor": minor
---

Extensible inline-markup rules (#367): `inlineMarkup(rules)` lets hosts register inline-markup shapes (single `pattern`, or `open`/`close` pair with an optional `contentClass`) that decorate as `brink-markup-<name>` marks with `data-*` attributes from named capture groups. Matching is content-region scoped — rules run only within the narrative content text of classified lines and never over ink syntax (glue `<>`, threads `<-`, divert arrows, choice brackets, sigil prefixes, hidden screenplay sigils). Ships zero rules by default; the RMMZ-style angle-tag rule is exported as the optional `rmmzAngleTagRule` preset. Styling is entirely host-side (classes only, per #363).
