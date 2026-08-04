---
"@brink-lang/web": patch
---

Issue #2165 (`docs/decision-log.md` 2026-08-03 "`fn conventions()` is
DISSOLVED — handler precedence is a property of the `@[element]`
annotation"): deletes the `fn conventions()`/`register` machinery the
2026-08-03 ruling dissolved. `register` was never wired to any real
end-to-end behavior beyond confinement/effect bookkeeping — there are zero
real `register(...)` calls anywhere in the tree — but its presence was
itself observable:

- **`register` is no longer a recognized intrinsic name.** An unresolved
  call to `register(...)` now surfaces the ordinary `E025`
  (unresolved-name) diagnostic again, exactly as any other undeclared
  identifier does — it is no longer silently accepted pending a separate
  `E175` placement check.
- **`E175` no longer exists.** `DiagnosticCode::from_str_code("E175")` now
  returns `None`; a project's `[lints]` table naming `E175` no longer
  targets anything (the code is retired, not reassigned).
- **`conventions_registry` is no longer a recognized effects-assertion
  cell name.** `@[effects(writes(conventions_registry))]` now fails with
  `E102` (unknown name) instead of matching the (now-deleted)
  compiler-owned registry cell.

No project in the wild is expected to hit any of these: the intrinsic was
only ever legal inside a project's conventions module's `fn conventions()`,
a function no real `.brink` project has ever declared.
