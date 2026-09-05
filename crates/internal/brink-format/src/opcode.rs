use core::fmt;

use alloc::vec::Vec;

use crate::codec::{
    read_def_id, read_f32, read_i32, read_u8, read_u16, read_u32, write_def_id, write_f32,
    write_i32, write_u8, write_u16, write_u32,
};
use crate::id::DefinitionId;

// ── Discriminant bytes ──────────────────────────────────────────────────────

// Stack & literals
const PUSH_INT: u8 = 0x01;
const PUSH_FLOAT: u8 = 0x02;
const PUSH_BOOL: u8 = 0x03;
const PUSH_STRING: u8 = 0x04;
const PUSH_LIST: u8 = 0x05;
const PUSH_DIVERT_TARGET: u8 = 0x06;
const PUSH_NULL: u8 = 0x07;
const POP: u8 = 0x08;
const DUPLICATE: u8 = 0x09;

// Arithmetic
const ADD: u8 = 0x10;
const SUBTRACT: u8 = 0x11;
const MULTIPLY: u8 = 0x12;
const DIVIDE: u8 = 0x13;
const MODULO: u8 = 0x14;
const NEGATE: u8 = 0x15;

// Comparison
const EQUAL: u8 = 0x20;
const NOT_EQUAL: u8 = 0x21;
const GREATER: u8 = 0x22;
const GREATER_OR_EQUAL: u8 = 0x23;
const LESS: u8 = 0x24;
const LESS_OR_EQUAL: u8 = 0x25;

// Logic
const NOT: u8 = 0x28;
const AND: u8 = 0x29;
const OR: u8 = 0x2A;

// Global vars
const GET_GLOBAL: u8 = 0x30;
const SET_GLOBAL: u8 = 0x31;

// Temp vars
const DECLARE_TEMP: u8 = 0x34;
const GET_TEMP: u8 = 0x35;
const SET_TEMP: u8 = 0x36;
const GET_TEMP_RAW: u8 = 0x37;

// Variable pointers
const PUSH_VAR_POINTER: u8 = 0x38;
const PUSH_TEMP_POINTER: u8 = 0x39;

// Control flow
const JUMP: u8 = 0x40;
const JUMP_IF_FALSE: u8 = 0x41;
const GOTO: u8 = 0x42;
const GOTO_IF: u8 = 0x43;
const GOTO_VARIABLE: u8 = 0x44;

// Container flow
const ENTER_CONTAINER: u8 = 0x48;
const EXIT_CONTAINER: u8 = 0x49;

// Functions / tunnels
const CALL: u8 = 0x50;
const RETURN: u8 = 0x51;
const TUNNEL_CALL: u8 = 0x52;
const TUNNEL_RETURN: u8 = 0x53;
const TUNNEL_CALL_VARIABLE: u8 = 0x54;
const CALL_VARIABLE: u8 = 0x55;

// Threads
const THREAD_CALL: u8 = 0x57;
const THREAD_START: u8 = 0x58;
const THREAD_DONE: u8 = 0x59;

// Output
const EMIT_LINE: u8 = 0x60;
const EMIT_VALUE: u8 = 0x61;
const EMIT_NEWLINE: u8 = 0x62;
const SPRING: u8 = 0x67;
const GLUE: u8 = 0x63;
const BEGIN_TAG: u8 = 0x64;
const END_TAG: u8 = 0x65;
const EVAL_LINE: u8 = 0x66;
const BEGIN_FRAGMENT: u8 = 0x68;
const END_FRAGMENT: u8 = 0x69;
const ATTACH_ELEMENT: u8 = 0x6A;
const END_ELEMENT_RUN: u8 = 0x6B;
// Peephole superinstruction (`docs/optimizer-peephole.md`): the fused form of
// `EmitLine` immediately followed by `EmitNewline`. Emitted by the optimizer
// only; codegen never produces it, so the fence's control artifact never
// contains it.
const EMIT_LINE_NL: u8 = 0x6C;

// Peephole superinstructions, second family (`docs/optimizer-peephole.md`
// §1): a binary operator fused with the `PushInt` immediate that feeds its
// right operand and/or the `JumpIfFalse` that consumes its result. Each
// carries a [`BinaryKind`] byte naming the operator. Optimizer-only, like
// `EMIT_LINE_NL`.
const BINARY_IMM: u8 = 0x6D;
const BINARY_JUMP_IF_FALSE: u8 = 0x6E;
const BINARY_IMM_JUMP_IF_FALSE: u8 = 0x6F;

// Choices
const BEGIN_CHOICE: u8 = 0x72;
const END_CHOICE: u8 = 0x73;
// Sequences
const SEQUENCE: u8 = 0x78;
const SEQUENCE_BRANCH: u8 = 0x79;

// Intrinsics
const VISIT_COUNT: u8 = 0x80;
const TURNS_SINCE: u8 = 0x81;
const TURN_INDEX: u8 = 0x82;
const CHOICE_COUNT: u8 = 0x83;
const RANDOM: u8 = 0x84;
const SEED_RANDOM: u8 = 0x85;
const CURRENT_VISIT_COUNT: u8 = 0x86;
const TOUCH_VISIT: u8 = 0x87;
const SHUFFLE_INDEX_OF: u8 = 0x88;

// Casts / math
const CAST_TO_INT: u8 = 0x90;
const CAST_TO_FLOAT: u8 = 0x91;
const FLOOR: u8 = 0x92;
const CEILING: u8 = 0x93;
const POW: u8 = 0x94;
const MIN: u8 = 0x95;
const MAX: u8 = 0x96;

// External fns
const CALL_EXTERNAL: u8 = 0xA0;

// The fn-value verb layer (`docs/stdlib-spec.md` §4, issue #1679) — the pure
// quartet `map`/`filter`/`fold`/`filter_map` plus the effectful spellings
// `each`/`map_each`. ONE discriminant byte with a [`SeqVerbOp`] kind
// immediate, the `Tower`/`Collect` economy applied a third time, sized for the
// whole ruled family.
//
// **Why 0xA1 rather than the high tail.** The stdlib blocks above walked
// `0xF7`-`0xFD` down to a single free byte (`0xFF`); `0xCB`/`0xCC` are held
// for `StoreVarIfNew`/`EqVars`. `0xFF` is deliberately left unclaimed as the
// escape byte a future extended-opcode prefix would want, so this family takes
// the first byte of the next contiguous free run instead — `0xA1`-`0xAF`,
// immediately after `CallExternal`.
//
//   0xA1 SeqVerb(kind)  the kind byte selects the verb; see [`SeqVerbOp`] for
//                       each one's stack shape. Every kind takes its callback
//                       as a function value (`FnRef`/`Closure`) and evaluates
//                       it re-entrantly, exactly like `SeqSortedBy`.
const SEQ_VERB: u8 = 0xA1;

// v4 collection opcodes (`docs/format-v4-rfc.md` §3 "Collections (T1a)") —
// numeric assignments frozen by the §9 one-bump rule, contiguous and
// adjacent to the existing List ops block below. Live as of T1b-2 (#570):
// `Opcode` variants + VM execution exist for the whole block, though only a
// subset (`ArrayNew`, `MapNew`, `IndexGet`, `IndexSet`, `Len`, `Keys`,
// `PushLiteral`) is emitted by the compiler in T1b-2 — the map-mutator ops
// (`MapGet`, `MapInsert`, `MapRemove`, `MapContains`, `Values`) become
// compiler-reachable when the stdlib slice (T1b-3, `docs/t1b-surface-spec.md`
// §5) lands, matching the RFC's "inert until each milestone's compiler work
// emits them" discipline.
//   0xBE ArrayNew(n)     0xBF MapNew(n)        0xC0 IndexGet
//   0xC1 IndexSet        0xC2 Len              0xC3 MapGet
//   0xC4 MapInsert       0xC5 MapRemove        0xC6 MapContains
//   0xC7 Keys            0xC8 Values           0xC9 PushLiteral(u32)
const ARRAY_NEW: u8 = 0xBE;
const MAP_NEW: u8 = 0xBF;
const INDEX_GET: u8 = 0xC0;
const INDEX_SET: u8 = 0xC1;
const COLLECTION_LEN: u8 = 0xC2;
const MAP_GET: u8 = 0xC3;
const MAP_INSERT: u8 = 0xC4;
const MAP_REMOVE: u8 = 0xC5;
const MAP_CONTAINS: u8 = 0xC6;
const COLLECTION_KEYS: u8 = 0xC7;
const COLLECTION_VALUES: u8 = 0xC8;
const PUSH_LITERAL: u8 = 0xC9;
// `PushLiteral(u32)` is the T1b `LiteralPool` reference opcode (RFC §2).
// `PushList`/`ListLiterals` are unaffected by this PR — the RFC's absorption
// of `ListLiterals` into `LiteralPool` is a separate, larger migration (see
// PR description scopeNotes) that would require regenerating every checked-in
// oracle `.inkb` fixture; out of scope here by construction (nothing in this
// PR touches `PushList`/`ListLiterals` emission or decoding).
//
// Sharing-discipline ops (`TakeVar`, `StoreVarIfNew`, `EqVars`) and later
// Tier-1 groups (functions, handles, projections, records) remain named in
// the RFC but out of this reservation — each gets its own contiguous block,
// numbered when its own issue lands.

// v4 sharing-discipline opcodes (`docs/format-v4-rfc.md` §3 "Sharing
// discipline (T1a)"; semantics in `docs/value-model-spec.md` §5/§6) —
// numeric assignments frozen by the §9 one-bump rule, contiguous and
// adjacent to the collection block above. Live as of T1b-4 (#576):
//   0xCA TakeGlobal(DefinitionId)   0xCB StoreVarIfNew (reserved)
//   0xCC EqVars(a, b) (reserved)    0xCD TakeTemp(u16)
// `TakeGlobal`/`TakeTemp` are the RFC's generic `TakeVar(slot)` split into
// its two concrete slot kinds (global `DefinitionId` vs temp `u16` — they
// don't share an operand encoding, so one opcode can't cover both): each
// moves the slot's current value out and leaves `Value::Null` behind — the
// take-half of the take → `make_mut` → write-back RMW discipline (spec §5)
// that closes the indexed-write COW cliff. `TakeTemp` mirrors `GetTemp`'s
// pointer auto-dereference (`ref` params): if the temp holds a
// `VariablePointer`/`TempPointer`, the *pointed-to* location is taken, not
// the pointer value itself (see `vm.rs`'s `Opcode::TakeTemp` arm).
// `TakeGlobal` does not auto-dereference — `GetGlobal`/`SetGlobal` don't
// either, since ref-params live in temps, never in globals themselves.
// `0xCD` is claimed fresh, adjacent to this block, rather than reusing
// `0xCB`/`0xCC` — those stay reserved for `StoreVarIfNew`/`EqVars` exactly
// as the RFC named them; splitting `TakeVar` doesn't touch their
// numbering. `StoreVarIfNew` and `EqVars` remain reserved (comments only,
// no `Opcode` variants — `decode`'s catch-all keeps rejecting both bytes)
// — the optional ref-collapsing sites from spec §6 (store-time keep-old-Arc
// cutoff; fused compare with optional collapse on structural equality),
// pure peephole optimizations, never required for correctness. Later
// Tier-1 groups (functions, handles, projections, records) remain named in
// the RFC but out of this reservation — each gets its own contiguous
// block, numbered when its own issue lands.
const TAKE_GLOBAL: u8 = 0xCA;
const TAKE_TEMP: u8 = 0xCD;

// v4 record opcodes (TM-4, `docs/typed-mode-spec.md` §6; named but
// numerically unallocated in `docs/format-v4-rfc.md` §3 — "design the exact
// encoding against the reserved space" — assigned here) — contiguous and
// adjacent to the sharing-discipline block above:
//   0xCE RecordNew(ShapeId)      0xCF RecordGetDyn(NameId)
//   0xD0 RecordSetDyn(NameId)    0xD1 RecordGet(offset)
//   0xD2 RecordSet(offset)
// `RecordNew`/`RecordGetDyn`/`RecordSetDyn` (PR #620/TM-4 foundation) are the
// by-name field ops every dialect can use correctly. `RecordGet`/`RecordSet`
// (TM-4c, #666) are the static-offset field ops, the strict-mode-only
// performance payoff typed-mode-spec §6 anticipates: `brink-ir`'s LIR
// lowering only emits them when a field access's record shape is proven at
// compile time (see `docs/typed-mode-spec.md` §6 and the TM-4c PR
// description) — the operand is a flat `u16` index into the record's own
// field vector, checked only against that vector's bounds at runtime (no
// shape re-verification — the "offset" payoff is skipping exactly that
// lookup), so out-of-range is a turn-terminating fault
// (`RuntimeError::RecordFieldOffsetOutOfRange`), never UB/panic.
const RECORD_NEW: u8 = 0xCE;
const RECORD_GET_DYN: u8 = 0xCF;
const RECORD_SET_DYN: u8 = 0xD0;
const RECORD_GET: u8 = 0xD1;
const RECORD_SET: u8 = 0xD2;

// TM-3 completion conversion intrinsics (`docs/typed-mode-spec.md` §4,
// maintainer ruling 2026-07-13, issue #659) — contiguous and adjacent to the
// record block above, this PR's own reservation (no prior RFC allocation for
// these three; the record block's own "assigned here" precedent applies).
const CONVERT_INT: u8 = 0xD3;
const CONVERT_FLOAT: u8 = 0xD4;
const CONVERT_STRING: u8 = 0xD5;

// Function-value opcodes (T1c, `docs/format-v4-rfc.md` §3 "Functions" —
// named there, numerically unallocated; assigned here, contiguous and
// adjacent to the conversion block above, this PR's own reservation). First
// live emission of the reserved function-value surface (`docs/t1c-spec.md`
// §11 T1c-2).
//   0xD6 PushFnRef(DefinitionId)          0xD7 MakeClosure(env descriptor)
//   0xD8 CallValue(argc)                   0xD9 BindValue(argc)
// `PushFnRef` pushes the zero-bound `Value::FnRef`. `MakeClosure`'s operand is
// the target `DefinitionId` then a u16-counted descriptor of `{NameId, kind
// u8 (0=val,1=ref)}` entries — one per bound arg, in declared order — and it
// pops that many values off the stack (bound in order) to build a
// `Value::Closure`. `CallValue(argc)` pops the callee (top of stack) then the
// `argc` supplied (val-only) args below it, dispatching through the function
// value (`docs/t1c-spec.md` §3): non-function callee / wrong arity /
// rehydration mismatch / cross-flow ref-`#@local` are turn-terminating faults.
// `BindValue(argc)` (T1c-3, `bind(f, args…)` stdlib intrinsic) pops the callee
// (top of stack) then the `argc` supplied (val-only) args below it and returns
// a *new* function value with those args appended to the callee's bound-arg
// row (val-only currying — consuming the head of the remaining param row). The
// newly bound entries take their name/mode from the target's own signature at
// the appended positions (always `val`, since `ref` params are bound away at
// creation). Faults (turn-terminating): callee is not a function value;
// binding more args than the target has remaining params.
const PUSH_FN_REF: u8 = 0xD6;
const MAKE_CLOSURE: u8 = 0xD7;
const CALL_VALUE: u8 = 0xD8;
const BIND_VALUE: u8 = 0xD9;

// Projection opcodes (T1e, `docs/format-v4-rfc.md` §3 "Projections" — named
// there, numerically unallocated; assigned here, contiguous and adjacent to
// the function-value block above, this PR's own reservation). First live
// emission of the reserved projection surface (`docs/t1e-spec.md` §3/§8
// T1e-2).
//   0xDA MakeProjection(root, segment_count)   0xDB ProjRead
//   0xDC ProjWrite
// `MakeProjection` pops `segment_count` values off the stack (pushed by
// codegen in source order; the VM's LIFO pop collects them reversed, then
// reverses once more to restore source order — same shape `MakeClosure`'s
// bound-arg row uses) and classifies each into a `ProjSegment` (`Int` →
// `Index`, else → `Key`,
// `docs/format-v4-rfc.md` §1), building a `Value::Projection` rooted at
// `root`. `ProjRead`/`ProjWrite` implement the spec's root-cell RMW
// discipline (take root → walk → `make_mut` spine → write → store back);
// both fault `RuntimeError::ProjectionInvalidated` on a path that no longer
// resolves (shrunk array, missing key, removed struct field — spec §1(2)).
// The compiler's own emission path for dereferencing a projection-bound
// `ref` parameter reuses the *same* underlying walk (`brink_runtime::proj_ops`)
// through `GetTemp`/`SetTemp`/`TakeTemp`'s additive `Value::Projection`
// dispatch arm rather than interleaving these bytes at every param access —
// see those opcodes' VM dispatch for the shared implementation. `ProjRead`/
// `ProjWrite` remain real, independently encodable/dispatchable opcodes.
const MAKE_PROJECTION: u8 = 0xDA;
const PROJ_READ: u8 = 0xDB;
const PROJ_WRITE: u8 = 0xDC;

// `char_at(s, i)` stdlib pure function (T1b stdlib slice 1 completion, issue
// #857) — contiguous and adjacent to the projection block above, this PR's
// own reservation (no prior RFC allocation for this one; same "assigned
// here" precedent as the record/conversion/function-value/projection blocks
// above it). Pops `i` then `s`, pushes the single-character `String` at
// Unicode-scalar-value index `i` (chars, not UTF-8 bytes — author sanity per
// the issue). Turn-terminating faults (value-model-spec §11c): `s` isn't a
// `String` (`RuntimeError::NotIndexable`); `i` isn't an `Int`
// (`RuntimeError::CharAtIndexNotInt`); `i` outside `[0, char_count)`
// (`RuntimeError::CharAtOutOfBounds`, `len` = char count).
const CHAR_AT: u8 = 0xDD;

// NS-A1 Option[T] + the ruled stdlib flips (`docs/stdlib-spec.md`
// §1.1/§1.4, §§3-5; `docs/stdlib-sequencing.md` §2 Wave A1) — this PR's own
// reservation, same "assigned here" precedent as the record/conversion/
// function-value/projection/char_at blocks above. `PUSH_NONE`/`MAKE_SOME`
// take the two bytes remaining before the string-eval block (0xE0/0xE1);
// the verb flips continue contiguously after it at 0xE2.
//
// Option construction:
//   0xDE PushNone   `[]` → `none`  (`Value::OptionVal(None)`)
//   0xDF MakeSome   `[x]` → `some(x)` — total over every value
//
// The verb flips, all brink-dialect intrinsics returning `Option` (absence
// = `none`, never a fault; malformed *questions* — wrong container type,
// unorderable elements — stay turn-terminating faults, the ruled
// fault-vs-absence doctrine):
//   0xE2 StrFind         `[s, sub]` → `Option[int]` (USV index, not bytes)
//   0xE3 SeqIndexOf      `[a, x]` → `Option[int]` (structural equality)
//   0xE4 SeqMin          `[a]` → `Option[T]` (empty → none)
//   0xE5 SeqMax          `[a]` → `Option[T]`
//   0xE6 SeqFirst        `[a]` → `Option[T]`
//   0xE7 SeqLast         `[a]` → `Option[T]`
//   0xE8 SeqPop          `[a]` → pushes `Option[T]` (popped element or
//                        none), then the shrunk array on top — codegen
//                        brackets it Take*/SeqPop/Set* so the array writes
//                        back to its root cell and the Option remains as
//                        the expression value
//   0xE9 MapGetOpt       `[m, k]` → `Option[V]` (missing key → none; a
//                        non-scalar key is a fault — malformed question)
//   0xEA MapContainsValue `[m, v]` → `Bool` (content-equality scan, O(n))
//   0xEB MapClear        `[m]` → empty map (statement-only mutator;
//                        in-place-ness comes from the RMW write-back)
//
// `SeqMin`/`SeqMax` order int/float (numeric promotion), bool, string for
// now, with float NaN placed by the ruled PROD pinned order (§4b: NaN
// greater than everything, NaN-vs-NaN ties, -0 == +0). The dev-mode
// NaN-fault and the full orderable roster (arrays-lexicographic, compare
// protocol) land with wave A4 — the rows are mode-independent either way.
const PUSH_NONE: u8 = 0xDE;
const MAKE_SOME: u8 = 0xDF;
const STR_FIND: u8 = 0xE2;
const SEQ_INDEX_OF: u8 = 0xE3;
const SEQ_MIN: u8 = 0xE4;
const SEQ_MAX: u8 = 0xE5;
const SEQ_FIRST: u8 = 0xE6;
const SEQ_LAST: u8 = 0xE7;
const SEQ_POP: u8 = 0xE8;
const MAP_GET_OPT: u8 = 0xE9;
const MAP_CONTAINS_VALUE: u8 = 0xEA;
const MAP_CLEAR: u8 = 0xEB;

