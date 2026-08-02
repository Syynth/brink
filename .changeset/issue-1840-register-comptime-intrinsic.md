---
"@brink-lang/web": patch
---

Issue #1840 Q5 (`docs/decision-log.md` 2026-08-02 "`register` is a comptime-only intrinsic"): `register` now resolves as a recognized T1b intrinsic name (no `E025`) but is legal only inside the project's configured conventions module's well-known `fn conventions()` — every other use is a new diagnostic, `E175`. Observable through `@brink-lang/web`: a `.brink` project calling `register(...)` outside `fn conventions()` now surfaces `E175` instead of `E025`, and a legal call inside `fn conventions()` compiles cleanly (with no runtime effect yet — its interim lowering evaluates and discards the argument; the comptime evaluator that will actually collect an ordered registry is a separate, tracked follow-up).
