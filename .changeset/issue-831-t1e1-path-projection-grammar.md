---
"@brink-lang/web": patch
---

T1e-1: `ref lvalue-path` path-projection grammar, HIR, and creation-site
checks (docs/t1e-spec.md §2/§6, issue #831, tracking #828). No LIR/VM
support lands in this slice — every path projection still hits a
deliberate "not yet lowerable" fence (see `E099` below).

- New expression-position grammar: `ref` followed by an lvalue-shaped
  operand — a plain path, a dotted field chain, `[…]` indexing, or a mix
  (`ref npc.hp`, `ref party[leader].hp`) — legal only as a direct argument
  of a call, `#fn(…)`, or `bind(…)`. Superset grammar (always parses);
  under `dialect = strict-ink` it's a hard `E051` at analysis, same as
  every other brink extension — the oracle/strict-ink corpus is untouched.
- **`E080`** (reused, not a new code) now also covers the `ref
  lvalue-path` form: a projection's root must be a durable global `VAR`
  (`#@local` flow-locals included) — a `temp`/param root or a `CONST` root
  is a compile error, same rule T1c's unmarked ref-argument form already
  enforces.
- **`E097`** — a `ref` projection outside ref-argument position (a
  standalone value, or nested inside another expression) — a deliberate
  v1 narrowing, tracked as icebox #825.
- **`E098`** — under `types = strict` only, a projection segment (dotted
  field or `[…]` index) that disagrees with the root's statically-known
  declared shape (`VAR name: Shape = …`).
- **`E099`** — a path projection with at least one real segment (dotted
  field or `[…]` index — not a bare single-name `ref`) reaches lowering:
  no `MakeProjection`/`ProjRead` support exists yet (lands in T1e-2,
  tracking #828), so this is a clean, targeted stop rather than a silent
  drop or a miscompile. A bare single-name `ref x` (zero segments) is not
  a real projection and lowers exactly like today's unmarked
  ref-argument form — never hits this fence.