// NS-A6 rand verbs (`docs/stdlib-spec.md` §7; this PR's own reservation,
// same "assigned here" precedent as the NS-A1 block above): the four draw
// ops fill the remaining bytes before the lifecycle block (0xF0+),
// contiguously after NS-A1's 0xEB. `seed(n)` reuses the frozen
// `SEED_RANDOM` byte (0x85) — one cell, two surfaces, no drift.
const RAND_FLOAT: u8 = 0xEC;
const RAND_CHANCE: u8 = 0xED;
const RAND_PICK: u8 = 0xEE;
const RAND_SHUFFLE: u8 = 0xEF;

// NS-A5 range ops (`docs/stdlib-spec.md` §7, F7 ruled 2026-07-19; this
// PR's own reservation). The 0xEC-0xEF block is full and 0xF0-0xF3 are
// lifecycle, so these take the next free bytes after the lifecycle block's
// tail. Two construction ops rather than one flag-operand op keeps the
// whole rand/range family operand-free (disasm and roundtrip stay
// table-driven). `rand::int` deliberately has NO byte here — it rides the
// existing `CONVERT_INT` (0xE2): `int(x)` is ONE value-directed verb whose
// range leg is the draw (see `brink-runtime::vm`'s `ConvertInt` dispatch).
const RANGE_MAKE_EXCL: u8 = 0xF4;
const RANGE_MAKE_INCL: u8 = 0xF5;
const RANGE_NON_EMPTY: u8 = 0xF6;

// NS-A8 numeric tower (`docs/tower-mini-spec.md`, issue #1114; this PR's own
// reservation, same "assigned here" precedent as the NS-A1/NS-A6/NS-A5
// blocks above). ONE discriminant byte with a `u8` kind immediate —
// deliberate opcode-space economy: after NS-A5 the free one-byte space is
// down to 0xF7-0xFD + 0xFF (0xF0-0xF3 are lifecycle, 0xF4-0xF6 the range
// ops; 0xFE held the retired debug opcode, freed by #3180 — see the
// retirement comment below `END_STRING_EVAL`), so the tower's thirteen
// operations share a single `Tower(TowerOp)` opcode instead of eating
// most of what remains. Wire:
// `0xF7`, then the [`TowerOp`] kind byte (a `SequenceKind`-style closed
// sub-enum; unknown kinds are a decode error, the same reserved-tag
// discipline as everywhere else).
const TOWER: u8 = 0xF7;

// NS-A4 ordering verbs (`docs/stdlib-spec.md` §4b, issue #1110; this PR's
// own reservation, same "assigned here" precedent as the NS-A1/A5/A6/A8
// blocks above). Two ops serve all four source verbs: `sort(a)` /
// `sorted(a)` share `SeqSorted` (in-place-ness comes from the RMW
// write-back, the `shuffle`/`shuffled` precedent), and `sort_by(a, cmp)` /
// `sorted_by(a, cmp)` share `SeqSortedBy`. NaN placement is
// mode-dependent (§4b: dev fault / prod pinned order) — the *mode* lives
// in the runtime (host knob), never in the bytecode, so one compiled
// story serves both modes.
const SEQ_SORTED: u8 = 0xF8;
const SEQ_SORTED_BY: u8 = 0xF9;

// NS-A7 collections+ (`docs/stdlib-spec.md` §8, issue #1113; this PR's own
// reservation, same "assigned here" precedent as the NS-A1/A4/A5/A6/A8
// blocks above). ONE discriminant byte with a `u8` kind immediate — the
// NS-A8 `Tower` economy applied again: after NS-A4 the free one-byte space
// is down to 0xFA-0xFD + 0xFF, so the five `Weighted`/heap operations share
// a single `Collect(CollectOp)` opcode instead of eating all of it. Wire:
// `0xFA`, then the [`CollectOp`] kind byte (a `TowerOp`-style closed
// sub-enum; unknown kinds are a decode error, the same reserved-tag
// discipline as everywhere else). All five kinds are operand-free:
// `weighted_new` takes its flattened pair row as ONE array value built by
// the preceding `ArrayNew` (a transient codegen artifact), so disasm and
// roundtrip stay table-driven — the NS-A5 "keep the family operand-free"
// discipline.
const COLLECT: u8 = 0xFA;

// B1 `or`-coalescing (`docs/stdlib-spec.md` §1.6a, `docs/decision-log.md`
// "Option[T] ruled" 2026-07-18; issue #1460 — this PR's own reservation,
// same "assigned here" precedent as the blocks above). Four free bytes
// remained after NS-A7 (`0xFB`-`0xFD` + `0xFF`); this claims the first.
// Native-surface-only: the surface spelling reuses the literal `or`
// keyword, but `InfixOp::Or` (ink's boolean `||`, oracle-frozen) is left
// untouched — `InfixOp::Coalesce` is a distinct HIR op that native
// lowering alone produces, so this opcode is unreachable from the
// oracle-covered brink/ink dialects.
//
// **RETIRED (issue #1471): the binary `Coalesce` opcode this byte
// originally held is gone.** It evaluated both operands unconditionally
// before combining them — an unruled implementation decision (PR
// #1469/#1460) the maintainer then ruled must short-circuit instead: `x or
// expensive()` may only run `expensive()` when `x` is `none`. A binary
// opcode cannot do that (both operands are already on the stack by the time
// it runs), so `InfixOp::Coalesce` lowers to a real branch
// (`lir::ExprKind::Coalesce`) instead of the generic `Infix` → binary-opcode
// path every other operator uses. This byte is reused for the branch
// primitive that replaces it:
//
//   0xFB CoalesceSome(rel)  pops `lhs` (must be `OptionVal`); `some(v)`
//                  pushes the unwrapped `v` and jumps `rel` bytes forward
//                  (same relative-offset convention as `Jump`/
//                  `JumpIfFalse`); `none` pushes nothing and falls through
//                  to the very next instruction, which evaluates `rhs` —
//                  the short-circuit itself: `rhs`'s bytecode is simply
//                  never reached when `lhs` is `some`. Codegen emits a
//                  `MakeSome` at the jump target when — and only when — the
//                  analyzer recorded `CoalesceShape::PreserveOption` for
//                  that step, re-wrapping the unwrapped value so both
//                  branches agree on shape at the join.
const COALESCE_SOME: u8 = 0xFB;

// B1b the `as` binding (`docs/decision-log.md` 2026-07-26 "The `as`
// binding: one construct, both condition positions, `{if}` spelling";
// issue #1475 — this PR's own reservation, same "assigned here" precedent
// as the blocks above). Three free bytes remained after B1's `COALESCE_SOME`
// (`0xFC`-`0xFD` + `0xFF`); this claims the first. Native-surface-only:
// nothing in the ink/brink dialects has an `as` binding to lower.
//
//   0xFC OptionBind(slot)  `[opt]` → `[bool]`: `opt` must be an
//                          `OptionVal`. `some(v)` writes `v` — the
//                          UNWRAPPED payload, typed `T` — into temp
//                          `slot` and pushes `true`; `none` leaves the
//                          slot untouched and pushes `false`. A
//                          non-`OptionVal` operand is a runtime fault
//                          (`RuntimeError::AsBindingNotOption`), the
//                          gradual-mode residual of the strict-mode `E147`
//                          the checker raises for the same shape.
//
// Test-and-bind is deliberately ONE op rather than a dup/compare/unwrap
// sequence: it keeps the whole binding inside condition evaluation, which
// is what makes `while EXPR as n { … }` rebind per iteration for free and
// lets an inline `{if EXPR as n: …}` bind without hoisting a statement out
// of its content line. The write is a plain frame-local store with no
// `ref`/pointer write-through (unlike `SetTemp`): an `as` binding always
// declares a FRESH slot, so it can never alias a `ref` parameter's cell.
const OPTION_BIND: u8 = 0xFC;

// Seq `remove_at` (issue #1484, decision log "Quick-docket closures"
// 2026-07-26; `docs/stdlib-spec.md` §4/§10 — this PR's own reservation,
// same "assigned here" precedent as the blocks above). `OptionBind` claimed
// the first of the three bytes `Coalesce`'s comment noted free
// (`0xFC`-`0xFD` + `0xFF` remained after `0xFB`); this claims the next
// (`0xFD`). `MapRemove` (`0xC5`) is restricted to maps as of this PR — the
// array-index leg it used to generalize over moves here under its own name,
// joining the `_at` faulting-index family with `CharAt`.
//
//   0xFD SeqRemoveAt  `[a, i]` → updated array with the element at `i`
//                     removed (shifts later elements left). Turn-terminating
//                     fault on `i` out of `[0, len)` (`IndexOutOfBounds`,
//                     matching `IndexGet`/`IndexSet`) or a non-array `a`
//                     (`NotIndexable`).
const SEQ_REMOVE_AT: u8 = 0xFD;

// List ops
const LIST_CONTAINS: u8 = 0xB0;
const LIST_NOT_CONTAINS: u8 = 0xB1;
const LIST_INTERSECT: u8 = 0xB2;
const LIST_ALL: u8 = 0xB5;
const LIST_INVERT: u8 = 0xB6;
const LIST_COUNT: u8 = 0xB7;
const LIST_MIN: u8 = 0xB8;
const LIST_MAX: u8 = 0xB9;
const LIST_VALUE: u8 = 0xBA;
const LIST_RANGE: u8 = 0xBB;
const LIST_FROM_INT: u8 = 0xBC;
const LIST_RANDOM: u8 = 0xBD;

// Lifecycle
const DONE: u8 = 0xF0;
const YIELD: u8 = 0xF3;
const END: u8 = 0xF1;
const NOP: u8 = 0xF2;

// String eval
const BEGIN_STRING_EVAL: u8 = 0xE0;
const END_STRING_EVAL: u8 = 0xE1;

// `0xFE` — **RETIRED (issue #3180, ruled Q-R1 2026-07-19)**: this byte
// held the lossy line:col `SourceLocation` opcode, evaluated as a no-op
// (grouped with `Nop`) and never emitted by codegen. The ruling replaces
// it with a strippable `SectionKind::DebugInfo` section (tag `0x11`,
// carrying real `FileId`s) instead of interleaving debug instructions the
// VM's step limit would count against — a binary opcode can't carry that
// design at all, so there is no replacement opcode to reuse this byte
// immediately (contrast the `Coalesce` → `CoalesceSome` retirement at
// `0xFB` above, which reused its byte in the same PR that retired it).
// `docs/format-v4-rfc.md` §5: approving the RFC froze the opcode
// *inventory* (names/encodings/reservation status), not numeric byte
// assignments — those are an implementation detail inside the reserved
// block. `0xFE` is therefore genuinely free for a future opcode to claim;
// nothing marks it reserved, following the same precedent.

// ── Types ───────────────────────────────────────────────────────────────────

/// The tower operation selected by an [`Opcode::Tower`] instruction's kind
/// byte (NS-A8, `docs/tower-mini-spec.md`; `docs/stdlib-spec.md` §2b).
///
/// Constructors pop their lanes/columns and push the built value; verbs pop
/// their operands and push the result. All semantics are glam's (T3:
/// conventions per glam, wholesale); all operations are pure. The
/// `+`/`-`/`*` operator family does NOT live here — it rides the existing
/// `Add`/`Subtract`/`Multiply`/`Negate` opcodes via `value_ops::binary_op`'s
/// tower arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TowerOp {
    /// `[x, y]` → `vec2`. Numeric lanes (ints promote to f32).
    MakeVec2,
    /// `[x, y, z]` → `vec3`.
    MakeVec3,
    /// `[x, y, z, w]` → `vec4`.
    MakeVec4,
    /// `[x, y, z, w]` → `quat` — raw lanes, glam `Quat::from_xyzw`.
    MakeQuat,
    /// `[col0, col1]` → `mat2` from `vec2` columns (column-major, T3).
    MakeMat2,
    /// `[col0, col1, col2]` → `mat3` from `vec3` columns.
    MakeMat3,
    /// `[col0, col1, col2, col3]` → `mat4` from `vec4` columns.
    MakeMat4,
    /// `[a, b]` → `float` — dot product of two same-size vectors.
    Dot,
    /// `[a, b]` → `vec3` — cross product, `vec3` only.
    Cross,
    /// `[a, b]` → componentwise minimum of two same-kind vectors.
    Min,
    /// `[a, b]` → componentwise maximum of two same-kind vectors.
    Max,
    /// `[x, lo, hi]` → componentwise clamp of three same-kind vectors.
    Clamp,
    /// `[a, b, t]` → linear interpolation with scalar `t`: vectors
    /// componentwise, quats via glam's normalizing `Quat::lerp`.
    Lerp,
}

impl TowerOp {
    /// Every tower op, in kind-byte order (tests + tooling).
    pub const ALL: [TowerOp; 13] = [
        Self::MakeVec2,
        Self::MakeVec3,
        Self::MakeVec4,
        Self::MakeQuat,
        Self::MakeMat2,
        Self::MakeMat3,
        Self::MakeMat4,
        Self::Dot,
        Self::Cross,
        Self::Min,
        Self::Max,
        Self::Clamp,
        Self::Lerp,
    ];

    fn to_byte(self) -> u8 {
        match self {
            Self::MakeVec2 => 0,
            Self::MakeVec3 => 1,
            Self::MakeVec4 => 2,
            Self::MakeQuat => 3,
            Self::MakeMat2 => 4,
            Self::MakeMat3 => 5,
            Self::MakeMat4 => 6,
            Self::Dot => 7,
            Self::Cross => 8,
            Self::Min => 9,
            Self::Max => 10,
            Self::Clamp => 11,
            Self::Lerp => 12,
        }
    }

    fn from_byte(b: u8) -> Result<Self, DecodeError> {
        match b {
            0 => Ok(Self::MakeVec2),
            1 => Ok(Self::MakeVec3),
            2 => Ok(Self::MakeVec4),
            3 => Ok(Self::MakeQuat),
            4 => Ok(Self::MakeMat2),
            5 => Ok(Self::MakeMat3),
            6 => Ok(Self::MakeMat4),
            7 => Ok(Self::Dot),
            8 => Ok(Self::Cross),
            9 => Ok(Self::Min),
            10 => Ok(Self::Max),
            11 => Ok(Self::Clamp),
            12 => Ok(Self::Lerp),
            _ => Err(DecodeError::InvalidTowerOp(b)),
        }
    }

    /// The `.inkt` mnemonic for this kind (also the `program_model`
    /// disassembly text). Stable, boring, `snake_case`.
    #[must_use]
    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::MakeVec2 => "make_vec2",
            Self::MakeVec3 => "make_vec3",
            Self::MakeVec4 => "make_vec4",
            Self::MakeQuat => "make_quat",
            Self::MakeMat2 => "make_mat2",
            Self::MakeMat3 => "make_mat3",
            Self::MakeMat4 => "make_mat4",
            Self::Dot => "dot",
            Self::Cross => "cross",
            Self::Min => "tower_min",
            Self::Max => "tower_max",
            Self::Clamp => "tower_clamp",
            Self::Lerp => "tower_lerp",
        }
    }

    /// Inverse of [`mnemonic`](Self::mnemonic) for the `.inkt` reader.
    #[must_use]
    pub fn from_mnemonic(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|op| op.mnemonic() == s)
    }
}

/// The collections+ operation selected by an [`Opcode::Collect`]
/// instruction's kind byte (NS-A7, `docs/stdlib-spec.md` §8, issue #1113).
///
/// `Weighted[T]` construction plus `rand::roll`, and the humble heap —
/// verbs over ordinary arrays, min-heap, ordering per the ruled §4b
/// doctrine (`total_order_cmp`, the one comparison core). `RandRoll` is
/// the only draw (one RNG-cell write); everything else is placement or
/// pure reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectOp {
    /// `[pairs]` → `Weighted[T]` — pops ONE array of flattened
    /// `weight, value, weight, value, …` entries (built by the preceding
    /// `ArrayNew`; a transient codegen artifact, never observable).
    /// Evidence-by-construction (§8): faults on a malformed pair row, a
    /// non-int weight, or a non-positive weight — so a `Weighted` that
    /// exists is always rollable.
    WeightedNew,
    /// `[w]` → `T` — one weighted draw from a `Weighted[T]` table. Total
    /// over any table that exists (construction is the validator); writes
    /// the RNG cell like every draw.
    RandRoll,
    /// `[a, x]` → `[a']` — push `x` into the min-heap maintained over the
    /// array (sift-up; in-place-ness comes from the RMW write-back, the
    /// `SeqSorted` precedent). §4b entry-check: DEV faults on a NaN in
    /// the entering element; PROD places it by the pinned total order.
    HeapPush,
    /// `[a]` → pushes `Option[T]` (the extracted minimum, `none` on
    /// empty), then the shrunk re-heapified array on top of it — the
    /// `SeqPop` stack contract, so the codegen take/store bracket writes
    /// the array back and leaves the Option as the expression value.
    HeapPop,
    /// `[a]` → `Option[T]` — the minimum without extraction (`none` on
    /// empty). Pure read.
    HeapPeek,
}

impl CollectOp {
    /// Every collections+ op, in kind-byte order (tests + tooling).
    pub const ALL: [CollectOp; 5] = [
        Self::WeightedNew,
        Self::RandRoll,
        Self::HeapPush,
        Self::HeapPop,
        Self::HeapPeek,
    ];

    fn to_byte(self) -> u8 {
        match self {
            Self::WeightedNew => 0,
            Self::RandRoll => 1,
            Self::HeapPush => 2,
            Self::HeapPop => 3,
            Self::HeapPeek => 4,
        }
    }

    fn from_byte(b: u8) -> Result<Self, DecodeError> {
        match b {
            0 => Ok(Self::WeightedNew),
            1 => Ok(Self::RandRoll),
            2 => Ok(Self::HeapPush),
            3 => Ok(Self::HeapPop),
            4 => Ok(Self::HeapPeek),
            _ => Err(DecodeError::InvalidCollectOp(b)),
        }
    }

    /// The `.inkt` mnemonic for this kind (also the `program_model`
    /// disassembly text). Stable, boring, `snake_case`.
    #[must_use]
    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::WeightedNew => "weighted_new",
            Self::RandRoll => "rand_roll",
            Self::HeapPush => "heap_push",
            Self::HeapPop => "heap_pop",
            Self::HeapPeek => "heap_peek",
        }
    }

    /// Inverse of [`mnemonic`](Self::mnemonic) for the `.inkt` reader.
    #[must_use]
    pub fn from_mnemonic(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|op| op.mnemonic() == s)
    }
}

