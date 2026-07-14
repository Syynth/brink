---
"@brink-lang/web": patch
---

T1c-2 completion gap fix (#721): the direct-call form `f(args…)` — where
`f` is a variable/temp holding a function value — dispatches through the
same `call_variable` opcode as the classic divert-target-variable call.
That opcode carried no argument count, so the popped-arg count for the
function-value arm was derived from the resolved target's arity instead
of the count actually supplied at the call site; a gradual-mode arity
mismatch on the direct form could leave a stray value on the stack
instead of faulting.

`call_variable` now carries an explicit `argc` operand (codegen emits the
exact count pushed at that call site; the divert-target-variable-call arm
ignores it, unchanged). Observable through `@brink-lang/web`:

- **Disassembly**: `call_variable` now renders as `call_variable
  argc=<n>` in program-model output.
- **Runtime dispatch**: a wrong-arity direct call `f(args…)` now faults
  with the same `FunctionValueArity` turn-terminating fault as the
  explicit `call(f, args…)` form, instead of risking a corrupted value
  stack.
