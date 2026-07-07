---
"@brink-lang/web": minor
---

Add Tier-1 fragment support to `StoryRunnerHandle.evaluate()` (F5.1, part of
#440): an arbitrary author-typed expression (`"has(sword) && gold > 2"`,
`"gold"`), content (`"You have {gold}"`), or lone divert (`"-> cellar"`) — not
just a bare knot path or a literal-arg call (Tier 0) — now evaluates instead
of coming back as a dead-end diagnostic.

Mechanism: the fragment is wrapped as a synthetic knot/function
(`=== function __eval_<hash>() ===\n~ return (...)` for an expression,
`=== __eval_<hash> ===\n...` for content — classified by trying the
expression wrap first and falling back to content), recompiled against the
project's full sources via a new `brink-web` entrypoint,
`compile_fragment(entry, sources, syntheticSource)` (multi-file/`INCLUDE`-
aware, unlike the single-file `compile()`), then run through the already-
shipped F4 `Speculation` machinery: a fresh `StoryRunnerHandle` over the
recompiled program, seeded from the live runner's current state
(`load(liveRunner.save())`, name-keyed — globals by name, visit/turn counts
by content-hashed id, both stable across the recompile), `speculate()`, then
`evalFunction`/`goToPath` exactly as the Tier-0 path already does. The
speculation and its scratch runner are discarded when done; nothing touches
the live runner. `evaluate()`'s return shape (`SpeculationResult`) is
unchanged — Tier-1 is invisible to the caller beyond accepting more `source`.

Since a `StoryRunner` holds no reference to the file set it was compiled
from, `evaluate()` gains an `opts.projectSource: { entry, files }` option —
required only for a Tier-1 `source`, supplied by the consumer (the editor,
which has the project's live sources). Without it, or when a fragment fails
to compile as either an expression or content, `diagnostics` comes back
non-empty and nothing runs (no crash).

The scratch runner starts with no external bindings of its own, so
`evaluate()` copies the live runner's registered bindings and
lenient-unbound policy onto it first (`StoryRunner.binding_names`/
`get_binding`/`lenient_unbound`, new) — a query/effect external the fragment
touches resolves the same way it would on the live runner, matching Tier-0's
guarantee (Tier-0 gets this for free by forking the same runner).

Compiled fragments are cached per `StoryRunnerHandle`, keyed by
`(program checksum, fragment source)`: a fragment compiles once per program
version, then every re-evaluation (e.g. a watch panel re-running on every
step) is a cache hit. The cache is bounded (200 entries, FIFO eviction) so a
long session of one-off watches can't grow it without bound. A new
`StoryRunnerHandle.checksum()` (mirroring `StoryRunner::checksum` /
`programChecksum`, but read off the already-linked program so it survives
`reload`) keys the cache to the running program's identity.

The oracle corpus is unaffected — this is purely additive to the compiler's
web binding and the web/TS speculative-eval wrapper; the runtime's own
drive/episode path is untouched.
