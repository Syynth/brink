---
"@brink-lang/web": patch
---

TM-4c (#666): structs become executable — LIR lowering + codegen for
construction, field reads, and single-level field writes, per
`docs/typed-mode-spec.md` §6.

- **LIR** (brink-ir): `Expr::StructLiteral` lowers to `RecordNew(shape_id)`
  with initializers reordered into shape declaration order (each evaluated
  exactly once; see `lower_struct_literal`'s doc for the evaluation-order
  caveat when the author's field order differs from the shape's own).
  `Expr::FieldAccess` (and the ambiguous multi-segment-`Path` shape a bare
  `p.x` parses as) lowers to a `RecordGet` read, chaining through nested
  struct-typed fields. `p.field = expr`/`p.field op= expr` lowers through
  the ratified take → `make_mut` → write-back RMW discipline, mirroring
  `lower_indexed_assignment`'s single-level (`n == 1`) fast path — a
  **chained** write (`p.a.b = v`) or a **mixed** chain (`arr[i].field = v`)
  is a real, non-suppressible `E074` diagnostic (T1e boundary), never a
  silent miscompile. `E072` (the old "reject every struct construct"
  backstop) is retired; `E073` is its narrower replacement (a construction
  literal naming an unresolved shape reaching LIR).
- **Codegen** (brink-codegen-inkb): emits the `StructShapes` table (shape
  ids interned deterministically, declaration order — never `HashMap`
  iteration); field ops default to the by-name `RecordGetDyn`/`RecordSetDyn`
  forms. Under `types = strict`, when a field access's record shape is
  provably known at compile time (a `VAR`/`temp` carrying a TM-2
  struct-typed annotation, or a direct construction-literal chain — never
  general type inference, which `brink-ir` cannot depend on), it emits the
  static-offset `RecordGet`/`RecordSet` forms instead. `types = gradual`
  never emits the offset forms, even with an annotation present (the
  annotation is unenforced there, so trusting it would be unsound).
- **Runtime** (brink-runtime/brink-format): materializes the reserved
  `RecordGet`/`RecordSet` (`0xD1`/`0xD2`) opcodes — flat bounds-checked
  offset into the record's own field vector, no shape re-check (that's the
  performance payoff over the by-name forms); out-of-range is a
  turn-terminating `RuntimeError::RecordFieldOffsetOutOfRange`, never
  UB/panic. Same COW (take → `make_mut` → write-back) discipline as
  `RecordSetDyn`.
- **Gradual construction faults** (value-model-spec §11c): a construction
  literal missing a declared field or supplying an undeclared one compiles
  under `types = gradual` (strict already rejects it at `E069`/`E070`) to a
  deterministic runtime fault via a reserved sentinel `ShapeId`
  (`RuntimeError::InvalidShapeId`) — no new opcode needed.

Wasm-observable surface: `Opcode::RecordGet`/`RecordSet` are real variants
now (disassembler text in `brink-web::program_model` and the `.inkt`
writer both cover them); struct declarations, construction literals, field
reads, and single-level field writes all compile to real bytecode reachable
through `@brink-lang/web`'s compile/run surface for the first time.

Oracle corpus: unchanged, 5,577 passing episodes — no existing program uses
`STRUCT`/`Name#{…}`/field-access grammar.