/// The fn-value verb selected by an [`Opcode::SeqVerb`] instruction's kind
/// byte (`docs/stdlib-spec.md` §4, issue #1679).
///
/// The **pure quartet**: `map`, `filter`, `fold`, `filter_map`. Callbacks are
/// pure·silent by the 2026-07-18 ruling, which is what dissolves the
/// eager/lazy question — "one logical pass, order unobservable; the
/// implementation may fuse freely." Every kind evaluates its callback
/// re-entrantly with output isolated, the `SeqSortedBy` shape; a callback
/// that yields, presents a choice, calls a host external, or diverges is a
/// turn-terminating fault.
///
/// The ruled **effectful spellings**: `each`, `map_each`. Slice 2 of the
/// same issue — deliberately the opposite runtime contract: output reaches
/// the transcript instead of being captured, and the dev-mode world-write
/// guard is disarmed for their callback (`docs/stdlib-spec.md` §4: "the weird
/// thing gets the ugly method" — friction lives in the name, not in an
/// enforcement gate). Sequential in iteration order, never fused. They are
/// deliberately NOT gated by E119 (`brink_analyzer::comparator_contract`) —
/// their whole purpose is to be the legal home for the effects the pure
/// quartet's callbacks may not perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeqVerbOp {
    /// `[a, f]` → `[a']` — the array of `f(x)` for each element, in
    /// iteration order. `f: fn(T): U`.
    Map,
    /// `[a, pred]` → `[a']` — the elements for which `pred(x)` is `true`,
    /// in iteration order. `pred: fn(T): bool`; a non-bool return is a
    /// turn-terminating fault.
    Filter,
    /// `[a, init, f]` → `[acc]` — left fold: `acc` starts at `init` and
    /// becomes `f(acc, x)` for each element in iteration order.
    /// `f: fn(U, T): U`.
    Fold,
    /// `[a, f]` → `[a']` — the Option-mapper: `f(x)` for each element, kept
    /// unwrapped when `some(v)`, dropped when `none`, in iteration order.
    /// `f: fn(T): Option[U]`; a non-Option return is a turn-terminating
    /// fault. Still pure·silent-required — the natural companion of `map`
    /// under the §1.4 Option ruling, not a relaxation.
    FilterMap,
    /// `[a, f]` → `[null]` — the effectful "do something per element, no
    /// result" spelling: `f(x)` runs for each element, in iteration order,
    /// for its side effects; the return value is discarded. `f: fn(T)`.
    /// Effectful: writes and emitted output are legal.
    Each,
    /// `[a, f]` → `[a']` — the effectful transform: the array of `f(x)` for
    /// each element, in iteration order, sequential and never fused; `f`
    /// may write/emit. `f: fn(T): U`. `map`'s ugly, honest twin.
    MapEach,
}

impl SeqVerbOp {
    /// Every fn-value verb, in kind-byte order (tests + tooling).
    pub const ALL: [SeqVerbOp; 6] = [
        Self::Map,
        Self::Filter,
        Self::Fold,
        Self::FilterMap,
        Self::Each,
        Self::MapEach,
    ];

    fn to_byte(self) -> u8 {
        match self {
            Self::Map => 0,
            Self::Filter => 1,
            Self::Fold => 2,
            Self::FilterMap => 3,
            Self::Each => 4,
            Self::MapEach => 5,
        }
    }

    fn from_byte(b: u8) -> Result<Self, DecodeError> {
        match b {
            0 => Ok(Self::Map),
            1 => Ok(Self::Filter),
            2 => Ok(Self::Fold),
            3 => Ok(Self::FilterMap),
            4 => Ok(Self::Each),
            5 => Ok(Self::MapEach),
            _ => Err(DecodeError::InvalidSeqVerbOp(b)),
        }
    }

    /// The source spelling of this verb — also the `.inkt` mnemonic and the
    /// `program_model` disassembly text, and the verb name runtime faults
    /// report. Stable, boring, `snake_case`.
    #[must_use]
    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Filter => "filter",
            Self::Fold => "fold",
            Self::FilterMap => "filter_map",
            Self::Each => "each",
            Self::MapEach => "map_each",
        }
    }

    /// Whether this verb's callback runs under the pure·silent contract
    /// (E119-gated, output captured, dev-mode world-write guard armed) or
    /// the effectful contract (`each`/`map_each`: output reaches the
    /// transcript, writes are legal). The VM's `seq_map`/`seq_filter`/
    /// `seq_fold`/`seq_filter_map`/`seq_each`/`seq_map_each` each read this
    /// directly when entering their callback scope
    /// (`enter_callback_scope(flow, VERB, op.is_effectful())`), so it
    /// single-sources the classification `guard_comparator_write`'s posture
    /// ultimately keys off — there is exactly one place a new `SeqVerbOp`
    /// variant's pure/effectful contract can be gotten wrong.
    #[must_use]
    pub fn is_effectful(self) -> bool {
        matches!(self, Self::Each | Self::MapEach)
    }

    /// Inverse of [`mnemonic`](Self::mnemonic) for the `.inkt` reader.
    #[must_use]
    pub fn from_mnemonic(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|op| op.mnemonic() == s)
    }
}

/// The kind of sequence/shuffle container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequenceKind {
    Cycle,
    Stopping,
    OnceOnly,
    Shuffle,
}

impl SequenceKind {
    fn to_byte(self) -> u8 {
        match self {
            Self::Cycle => 0,
            Self::Stopping => 1,
            Self::OnceOnly => 2,
            Self::Shuffle => 3,
        }
    }

    fn from_byte(b: u8) -> Result<Self, DecodeError> {
        match b {
            0 => Ok(Self::Cycle),
            1 => Ok(Self::Stopping),
            2 => Ok(Self::OnceOnly),
            3 => Ok(Self::Shuffle),
            _ => Err(DecodeError::InvalidSequenceKind(b)),
        }
    }
}

/// The binary operator a fused superinstruction applies
/// (`docs/optimizer-peephole.md` §1). One byte on the wire; the mnemonic is
/// what `.inkt` prints. Exactly the operators that have a plain two-operand
/// opcode of their own — a fused form is always spelled out as that opcode
/// preceded by `PushInt` and/or followed by `JumpIfFalse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryKind {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

impl BinaryKind {
    /// Every kind, in wire-byte order.
    pub const ALL: [Self; 11] = [
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Modulo,
        Self::Equal,
        Self::NotEqual,
        Self::Greater,
        Self::GreaterOrEqual,
        Self::Less,
        Self::LessOrEqual,
    ];

    fn to_byte(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Subtract => 1,
            Self::Multiply => 2,
            Self::Divide => 3,
            Self::Modulo => 4,
            Self::Equal => 5,
            Self::NotEqual => 6,
            Self::Greater => 7,
            Self::GreaterOrEqual => 8,
            Self::Less => 9,
            Self::LessOrEqual => 10,
        }
    }

    fn from_byte(b: u8) -> Result<Self, DecodeError> {
        Self::ALL
            .get(b as usize)
            .copied()
            .ok_or(DecodeError::InvalidBinaryKind(b))
    }

    /// The `.inkt` spelling of the operator.
    #[must_use]
    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Subtract => "sub",
            Self::Multiply => "mul",
            Self::Divide => "div",
            Self::Modulo => "mod",
            Self::Equal => "eq",
            Self::NotEqual => "ne",
            Self::Greater => "gt",
            Self::GreaterOrEqual => "ge",
            Self::Less => "lt",
            Self::LessOrEqual => "le",
        }
    }

    /// Inverse of [`mnemonic`](Self::mnemonic).
    #[must_use]
    pub fn from_mnemonic(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.mnemonic() == s)
    }

    /// The plain two-operand opcode this kind fuses, if `op` is one.
    #[must_use]
    pub fn of_opcode(op: &Opcode) -> Option<Self> {
        Some(match op {
            Opcode::Add => Self::Add,
            Opcode::Subtract => Self::Subtract,
            Opcode::Multiply => Self::Multiply,
            Opcode::Divide => Self::Divide,
            Opcode::Modulo => Self::Modulo,
            Opcode::Equal => Self::Equal,
            Opcode::NotEqual => Self::NotEqual,
            Opcode::Greater => Self::Greater,
            Opcode::GreaterOrEqual => Self::GreaterOrEqual,
            Opcode::Less => Self::Less,
            Opcode::LessOrEqual => Self::LessOrEqual,
            _ => return None,
        })
    }
}

/// Flags packed into a `BeginChoice` instruction.
///
/// Under the single-pop protocol, `BeginChoice` pops at most **one** display
/// string from the stack when `has_start_content || has_choice_only_content`.
/// The two content flags are metadata indicating which parts of the original
/// ink choice contributed to that string — the runtime does not pop them
/// separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(clippy::struct_excessive_bools)]
pub struct ChoiceFlags {
    pub has_condition: bool,
    /// Original choice had `start` content (text before `[`).
    pub has_start_content: bool,
    /// Original choice had `choice_only` content (text inside `[]`).
    /// Under the single-pop protocol this is metadata only — no extra stack pop.
    pub has_choice_only_content: bool,
    pub once_only: bool,
    pub is_invisible_default: bool,
}

impl ChoiceFlags {
    fn to_byte(self) -> u8 {
        let mut b = 0u8;
        if self.has_condition {
            b |= 0x01;
        }
        if self.has_start_content {
            b |= 0x02;
        }
        if self.has_choice_only_content {
            b |= 0x04;
        }
        if self.once_only {
            b |= 0x08;
        }
        if self.is_invisible_default {
            b |= 0x10;
        }
        b
    }

    fn from_byte(b: u8) -> Self {
        Self {
            has_condition: b & 0x01 != 0,
            has_start_content: b & 0x02 != 0,
            has_choice_only_content: b & 0x04 != 0,
            once_only: b & 0x08 != 0,
            is_invisible_default: b & 0x10 != 0,
        }
    }
}

/// Errors that can occur when decoding from bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Not enough bytes remaining for the expected operand.
    UnexpectedEof,
    /// Unknown opcode discriminant byte.
    UnknownOpcode(u8),
    /// Invalid definition id (bad tag byte).
    InvalidDefinitionId(u64),
    /// Invalid sequence kind byte.
    InvalidSequenceKind(u8),
    /// Invalid binary-operator kind byte on a fused superinstruction.
    InvalidBinaryKind(u8),
    /// Invalid tower op kind byte (NS-A8 `Tower` opcode immediate).
    InvalidTowerOp(u8),
    /// Invalid collections+ op kind byte (NS-A7 `Collect` opcode immediate).
    InvalidCollectOp(u8),
    /// Invalid fn-value verb kind byte (`SeqVerb` opcode immediate,
    /// issue #1679).
    InvalidSeqVerbOp(u8),
    /// .inkb magic bytes are not `INKB`.
    BadMagic([u8; 4]),
    /// .inkb version is not supported.
    UnsupportedVersion(u16),
    /// A string field contained invalid UTF-8.
    InvalidUtf8,
    /// Unknown value type discriminant.
    InvalidValueType(u8),
    /// Unknown select key discriminant.
    InvalidSelectKey(u8),
    /// Unknown line part discriminant.
    InvalidLinePart(u8),
    /// Unknown line content discriminant.
    InvalidLineContent(u8),
    /// Unknown plural category discriminant.
    InvalidPluralCategory(u8),
    /// Unknown section kind tag in .inkb offset table.
    InvalidSectionKind(u8),
    /// Required section kind missing from .inkb offset table.
    MissingSectionKind(u8),
    /// File size field doesn't match actual buffer length.
    FileSizeMismatch { expected: u32, actual: usize },
    /// CRC-32 checksum of section data doesn't match header.
    ChecksumMismatch { expected: u32, actual: u32 },
    /// Section offset table is structurally invalid (out of bounds or not monotonic).
    InvalidSectionOffset { kind: u8, offset: u32 },
    /// `.inkl` magic bytes are not `INKL`.
    BadInklMagic([u8; 4]),
    /// `.inkl` version is not supported.
    UnsupportedInklVersion(u8),
    /// `VAL_ARRAY`/`VAL_MAP` nesting exceeded the decoder's recursion-depth
    /// cap (see `MAX_DECODE_DEPTH`). Guards against crafted files of deeply
    /// nested single-element collections stack-overflowing the reader.
    MaxDepthExceeded(usize),
    /// A section-locally-versioned section (e.g. `AliasTable`,
    /// `docs/modules-spec.md` §5) carried a version byte this reader doesn't
    /// know how to decode.
    UnsupportedSectionVersion { section: u8, version: u8 },
    /// A `VAL_PROJECTION` segment carried an unknown kind byte — either
    /// malformed bytecode or the RESERVED range-segment kind (`2`,
    /// `docs/format-v4-rfc.md` §1), which nothing emits in T1e and the
    /// reader therefore rejects (`docs/t1e-spec.md` §3).
    InvalidProjSegmentKind(u8),
    /// An `EffectRows` call atom carried an unknown capability-parameter tag
    /// (T2-3, `docs/effects-spec.md` §11). Only `Any` (`0`) is legal in this
    /// section version; path-granular tags are reserved (#826).
    InvalidEffectCapParam(u8),
    /// An `EffectRows` call atom carried a non-`None` handle-parameter slot
    /// (T2-3, `docs/effects-spec.md` §11, `docs/t1d-spec.md` §7). The slot is
    /// reserved — nothing emits a bound handle in this section version.
    InvalidEffectHandleParam(u8),
    /// A `DirectEffects` extension-flags byte (NS-A2, `EffectRows` section
    /// version 3) carried a set bit outside the known
    /// emits/tags/faults mask — the reserved bits (3–7) are rejected until
    /// a section version graduates them.
    InvalidEffectDimensions(u8),
    /// A `DebugLocalEntry` row's flags byte (`DebugInfo` section version 2,
    /// #3395) carried a set bit outside the known has-range/synthetic mask
    /// — the reserved bits (2–7) are rejected until a section version
    /// graduates them, same discipline as `InvalidEffectDimensions`.
    InvalidDebugLocalFlags(u8),
    /// A `ContainerDef`'s declared `param_count` disagreed with the number
    /// of per-param name/mode metadata entries that followed it (#954,
    /// sibling of the `.inkt` reader's same guard, #745). `ContainerDef`'s
    /// documented invariant is that `params.len()` always equals
    /// `param_count` whenever per-param metadata is present at all (empty
    /// `params` is the separate, legitimate "count only, no metadata" case).
    /// A mutated/corrupt `.inkb` asserting otherwise is malformed input.
    ParamCountMismatch { declared: u8, actual: usize },
    /// A `VAL_MAP` entry list carried the same key twice. A legitimate
    /// encoder never emits this — `OrderedMap::insert` de-duplicates on the
    /// write side — so a repeated key is a corrupt or crafted `.inkb`; the
    /// content-based `OrderedMap` `Eq` (issue #909) assumes each key appears
    /// once, so this is rejected rather than silently keeping the last
    /// occurrence (issue #985).
    DuplicateMapKey,
    /// An unknown discriminant tag in the conventions-projection wire codec
    /// (issue #2111 finding 2, `crate::conventions`) — a mode byte,
    /// attach-presence/resolution byte, or `SchemaTypeDef` tag outside the
    /// known range. One shared variant for all three: each is "an unknown
    /// byte in this one codec's own tag space", the same causal category, so
    /// this mirrors `InvalidSectionKind`'s single-variant-per-codec posture
    /// rather than minting three near-duplicate variants.
    InvalidConventionsProjectionTag(u8),
    /// A `DebugInfo` (`0x11`, `docs/debugger-spec.md` §2.3) file-table entry
    /// carried an unknown `surface` byte (only `0` Synthetic / `1` Ink / `2`
    /// Native are defined). Unlike the section's own `flags` reserved-bit
    /// tolerance (§2.2 — unknown bits are ignored, never rejected), an
    /// unknown surface tag is a structurally different file table this
    /// reader cannot interpret at all (it decides which `ProvenanceResolver`
    /// even applies), so it is rejected rather than silently mis-surfaced.
    InvalidFileSurface(u8),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of bytecode"),
            Self::UnknownOpcode(b) => write!(f, "unknown opcode: {b:#04x}"),
            Self::InvalidDefinitionId(raw) => {
                write!(f, "invalid definition id: {raw:#018x}")
            }
            Self::InvalidSequenceKind(b) => write!(f, "invalid sequence kind: {b}"),
            Self::InvalidBinaryKind(b) => write!(f, "invalid binary kind: {b}"),
            Self::InvalidTowerOp(b) => write!(f, "invalid tower op kind: {b:#04x}"),
            Self::InvalidCollectOp(b) => write!(f, "invalid collections+ op kind: {b:#04x}"),
            Self::InvalidSeqVerbOp(b) => write!(f, "invalid fn-value verb kind: {b:#04x}"),
            Self::BadMagic(m) => write!(f, "bad magic: {m:02x?}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported .inkb version: {v}"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 in string field"),
            Self::InvalidValueType(b) => write!(f, "invalid value type: {b:#04x}"),
            Self::InvalidSelectKey(b) => write!(f, "invalid select key: {b:#04x}"),
            Self::InvalidLinePart(b) => write!(f, "invalid line part: {b:#04x}"),
            Self::InvalidLineContent(b) => write!(f, "invalid line content: {b:#04x}"),
            Self::InvalidPluralCategory(b) => write!(f, "invalid plural category: {b:#04x}"),
            Self::InvalidSectionKind(b) => write!(f, "invalid section kind: {b:#04x}"),
            Self::MissingSectionKind(b) => write!(f, "missing required section kind: {b:#04x}"),
            Self::FileSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "file size mismatch: header says {expected}, actual {actual}"
                )
            }
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "checksum mismatch: header {expected:#010x}, computed {actual:#010x}"
                )
            }
            Self::InvalidSectionOffset { kind, offset } => {
                write!(
                    f,
                    "invalid section offset: kind {kind:#04x} at offset {offset}"
                )
            }
            Self::BadInklMagic(m) => write!(f, "bad .inkl magic: {m:02x?}"),
            Self::UnsupportedInklVersion(v) => write!(f, "unsupported .inkl version: {v}"),
            Self::MaxDepthExceeded(limit) => {
                write!(f, "value nesting exceeded max decode depth ({limit})")
            }
            Self::UnsupportedSectionVersion { section, version } => {
                write!(
                    f,
                    "unsupported section-local version {version} for section {section:#04x}"
                )
            }
            Self::InvalidProjSegmentKind(b) => {
                write!(f, "invalid projection segment kind: {b:#04x}")
            }
            Self::InvalidEffectCapParam(b) => {
                write!(f, "invalid effect capability-parameter tag: {b:#04x}")
            }
            Self::InvalidEffectHandleParam(b) => {
                write!(f, "reserved effect handle-parameter slot set: {b:#04x}")
            }
            Self::InvalidEffectDimensions(b) => {
                write!(f, "reserved effect-dimension flag bits set: {b:#04x}")
            }
            Self::InvalidDebugLocalFlags(b) => {
                write!(f, "reserved debug-local flag bits set: {b:#04x}")
            }
            Self::ParamCountMismatch { declared, actual } => {
                write!(
                    f,
                    "container params metadata count ({actual}) does not match declared param_count ({declared})"
                )
            }
            Self::DuplicateMapKey => write!(f, "duplicate key in map value"),
            Self::InvalidConventionsProjectionTag(b) => {
                write!(f, "invalid conventions-projection wire tag: {b:#04x}")
            }
            Self::InvalidFileSurface(b) => {
                write!(f, "invalid DebugInfo file-table surface tag: {b:#04x}")
            }
        }
    }
}

impl core::error::Error for DecodeError {}

/// A single VM instruction with its operands.
#[derive(Debug, Clone, PartialEq)]
pub enum Opcode {
    // ── Stack & literals ────────────────────────────────────────────────
    PushInt(i32),
    PushFloat(f32),
    PushBool(bool),
    PushString(u16),
    PushList(u16),
    PushDivertTarget(DefinitionId),
    PushNull,
    Pop,
    Duplicate,

    // ── Arithmetic ──────────────────────────────────────────────────────
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Negate,

    // ── Comparison ──────────────────────────────────────────────────────
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,

    // ── Logic ───────────────────────────────────────────────────────────
    Not,
    And,
    Or,

    // ── Global vars ─────────────────────────────────────────────────────
    GetGlobal(DefinitionId),
    SetGlobal(DefinitionId),

