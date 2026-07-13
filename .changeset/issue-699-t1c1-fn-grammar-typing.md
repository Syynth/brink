---
"@brink-lang/web": patch
---

T1c-1: `#fn(name, args…)` function-value creation — grammar, HIR, typing,
and strict call checking (#699, docs/t1c-spec.md §2/§4/§8). Observable
through editor diagnostics:

- `#fn(…)` parses in expression position (superset grammar, the
  `#[…]`/`#{…}`/`Name#{…}` sigil family); under `strict-ink` it rejects at
  analysis with the standard E051 "brink extension" diagnostic. Prose
  position is unchanged — `#` still opens a tag.
- New creation-site diagnostics under `dialect = brink`: E079 (target is
  not a statically-named function definition), E080 (a `ref` param unbound
  at creation, or bound to a non-durable lvalue — temps/params, CONSTs,
  rvalues, and field projections all reject; only VAR cells are durable),
  E081 (more args bound than the target declares).
- `fn(T…): R` type annotations are now legal (E062 retired — it no longer
  fires) and resolve to a real checker type; unknown names inside a fn type
  still flag E061.
- Under `types = strict`, calls through function values are statically
  checked via the existing TM-3 codes: Unknown callee → E065, Conflicted
  callee → E066, non-callable/arity/argument-type mismatches → E063 (the
  `int → float` coercion applies to call arguments).
- Compiling a program that actually uses `#fn` under `dialect = brink`
  still rejects at lowering with a targeted E052 ("not yet implemented" —
  LIR/codegen/VM land in T1c-2). No behavior change for strict-ink
  projects or gradual-mode diagnostics.
