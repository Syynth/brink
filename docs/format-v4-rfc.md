# Format VERSION 4 RFC — the Tier-1 value surface (one bump)

Status: **RFC for maintainer review** (#523; Tier-1 roadmap T1a step 1).
Per the §9 one-bump rule of `docs/value-model-spec.md`, VERSION 4
carries the wire surface for ALL of Tier-1 (T1a–T1e) — collections,
literal pool, move/mutate opcodes, function values, handles,
projections — plus **reserved** surface for the post-Tier-1 typed
dialect (records/structs, ruled 2026-07-11). Reserved means: tag/section
identifiers allocated and documented, encodings specified, emitted by
nothing, and the reader accepts-but-never-encounters them in 4.0.

## 1. Value tags

Existing (unchanged): `VAL_INT`, `VAL_FLOAT`, `VAL_BOOL`, `VAL_STRING`,
`VAL_LIST`, `VAL_DIVERT_TARGET`, `VAL_VAR_POINTER`, `VAL_FRAGMENT_REF`,
`VAL_NULL`.

New in 4.0 (emitted from T1a/T1b onward):

| Tag | Encoding | Notes |
|---|---|---|
| `VAL_ARRAY` | u32 len, then elements (recursive values) | tree encoding; sharing not preserved on the wire (spec §5 — save-time dedup is an optimization elsewhere, not a wire concern) |
| `VAL_MAP` | u32 len, then key/value pairs **in insertion order** | keys restricted to int/string/bool (ruled); order is semantic |

New in 4.0, emitted from later milestones (encodings final now):

| Tag | Encoding | Milestone |
|---|---|---|
| `VAL_FN_REF` | DefinitionId | T1c |
| `VAL_CLOSURE` | DefinitionId, u16 env count, then env entries: NameId, entry kind u8 (`0=val`, `1=ref`), then value (val) or cell reference (ref) | T1c |
| `VAL_HANDLE` | kind NameId, u64 id | T1d |
| `VAL_PROJECTION` | cell reference, u8 segment count, then segments (u8 kind: `0=index` i32 / `1=key` value) | T1e; the serialized form of a live path ref |

Reserved in 4.0 (typed-dialect era; allocated, never emitted):

| Tag | Encoding (specified now) | Rationale |
|---|---|---|
| `VAL_RECORD` | ShapeId (u32 into StructShapes), then field values in shape order | closed-shape records; flat fields + interned shape (ruled 2026-07-11) |

`cell reference` above = the existing `VAL_VAR_POINTER` payload shape,
reused not reinvented.

## 2. Sections

- **`LiteralPool` (new)**: content-hash-deduplicated constant values of
  any encodable tag, referenced by u32 index. **Absorbs `ListLiterals`**:
  in 4.0 the ListLiterals section is gone; `PushList(idx)` is retired in
  favor of `PushLiteral(idx)` (converter migration is mechanical — its
  list literals intern into the pool). Pool entries are loaded as Arc
  bumps at runtime; COW makes them unpoisonable.
- **`StructShapes` (reserved)**: count always 0 in 4.0. Entry encoding
  specified: ShapeId, name NameId, u16 field count, field NameIds in
  declaration order.
- No handle-kinds section: a handle's kind is a NameId; the kind
  *vocabulary* lives in the external manifest (analyzer side), not the
  format.

## 3. Opcodes

Named here; numeric assignment at implementation inside one contiguous
reserved block so future additions within Tier-1 need no renumbering.

**Collections (T1a)**: `ArrayNew(n)`, `MapNew(n)` (pop 2n),
`IndexGet`, `IndexSet`, `Len`, `MapGet`, `MapInsert`, `MapRemove`,
`MapContains`, `Keys`, `Values` (return arrays — v1 loop compilation
iterates arrays; dedicated iterator opcodes deliberately NOT allocated,
revisit only with profiling), `PushLiteral(u32)`.

**Sharing discipline (T1a)**: `TakeVar(slot)` (move out, leave Null —
the last-use elision target), `StoreVarIfNew` (store-time keep-old-Arc
cutoff, optional), `EqVars(a, b)` (fused compare with optional collapse
— reserved, optional per spec §6).

**Functions (T1c)**: `PushFnRef(DefinitionId)`, `MakeClosure(env
descriptor)`, `CallValue(argc)`.

**Handles (T1d)**: none — handles are values; bindings do the work.

**Projections (T1e)**: `MakeProjection(desc)`, `ProjRead`, `ProjWrite`
(root-cell RMW semantics per spec §7).

**Records (reserved, unallocated numerically but named)**:
`RecordNew(ShapeId)`, `RecordGet(offset)`, `RecordSet(offset)`,
`RecordGetDyn(NameId)` (untyped fallback).

**Methods need no wire surface** (completeness note): the typed
dialect's method calls are statically dispatched — `x.f(args)`
compiles to a direct call with the receiver as an implicit `ref`
projection (`f(ref x, args)`), so no dispatch opcodes, method tables,
or value tags exist beyond what this RFC already specifies. Data-
carried function values (closure in a map/record field) use
`VAL_FN_REF`/`VAL_CLOSURE` + `CallValue` as-is.

## 4. Compatibility & discipline

- Reader stays strict (`version != 4` rejects). Converter output is
  unchanged except mechanical ListLiterals→LiteralPool migration —
  byte-diff of converter output is expected and verified equivalent by
  the oracle corpus (behavior identical; the inkt dump normalizes).
- New opcodes are inert until each milestone's compiler work emits them
  — every intermediate merge stays oracle-neutral (roadmap ordering).
- The `.inkt` text format grows matching atoms per tag/section, printed
  only when present (converter/compiler dump parity preserved).
- Checked-in `.inkb` artifacts regenerate once, with the bump.

## 5. What sign-off means

Approving this RFC freezes: the tag/section/opcode *inventory* (names,
encodings, reservation status) and the ListLiterals absorption. It does
NOT freeze numeric assignments (implementation detail inside the
reserved block) or any runtime representation (that's `Value` in
brink-runtime, already ruled). Post-approval: #524 (runtime value core)
builds against this; #526 lands the bytes with it.