    // ── Temp vars ───────────────────────────────────────────────────────
    DeclareTemp(u16),
    GetTemp(u16),
    SetTemp(u16),
    /// Get a temp's raw value without auto-dereference (for passing a ref onward).
    GetTempRaw(u16),

    // ── Variable pointers ──────────────────────────────────────────────
    /// Push a pointer to a global variable onto the eval stack.
    PushVarPointer(DefinitionId),
    /// Push a pointer to a temp variable onto the eval stack.
    PushTempPointer(u16),

    // ── Control flow ────────────────────────────────────────────────────
    Jump(i32),
    JumpIfFalse(i32),
    Goto(DefinitionId),
    GotoIf(DefinitionId),
    GotoVariable,

    // ── Container flow ──────────────────────────────────────────────────
    EnterContainer(DefinitionId),
    ExitContainer,

    // ── Functions / tunnels ─────────────────────────────────────────────
    Call(DefinitionId),
    Return,
    TunnelCall(DefinitionId),
    TunnelReturn,
    TunnelCallVariable,
    /// Call through a variable holding either a divert target (classic ink
    /// function-via-variable) or a function value (T1c-2 direct-call form
    /// `f(args…)`) — both share this dispatch site. `argc` is the exact
    /// number of args codegen pushed before the callee at this call site
    /// (never derived from the resolved target's arity at runtime — issue
    /// #721: doing so made a gradual-mode direct-call arity mismatch leave
    /// a stray value on the stack instead of faulting). The divert-target
    /// arm ignores `argc` (unchanged oracle-verified behavior); the
    /// function-value arm pops exactly `argc` supplied args.
    CallVariable(u8),

    // ── Threads ─────────────────────────────────────────────────────────
    ThreadCall(DefinitionId),
    ThreadStart,
    ThreadDone,

    // ── Output ──────────────────────────────────────────────────────────
    EmitLine(u16, u8),
    EmitValue,
    EmitNewline,
    /// `EmitLine(idx, slots)` immediately followed by `EmitNewline`, as one
    /// instruction — the optimizer's fusion of the single most common
    /// instruction pair in real stories (`docs/optimizer-peephole.md`). Its
    /// effect is exactly the two in sequence; the runtime shares their
    /// bodies. Never emitted by codegen.
    EmitLineNl(u16, u8),
    /// `PushInt(imm)` followed by the binary operator `kind`, as one
    /// instruction: pops the left operand, applies `kind` with `imm` as the
    /// right operand, pushes the result. Optimizer-only
    /// (`docs/optimizer-peephole.md` §1); never emitted by codegen.
    BinaryImm(BinaryKind, i32),
    /// The binary operator `kind` followed by `JumpIfFalse(rel)`, as one
    /// instruction: pops both operands, and jumps by `rel` (relative to the
    /// end of this instruction, as every relative jump is) when the result is
    /// not truthy. The result is not left on the stack. Optimizer-only.
    BinaryJumpIfFalse(BinaryKind, i32),
    /// `PushInt(imm)`, the binary operator `kind`, then `JumpIfFalse(rel)`,
    /// as one instruction — the shape of every `if x <= 1` and `{ x == 3: }`
    /// in real stories. Operands are `(kind, imm, rel)`. Optimizer-only.
    BinaryImmJumpIfFalse(BinaryKind, i32, i32),
    /// Word break — renders as a single space between content parts.
    Spring,
    Glue,
    BeginTag,
    EndTag,
    EvalLine(u16, u8),
    /// Begin capturing output into a fragment (structural preservation).
    BeginFragment,
    /// End fragment capture — store parts and push `Value::FragmentRef`.
    EndFragment,
    /// An `attach = StructName` convention handler's claimed line (issue
    /// #2108) — see `brink_ir::hir::Stmt::AttachElement`'s doc. Pops the
    /// call's result off the value stack; when it is a `Value::Record`
    /// matching a known `StructShapes` entry, merges its fields (converted
    /// via the same `stringify` display path as `string()`/interpolation)
    /// into the VM's per-block attachment state — no output, no `Step::Line`.
    AttachElement,
    /// Closes the run an `AttachElement` opened — see
    /// `brink_ir::hir::Stmt::EndElementRun`'s doc. Clears the VM's
    /// accumulated attachment data and starts a fresh block.
    EndElementRun,

    // ── Choices ─────────────────────────────────────────────────────────
    BeginChoice(ChoiceFlags, DefinitionId),
    EndChoice,

    // ── Sequences ───────────────────────────────────────────────────────
    Sequence(SequenceKind, u8),
    SequenceBranch(i32),

    // ── Intrinsics ──────────────────────────────────────────────────────
    /// Pop a `DivertTarget` from the stack, push its visit count.
    VisitCount,
    /// Push the visit count of the *current* container (no stack input).
    CurrentVisitCount,
    /// #3273 (line-variant groups): pop a `DivertTarget`, increment that
    /// container's visit count, and push the **pre**-increment count as an
    /// `Int` — the 0-based "how many times has this alternative been
    /// viewed" index a shared inline alternative's branch selection is
    /// computed from. The increment is the point: the container is never
    /// *entered* (its text lives in the enumerated line-variant table, not
    /// in its body), so this is the one place its view is recorded.
    /// A non-`DivertTarget` operand pushes 0 and records nothing,
    /// mirroring [`Opcode::VisitCount`]'s malformed-input tolerance.
    TouchVisit,
    /// #3273 (line-variant groups): pop a `DivertTarget`, then
    /// `num_elements`, then `seq_count` (both `Int`), and push the shuffle
    /// branch index for THAT container — the same partial-Fisher–Yates
    /// selection [`Opcode::Sequence`]`(Shuffle)` performs, but seeded by
    /// the *named* container's `path_hash` instead of the current one's.
    /// Two shared shuffles on one line must not share a seed, or their
    /// permutations correlate; the current container (the line's scope) is
    /// the same for both, so the current-container form cannot serve.
    ShuffleIndexOf,
    TurnsSince,
    TurnIndex,
    ChoiceCount,
    Random,
    SeedRandom,

    // ── Casts / math ────────────────────────────────────────────────────
    CastToInt,
    CastToFloat,
    Floor,
    Ceiling,
    Pow,
    Min,
    Max,

    // ── External fns ────────────────────────────────────────────────────
    CallExternal(DefinitionId, u8),

    // ── List ops ────────────────────────────────────────────────────────
    ListContains,
    ListNotContains,
    ListIntersect,
    ListAll,
    ListInvert,
    ListCount,
    ListMin,
    ListMax,
    ListValue,
    ListRange,
    ListFromInt,
    ListRandom,

    // ── Collections (T1b, `docs/format-v4-rfc.md` §3 "Collections (T1a)") ─
    /// `[elem_0, …, elem_{n-1}]` → `Array([elem_0, …, elem_{n-1}])`.
    ArrayNew(u32),
    /// `[k_0, v_0, …, k_{n-1}, v_{n-1}]` → `Map({k_0: v_0, …})` (insertion
    /// order = argument order; a repeated key keeps its first position and
    /// takes the last value, matching `OrderedMap::insert`).
    MapNew(u32),
    /// `[container, index]` → element/value. Turn-terminating fault on
    /// out-of-bounds array index or missing map key (value-model-spec §6).
    IndexGet,
    /// `[container, index, value]` → updated container (take → `make_mut` →
    /// write-back). Turn-terminating fault on out-of-bounds array index or
    /// missing map key — no silent growth on write-past-end (spec §6).
    IndexSet,
    /// `[container]` → `Int(len)`. Array or map.
    CollectionLen,
    /// `[map, key]` → value. Turn-terminating fault on missing key.
    MapGet,
    /// `[map, key, value]` → updated map (insert-or-overwrite; unlike
    /// `IndexSet`, a missing key is not a fault — this is the stdlib
    /// `insert()` mutator's primitive).
    MapInsert,
    /// `[map, key]` → updated map with `key` removed (no-op if absent — the
    /// stdlib `remove()` mutator's primitive). Map-only as of issue #1484:
    /// a non-map container is a turn-terminating fault (`NotIndexable`).
    /// The array-index leg this op used to generalize over is
    /// [`SeqRemoveAt`](Self::SeqRemoveAt).
    MapRemove,
    /// `[map, key]` → `Bool`.
    MapContains,
    /// `[map]` → `Array` of keys in insertion order.
    CollectionKeys,
    /// `[map]` → `Array` of values in insertion order.
    CollectionValues,
    /// `LiteralPool[idx]` → cloned value (an `Arc` bump for collections).
    PushLiteral(u32),

    // ── Sharing discipline (T1b-4, `docs/format-v4-rfc.md` §3) ──────────
    /// Move a global's current value out, leaving `Value::Null` behind —
    /// the take-half of the take → `make_mut` → write-back RMW discipline
    /// (value-model-spec §5). No stack input; pushes the taken value.
    /// Unlike `GetGlobal`, never auto-dereferences (globals can't hold
    /// `ref`-param pointers — those live in temps).
    TakeGlobal(DefinitionId),
    /// Move a temp's current value out, leaving `Value::Null` behind —
    /// mirrors `TakeGlobal` for temp slots. Auto-dereferences like
    /// `GetTemp`: if the temp holds a `VariablePointer`/`TempPointer`, the
    /// *pointed-to* location is taken (and left `Null`), not the pointer
    /// value itself, which stays in this slot untouched.
    TakeTemp(u16),

    // ── Records (TM-4, `docs/typed-mode-spec.md` §6) ─────────────────────
    /// `[field_0, …, field_{n-1}]` → `Record` (n = the shape's declared
    /// field count, looked up from `StructShapes`; fields popped/assigned in
    /// shape declaration order). The `u32` operand is the `ShapeId`.
    RecordNew(u32),
    /// `[record]` → field value, looked up by name (`NameId` operand) in the
    /// record's own shape. Turn-terminating fault if the shape has no field
    /// by that name (value-model-spec §11c).
    RecordGetDyn(u16),
    /// `[record, value]` → updated record (take → `make_mut` → write-back),
    /// field selected by name (`NameId` operand). Turn-terminating fault if
    /// the shape has no field by that name.
    RecordSetDyn(u16),
    /// `[record]` → field value, looked up by flat offset into the record's
    /// own field vector (TM-4c, `docs/typed-mode-spec.md` §6 static-offset
    /// payoff). Emitted only when the record's shape is compile-time known
    /// (`types = strict`); turn-terminating fault if the offset is out of
    /// range for the popped record's field count — no shape re-check.
    RecordGet(u16),
    /// `[record, value]` → updated record (take → `make_mut` → write-back),
    /// field selected by flat offset (TM-4c). Turn-terminating fault if the
    /// offset is out of range.
    RecordSet(u16),

    // ── Conversion intrinsics (TM-3 completion, `docs/typed-mode-spec.md`
    // §4, maintainer ruling 2026-07-13, issue #659) ──────────────────────
    /// `[x]` → `Int`. The `int(x)` pure conversion intrinsic: `Int`
    /// (identity), `Float` (truncate toward zero, matching vanilla ink's
    /// `INT()`), `Bool` (`true` → 1, `false` → 0), `String` (parse).
    /// Turn-terminating fault on a string that fails to parse, or on any
    /// value outside this permissive numeric+bool domain (divert targets,
    /// LIST values, arrays, maps, records) — value-model-spec §11c.
    ConvertInt,
    /// `[x]` → `Float`. The `float(x)` pure conversion intrinsic: `Float`
    /// (identity), `Int` (widen), `Bool` (`true` → 1.0, `false` → 0.0),
    /// `String` (parse). Same fault domain as `ConvertInt`.
    ConvertFloat,
    /// `[x]` → `String`. The `string(x)` pure conversion intrinsic: display
    /// form, identical to interpolation (`{x}`) — total over every `Value`,
    /// never faults (typed-mode-spec §4: "display is universal, not a
    /// coercion").
    ConvertString,

    // ── Function values (T1c, `docs/t1c-spec.md` §3/§6) ──────────────────
    /// `[]` → `FnRef`. Push a zero-bound function value for the target
    /// `DefinitionId` (`#fn(name)` where the target has no `ref` params).
    PushFnRef(DefinitionId),
    /// `[bound_0, …, bound_{n-1}]` → `Closure`. Pop the `n` = `bound_count`
    /// bound args (in declared order) and pair each with its param name/mode
    /// read from the target container's own [`ContainerDef::params`] table
    /// (the bound prefix `params[0..n]`) to build a `Closure`.
    /// A `ref` bound arg is a `VariablePointer` (a captured durable cell); a
    /// `val` bound arg is a snapshot. The names/modes are read from the
    /// signature (not baked into the opcode) so there is one source of truth
    /// the rehydration check compares against.
    MakeClosure {
        target: DefinitionId,
        bound_count: u8,
    },
    /// `[arg_0, …, arg_{argc-1}, callee]` → return value. Pop the callee
    /// function value then the `argc` supplied (val-only) args, splice the
    /// closure's bound prefix ahead of them, and enter the target. Faults
    /// (turn-terminating, `docs/t1c-spec.md` §3): callee is not a function
    /// value; `bound + argc` ≠ the target's declared arity; a rehydrated env
    /// entry's name/mode no longer matches the current signature; the callee
    /// `ref`-binds a `#@local` and is invoked from a non-creating flow.
    CallValue(u8),
    /// `[arg_0, …, arg_{argc-1}, callee]` → new function value. The
    /// `bind(f, args…)` stdlib intrinsic (T1c-3, `docs/t1c-spec.md` §3):
    /// pop the callee function value then the `argc` supplied (val-only)
    /// args, append them to the callee's bound-arg row (val-only currying,
    /// consuming the head of the remaining param row), and push the new
    /// function value. The appended entries take their param name/mode from
    /// the target's signature (always `val`). Faults (turn-terminating):
    /// callee is not a function value; `bound + argc` exceeds the target's
    /// declared arity.
    BindValue(u8),

    // ── Path projections (T1e, `docs/t1e-spec.md` §3) ─────────────────────
    /// `[seg_0, …, seg_{n-1}]` → `Projection` (`n` = `segment_count`, pushed
    /// by codegen in source order; the VM's LIFO pop-then-reverse restores
    /// it). Each popped value is classified `Int` → `ProjSegment::Index`, else →
    /// `ProjSegment::Key` and paired with the static `root` cell to build a
    /// `Value::Projection` (`docs/format-v4-rfc.md` §1). Emitted at every
    /// real path-projection `ref`-argument creation site (`ref
    /// npc.inventory[3]`) — the T1e-1 `E099` lowering fence this replaces.
    MakeProjection {
        root: DefinitionId,
        segment_count: u8,
    },
    /// `[projection]` → value. Root-cell RMW read: take the root cell's
    /// *current* value, walk the segment chain, push the result. Faults
    /// `ProjectionInvalidated` (turn-terminating) if the path no longer
    /// resolves (spec §1(2)).
    ProjRead,
    /// `[projection, value]` → (assigns, pushes nothing). Root-cell RMW
    /// write: take root → walk → `make_mut` spine → write the final segment
    /// → store back (spec §3). Faults `ProjectionInvalidated` on an
    /// unresolved path, same domain as `ProjRead`.
    ProjWrite,

    // ── Stdlib slice 1 completion (`docs/t1b-surface-spec.md` §5, issue
    // #857) ───────────────────────────────────────────────────────────────
    /// `[s, i]` → single-character `String`. The `char_at(s, i)` stdlib pure
    /// function: `i` indexes Unicode scalar values ("chars"), not UTF-8
    /// bytes. Turn-terminating fault (value-model-spec §11c) on a non-`Int`
    /// `i`, a non-`String` `s`, or `i` outside `[0, char_count)`.
    CharAt,

    // ── NS-A1: Option[T] + the ruled stdlib flips (`docs/stdlib-spec.md`
    // §1.1/§1.4, §§3-5) ──────────────────────────────────────────────────
    /// `[]` → `none`. Push the `Option[T]` absence value.
    PushNone,
    /// `[x]` → `some(x)`. Wrap the top of stack — total over every value.
    MakeSome,
    /// `[s, sub]` → `Option[int]`: index of `sub`'s first occurrence in
    /// `s`, counted in Unicode scalar values (chars, not bytes — the §3
    /// indexing unit `char_at` already uses); absent → `none`.
    /// Turn-terminating fault on non-string arguments.
    StrFind,
    /// `[a, x]` → `Option[int]`: index of the first element structurally
    /// equal to `x`; absent → `none`. Fault on a non-array container.
    SeqIndexOf,
    /// `[a]` → `Option[T]`: least element (empty → `none`). Orders
    /// int/float (numeric promotion, NaN per the §4b pinned prod order),
    /// bool, string; anything else faults (unorderable — wave A4 grows the
    /// roster). Ties keep the first occurrence.
    SeqMin,
    /// `[a]` → `Option[T]`: greatest element — see [`SeqMin`](Self::SeqMin).
    SeqMax,
    /// `[a]` → `Option[T]`: first element (empty → `none`).
    SeqFirst,
    /// `[a]` → `Option[T]`: last element (empty → `none`).
    SeqLast,
    /// `[a]` → pushes `Option[T]` (the removed last element, or `none` on
    /// empty), then the shrunk array on top of it. Codegen brackets this
    /// `TakeGlobal`/`TakeTemp` … `SetGlobal`/`SetTemp` so the array writes
    /// back to its root cell and the Option remains as the expression's
    /// value. Fault on a non-array.
    SeqPop,
    /// `[m, k]` → `Option[V]`: the non-faulting map read (`get(m, k)`,
    /// §5 — martyr #3 redeemed). Missing key → `none`; a key outside the
    /// int/string/bool key domain is a turn-terminating fault (malformed
    /// question), as is a non-map container. The faulting `m[k]`
    /// ([`MapGet`](Self::MapGet)) stays the "I expect it there" read.
    MapGetOpt,
    /// `[m, v]` → `Bool`: content-equality scan over the map's values
    /// (§5 — honest O(n)). Fault on a non-map.
    MapContainsValue,
    /// `[m]` → empty map. The `clear(m)` statement-only mutator's
    /// primitive; in-place-ness comes from the RMW write-back, exactly
    /// like [`MapInsert`](Self::MapInsert)/[`MapRemove`](Self::MapRemove).
    /// Fault on a non-map.
    MapClear,

