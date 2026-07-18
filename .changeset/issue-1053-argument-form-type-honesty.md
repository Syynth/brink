---
"@brink-lang/editor": patch
---

Argument Form: an unregistered semantic type's field label now shows the
same honesty marker hover/signature help use, instead of a bare, confident
type name (issue #1053, extending #1027).

`FormField` gains an optional `typeDisplay` — when the brink-ide-supplied
`CallWidgetSite` carries it, the Form's label renders it in place of the raw
`typeName` (e.g. `id: var_id ⚠ unregistered semantic type — E040`); a
registered type's label is unchanged. A producer that hasn't upgraded (no
`typeDisplay`) still gets the previous bare-name label — this is additive,
not a breaking change to `FormField`.
