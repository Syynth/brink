---
"@brink-lang/web": patch
---

T1c-3 (#701): the `bind`/`call` function-value stdlib, the authoritative
display form, and structural equality land — all observable through
`@brink-lang/web`:

- **`bind(f, args…)` stdlib intrinsic**: val-only currying over an existing
  function value — consumes the head of the remaining param row and returns
  a new function value (lowercase, brink-dialect-gated, author-shadowable
  with the E035-class warning, effect-transparent). Lowers to the new
  `bind_value` opcode (`0xD9`), which disassembles alongside `call_value`.
  Over-binding more args than the target has remaining params, or binding a
  non-function value, is a turn-terminating fault (spec §3).
- **Display form**: `string(f)` and `{f}` interpolation now render the stable
  signature-like form — `fn heal(ref hp = player_hp, amount)` (bound `val`
  args print their value, bound `ref` args print the captured cell name,
  unbound params print bare). This is a permanently observable surface (spec
  §5), property-tested for stability.
- **Structural equality**: `==`/`!=` on two function values compare
  structurally (same fn token + equal bound rows); any ordering operator
  (`<`, `>=`, …) is a runtime fault in gradual mode / a type error in strict
  (spec §5). Function values remain rejected as map keys.

Crates-only work (bevy-brink also gains the host callback-invocation surface,
`call_ink_function_value`), but the runtime-observable behavior above flows
through `@brink-lang/web`, so it carries a patch per the wasm-observable rule.
