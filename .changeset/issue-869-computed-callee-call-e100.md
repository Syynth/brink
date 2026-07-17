---
"@brink-lang/web": patch
---

Fixed a silent-no-op compiler bug (#869): a direct call through a computed
fn-value callee — `handlers[state]()`, `obj.field()`, `get_handler()()` —
used to compile clean and silently drop the call entirely (the parser left
the trailing `(args…)` unconsumed, so it resurfaced as prose text on the
content line instead of being parsed as part of the call). Direct-call
syntax is scoped to a bare variable/temp/param callee (t1c-spec §3); any
other callee shape now parses as a real (if always-rejected) `CALL_EXPR`
node and produces a loud, unconditional compile error (`E100`) naming the
ratified `call(f, args…)` form as the fix, in every dialect and mode.

Compat: previously-compiling sources using one of these computed-callee
shapes as a direct-call target now fail to compile with `E100` instead of
silently dropping the call — the only prior alternative was a wrong,
silently-corrupted output, so this is a strict improvement, not a
regression. `call(f, args…)` (the explicit form) is untouched and already
dispatches through exactly these callee shapes correctly.
