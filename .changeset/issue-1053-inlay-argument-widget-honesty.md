---
"@brink-lang/web": patch
---

IDE: extend semantic-type honesty to inlay hints and the argument-widget
slot data (issue #1053).

#1027 made hover and signature help honest about an unregistered semantic
type — an explicit warning marker and `E040` cross-reference instead of a
bare, confident name. Parameter-name inlay hints and the `type_name` carried
on argument-widget slots (`getArgumentWidgetsDoc`) still rendered the bare
name regardless of registration.

Both now reuse #1027's `ResolvedType::is_registered()` / `honest_type_display`
convention exactly: an inlay hint's type portion renders
`id: var_id ⚠ unregistered semantic type — E040` for an unregistered type,
unchanged for a registered one. Argument-widget slots gain a new
`type_display` field carrying the same honest string — `type_name` itself
stays the bare written name (widget-kind matching, e.g.
`matchHostWidget`'s `type_name` fallback, depends on it being raw).
