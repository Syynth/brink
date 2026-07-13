---
"@brink-lang/web": patch
---

Fixed #673: a collection or struct literal used as a `VAR`/`CONST`
declaration default (`VAR arr = #[1, 2, 3]`, `VAR m = #{"a": 1}`, `VAR p =
Point#{x: 1.0, y: 2.0}`) used to compile silently to `Value::Null` with no
diagnostic — `brink-ir::lir::lower::decls::eval_const_expr` (the
compile-time constant-folding path `VAR`/`CONST` defaults go through) had
no arm for `ArrayLiteral`/`MapLiteral`/`StructLiteral` and fell through to
its catch-all `_ => ConstValue::Null`.

- Array/map literal defaults (including nested ones, and constant
  references inside them, e.g. `#[SOME_CONST, 2]`) now constant-fold into
  the real `ConstValue::Array`/`Map` — the same representation
  `brink-codegen-inkb` already materializes into a real `Value::array`/
  `Value::map` global default (this wiring already existed for
  expression-position array/map literals; declaration defaults now share
  it). A map key that isn't a compile-time-constant scalar (int/string/
  bool) in a declaration default is a new compile error (`E076`) — a
  declaration default has no runtime `MapNew` construction step left to
  fault at the way a mid-story map literal does.
- A struct construction literal used directly as a declaration default is
  a new compile error (`E075`) — `ConstValue` has no record-carrying
  variant (adding one is a format question outside this fix), and unlike
  arrays/maps there's no existing runtime-construction step for a
  declaration default to defer to. Construct the struct via an ordinary
  assignment after declaration instead (`VAR p = 0` then `~ p =
  Point#{...}`).

Both `E075` and `E076` are LIR-lowering diagnostics, so — like `E053`/
`E073`/`E074` — they're never suppressible via `// brink-disable`/
`// brink-disable-all`.

Oracle corpus: unchanged, 5,577 passing episodes — vanilla ink has no
collection/struct sigil literals for this to affect.