    // ── B1: `or`-coalescing (`docs/stdlib-spec.md` §1.6a, issue #1460),
    // short-circuited per issue #1471's ruling ──────────────────────────
    /// Pops `lhs` (must be an `OptionVal`). `some(v)` pushes the unwrapped
    /// `v` and jumps `rel` bytes forward (the same relative-offset
    /// convention as [`Jump`](Self::Jump)/[`JumpIfFalse`](Self::JumpIfFalse));
    /// `none` pushes nothing and falls through to the next instruction,
    /// which evaluates `rhs`. That fall-through *is* the short-circuit:
    /// `rhs`'s bytecode is only ever reached when `lhs` is `none` — `x or
    /// expensive()` runs `expensive()` exactly once, and only on `none`
    /// (RULED, issue #1471, flipping the eager evaluation PR #1469/#1460
    /// landed and flagged as unruled). Native-surface only: reachable
    /// exclusively through `InfixOp::Coalesce`, which the native lowering
    /// path alone produces (`InfixOp::Or`, ink's boolean `||`, is untouched
    /// and oracle-frozen).
    ///
    /// The jump target is where the two branches join: the `some(v)` branch
    /// has already unwrapped to `v`, and the `none` branch pushed `rhs`
    /// as-is. Codegen emits a [`MakeSome`](Self::MakeSome) right at that
    /// target exactly when the step's recorded typing says `rhs` is itself
    /// `Option[U]` (the two-Option form, `(Option[T],Option[T]) ->
    /// Option[U]`, preserving optionality for chaining), so both branches
    /// agree on shape at the join; for the collapse form
    /// (`(Option[T],T)->T`) no `MakeSome` is emitted and `v` stands
    /// unwrapped. The retired binary opcode decided that from `rhs`'s
    /// *runtime* value; short-circuiting rules that out (`rhs` may never
    /// run by the time the answer is needed), so the decision is made at
    /// lowering time from the analyzer's recorded types — see below.
    ///
    /// ## Where the collapse-vs-preserve answer comes from
    ///
    /// RULED (maintainer, 2026-07-26, issue #1492 — `docs/decision-log.md`
    /// "Lowering consumes analyzer types"): typing verdicts belong to
    /// `brink-analyzer`, which records each `or` step's operand/result
    /// types for LIR lowering (`brink_analyzer::coalesce_types`, threaded
    /// to lowering as `brink_ir::lir::CoalesceLookup`). Lowering *consumes*
    /// that verdict; it never re-derives it from syntax. Under `types =
    /// strict` an ill-typed chain never reaches codegen at all — `E066`
    /// rejects it during analysis — so this op only ever executes a chain
    /// analysis either accepted or could not statically pin.
    ///
    /// ## The runtime check *is* the semantics for an unpinned `lhs`
    ///
    /// That second case is the gradual-mode posture, and it is deliberate:
    /// when the left-hand type is unknown (brink dialect, `types =
    /// gradual` — the un-overridden native default), **the check this op
    /// performs is the operator's semantics**, not a fallback for a missing
    /// one. An `OptionVal` coalesces; a plain value raises the `TypeError`
    /// fault (`brink_runtime::value_ops::coalesce_unwrap_some`) — the same
    /// class as every other gradual runtime check. Strict/native never
    /// reaches this path with an unpinned `lhs`, and the analyzer records
    /// exactly this case as `CoalesceShape::RuntimeCheck`, on which codegen
    /// emits no `MakeSome`: with `rhs` possibly never evaluated there is no
    /// value to read a shape off, so the unwrapped collapse form is the one
    /// shape that stays sound for the `(Option[T],T)->T` reading the check
    /// admits.
    CoalesceSome(i32),

    // ── B1b: the `as` binding (`docs/decision-log.md` 2026-07-26; issue
    //    #1475) ──────────────────────────────────────────────────────────
    /// `[opt]` → `[bool]` — the `as` binding's fused test-and-bind
    /// (`if EXPR as name { … }`, `while EXPR as name { … }`,
    /// `{if EXPR as name: … else: …}`). `opt` must be an `OptionVal`:
    /// `some(v)` stores the **unwrapped** `v` in temp `slot` and pushes
    /// `true`; `none` leaves `slot` untouched and pushes `false`. A
    /// non-`OptionVal` operand faults
    /// ([`RuntimeError::AsBindingNotOption`](crate::opcode) — the
    /// gradual-mode residual of the checker's `E147`).
    ///
    /// The slot is always freshly allocated by the binding itself, so —
    /// unlike [`SetTemp`](Self::SetTemp) — the write needs no
    /// pointer/projection write-through: an `as` binding can never land on
    /// a `ref` parameter's cell. Native-surface only.
    OptionBind(u16),

    // ── Seq `remove_at` (issue #1484, `docs/stdlib-spec.md` §4/§10) ───────
    /// `[a, i]` → updated array with the element at `i` removed (shifts
    /// later elements left) — the stdlib `remove_at()` mutator's primitive,
    /// the array-index leg [`MapRemove`](Self::MapRemove) generalized over
    /// before this PR. Array-only: a non-array `a` is a turn-terminating
    /// fault (`NotIndexable`). `i` must be an `Int` in `[0, len)` — strictly
    /// less than `len`, matching `IndexGet`/`IndexSet` (there is no element
    /// to remove at `len`, unlike `MapInsert`'s append-friendly `<=`).
    SeqRemoveAt,

    // ── NS-A6: the `std::rand` draw verbs (`docs/stdlib-spec.md` §7,
    // ruled 2026-07-18; `docs/stdlib-sequencing.md` §2 Wave A6). Every op
    // below draws through the ONE RNG state cell (`rng_seed` +
    // `previous_random` — the same cell ink's `RANDOM`/`SEED_RANDOM` have
    // always used; one cell, two surfaces, no drift) and is an ordinary
    // *write* to that cell in the effect row
    // (`DefinitionId::RNG_CELL`). `seed(n)` needs no new op — it lowers to
    // the frozen [`SeedRandom`](Self::SeedRandom). ────────────────────────
    /// `[]` → `Float` uniform in `[0,1)`. One draw. The value is built from
    /// the draw's top 24 bits (`draw >> 7`) divided by 2²⁴, so every result
    /// is exactly representable in the f32 payload and 1.0 is unreachable —
    /// part of the pinned-algorithm stability contract (see
    /// `brink-runtime::rand_ops`).
    RandFloat,
    /// `[p]` → `Bool`: one uniform `[0,1)` draw `u`, result `u < p` with
    /// `p` clamped to `[0,1]` and NaN → `false` (F3, ruled 2026-07-19:
    /// interpretation, not fabrication — total over the numeric domain).
    /// Always consumes exactly one draw, NaN included. Fault on a
    /// non-numeric `p` (malformed question).
    RandChance,
    /// `[coll]` → `Option[T]`: uniform draw of one element from an array
    /// (→ `some(elem)`) or a flags subset (→ `some(single-item list)`,
    /// mirroring the frozen `ListRandom` selection). Empty → `none`
    /// *without* consuming a draw. Fault on any other collection type.
    RandPick,
    /// `[a]` → `[a']`: Fisher-Yates shuffle of an array, `len-1` draws
    /// (none for `len < 2`), each advancing the RNG cell. One op serves
    /// both surfaces: `shuffle(a)` (statement-only, RMW write-back) and
    /// `shuffled(a)` (functional). Fault on a non-array.
    RandShuffle,
    /// `[start, end]` → `Range` (NS-A5, F7): construct an exclusive
    /// (`start..end`) range value from two int bounds. Fault on non-int
    /// bounds (malformed question — the T1b stdlib doctrine; no numeric
    /// coercion, range bounds are ints by ruling).
    RangeMakeExcl,
    /// `[start, end]` → `Range` (NS-A5, F7): construct an inclusive
    /// (`start..=end`) range value from two int bounds. Same fault
    /// contract as [`RangeMakeExcl`](Self::RangeMakeExcl).
    RangeMakeIncl,
    /// `[r]` → `Option[Range]` (NS-A5, the `non_empty(r)` validator —
    /// S2 ruled 2026-07-19): `some(r)` when the range denotes at least one
    /// element, `none` when it is empty. The Option tax sits once at the
    /// boundary where dynamic bounds enter; the checker types the `some`
    /// payload as the inhabited-range refinement. Pure — no draw, no
    /// write. Fault on a non-range operand.
    RangeNonEmpty,

    // ── NS-A4: the ordering verbs (`docs/stdlib-spec.md` §4b, issue
    // #1110) ────────────────────────────────────────────────────────────
    /// `[a]` → `[a']`: the array sorted ascending by the §4b ordering
    /// doctrine — int/float (numeric promotion), bool (`false < true`),
    /// string (USV-lexicographic), arrays lexicographic element-wise
    /// (recursively). Stable (equal elements keep their input order).
    /// Float NaN is mode-dependent: DEV mode faults on any NaN comparand
    /// (`UnorderedComparand` — the upstream bug surfaces at its first
    /// ordering consumption); PROD mode places it by the pinned
    /// non-fabricating total order (`-0 == +0` ties, NaN greatest,
    /// NaN-vs-NaN ties). One op serves both surfaces: `sort(a)`
    /// (statement-only, RMW write-back) and `sorted(a)` (functional) —
    /// the `RandShuffle` precedent. Fault on a non-array or unorderable
    /// elements (structs/enums without a registered `compare`, maps,
    /// flags subsets, divert targets — malformed question, all modes).
    SeqSorted,
    /// `[a, cmp]` → `[a']`: the array sorted ascending by a user
    /// comparator — `cmp` is a function value (`FnRef`/`Closure`) of
    /// shape `fn(T, T): int` (negative = less, zero = tie, positive =
    /// greater; F0 ruled 2026-07-19). Stable. The comparator runs under
    /// the pure·silent contract (checker-enforced where provable); the
    /// VM evaluates it re-entrantly with output isolated and faults if it
    /// yields, presents choices, calls an external, or returns a
    /// non-int. No NaN check here — F14: `sort_by` does not inherit
    /// `F:float`; the comparator owns the element semantics. One op
    /// serves `sort_by(a, cmp)` (statement-only, RMW write-back) and
    /// `sorted_by(a, cmp)` (functional). Fault on a non-array or
    /// non-function comparator.
    SeqSortedBy,

    // ── NS-A8: the numeric tower (`docs/tower-mini-spec.md`, issue
    // #1114) ────────────────────────────────────────────────────────────
    /// One opcode, thirteen operations: the [`TowerOp`] immediate selects
    /// the constructor or verb (see its per-kind docs for stack shapes).
    /// All pure; wrong-operand-type is a turn-terminating fault (a
    /// malformed question, per the ruled fault-vs-absence doctrine). The
    /// tower's operator family (`+`/`-`/`*`, `mat*vec`, `quat*quat`,
    /// `quat*vec`) rides the frozen arithmetic opcodes instead — see
    /// `value_ops::binary_op`.
    Tower(TowerOp),

    // ── NS-A7: collections+ (`docs/stdlib-spec.md` §8, issue #1113) ────
    /// One opcode, five operations: the [`CollectOp`] immediate selects
    /// `Weighted[T]` construction, the `rand::roll` draw, or one of the
    /// heap verbs (see its per-kind docs for stack shapes). `RandRoll`
    /// writes the RNG cell; `HeapPush` carries the §4b dev/prod NaN
    /// entry-check; everything else is pure over its operands.
    Collect(CollectOp),

    // ── The fn-value verb layer (`docs/stdlib-spec.md` §4, issue #1679) ──
    /// One opcode, one operation per [`SeqVerbOp`] kind: the pure trio
    /// `map`/`filter`/`fold`. Every kind pops a callback function value
    /// (`FnRef`/`Closure`) and evaluates it re-entrantly per element with
    /// output isolated — the `SeqSortedBy` machinery, one callback contract.
    /// See the per-kind docs for stack shapes.
    SeqVerb(SeqVerbOp),

    // ── Lifecycle ───────────────────────────────────────────────────────
    Done,
    /// Pause for choice presentation. Like `Done` but does NOT set
    /// `did_safe_exit` — if no choices are pending, the story ran
    /// out of content rather than reaching an explicit `-> DONE`.
    Yield,
    End,
    Nop,

    // ── String eval ─────────────────────────────────────────────────────
    BeginStringEval,
    EndStringEval,
}

// ── Opcode encode / decode ──────────────────────────────────────────────────

// `Opcode::peek_static`'s classification table: discriminant byte → one of
// the `CLASS_*` codes below, `0` for every instruction that carries no
// static operand. Built once, at compile time, from the same constants the
// encoder uses, so it cannot drift from them.
const CLASS_GOTO: u8 = 1;
const CLASS_GOTO_IF: u8 = 2;
const CLASS_ENTER_CONTAINER: u8 = 3;
const CLASS_CALL: u8 = 4;
const CLASS_TUNNEL_CALL: u8 = 5;
const CLASS_THREAD_CALL: u8 = 6;
const CLASS_BEGIN_CHOICE: u8 = 7;
const CLASS_GET_GLOBAL: u8 = 8;
const CLASS_SET_GLOBAL: u8 = 9;
const CLASS_TAKE_GLOBAL: u8 = 10;

const STATIC_CLASS: [u8; 256] = {
    let mut table = [0u8; 256];
    table[GOTO as usize] = CLASS_GOTO;
    table[GOTO_IF as usize] = CLASS_GOTO_IF;
    table[ENTER_CONTAINER as usize] = CLASS_ENTER_CONTAINER;
    table[CALL as usize] = CLASS_CALL;
    table[TUNNEL_CALL as usize] = CLASS_TUNNEL_CALL;
    table[THREAD_CALL as usize] = CLASS_THREAD_CALL;
    table[BEGIN_CHOICE as usize] = CLASS_BEGIN_CHOICE;
    table[GET_GLOBAL as usize] = CLASS_GET_GLOBAL;
    table[SET_GLOBAL as usize] = CLASS_SET_GLOBAL;
    table[TAKE_GLOBAL as usize] = CLASS_TAKE_GLOBAL;
    table
};

/// The static-target instructions: each carries exactly one `DefinitionId`
/// operand, and that operand names the address the instruction jumps to or
/// calls. See [`Opcode::peek_target`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Goto,
    GotoIf,
    EnterContainer,
    Call,
    TunnelCall,
    ThreadCall,
    /// The choice's flags byte precedes its target operand.
    BeginChoice(ChoiceFlags),
}

/// Where a static-target instruction's operand sits: `buf[operand..end]`
/// holds the `DefinitionId` (or the linked layer's replacement for it), and
/// `end` is the offset of the next instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetSite {
    pub kind: TargetKind,
    pub operand: usize,
    pub end: usize,
}

/// The static-global instructions: each carries exactly one `DefinitionId`
/// operand naming the global variable it reads or writes. `PushVarPointer`
/// is deliberately not one — its operand becomes a `Value::VariablePointer`
/// the story can hold and pass around, so it must stay an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalKind {
    Get,
    Set,
    Take,
}

/// Every instruction whose sole `DefinitionId` operand is static — a jump
/// or call address, or a global variable — as [`Opcode::peek_static`]
/// classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticKind {
    Target(TargetKind),
    Global(GlobalKind),
}

/// Where a static-operand instruction's operand sits: `buf[operand..end]`
/// holds the `DefinitionId` (or the linked layer's replacement for it), and
/// `end` is the offset of the next instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticSite {
    pub kind: StaticKind,
    pub operand: usize,
    pub end: usize,
}

