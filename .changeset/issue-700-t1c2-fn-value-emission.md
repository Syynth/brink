---
"@brink-lang/web": patch
---

T1c-2 (#700): function values (`#fn(…)`) now lower, execute, and persist —
the first live use of the V4-reserved `PushFnRef`/`MakeClosure`/`CallValue`
opcodes and `VAL_FN_REF`/`VAL_CLOSURE` value tags. Observable through
`@brink-lang/web`:

- **Program model + disassembly**: a `#fn(…)` baked into a declaration
  default renders as a function-value (`fn <path>(…)`) rather than
  erroring or showing `null`, and the new opcodes disassemble
  (`push_fn_ref` / `make_closure` / `call_value`).
- **Speculation / eval-function results**: a function value crosses the
  typed-value JSON boundary as an opaque token (`{ "type": "fn", target,
  bound }`) — the host never dereferences the env (spec §6); the
  callback-invocation surface lands in T1c-3.
- **Runtime dispatch**: calling a function value (direct `f(args…)` or
  explicit `call(f, args…)`) works; a non-function callee, a wrong-arity
  explicit call, a rehydration mismatch (a saved closure whose target
  param was renamed/re-moded after a recompile), or invoking a closure
  that `ref`-binds a flow-private `#@local` cell are turn-terminating
  faults — never silent garbage.
- **Persistence**: function values save/load as ordinary values (save
  state, journal, speculation snapshots); `ref`-bound cells round-trip
  losslessly through the transcript codec.

The `#inkb` wire format gains per-container parameter name/mode metadata
(an additive trailing field) so a rehydrated closure can be validated
against the current signature.
