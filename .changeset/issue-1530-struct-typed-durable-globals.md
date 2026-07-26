---
"@brink-lang/web": patch
---

Compiler: a struct construction literal is now a legal `VAR`/`CONST`
declaration default, so struct-typed durable globals are spellable (issue
#1530).

`VAR p = Point#{x: 1.0, y: 2.0}` — and, on the native surface,
`var p: Point = Point { x: 1.0, y: 2.0 }` — used to be refused outright with
`E075`, because the LIR's compile-time constant domain had no
record-carrying value. A well-formed literal now folds into a real record
that is baked into the compiled story, so reading a field of such a global
before anything writes to it yields the declared value.

That unblocks the T1e projection-receiver path end to end: a projection's
root must be a durable cell, so `g.hp.heal(5)` — a method call whose
receiver is a projection off a global — had no spelling that could reach it,
and `E143`'s own advice ("bind the receiver to a durable cell") pointed at
something the language could not express.

`E075` is narrowed rather than removed: a declaration default is baked into
the story with no runtime construction step left to fault at, so a literal
that omits a declared field or supplies an undeclared one stays a compile
error under both `types` policies (under `types = strict` the analyzer's
more precise `E069`/`E070` reports first). Its message changes accordingly.
Two knock-on diagnostic changes: an unresolved shape name in that position
now reports `E073` (the same code the expression-position path uses), and a
never-constant *field value* now reports `E077`, the same code an array
element or map value in that position already did — previously the whole
literal was rejected before any field was examined.
