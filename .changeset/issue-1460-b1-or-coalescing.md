---
"@brink-lang/web": patch
---

B1 (#1460): the `or`-coalescing surface spelling lands on the native
`.brink` dialect — `x or default`, per the ruled typing
(`docs/stdlib-spec.md` §1.6a, `docs/decision-log.md` "Option[T] ruled"
2026-07-18): `(Option[T], T) -> T` unwraps `some(v)` and falls back to
`default` on `none`; `(Option[T], Option[T]) -> Option[T]` preserves
optionality so a chain (`a or b or default`) associates left. New keyword
`or` in the native lexer/parser (`InfixOp::Coalesce`, distinct from
`InfixOp::Or` — ink's boolean `||`, oracle-frozen and untouched), one new
opcode `Coalesce` at `0xFB`. The web package's disassembly view
(`program_model.rs`) and `.inkt` text format (read + write) both gain the
`coalesce` mnemonic. Vanilla-ink and brink-dialect stories are
byte-identical; the oracle corpus is unaffected — the new opcode is
reachable only through native lowering.

The condition-position `as`-binding (`if EXPR as NAME`) named alongside
`or`-coalescing in issue #1460 is **not** included in this patch — its
precise grammar is unruled beyond a usage sketch in a DRAFT sequencing
document (`docs/stdlib-sequencing.md`, Finding F16, never promoted to a
decision-log ruling), so it is deferred per house rule 7 pending a design
round.

Review follow-up: a statically-detectable coalescing mismatch (a
non-Option left-hand side, or a fallback type that disagrees with the
Option's element type — `{5 or 9}`, `{some(1) or "text"}`) now raises
`E066` at the coalescing expression's own site under `types = strict`,
instead of silently collapsing to an unreported `Conflicted` type. The
mnemonic/opcode assignment and the typing/runtime semantics (including
eager evaluation of both operands — no short-circuiting) are unchanged
from the original patch; only diagnostic coverage improved.