impl Opcode {
    /// Encode this instruction into the byte buffer.
    #[expect(clippy::too_many_lines)]
    pub fn encode(&self, buf: &mut Vec<u8>) {
        match *self {
            // Stack & literals
            Self::PushInt(v) => {
                write_u8(buf, PUSH_INT);
                write_i32(buf, v);
            }
            Self::PushFloat(v) => {
                write_u8(buf, PUSH_FLOAT);
                write_f32(buf, v);
            }
            Self::PushBool(v) => {
                write_u8(buf, PUSH_BOOL);
                write_u8(buf, u8::from(v));
            }
            Self::PushString(idx) => {
                write_u8(buf, PUSH_STRING);
                write_u16(buf, idx);
            }
            Self::PushList(idx) => {
                write_u8(buf, PUSH_LIST);
                write_u16(buf, idx);
            }
            Self::PushDivertTarget(id) => {
                write_u8(buf, PUSH_DIVERT_TARGET);
                write_def_id(buf, id);
            }
            Self::PushNull => write_u8(buf, PUSH_NULL),
            Self::Pop => write_u8(buf, POP),
            Self::Duplicate => write_u8(buf, DUPLICATE),

            // Arithmetic
            Self::Add => write_u8(buf, ADD),
            Self::Subtract => write_u8(buf, SUBTRACT),
            Self::Multiply => write_u8(buf, MULTIPLY),
            Self::Divide => write_u8(buf, DIVIDE),
            Self::Modulo => write_u8(buf, MODULO),
            Self::Negate => write_u8(buf, NEGATE),

            // Comparison
            Self::Equal => write_u8(buf, EQUAL),
            Self::NotEqual => write_u8(buf, NOT_EQUAL),
            Self::Greater => write_u8(buf, GREATER),
            Self::GreaterOrEqual => write_u8(buf, GREATER_OR_EQUAL),
            Self::Less => write_u8(buf, LESS),
            Self::LessOrEqual => write_u8(buf, LESS_OR_EQUAL),

            // Logic
            Self::Not => write_u8(buf, NOT),
            Self::And => write_u8(buf, AND),
            Self::Or => write_u8(buf, OR),

            // Global vars
            Self::GetGlobal(id) => {
                write_u8(buf, GET_GLOBAL);
                write_def_id(buf, id);
            }
            Self::SetGlobal(id) => {
                write_u8(buf, SET_GLOBAL);
                write_def_id(buf, id);
            }

            // Temp vars
            Self::DeclareTemp(idx) => {
                write_u8(buf, DECLARE_TEMP);
                write_u16(buf, idx);
            }
            Self::GetTemp(idx) => {
                write_u8(buf, GET_TEMP);
                write_u16(buf, idx);
            }
            Self::SetTemp(idx) => {
                write_u8(buf, SET_TEMP);
                write_u16(buf, idx);
            }
            Self::GetTempRaw(idx) => {
                write_u8(buf, GET_TEMP_RAW);
                write_u16(buf, idx);
            }

            // Variable pointers
            Self::PushVarPointer(id) => {
                write_u8(buf, PUSH_VAR_POINTER);
                write_def_id(buf, id);
            }
            Self::PushTempPointer(slot) => {
                write_u8(buf, PUSH_TEMP_POINTER);
                write_u16(buf, slot);
            }

            // Control flow
            Self::Jump(offset) => {
                write_u8(buf, JUMP);
                write_i32(buf, offset);
            }
            Self::JumpIfFalse(offset) => {
                write_u8(buf, JUMP_IF_FALSE);
                write_i32(buf, offset);
            }
            Self::Goto(id) => {
                write_u8(buf, GOTO);
                write_def_id(buf, id);
            }
            Self::GotoIf(id) => {
                write_u8(buf, GOTO_IF);
                write_def_id(buf, id);
            }
            Self::GotoVariable => write_u8(buf, GOTO_VARIABLE),

            // Container flow
            Self::EnterContainer(id) => {
                write_u8(buf, ENTER_CONTAINER);
                write_def_id(buf, id);
            }
            Self::ExitContainer => write_u8(buf, EXIT_CONTAINER),

            // Functions / tunnels
            Self::Call(id) => {
                write_u8(buf, CALL);
                write_def_id(buf, id);
            }
            Self::Return => write_u8(buf, RETURN),
            Self::TunnelCall(id) => {
                write_u8(buf, TUNNEL_CALL);
                write_def_id(buf, id);
            }
            Self::TunnelReturn => write_u8(buf, TUNNEL_RETURN),
            Self::TunnelCallVariable => write_u8(buf, TUNNEL_CALL_VARIABLE),
            Self::CallVariable(argc) => {
                write_u8(buf, CALL_VARIABLE);
                write_u8(buf, argc);
            }

            // Threads
            Self::ThreadCall(id) => {
                write_u8(buf, THREAD_CALL);
                write_def_id(buf, id);
            }
            Self::ThreadStart => write_u8(buf, THREAD_START),
            Self::ThreadDone => write_u8(buf, THREAD_DONE),

            // Output
            Self::EmitLine(idx, slot_count) => {
                write_u8(buf, EMIT_LINE);
                write_u16(buf, idx);
                write_u8(buf, slot_count);
            }
            Self::EmitValue => write_u8(buf, EMIT_VALUE),
            Self::EmitNewline => write_u8(buf, EMIT_NEWLINE),
            Self::EmitLineNl(idx, slot_count) => {
                write_u8(buf, EMIT_LINE_NL);
                write_u16(buf, idx);
                write_u8(buf, slot_count);
            }
            Self::BinaryImm(kind, imm) => {
                write_u8(buf, BINARY_IMM);
                write_u8(buf, kind.to_byte());
                write_i32(buf, imm);
            }
            Self::BinaryJumpIfFalse(kind, rel) => {
                write_u8(buf, BINARY_JUMP_IF_FALSE);
                write_u8(buf, kind.to_byte());
                write_i32(buf, rel);
            }
            Self::BinaryImmJumpIfFalse(kind, imm, rel) => {
                write_u8(buf, BINARY_IMM_JUMP_IF_FALSE);
                write_u8(buf, kind.to_byte());
                write_i32(buf, imm);
                write_i32(buf, rel);
            }
            Self::Spring => write_u8(buf, SPRING),
            Self::Glue => write_u8(buf, GLUE),
            Self::BeginTag => write_u8(buf, BEGIN_TAG),
            Self::EndTag => write_u8(buf, END_TAG),
            Self::EvalLine(idx, slot_count) => {
                write_u8(buf, EVAL_LINE);
                write_u16(buf, idx);
                write_u8(buf, slot_count);
            }
            Self::BeginFragment => write_u8(buf, BEGIN_FRAGMENT),
            Self::EndFragment => write_u8(buf, END_FRAGMENT),
            Self::AttachElement => write_u8(buf, ATTACH_ELEMENT),
            Self::EndElementRun => write_u8(buf, END_ELEMENT_RUN),

            // Choices
            Self::BeginChoice(flags, target) => {
                write_u8(buf, BEGIN_CHOICE);
                write_u8(buf, flags.to_byte());
                write_def_id(buf, target);
            }
            Self::EndChoice => write_u8(buf, END_CHOICE),

            // Sequences
            Self::Sequence(kind, count) => {
                write_u8(buf, SEQUENCE);
                write_u8(buf, kind.to_byte());
                write_u8(buf, count);
            }
            Self::SequenceBranch(offset) => {
                write_u8(buf, SEQUENCE_BRANCH);
                write_i32(buf, offset);
            }

            // Intrinsics
            Self::VisitCount => write_u8(buf, VISIT_COUNT),
            Self::CurrentVisitCount => write_u8(buf, CURRENT_VISIT_COUNT),
            Self::TouchVisit => write_u8(buf, TOUCH_VISIT),
            Self::ShuffleIndexOf => write_u8(buf, SHUFFLE_INDEX_OF),
            Self::TurnsSince => write_u8(buf, TURNS_SINCE),
            Self::TurnIndex => write_u8(buf, TURN_INDEX),
            Self::ChoiceCount => write_u8(buf, CHOICE_COUNT),
            Self::Random => write_u8(buf, RANDOM),
            Self::SeedRandom => write_u8(buf, SEED_RANDOM),

            // Casts / math
            Self::CastToInt => write_u8(buf, CAST_TO_INT),
            Self::CastToFloat => write_u8(buf, CAST_TO_FLOAT),
            Self::Floor => write_u8(buf, FLOOR),
            Self::Ceiling => write_u8(buf, CEILING),
            Self::Pow => write_u8(buf, POW),
            Self::Min => write_u8(buf, MIN),
            Self::Max => write_u8(buf, MAX),

            // External fns
            Self::CallExternal(id, argc) => {
                write_u8(buf, CALL_EXTERNAL);
                write_def_id(buf, id);
                write_u8(buf, argc);
            }

            // List ops
            Self::ListContains => write_u8(buf, LIST_CONTAINS),
            Self::ListNotContains => write_u8(buf, LIST_NOT_CONTAINS),
            Self::ListIntersect => write_u8(buf, LIST_INTERSECT),
            Self::ListAll => write_u8(buf, LIST_ALL),
            Self::ListInvert => write_u8(buf, LIST_INVERT),
            Self::ListCount => write_u8(buf, LIST_COUNT),
            Self::ListMin => write_u8(buf, LIST_MIN),
            Self::ListMax => write_u8(buf, LIST_MAX),
            Self::ListValue => write_u8(buf, LIST_VALUE),
            Self::ListRange => write_u8(buf, LIST_RANGE),
            Self::ListFromInt => write_u8(buf, LIST_FROM_INT),
            Self::ListRandom => write_u8(buf, LIST_RANDOM),

            // Collections
            Self::ArrayNew(n) => {
                write_u8(buf, ARRAY_NEW);
                write_u32(buf, n);
            }
            Self::MapNew(n) => {
                write_u8(buf, MAP_NEW);
                write_u32(buf, n);
            }
            Self::IndexGet => write_u8(buf, INDEX_GET),
            Self::IndexSet => write_u8(buf, INDEX_SET),
            Self::CollectionLen => write_u8(buf, COLLECTION_LEN),
            Self::MapGet => write_u8(buf, MAP_GET),
            Self::MapInsert => write_u8(buf, MAP_INSERT),
            Self::MapRemove => write_u8(buf, MAP_REMOVE),
            Self::MapContains => write_u8(buf, MAP_CONTAINS),
            Self::CollectionKeys => write_u8(buf, COLLECTION_KEYS),
            Self::CollectionValues => write_u8(buf, COLLECTION_VALUES),
            Self::PushLiteral(idx) => {
                write_u8(buf, PUSH_LITERAL);
                write_u32(buf, idx);
            }

            // Sharing discipline
            Self::TakeGlobal(id) => {
                write_u8(buf, TAKE_GLOBAL);
                write_def_id(buf, id);
            }
            Self::TakeTemp(idx) => {
                write_u8(buf, TAKE_TEMP);
                write_u16(buf, idx);
            }

            // Records
            Self::RecordNew(shape_id) => {
                write_u8(buf, RECORD_NEW);
                write_u32(buf, shape_id);
            }
            Self::RecordGetDyn(name_id) => {
                write_u8(buf, RECORD_GET_DYN);
                write_u16(buf, name_id);
            }
            Self::RecordSetDyn(name_id) => {
                write_u8(buf, RECORD_SET_DYN);
                write_u16(buf, name_id);
            }
            Self::RecordGet(offset) => {
                write_u8(buf, RECORD_GET);
                write_u16(buf, offset);
            }
            Self::RecordSet(offset) => {
                write_u8(buf, RECORD_SET);
                write_u16(buf, offset);
            }

            // Conversion intrinsics (TM-3 completion, #659)
            Self::ConvertInt => write_u8(buf, CONVERT_INT),
            Self::ConvertFloat => write_u8(buf, CONVERT_FLOAT),
            Self::ConvertString => write_u8(buf, CONVERT_STRING),

            // Function values (T1c, #700)
            Self::PushFnRef(id) => {
                write_u8(buf, PUSH_FN_REF);
                write_def_id(buf, id);
            }
            Self::MakeClosure {
                target,
                bound_count,
            } => {
                write_u8(buf, MAKE_CLOSURE);
                write_def_id(buf, target);
                write_u8(buf, bound_count);
            }
            Self::CallValue(argc) => {
                write_u8(buf, CALL_VALUE);
                write_u8(buf, argc);
            }
            Self::BindValue(argc) => {
                write_u8(buf, BIND_VALUE);
                write_u8(buf, argc);
            }

            // Path projections (T1e)
            Self::MakeProjection {
                root,
                segment_count,
            } => {
                write_u8(buf, MAKE_PROJECTION);
                write_def_id(buf, root);
                write_u8(buf, segment_count);
            }
            Self::ProjRead => write_u8(buf, PROJ_READ),
            Self::ProjWrite => write_u8(buf, PROJ_WRITE),

            // Stdlib slice 1 completion (#857)
            Self::CharAt => write_u8(buf, CHAR_AT),

            // NS-A1 Option + stdlib flips
            Self::PushNone => write_u8(buf, PUSH_NONE),
            Self::MakeSome => write_u8(buf, MAKE_SOME),
            Self::StrFind => write_u8(buf, STR_FIND),
            Self::SeqIndexOf => write_u8(buf, SEQ_INDEX_OF),
            Self::SeqMin => write_u8(buf, SEQ_MIN),
            Self::SeqMax => write_u8(buf, SEQ_MAX),
            Self::SeqFirst => write_u8(buf, SEQ_FIRST),
            Self::SeqLast => write_u8(buf, SEQ_LAST),
            Self::SeqPop => write_u8(buf, SEQ_POP),
            Self::MapGetOpt => write_u8(buf, MAP_GET_OPT),
            Self::MapContainsValue => write_u8(buf, MAP_CONTAINS_VALUE),
            Self::MapClear => write_u8(buf, MAP_CLEAR),

            // B1 `or`-coalescing, short-circuited (#1471)
            Self::CoalesceSome(offset) => {
                write_u8(buf, COALESCE_SOME);
                write_i32(buf, offset);
            }
            Self::OptionBind(slot) => {
                write_u8(buf, OPTION_BIND);
                write_u16(buf, slot);
            }
            Self::SeqRemoveAt => write_u8(buf, SEQ_REMOVE_AT),
            Self::RandFloat => write_u8(buf, RAND_FLOAT),
            Self::RandChance => write_u8(buf, RAND_CHANCE),
            Self::RandPick => write_u8(buf, RAND_PICK),
            Self::RandShuffle => write_u8(buf, RAND_SHUFFLE),
            Self::RangeMakeExcl => write_u8(buf, RANGE_MAKE_EXCL),
            Self::RangeMakeIncl => write_u8(buf, RANGE_MAKE_INCL),
            Self::RangeNonEmpty => write_u8(buf, RANGE_NON_EMPTY),

            // NS-A4 ordering verbs
            Self::SeqSorted => write_u8(buf, SEQ_SORTED),
            Self::SeqSortedBy => write_u8(buf, SEQ_SORTED_BY),

            // NS-A7 collections+: discriminant + CollectOp kind byte.
            Self::Collect(op) => {
                write_u8(buf, COLLECT);
                write_u8(buf, op.to_byte());
            }

            // The fn-value verbs: discriminant + SeqVerbOp kind byte.
            Self::SeqVerb(op) => {
                write_u8(buf, SEQ_VERB);
                write_u8(buf, op.to_byte());
            }

            // NS-A8 numeric tower: discriminant + TowerOp kind byte.
            Self::Tower(op) => {
                write_u8(buf, TOWER);
                write_u8(buf, op.to_byte());
            }

            // Lifecycle
            Self::Done => write_u8(buf, DONE),
            Self::Yield => write_u8(buf, YIELD),
            Self::End => write_u8(buf, END),
            Self::Nop => write_u8(buf, NOP),

            // String eval
            Self::BeginStringEval => write_u8(buf, BEGIN_STRING_EVAL),
            Self::EndStringEval => write_u8(buf, END_STRING_EVAL),
        }
    }

    /// The width of a static-target operand: one `DefinitionId`.
    pub const TARGET_OPERAND_LEN: usize = 8;

    /// Classify the instruction at `buf[offset]` as one whose only
    /// `DefinitionId` operand is static — a jump/call address or a global
    /// variable — and locate that operand, **without decoding it**.
    ///
    /// This is an encoding fact about the instruction stream, offered to the
    /// runtime's linker: it resolves each such operand once and, in its own
    /// linked copy of the code, replaces the id bytes with a resolved form
    /// of its choosing. `Opcode::decode` is not defined over that copy (a
    /// replaced operand is no longer a valid `DefinitionId`), which is why
    /// the runtime keeps the symbolic bytecode for every decoder besides its
    /// own dispatch. Returns `None` for any other instruction, and for a
    /// truncated buffer.
    #[must_use]
    #[inline]
    pub fn peek_static(buf: &[u8], offset: usize) -> Option<StaticSite> {
        // One table load decides the common case (not a static-operand
        // instruction) — this runs on every VM fetch, so a sparse `match`
        // over the discriminants is too expensive here.
        let class = STATIC_CLASS[*buf.get(offset)? as usize];
        if class == 0 {
            return None;
        }
        let (kind, operand) = match class {
            CLASS_GOTO => (StaticKind::Target(TargetKind::Goto), offset + 1),
            CLASS_GOTO_IF => (StaticKind::Target(TargetKind::GotoIf), offset + 1),
            CLASS_ENTER_CONTAINER => (StaticKind::Target(TargetKind::EnterContainer), offset + 1),
            CLASS_CALL => (StaticKind::Target(TargetKind::Call), offset + 1),
            CLASS_TUNNEL_CALL => (StaticKind::Target(TargetKind::TunnelCall), offset + 1),
            CLASS_THREAD_CALL => (StaticKind::Target(TargetKind::ThreadCall), offset + 1),
            CLASS_BEGIN_CHOICE => {
                let flags = ChoiceFlags::from_byte(*buf.get(offset + 1)?);
                (
                    StaticKind::Target(TargetKind::BeginChoice(flags)),
                    offset + 2,
                )
            }
            CLASS_GET_GLOBAL => (StaticKind::Global(GlobalKind::Get), offset + 1),
            CLASS_SET_GLOBAL => (StaticKind::Global(GlobalKind::Set), offset + 1),
            CLASS_TAKE_GLOBAL => (StaticKind::Global(GlobalKind::Take), offset + 1),
            _ => return None,
        };
        let end = operand + Self::TARGET_OPERAND_LEN;
        (end <= buf.len()).then_some(StaticSite { kind, operand, end })
    }

    /// [`Self::peek_static`] restricted to the jump/call targets.
    #[must_use]
    pub fn peek_target(buf: &[u8], offset: usize) -> Option<TargetSite> {
        let site = Self::peek_static(buf, offset)?;
        match site.kind {
            StaticKind::Target(kind) => Some(TargetSite {
                kind,
                operand: site.operand,
                end: site.end,
            }),
            StaticKind::Global(_) => None,
        }
    }

    /// Decode a single instruction from `buf` starting at `*offset`.
    ///
    /// On success, `*offset` is advanced past the consumed bytes.
    #[expect(clippy::too_many_lines)]
    pub fn decode(buf: &[u8], offset: &mut usize) -> Result<Self, DecodeError> {
        let disc = read_u8(buf, offset)?;

        let op = match disc {
            // Stack & literals
            PUSH_INT => Self::PushInt(read_i32(buf, offset)?),
            PUSH_FLOAT => Self::PushFloat(read_f32(buf, offset)?),
            PUSH_BOOL => Self::PushBool(read_u8(buf, offset)? != 0),
            PUSH_STRING => Self::PushString(read_u16(buf, offset)?),
            PUSH_LIST => Self::PushList(read_u16(buf, offset)?),
            PUSH_DIVERT_TARGET => Self::PushDivertTarget(read_def_id(buf, offset)?),
            PUSH_NULL => Self::PushNull,
            POP => Self::Pop,
            DUPLICATE => Self::Duplicate,

            // Arithmetic
            ADD => Self::Add,
            SUBTRACT => Self::Subtract,
            MULTIPLY => Self::Multiply,
            DIVIDE => Self::Divide,
            MODULO => Self::Modulo,
            NEGATE => Self::Negate,

            // Comparison
            EQUAL => Self::Equal,
            NOT_EQUAL => Self::NotEqual,
            GREATER => Self::Greater,
            GREATER_OR_EQUAL => Self::GreaterOrEqual,
            LESS => Self::Less,
            LESS_OR_EQUAL => Self::LessOrEqual,

            // Logic
            NOT => Self::Not,
            AND => Self::And,
            OR => Self::Or,

            // Global vars
            GET_GLOBAL => Self::GetGlobal(read_def_id(buf, offset)?),
            SET_GLOBAL => Self::SetGlobal(read_def_id(buf, offset)?),

            // Temp vars
            DECLARE_TEMP => Self::DeclareTemp(read_u16(buf, offset)?),
            GET_TEMP => Self::GetTemp(read_u16(buf, offset)?),
            SET_TEMP => Self::SetTemp(read_u16(buf, offset)?),
            GET_TEMP_RAW => Self::GetTempRaw(read_u16(buf, offset)?),

            // Variable pointers
            PUSH_VAR_POINTER => Self::PushVarPointer(read_def_id(buf, offset)?),
            PUSH_TEMP_POINTER => Self::PushTempPointer(read_u16(buf, offset)?),

            // Control flow
            JUMP => Self::Jump(read_i32(buf, offset)?),
            JUMP_IF_FALSE => Self::JumpIfFalse(read_i32(buf, offset)?),
            GOTO => Self::Goto(read_def_id(buf, offset)?),
            GOTO_IF => Self::GotoIf(read_def_id(buf, offset)?),
            GOTO_VARIABLE => Self::GotoVariable,

            // Container flow
            ENTER_CONTAINER => Self::EnterContainer(read_def_id(buf, offset)?),
            EXIT_CONTAINER => Self::ExitContainer,

            // Functions / tunnels
            CALL => Self::Call(read_def_id(buf, offset)?),
            RETURN => Self::Return,
            TUNNEL_CALL => Self::TunnelCall(read_def_id(buf, offset)?),
            TUNNEL_RETURN => Self::TunnelReturn,
            TUNNEL_CALL_VARIABLE => Self::TunnelCallVariable,
            CALL_VARIABLE => Self::CallVariable(read_u8(buf, offset)?),

            // Threads
            THREAD_CALL => Self::ThreadCall(read_def_id(buf, offset)?),
            THREAD_START => Self::ThreadStart,
            THREAD_DONE => Self::ThreadDone,

            // Output
            EMIT_LINE => {
                let idx = read_u16(buf, offset)?;
                let slot_count = read_u8(buf, offset)?;
                Self::EmitLine(idx, slot_count)
            }
            EMIT_VALUE => Self::EmitValue,
            EMIT_NEWLINE => Self::EmitNewline,
            EMIT_LINE_NL => {
                let idx = read_u16(buf, offset)?;
                let slot_count = read_u8(buf, offset)?;
                Self::EmitLineNl(idx, slot_count)
            }
            BINARY_IMM => {
                let kind = BinaryKind::from_byte(read_u8(buf, offset)?)?;
                Self::BinaryImm(kind, read_i32(buf, offset)?)
            }
            BINARY_JUMP_IF_FALSE => {
                let kind = BinaryKind::from_byte(read_u8(buf, offset)?)?;
                Self::BinaryJumpIfFalse(kind, read_i32(buf, offset)?)
            }
            BINARY_IMM_JUMP_IF_FALSE => {
                let kind = BinaryKind::from_byte(read_u8(buf, offset)?)?;
                let imm = read_i32(buf, offset)?;
                Self::BinaryImmJumpIfFalse(kind, imm, read_i32(buf, offset)?)
            }
            SPRING => Self::Spring,
            GLUE => Self::Glue,
            BEGIN_TAG => Self::BeginTag,
            END_TAG => Self::EndTag,
            EVAL_LINE => {
                let idx = read_u16(buf, offset)?;
                let slot_count = read_u8(buf, offset)?;
                Self::EvalLine(idx, slot_count)
            }
            BEGIN_FRAGMENT => Self::BeginFragment,
            END_FRAGMENT => Self::EndFragment,
            ATTACH_ELEMENT => Self::AttachElement,
            END_ELEMENT_RUN => Self::EndElementRun,

            // Choices
            BEGIN_CHOICE => {
                let flags = ChoiceFlags::from_byte(read_u8(buf, offset)?);
                let target = read_def_id(buf, offset)?;
                Self::BeginChoice(flags, target)
            }
            END_CHOICE => Self::EndChoice,

            // Sequences
            SEQUENCE => {
                let kind = SequenceKind::from_byte(read_u8(buf, offset)?)?;
                let count = read_u8(buf, offset)?;
                Self::Sequence(kind, count)
            }
            SEQUENCE_BRANCH => Self::SequenceBranch(read_i32(buf, offset)?),

            // Intrinsics
            VISIT_COUNT => Self::VisitCount,
            CURRENT_VISIT_COUNT => Self::CurrentVisitCount,
            TOUCH_VISIT => Self::TouchVisit,
            SHUFFLE_INDEX_OF => Self::ShuffleIndexOf,
            TURNS_SINCE => Self::TurnsSince,
            TURN_INDEX => Self::TurnIndex,
            CHOICE_COUNT => Self::ChoiceCount,
            RANDOM => Self::Random,
            SEED_RANDOM => Self::SeedRandom,

            // Casts / math
            CAST_TO_INT => Self::CastToInt,
            CAST_TO_FLOAT => Self::CastToFloat,
            FLOOR => Self::Floor,
            CEILING => Self::Ceiling,
            POW => Self::Pow,
            MIN => Self::Min,
            MAX => Self::Max,

            // External fns
            CALL_EXTERNAL => {
                let id = read_def_id(buf, offset)?;
                let argc = read_u8(buf, offset)?;
                Self::CallExternal(id, argc)
            }

            // List ops
            LIST_CONTAINS => Self::ListContains,
            LIST_NOT_CONTAINS => Self::ListNotContains,
            LIST_INTERSECT => Self::ListIntersect,
            LIST_ALL => Self::ListAll,
            LIST_INVERT => Self::ListInvert,
            LIST_COUNT => Self::ListCount,
            LIST_MIN => Self::ListMin,
            LIST_MAX => Self::ListMax,
            LIST_VALUE => Self::ListValue,
            LIST_RANGE => Self::ListRange,
            LIST_FROM_INT => Self::ListFromInt,
            LIST_RANDOM => Self::ListRandom,

            // Collections
            ARRAY_NEW => Self::ArrayNew(read_u32(buf, offset)?),
            MAP_NEW => Self::MapNew(read_u32(buf, offset)?),
            INDEX_GET => Self::IndexGet,
            INDEX_SET => Self::IndexSet,
            COLLECTION_LEN => Self::CollectionLen,
            MAP_GET => Self::MapGet,
            MAP_INSERT => Self::MapInsert,
            MAP_REMOVE => Self::MapRemove,
            MAP_CONTAINS => Self::MapContains,
            COLLECTION_KEYS => Self::CollectionKeys,
            COLLECTION_VALUES => Self::CollectionValues,
            PUSH_LITERAL => Self::PushLiteral(read_u32(buf, offset)?),

            // Sharing discipline
            TAKE_GLOBAL => Self::TakeGlobal(read_def_id(buf, offset)?),
            TAKE_TEMP => Self::TakeTemp(read_u16(buf, offset)?),

            // Records
            RECORD_NEW => Self::RecordNew(read_u32(buf, offset)?),
            RECORD_GET_DYN => Self::RecordGetDyn(read_u16(buf, offset)?),
            RECORD_SET_DYN => Self::RecordSetDyn(read_u16(buf, offset)?),
            RECORD_GET => Self::RecordGet(read_u16(buf, offset)?),
            RECORD_SET => Self::RecordSet(read_u16(buf, offset)?),

            // Conversion intrinsics (TM-3 completion, #659)
            CONVERT_INT => Self::ConvertInt,
            CONVERT_FLOAT => Self::ConvertFloat,
            CONVERT_STRING => Self::ConvertString,

            // Function values (T1c, #700)
            PUSH_FN_REF => Self::PushFnRef(read_def_id(buf, offset)?),
            MAKE_CLOSURE => Self::MakeClosure {
                target: read_def_id(buf, offset)?,
                bound_count: read_u8(buf, offset)?,
            },
            CALL_VALUE => Self::CallValue(read_u8(buf, offset)?),
            BIND_VALUE => Self::BindValue(read_u8(buf, offset)?),

            // Path projections (T1e)
            MAKE_PROJECTION => Self::MakeProjection {
                root: read_def_id(buf, offset)?,
                segment_count: read_u8(buf, offset)?,
            },
            PROJ_READ => Self::ProjRead,
            PROJ_WRITE => Self::ProjWrite,

            // Stdlib slice 1 completion (#857)
            CHAR_AT => Self::CharAt,

            // NS-A1 Option + stdlib flips
            PUSH_NONE => Self::PushNone,
            MAKE_SOME => Self::MakeSome,
            STR_FIND => Self::StrFind,
            SEQ_INDEX_OF => Self::SeqIndexOf,
            SEQ_MIN => Self::SeqMin,
            SEQ_MAX => Self::SeqMax,
            SEQ_FIRST => Self::SeqFirst,
            SEQ_LAST => Self::SeqLast,
            SEQ_POP => Self::SeqPop,
            MAP_GET_OPT => Self::MapGetOpt,
            MAP_CONTAINS_VALUE => Self::MapContainsValue,
            MAP_CLEAR => Self::MapClear,

            // B1 `or`-coalescing, short-circuited (#1471)
            COALESCE_SOME => Self::CoalesceSome(read_i32(buf, offset)?),
            OPTION_BIND => Self::OptionBind(read_u16(buf, offset)?),
            SEQ_REMOVE_AT => Self::SeqRemoveAt,
            RAND_FLOAT => Self::RandFloat,
            RAND_CHANCE => Self::RandChance,
            RAND_PICK => Self::RandPick,
            RAND_SHUFFLE => Self::RandShuffle,
            RANGE_MAKE_EXCL => Self::RangeMakeExcl,
            RANGE_MAKE_INCL => Self::RangeMakeIncl,
            RANGE_NON_EMPTY => Self::RangeNonEmpty,

            // NS-A4 ordering verbs
            SEQ_SORTED => Self::SeqSorted,
            SEQ_SORTED_BY => Self::SeqSortedBy,

            // NS-A8 numeric tower: TowerOp kind byte follows; unknown
            // kinds are a decode error (reserved-tag discipline).
            TOWER => Self::Tower(TowerOp::from_byte(read_u8(buf, offset)?)?),

            // NS-A7 collections+: CollectOp kind byte follows; unknown
            // kinds are a decode error (reserved-tag discipline).
            COLLECT => Self::Collect(CollectOp::from_byte(read_u8(buf, offset)?)?),

            // The fn-value verbs: SeqVerbOp kind byte follows; unknown
            // kinds are a decode error (reserved-tag discipline).
            SEQ_VERB => Self::SeqVerb(SeqVerbOp::from_byte(read_u8(buf, offset)?)?),

            // Lifecycle
            DONE => Self::Done,
            YIELD => Self::Yield,
            END => Self::End,
            NOP => Self::Nop,

            // String eval
            BEGIN_STRING_EVAL => Self::BeginStringEval,
            END_STRING_EVAL => Self::EndStringEval,

            _ => return Err(DecodeError::UnknownOpcode(disc)),
        };

        Ok(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DefinitionTag;

    fn roundtrip(op: &Opcode) {
        let mut buf = Vec::new();
        op.encode(&mut buf);
        let mut offset = 0;
        let decoded = Opcode::decode(&buf, &mut offset).unwrap();
        assert_eq!(*op, decoded, "roundtrip failed for {op:?}");
        assert_eq!(offset, buf.len(), "not all bytes consumed for {op:?}");
    }

    fn test_id() -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, 0xBEEF)
    }

