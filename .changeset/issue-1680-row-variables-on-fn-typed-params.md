---
"@brink-lang/web": patch
---

Analyzer: a call through a fn-typed parameter is now a **row variable** the
caller instantiates, not the pessimal effect-row floor (part of issue #1680 —
docs/effects-spec.md §6 mechanism 1 / §6.1b, Fork B and Fork C ruled
2026-07-28).

A higher-order definition — one whose body calls through one of its own
`fn`-typed params — used to fall straight to the touches-everything floor, and
so did every one of its callers, however precisely that caller knew what it was
passing. Its row now carries a **hole** at that param's declaration index (the
"row with a hole" Fork C ruled for the wire), and each call site fills the hole
with the effect row of the fn value it actually passes. The definition read on
its own is exactly as unbounded as before; the precision arrives one hop up.

Both halves are harvested structurally by the existing body walk — a `#fn`
target is a syntactic name, and a local's origin summary is a syntactic write
set — so no inferred row ever decides a call-graph edge and the query graph
stays acyclic (§6.1a).

The user-visible effect is in the effect-row surfaces: `brink-ide`'s effects
display/hover, `brink-db`'s emitted `EffectRows` table, and the `@[effects(…)]`
contract. A definition that calls a higher-order knot with a traceable callback
now shows a real, non-opaque row where it previously showed the unbounded one,
and an `@[effects(…)]` bound covering that row is satisfied where it previously
reported an `E103` exceedance ("no effects assertion can cover this
definition"), or `E108`/`E109` against `silent`/`total`.

The conservative direction is preserved on every fallback: a param the body
reassigns or that is declared `ref`, an argument that did not trace to an
in-project creation site, a second call site passing something untraceable in
the same position, or a callback whose own row is still parametric all keep the
pessimal floor. The `.inkb` `EffectRows` section's **encoding** is unchanged —
a row still carrying a hole is closed to opaque on the way out, using the same
`EffectRowEntry` shape as before this change. But a caller whose row now
*instantiates* a filled hole emits real, non-opaque `reads`/`writes`/`calls`
where it previously emitted the pessimal placeholder, so the emitted bytes for
those definitions differ from `main` — that is this change's headline payoff,
not a wire-format no-op.
