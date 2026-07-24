---
"@brink-lang/web": patch
---

#1309 (B0.8 body-dialect seam, charter §4, `docs/decision-log.md`
2026-07-23 "Native interleaving & body-dialect spelling"): a `flow`/`fn`
declaration's body now honors the body-dialect selector on its opening
brace. Plain `{ }` is the per-keyword default — **`fn` bodies now default
to code-ground `STMT_BLOCK`, not prose-ground `BLOCK`** (the B0.8 Wave A
seam this issue was tracking); `flow` bodies keep defaulting to
prose-ground `BLOCK`. `~{ }` forces a code-ground body (charter §3's
"Compound guard" — a code-bodied `flow`, now honestly spellable); `>{ }`
forces a prose-ground body (a prose-bodied `fn`).

A code-ground body lowers its statements through the existing B0.8
`control_flow::lower_stmt_block` (`let`/assignment/expression statements,
`if`/`while`/`for`/`until`, `return`/`break`/`continue`) and wraps the
result as the container's sole `Stmt::LogicBlock` — the same shape a
brink-dialect container whose entire body is one `~ { … }` block already
produces. No new HIR nodes.

Reachable through any `@brink-lang/web` session that analyzes a
`.brink`-extensioned file (`brink-db`'s `lowered_query`): existing `.brink`
sources with a `fn` body written in prose (content lines, choices,
diverts) now need the `>{ }` override to keep parsing as prose — plain
`{ }` on a `fn` parses as code-ground statements instead. `flow` bodies are
unaffected unless authored with the new `~{ }` override.

Line-escapes ("grains" — `~ stmt` inside a prose body, `> text` inside a
code body) are NOT part of this slice — tracked as a follow-up, not yet
parsed.
