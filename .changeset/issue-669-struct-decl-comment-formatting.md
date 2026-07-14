---
"@brink-lang/web": patch
---

`brink-fmt`'s `STRUCT` declaration formatting (TM-4b) no longer silently
drops comments living inside the struct body. Observable through
`@brink-lang/web` via the `FormatKnot` code action
(`brink_ide::code_actions::format_region` → `brink_fmt::format`):

- Multiline `STRUCT` bodies now preserve leading, interleaved, and
  same-line trailing comments between/around fields instead of dropping
  them.
- Single-line `STRUCT` bodies preserve interleaved block comments instead
  of dropping them.
- Removed an unreachable dead branch in the multiline struct renderer.
