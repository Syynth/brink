---
"@brink-lang/web": patch
---

Runtime: `brink-runtime`'s uppercase `INT()`/`FLOAT()` builtins no longer
silently fold an unconvertible value to `0`/`0.0` (issue #955, the
cast-operator leg of the wildcard-fan-out class #950 explicitly scoped
out).

`value_ops::cast_to_int`/`cast_to_float` (backing `Opcode::CastToInt`/
`CastToFloat`) used to end in a `_ => Value::Int(0)` / `_ =>
Value::Float(0.0)` wildcard arm — so a future `Value` variant would
silently cast to zero instead of getting a considered answer, the same
hazard class #950 fixed for the marshal/serialize legs. The reachable
domain (`Int`/`Float`/`Bool`/`String`, including the legacy
silent-0-on-string-parse-failure fallback) is **unchanged** — verified
byte-identical against the oracle (5,577 episodes, unmoved). Every other
`Value` variant (`List`, `DivertTarget`, `VariablePointer`, `TempPointer`,
`Null`, `FragmentRef`, `Array`, `Map`, `Record`, `FnRef`, `Closure`,
`Handle`, `Projection`) now raises `RuntimeError::InvalidConversionDomain`
instead — none of `value-model-spec.md`, `t1c-spec.md`, `t1d-spec.md`, or
`t1e-spec.md` rules a conversion for these, so faulting is the conservative
default (the same value-model-spec §11c "no silent garbage" precedent the
T1b lowercase `int()`/`float()` intrinsics already follow), reusing the
same fault variant with an uppercase `target` label (`"INT"`/`"FLOAT"`) to
distinguish it from the lowercase intrinsics' own faults.

Observable through `@brink-lang/web`: any JS host driving a story through
`continue_single`/`continue_flow`/`advance` where the ink script calls
`INT()`/`FLOAT()` on one of the previously-wildcarded variants now sees the
call reject with a runtime-error `JsError` instead of silently continuing
with a zero. None of these variants are reachable from vanilla ink source
today (they're brink-only value kinds), so this cannot fire from a
plain-ink story — only from brink-specific constructs (records, function
values, handles, path projections) an author explicitly casts.
