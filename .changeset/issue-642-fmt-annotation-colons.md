---
"@brink-lang/web": patch
---

brink-fmt: canonicalize whitespace around type-annotation colons (#642).

Type annotations in knot parameters, return types, VAR/CONST/LIST declarations,
TEMP declarations, and struct fields now render with canonical spacing:
`name: type` (no space before colon, one space after), regardless of source
spacing. This normalizes `name:type` (no space), `name: type` (space), and
`name:  type` (multiple spaces) to a consistent canonical form, matching the
ink language reference's documented style.

Changes apply to:
- Knot headers: `=== function f(x:int, y: int): int ===` → `=== function f(x: int, y: int): int ===`
- Declarations: `VAR gold:int = 100` → `VAR gold: int = 100`
- Logic lines: `~ temp name:string = who` → `~ temp name: string = who`
- Struct fields: `STRUCT P = #{x:int, y: float}` → `STRUCT P = #{x: int, y: float}`

Formatting remains idempotent: re-formatting an already-canonical annotation
produces identical output.

Observable through `@brink-lang/web`: the editor's "Format knot" code action
now produces canonicalized annotation spacing in formatted output.
