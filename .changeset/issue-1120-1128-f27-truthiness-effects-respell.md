---
"@brink-lang/web": patch
---

F27 truthiness removal + `@[effects(…)]` paren respell + wake-gate gap
(#1120, #1128). `Option[T]` has no truthiness (F27, ruled 2026-07-19,
superseding NS-A1's falsy-none): a condition-position Option is now a
compile error under `types = strict` (new diagnostic E116) and a
turn-terminating runtime fault under gradual (`OptionTruthiness`) — write
`== none` / `== some(x)` instead. The `@[effects(…)]` clause grammar is
respelled to the Rust-meta-item paren shape — `@[effects(reads(gold, hp),
writes(mood), silent)]`; bare top-level idents are always flags, so a flag
can never be swallowed into an open clause; the colon spelling inside an
annotation is now E101. The deprecated `#@effects(…)` tag alias keeps its
legacy colon grammar frozen (E110 unchanged). The `await`-condition purity
gate (E105) now also rejects draw/fault-bearing stdlib intrinsics called
directly in the condition (`await chance(0.5)`, `await pop(a)`), consulting
the same intrinsic effect table effect inference harvests from. Vanilla-ink
stories are byte-identical; the oracle corpus is unaffected.