    /// `peek_target` finds exactly the static-target instructions, places
    /// their operand where `encode` wrote the id, and declines everything
    /// else — including a buffer that ends inside the operand.
    #[test]
    fn peek_target_locates_static_target_operands() {
        let flags = ChoiceFlags {
            has_condition: true,
            has_start_content: false,
            has_choice_only_content: true,
            once_only: false,
            is_invisible_default: false,
        };
        let ops = [
            Opcode::PushInt(7),
            Opcode::Goto(test_id()),
            Opcode::BeginChoice(flags, test_id()),
            Opcode::Call(test_id()),
            Opcode::Nop,
        ];
        let mut buf = Vec::new();
        let mut starts = Vec::new();
        for op in &ops {
            starts.push(buf.len());
            op.encode(&mut buf);
        }
        let ends: Vec<usize> = starts.iter().skip(1).copied().chain([buf.len()]).collect();

        assert_eq!(
            Opcode::peek_target(&buf, starts[0]),
            None,
            "PushInt is not a target op"
        );
        assert_eq!(
            Opcode::peek_target(&buf, starts[1]),
            Some(TargetSite {
                kind: TargetKind::Goto,
                operand: starts[1] + 1,
                end: ends[1],
            })
        );
        assert_eq!(
            Opcode::peek_target(&buf, starts[2]),
            Some(TargetSite {
                kind: TargetKind::BeginChoice(flags),
                operand: starts[2] + 2,
                end: ends[2],
            }),
            "the choice flags byte precedes the operand"
        );
        assert_eq!(
            Opcode::peek_target(&buf, starts[3]).map(|s| s.kind),
            Some(TargetKind::Call)
        );
        assert_eq!(
            Opcode::peek_target(&buf, starts[4]),
            None,
            "Nop is not a target op"
        );

        // Globals classify under `peek_static` only; a variable pointer is
        // not static at all.
        let mut gbuf = Vec::new();
        Opcode::GetGlobal(test_id()).encode(&mut gbuf);
        let set_at = gbuf.len();
        Opcode::SetGlobal(test_id()).encode(&mut gbuf);
        let take_at = gbuf.len();
        Opcode::TakeGlobal(test_id()).encode(&mut gbuf);
        let ptr_at = gbuf.len();
        Opcode::PushVarPointer(test_id()).encode(&mut gbuf);
        assert_eq!(
            Opcode::peek_static(&gbuf, 0).map(|s| (s.kind, s.operand, s.end)),
            Some((StaticKind::Global(GlobalKind::Get), 1, set_at))
        );
        assert_eq!(
            Opcode::peek_static(&gbuf, set_at).map(|s| s.kind),
            Some(StaticKind::Global(GlobalKind::Set))
        );
        assert_eq!(
            Opcode::peek_static(&gbuf, take_at).map(|s| s.kind),
            Some(StaticKind::Global(GlobalKind::Take))
        );
        assert_eq!(
            Opcode::peek_target(&gbuf, 0),
            None,
            "a global is not a target"
        );
        assert_eq!(
            Opcode::peek_static(&gbuf, ptr_at),
            None,
            "PushVarPointer stays symbolic"
        );
        assert_eq!(Opcode::peek_target(&buf, buf.len()), None, "past the end");
        assert_eq!(
            Opcode::peek_target(&buf[..ends[1] - 1], starts[1]),
            None,
            "a buffer that ends inside the operand is not a site"
        );

        // The operand bytes are the encoded id, byte for byte.
        let site = Opcode::peek_target(&buf, starts[1]).expect("site");
        let raw = u64::from_le_bytes(buf[site.operand..site.end].try_into().expect("8 bytes"));
        assert_eq!(DefinitionId::from_raw(raw), Some(test_id()));
    }

    fn global_id() -> DefinitionId {
        DefinitionId::new(DefinitionTag::GlobalVar, 42)
    }

    fn ext_id() -> DefinitionId {
        DefinitionId::new(DefinitionTag::ExternalFn, 0xCAFE)
    }

    #[test]
    fn roundtrip_stack_literals() {
        roundtrip(&Opcode::PushInt(0));
        roundtrip(&Opcode::PushInt(-1));
        roundtrip(&Opcode::PushInt(i32::MAX));
        roundtrip(&Opcode::PushInt(i32::MIN));
        roundtrip(&Opcode::PushFloat(0.0));
        roundtrip(&Opcode::PushFloat(3.125));
        roundtrip(&Opcode::PushFloat(f32::NEG_INFINITY));
        roundtrip(&Opcode::PushBool(true));
        roundtrip(&Opcode::PushBool(false));
        roundtrip(&Opcode::PushString(0));
        roundtrip(&Opcode::PushString(u16::MAX));
        roundtrip(&Opcode::PushList(7));
        roundtrip(&Opcode::PushDivertTarget(test_id()));
        roundtrip(&Opcode::PushNull);
        roundtrip(&Opcode::Pop);
        roundtrip(&Opcode::Duplicate);
    }

