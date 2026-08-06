---
"@brink-lang/web": patch
---

#1464 (B5 — the build of #1103, RULED 2026-07-23; `docs/stdlib-spec.md`
§9.6): the native surface gains the **one construction initializer**
`TypeName { … }`, and its meaning is **protocol dispatch**, not closed
compiler grammar.

`brink-syntax-native` produces one node shape — `CONSTRUCT_LITERAL` with
`CONSTRUCT_ENTRY` children covering the ruled element form (`Flags { Red,
Blue }`) and pair/field form (`Map { "a": 1 }`, `Point { x: 1 }`) — with a
Rust-style no-construct-literal restriction in `if`/`while`/`for` and
content-ground `{if …}`/`{match …}` heads so a head's brace still opens its
body (`(…)` lifts it again). Meaning comes from the new `construct`
registry, `brink_ir::hir::construct::ConstructTarget`: a closed enum (the
NS-A8 protocol-fence shape), **std-only this round** — `Map` →
`Expr::MapLiteral`, `Flags` → `Expr::ListLiteral`, `Weighted` → the
existing total `weighted(…)` intrinsic — with an unregistered name falling
through to the declared-struct reading. User-type opt-in (the `impl`
spelling), the validating `construct → Option` member's spelling, and the
spread form (`Map { ..other }`) stay deferred with the ruling; none is
stubbed.

Two new diagnostics: **E138**, a duplicate key in a map literal
(#1103's cascade ruling (A) — a compile error, not a silent last-wins
overwrite), and **E139**, entries in the wrong form for their target
type.

Web-observable on two paths. (1) A `.brink` entry compiled through
`compile_project`/`compile_fragment` reaches the native pipeline
(`brink_compiler::compile` dispatches on the entry's extension), so
construction literals now compile and play instead of failing to parse.
(2) **E138 also fires for the brink dialect's own `#{…}` spelling** — both
surfaces lower to the same `MapLiteral`, so any `dialect = brink` source
with `#{k: 1, k: 1}` in it now fails to compile where it previously
last-won silently.