    #[test]
    fn roundtrip_arithmetic() {
        for op in [
            Opcode::Add,
            Opcode::Subtract,
            Opcode::Multiply,
            Opcode::Divide,
            Opcode::Modulo,
            Opcode::Negate,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_comparison() {
        for op in [
            Opcode::Equal,
            Opcode::NotEqual,
            Opcode::Greater,
            Opcode::GreaterOrEqual,
            Opcode::Less,
            Opcode::LessOrEqual,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_logic() {
        for op in [Opcode::Not, Opcode::And, Opcode::Or] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_globals() {
        roundtrip(&Opcode::GetGlobal(global_id()));
        roundtrip(&Opcode::SetGlobal(global_id()));
    }

    #[test]
    fn roundtrip_temps() {
        roundtrip(&Opcode::DeclareTemp(0));
        roundtrip(&Opcode::GetTemp(5));
        roundtrip(&Opcode::SetTemp(u16::MAX));
        roundtrip(&Opcode::GetTempRaw(3));
    }

    #[test]
    fn roundtrip_var_pointer() {
        roundtrip(&Opcode::PushVarPointer(global_id()));
        roundtrip(&Opcode::PushTempPointer(0));
        roundtrip(&Opcode::PushTempPointer(u16::MAX));
    }

    #[test]
    fn roundtrip_control_flow() {
        roundtrip(&Opcode::Jump(0));
        roundtrip(&Opcode::Jump(-42));
        roundtrip(&Opcode::JumpIfFalse(100));
        roundtrip(&Opcode::Goto(test_id()));
        roundtrip(&Opcode::GotoIf(test_id()));
        roundtrip(&Opcode::GotoVariable);
    }

    #[test]
    fn roundtrip_container_flow() {
        roundtrip(&Opcode::EnterContainer(test_id()));
        roundtrip(&Opcode::ExitContainer);
    }

    #[test]
    fn roundtrip_functions_tunnels() {
        roundtrip(&Opcode::Call(test_id()));
        roundtrip(&Opcode::Return);
        roundtrip(&Opcode::TunnelCall(test_id()));
        roundtrip(&Opcode::TunnelReturn);
        roundtrip(&Opcode::TunnelCallVariable);
        roundtrip(&Opcode::CallVariable(0));
        roundtrip(&Opcode::CallVariable(3));
    }

    #[test]
    fn roundtrip_threads() {
        roundtrip(&Opcode::ThreadCall(test_id()));
        roundtrip(&Opcode::ThreadStart);
        roundtrip(&Opcode::ThreadDone);
    }

    #[test]
    fn roundtrip_output() {
        roundtrip(&Opcode::EmitLine(0, 0));
        roundtrip(&Opcode::EmitLine(999, 3));
        roundtrip(&Opcode::EmitValue);
        roundtrip(&Opcode::EmitNewline);
        roundtrip(&Opcode::EmitLineNl(0x1234, 3));
        for kind in BinaryKind::ALL {
            roundtrip(&Opcode::BinaryImm(kind, -7));
            roundtrip(&Opcode::BinaryJumpIfFalse(kind, 300));
            roundtrip(&Opcode::BinaryImmJumpIfFalse(kind, i32::MIN, -12));
            assert_eq!(BinaryKind::from_mnemonic(kind.mnemonic()), Some(kind));
        }
        assert_eq!(
            Opcode::decode(&[BINARY_IMM, 11, 0, 0, 0, 0], &mut 0),
            Err(DecodeError::InvalidBinaryKind(11))
        );
        roundtrip(&Opcode::Spring);
        roundtrip(&Opcode::Glue);
        roundtrip(&Opcode::BeginTag);
        roundtrip(&Opcode::EndTag);
        roundtrip(&Opcode::EvalLine(0, 0));
        roundtrip(&Opcode::EvalLine(42, 2));
        roundtrip(&Opcode::AttachElement);
        roundtrip(&Opcode::EndElementRun);
    }

    #[test]
    fn roundtrip_choices() {
        roundtrip(&Opcode::BeginChoice(
            ChoiceFlags {
                has_condition: true,
                has_start_content: false,
                has_choice_only_content: true,
                once_only: false,
                is_invisible_default: true,
            },
            test_id(),
        ));
        roundtrip(&Opcode::BeginChoice(
            ChoiceFlags {
                has_condition: false,
                has_start_content: true,
                has_choice_only_content: false,
                once_only: true,
                is_invisible_default: false,
            },
            test_id(),
        ));
        roundtrip(&Opcode::EndChoice);
    }

    #[test]
    fn roundtrip_sequences() {
        for kind in [
            SequenceKind::Cycle,
            SequenceKind::Stopping,
            SequenceKind::OnceOnly,
            SequenceKind::Shuffle,
        ] {
            roundtrip(&Opcode::Sequence(kind, 5));
        }
        roundtrip(&Opcode::SequenceBranch(-10));
        roundtrip(&Opcode::SequenceBranch(0));
    }

    #[test]
    fn roundtrip_intrinsics() {
        for op in [
            Opcode::VisitCount,
            Opcode::CurrentVisitCount,
            Opcode::TouchVisit,
            Opcode::ShuffleIndexOf,
            Opcode::TurnsSince,
            Opcode::TurnIndex,
            Opcode::ChoiceCount,
            Opcode::Random,
            Opcode::SeedRandom,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_casts_math() {
        for op in [
            Opcode::CastToInt,
            Opcode::CastToFloat,
            Opcode::Floor,
            Opcode::Ceiling,
            Opcode::Pow,
            Opcode::Min,
            Opcode::Max,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_call_external() {
        roundtrip(&Opcode::CallExternal(ext_id(), 3));
        roundtrip(&Opcode::CallExternal(ext_id(), 0));
    }

    #[test]
    fn roundtrip_list_ops() {
        for op in [
            Opcode::ListContains,
            Opcode::ListNotContains,
            Opcode::ListIntersect,
            Opcode::ListAll,
            Opcode::ListInvert,
            Opcode::ListCount,
            Opcode::ListMin,
            Opcode::ListMax,
            Opcode::ListValue,
            Opcode::ListRange,
            Opcode::ListFromInt,
            Opcode::ListRandom,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_collections() {
        for op in [
            Opcode::ArrayNew(0),
            Opcode::ArrayNew(1),
            Opcode::ArrayNew(u32::MAX),
            Opcode::MapNew(0),
            Opcode::MapNew(3),
            Opcode::IndexGet,
            Opcode::IndexSet,
            Opcode::CollectionLen,
            Opcode::MapGet,
            Opcode::MapInsert,
            Opcode::MapRemove,
            Opcode::MapContains,
            Opcode::CollectionKeys,
            Opcode::CollectionValues,
            Opcode::PushLiteral(0),
            Opcode::PushLiteral(u32::MAX),
        ] {
            roundtrip(&op);
        }
    }

    /// The collection opcode block is contiguous (`docs/format-v4-rfc.md`
    /// §3): `0xBE`-`0xC9` inclusive, no gaps, no overlap with the adjacent
    /// list-ops block (`0xB0`-`0xBD`) or the lifecycle block (`0xF0`+).
    #[test]
    fn collection_opcode_block_is_contiguous_and_matches_rfc_layout() {
        let expected: [(u8, Opcode); 12] = [
            (0xBE, Opcode::ArrayNew(0)),
            (0xBF, Opcode::MapNew(0)),
            (0xC0, Opcode::IndexGet),
            (0xC1, Opcode::IndexSet),
            (0xC2, Opcode::CollectionLen),
            (0xC3, Opcode::MapGet),
            (0xC4, Opcode::MapInsert),
            (0xC5, Opcode::MapRemove),
            (0xC6, Opcode::MapContains),
            (0xC7, Opcode::CollectionKeys),
            (0xC8, Opcode::CollectionValues),
            (0xC9, Opcode::PushLiteral(0)),
        ];
        for (byte, op) in expected {
            let mut buf = Vec::new();
            op.encode(&mut buf);
            assert_eq!(buf[0], byte, "{op:?} encoded to unexpected discriminant");
        }
    }

    #[test]
    fn roundtrip_ns_a1_option_and_stdlib_flips() {
        for op in [
            Opcode::PushNone,
            Opcode::MakeSome,
            Opcode::StrFind,
            Opcode::SeqIndexOf,
            Opcode::SeqMin,
            Opcode::SeqMax,
            Opcode::SeqFirst,
            Opcode::SeqLast,
            Opcode::SeqPop,
            Opcode::MapGetOpt,
            Opcode::MapContainsValue,
            Opcode::MapClear,
        ] {
            roundtrip(&op);
        }
    }

    /// The NS-A1 block layout: `PushNone`/`MakeSome` fill the two bytes
    /// before the string-eval block (0xE0/0xE1), the verb flips continue
    /// contiguously after it (0xE2-0xEB).
    #[test]
    fn ns_a1_opcode_block_layout() {
        let expected: [(u8, Opcode); 12] = [
            (0xDE, Opcode::PushNone),
            (0xDF, Opcode::MakeSome),
            (0xE2, Opcode::StrFind),
            (0xE3, Opcode::SeqIndexOf),
            (0xE4, Opcode::SeqMin),
            (0xE5, Opcode::SeqMax),
            (0xE6, Opcode::SeqFirst),
            (0xE7, Opcode::SeqLast),
            (0xE8, Opcode::SeqPop),
            (0xE9, Opcode::MapGetOpt),
            (0xEA, Opcode::MapContainsValue),
            (0xEB, Opcode::MapClear),
        ];
        for (byte, op) in expected {
            let mut buf = Vec::new();
            op.encode(&mut buf);
            assert_eq!(buf[0], byte, "{op:?} encoded to unexpected discriminant");
        }
    }

    /// B1 `or`-coalescing (issue #1460): one opcode, one byte, no operand.
    #[test]
    fn roundtrip_b1_coalesce_some() {
        roundtrip(&Opcode::CoalesceSome(0));
        roundtrip(&Opcode::CoalesceSome(-42));
        roundtrip(&Opcode::CoalesceSome(100));
    }

    /// `CoalesceSome` claims the first of the four bytes free after NS-A7's
    /// `Collect` (`0xFA`) — `docs/format-v4-rfc.md` §5 explicitly does NOT
    /// freeze numeric opcode assignments (only the name/encoding
    /// *inventory*); the reservation comment above `COALESCE_SOME` is the
    /// actual (implementation-level) source of truth this test pins. Same
    /// byte the retired binary `Coalesce` opcode held (#1460) — reused, not
    /// freed, since #1471 replaced that opcode's job rather than adding a
    /// new one alongside it.
    #[test]
    fn coalesce_some_opcode_byte_is_0xfb() {
        let mut buf = Vec::new();
        Opcode::CoalesceSome(0).encode(&mut buf);
        assert_eq!(
            buf[0], 0xFB,
            "CoalesceSome encoded to unexpected discriminant"
        );
    }

    /// B1b the `as` binding (issue #1475): one opcode with a `u16` slot
    /// immediate, encoded like `SetTemp`/`GetTemp`.
    #[test]
    fn roundtrip_b1b_option_bind() {
        roundtrip(&Opcode::OptionBind(0));
        roundtrip(&Opcode::OptionBind(7));
        roundtrip(&Opcode::OptionBind(u16::MAX));
    }

    /// `OptionBind` claims the first byte free after B1's `Coalesce`
    /// (`0xFB`) — same "the reservation comment is the source of truth"
    /// posture as `coalesce_opcode_byte_is_0xfb` above.
    #[test]
    fn option_bind_opcode_byte_is_0xfc() {
        let mut buf = Vec::new();
        Opcode::OptionBind(1).encode(&mut buf);
        assert_eq!(
            buf[0], 0xFC,
            "OptionBind encoded to unexpected discriminant"
        );
    }

    /// Seq `remove_at` (issue #1484): one opcode, one byte, no operand —
    /// same shape as `Coalesce` above.
    #[test]
    fn roundtrip_seq_remove_at() {
        roundtrip(&Opcode::SeqRemoveAt);
    }

    /// `SeqRemoveAt` claims the byte free after `OptionBind`
    /// (`0xFC`-`0xFD` + `0xFF` remained after `0xFB`; `OptionBind` claimed
    /// `0xFC`) — the reservation comment above `SEQ_REMOVE_AT` is the actual
    /// (implementation-level) source of truth this test pins.
    #[test]
    fn seq_remove_at_opcode_byte_is_0xfd() {
        let mut buf = Vec::new();
        Opcode::SeqRemoveAt.encode(&mut buf);
        assert_eq!(
            buf[0], 0xFD,
            "SeqRemoveAt encoded to unexpected discriminant"
        );
    }

    #[test]
    fn roundtrip_ns_a6_rand_verbs() {
        for op in [
            Opcode::RandFloat,
            Opcode::RandChance,
            Opcode::RandPick,
            Opcode::RandShuffle,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_ns_a5_range_ops() {
        for op in [
            Opcode::RangeMakeExcl,
            Opcode::RangeMakeIncl,
            Opcode::RangeNonEmpty,
        ] {
            roundtrip(&op);
        }
    }

    /// The NS-A5 block layout: the three range ops take the first free
    /// bytes after the lifecycle block (0xF0-0xF3). `rand::int` has NO
    /// byte — it rides the existing `ConvertInt` (0xE2), a value-directed
    /// dispatch in the VM.
    #[test]
    fn ns_a5_opcode_block_layout() {
        let expected: [(u8, Opcode); 3] = [
            (0xF4, Opcode::RangeMakeExcl),
            (0xF5, Opcode::RangeMakeIncl),
            (0xF6, Opcode::RangeNonEmpty),
        ];
        for (byte, op) in expected {
            let mut buf = Vec::new();
            op.encode(&mut buf);
            assert_eq!(buf[0], byte, "{op:?} encoded to unexpected discriminant");
        }
    }

    /// The NS-A6 block layout: the four rand draw ops fill 0xEC-0xEF,
    /// contiguously after NS-A1's 0xEB, up against the lifecycle block
    /// (0xF0+). `seed(n)` deliberately has no byte here — it reuses the
    /// frozen `SeedRandom` (0x85): one RNG cell, two surfaces, no drift.
    #[test]
    fn ns_a6_opcode_block_layout() {
        let expected: [(u8, Opcode); 4] = [
            (0xEC, Opcode::RandFloat),
            (0xED, Opcode::RandChance),
            (0xEE, Opcode::RandPick),
            (0xEF, Opcode::RandShuffle),
        ];
        for (byte, op) in expected {
            let mut buf = Vec::new();
            op.encode(&mut buf);
            assert_eq!(buf[0], byte, "{op:?} encoded to unexpected discriminant");
        }
    }

    /// The NS-A4 block layout: the two ordering ops take the next free
    /// bytes after NS-A8's `Tower` discriminant (0xF7). Two ops serve
    /// four source verbs — `sort`/`sorted` share `SeqSorted`,
    /// `sort_by`/`sorted_by` share `SeqSortedBy` (in-place-ness comes
    /// from the RMW write-back, the `shuffle`/`shuffled` precedent).
    #[test]
    fn ns_a4_opcode_block_layout() {
        let expected: [(u8, Opcode); 2] = [(0xF8, Opcode::SeqSorted), (0xF9, Opcode::SeqSortedBy)];
        for (byte, op) in expected {
            let mut buf = Vec::new();
            op.encode(&mut buf);
            assert_eq!(buf[0], byte, "{op:?} encoded to unexpected discriminant");
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_ns_a8_tower_ops() {
        for kind in TowerOp::ALL {
            roundtrip(&Opcode::Tower(kind));
        }
    }

    /// The NS-A8 layout: ONE discriminant byte (0xF7 — the first free byte
    /// after the NS-A5 range ops at 0xF4-0xF6) with the `TowerOp` kind as a
    /// u8 immediate in kind-byte order 0..=12. Deliberate opcode-space
    /// economy — see the `TOWER` const's comment.
    #[test]
    fn ns_a8_tower_opcode_layout() {
        for (i, kind) in TowerOp::ALL.into_iter().enumerate() {
            let mut buf = Vec::new();
            Opcode::Tower(kind).encode(&mut buf);
            #[expect(clippy::cast_possible_truncation, reason = "13 kinds")]
            let expected_kind = i as u8;
            assert_eq!(buf, [0xF7, expected_kind], "{kind:?} layout drifted");
        }
    }

    /// An unknown tower kind byte is a decode error, not a silent skip —
    /// the same reserved-tag discipline as every other closed sub-enum.
    #[test]
    fn decode_unknown_tower_kind_rejected() {
        let buf = [0xF7, 13];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::InvalidTowerOp(13));
    }

    #[test]
    fn roundtrip_ns_a7_collect_ops() {
        for kind in CollectOp::ALL {
            roundtrip(&Opcode::Collect(kind));
        }
    }

    /// The NS-A7 layout: ONE discriminant byte (0xFA — the first free byte
    /// after the NS-A4 ordering verbs at 0xF8/0xF9) with the `CollectOp`
    /// kind as a u8 immediate in kind-byte order 0..=4. The NS-A8 Tower
    /// opcode-space economy applied again — see the `COLLECT` const's
    /// comment.
    #[test]
    fn ns_a7_collect_opcode_layout() {
        for (i, kind) in CollectOp::ALL.into_iter().enumerate() {
            let mut buf = Vec::new();
            Opcode::Collect(kind).encode(&mut buf);
            #[expect(clippy::cast_possible_truncation, reason = "5 kinds")]
            let expected_kind = i as u8;
            assert_eq!(buf, [0xFA, expected_kind], "{kind:?} layout drifted");
        }
    }

    /// An unknown collections+ kind byte is a decode error, not a silent
    /// skip — same reserved-tag discipline as `TowerOp`.
    #[test]
    fn decode_unknown_collect_kind_rejected() {
        let buf = [0xFA, 5];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::InvalidCollectOp(5));
    }

    #[test]
    fn roundtrip_seq_verb_ops() {
        for kind in SeqVerbOp::ALL {
            roundtrip(&Opcode::SeqVerb(kind));
        }
    }

    /// The fn-value verb layout (issue #1679): ONE discriminant byte
    /// (`0xA1` — the first byte of the `0xA1`-`0xAF` run after
    /// `CallExternal`, chosen over the high tail's last free byte `0xFF`,
    /// which stays unclaimed as a future extended-opcode prefix) with the
    /// `SeqVerbOp` kind as a u8 immediate in kind-byte order 0..=5. See the
    /// `SEQ_VERB` const's comment.
    #[test]
    fn seq_verb_opcode_layout() {
        for (i, kind) in SeqVerbOp::ALL.into_iter().enumerate() {
            let mut buf = Vec::new();
            Opcode::SeqVerb(kind).encode(&mut buf);
            #[expect(clippy::cast_possible_truncation, reason = "6 kinds")]
            let expected_kind = i as u8;
            assert_eq!(buf, [0xA1, expected_kind], "{kind:?} layout drifted");
        }
    }

    /// An unknown fn-value verb kind byte is a decode error, not a silent
    /// skip — same reserved-tag discipline as `TowerOp`/`CollectOp`.
    #[test]
    fn decode_unknown_seq_verb_kind_rejected() {
        let buf = [0xA1, 6];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::InvalidSeqVerbOp(6));
    }

    /// Mnemonics round-trip through the `.inkt` reader's inverse, and are
    /// exactly the source spellings the verbs ship under.
    #[test]
    fn seq_verb_mnemonics_round_trip() {
        for kind in SeqVerbOp::ALL {
            assert_eq!(SeqVerbOp::from_mnemonic(kind.mnemonic()), Some(kind));
        }
        assert_eq!(SeqVerbOp::Map.mnemonic(), "map");
        assert_eq!(SeqVerbOp::Filter.mnemonic(), "filter");
        assert_eq!(SeqVerbOp::Fold.mnemonic(), "fold");
        assert_eq!(SeqVerbOp::FilterMap.mnemonic(), "filter_map");
        assert_eq!(SeqVerbOp::Each.mnemonic(), "each");
        assert_eq!(SeqVerbOp::MapEach.mnemonic(), "map_each");
        assert_eq!(
            SeqVerbOp::from_mnemonic("map_each"),
            Some(SeqVerbOp::MapEach)
        );
        assert_eq!(SeqVerbOp::from_mnemonic("not_a_verb"), None);
    }

    /// [`SeqVerbOp::is_effectful`] is the VM dispatch/`guard_comparator_write`
    /// posture switch (issue #1679 slice 2): the pure quartet is `false`,
    /// the effectful pair is `true`.
    #[test]
    fn seq_verb_effectful_split() {
        for kind in [
            SeqVerbOp::Map,
            SeqVerbOp::Filter,
            SeqVerbOp::Fold,
            SeqVerbOp::FilterMap,
        ] {
            assert!(!kind.is_effectful(), "{kind:?} must be pure");
        }
        for kind in [SeqVerbOp::Each, SeqVerbOp::MapEach] {
            assert!(kind.is_effectful(), "{kind:?} must be effectful");
        }
    }

    #[test]
    fn roundtrip_lifecycle() {
        for op in [Opcode::Done, Opcode::Yield, Opcode::End, Opcode::Nop] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_string_eval() {
        roundtrip(&Opcode::BeginStringEval);
        roundtrip(&Opcode::EndStringEval);
    }

    /// `0xFE` held the retired `SourceLocation` opcode (issue #3180, Q-R1
    /// 2026-07-19). Nothing claims the byte yet (see the retirement comment
    /// at the end of the const block, below `END_STRING_EVAL`), so it must
    /// decode as unknown — pinning that the byte is truly gone from the
    /// decoder, not just unreachable from encode.
    #[test]
    fn decode_retired_source_location_byte_is_unknown_opcode() {
        let buf = [0xFE];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::UnknownOpcode(0xFE));
    }

    #[test]
    fn decode_unknown_opcode() {
        let buf = [0xFF];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::UnknownOpcode(0xFF));
    }

    /// The v4 collection opcode block (`0xBE`-`0xC9`, `docs/format-v4-rfc.md`
    /// §3 "Collections (T1a)") went live in T1b-2 (#570) — every byte in the
    /// block now decodes to a real `Opcode` variant (superseding the T1a-era
    /// "still rejected" test this replaces). `ArrayNew`/`MapNew`/
    /// `PushLiteral` carry a `u32` operand so a bare 1-byte buffer isn't
    /// enough for those three; this asserts every discriminant byte decodes
    /// to *some* `Opcode` (not `UnknownOpcode`), operand length aside.
    #[test]
    fn collection_opcode_block_no_longer_rejected() {
        for disc in 0xBEu8..=0xC9u8 {
            // Pad with zero bytes so the 4-byte `u32` operand opcodes
            // (`ArrayNew`/`MapNew`/`PushLiteral`) have enough to decode too.
            let buf = [disc, 0, 0, 0, 0];
            let mut offset = 0;
            let result = Opcode::decode(&buf, &mut offset);
            assert!(
                !matches!(result, Err(DecodeError::UnknownOpcode(_))),
                "0x{disc:02x} should decode to a real Opcode, got {result:?}"
            );
        }
    }

    /// `StoreVarIfNew`/`EqVars` (`0xCB`-`0xCC`, `docs/format-v4-rfc.md` §3
    /// "Sharing discipline (T1a)") stay numbered but deliberately not wired
    /// into `Opcode` — the strict reader must keep rejecting both bytes
    /// until their own milestone lands (spec §6's optional ref-collapsing
    /// sites, not part of T1b-4/#576).
    #[test]
    fn decode_reserved_sharing_discipline_opcodes_still_rejected() {
        for disc in 0xCBu8..=0xCCu8 {
            let buf = [disc];
            let mut offset = 0;
            let err = Opcode::decode(&buf, &mut offset).unwrap_err();
            assert_eq!(err, DecodeError::UnknownOpcode(disc));
        }
    }

    /// `TakeGlobal`/`TakeTemp` (`0xCA`, `0xCD` — T1b-4/#576) round-trip
    /// through encode/decode.
    #[test]
    fn roundtrip_take_opcodes() {
        roundtrip(&Opcode::TakeGlobal(global_id()));
        roundtrip(&Opcode::TakeTemp(0));
        roundtrip(&Opcode::TakeTemp(u16::MAX));
    }

    /// `TakeGlobal`/`TakeTemp` land at the exact bytes the RFC comment in
    /// `opcode.rs` documents — `0xCA` (splitting the RFC's generic
    /// `TakeVar(slot)`) and `0xCD` (freshly claimed, adjacent to the
    /// reserved block, leaving `0xCB`/`0xCC` untouched for
    /// `StoreVarIfNew`/`EqVars`).
    #[test]
    fn take_opcodes_land_at_documented_bytes() {
        let mut buf = Vec::new();
        Opcode::TakeGlobal(global_id()).encode(&mut buf);
        assert_eq!(buf[0], 0xCA);

        let mut buf = Vec::new();
        Opcode::TakeTemp(0).encode(&mut buf);
        assert_eq!(buf[0], 0xCD);
    }

    /// All five record opcodes (`0xCE`-`0xD2` — TM-4/TM-4c) round-trip
    /// through encode/decode at their documented bytes.
    #[test]
    fn roundtrip_record_opcodes() {
        roundtrip(&Opcode::RecordNew(0));
        roundtrip(&Opcode::RecordNew(u32::MAX));
        roundtrip(&Opcode::RecordGetDyn(0));
        roundtrip(&Opcode::RecordGetDyn(u16::MAX));
        roundtrip(&Opcode::RecordSetDyn(0));
        roundtrip(&Opcode::RecordSetDyn(u16::MAX));
        roundtrip(&Opcode::RecordGet(0));
        roundtrip(&Opcode::RecordGet(u16::MAX));
        roundtrip(&Opcode::RecordSet(0));
        roundtrip(&Opcode::RecordSet(u16::MAX));

        let mut buf = Vec::new();
        Opcode::RecordNew(1).encode(&mut buf);
        assert_eq!(buf[0], 0xCE);

        let mut buf = Vec::new();
        Opcode::RecordGetDyn(1).encode(&mut buf);
        assert_eq!(buf[0], 0xCF);

        let mut buf = Vec::new();
        Opcode::RecordSetDyn(1).encode(&mut buf);
        assert_eq!(buf[0], 0xD0);

        let mut buf = Vec::new();
        Opcode::RecordGet(1).encode(&mut buf);
        assert_eq!(buf[0], 0xD1);

        let mut buf = Vec::new();
        Opcode::RecordSet(1).encode(&mut buf);
        assert_eq!(buf[0], 0xD2);
    }

    /// The three TM-3-completion conversion-intrinsic opcodes (`0xD3`-`0xD5`
    /// — issue #659) round-trip and sit contiguous and adjacent to the
    /// record block, matching the reservation comment above `CONVERT_INT`.
    #[test]
    fn roundtrip_conversion_opcodes() {
        for op in [
            Opcode::ConvertInt,
            Opcode::ConvertFloat,
            Opcode::ConvertString,
        ] {
            roundtrip(&op);
        }

        let mut buf = Vec::new();
        Opcode::ConvertInt.encode(&mut buf);
        assert_eq!(buf[0], 0xD3);

        let mut buf = Vec::new();
        Opcode::ConvertFloat.encode(&mut buf);
        assert_eq!(buf[0], 0xD4);

        let mut buf = Vec::new();
        Opcode::ConvertString.encode(&mut buf);
        assert_eq!(buf[0], 0xD5);
    }

    /// The `char_at(s, i)` stdlib-slice-1-completion opcode (`0xDD` — issue
    /// #857) round-trips and sits contiguous and adjacent to the projection
    /// block, matching the reservation comment above `CHAR_AT`.
    #[test]
    fn roundtrip_char_at_opcode() {
        roundtrip(&Opcode::CharAt);

        let mut buf = Vec::new();
        Opcode::CharAt.encode(&mut buf);
        assert_eq!(buf[0], 0xDD);
    }

    #[test]
    fn decode_unexpected_eof() {
        // PushInt needs 4 more bytes after the discriminant.
        let buf = [PUSH_INT, 0x00];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::UnexpectedEof);
    }

    #[test]
    fn decode_multiple_instructions() {
        let ops = vec![
            Opcode::PushInt(42),
            Opcode::PushBool(true),
            Opcode::Add,
            Opcode::Done,
        ];
        let mut buf = Vec::new();
        for op in &ops {
            op.encode(&mut buf);
        }
        let mut offset = 0;
        for expected in &ops {
            let decoded = Opcode::decode(&buf, &mut offset).unwrap();
            assert_eq!(*expected, decoded);
        }
        assert_eq!(offset, buf.len());
    }

    #[test]
    fn choice_flags_roundtrip() {
        for bits in 0..32u8 {
            let flags = ChoiceFlags::from_byte(bits);
            assert_eq!(flags.to_byte(), bits);
        }
    }
}
