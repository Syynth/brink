# `@brink-lang/web` consumer guide — speculative evaluation

This is the consumer-facing guide to the **speculative evaluation** surface of
`@brink-lang/web` (the wasm runtime binding). It's a **runtime** capability — it
has nothing to do with `@brink-lang/editor`; you can use it with or without the
editor package.

**What it is:** run the story *hypothetically* from its current state — evaluate
an expression to a value, or preview the transcript a divert/knot/content would
produce — and throw the run away. **Side-effect-proof by construction:** a
speculation runs over a sandboxed fork; nothing it does touches the live story
(no global writes, no visit-count changes, no RNG advance escape). It's the
mechanism behind a watch/eval panel.

## Two entry points

Both hang off a `StoryRunnerHandle` (your live, running story):

- **`evaluate(source, opts?)`** — a thin async convenience for the common case:
  hand it a string, get back a value or a transcript. Start here.
- **`speculate(opts?)`** — the composable primitive: gives you a
  `SpeculationHandle` you drive with the ordinary verbs (`goToPath`, `advance`,
  `choose`, `evalFunction`, …). Reach for it when `evaluate`'s shape doesn't fit.

`evaluate` is sugar *over* `speculate` — it never hides it.

## `evaluate(source, opts?)`

```ts
const result: SpeculationResult = await runner.evaluate("gold > 10");
// result.value  → { type: "bool", value: true }

const preview = await runner.evaluate("cellar.intro");
// preview.transcript → [{ text: "You descend into the cellar…", tags: [] }, …]
```

`source` is classified automatically, and **the classification is invisible to
you** — you always call the same method:

| `source` | Handled as |
|---|---|
| a bare knot/stitch path (`"cellar.intro"`) | **Tier 0** — jump + run, no compile |
| a function call with **literal** args (`"has_item(\"sword\")"`) | **Tier 0** — eval, no compile |
| anything else — an arbitrary expression (`"has(sword) && gold > 2"`) or content (`"You have {gold}"`) | **Tier 1** — compiled as a fragment (see *Tier 1* below) |

### The result

```ts
interface SpeculationResult {
  value?: TypedValue;              // present when source was an expression / function call
  transcript: SpeculationLine[];   // { text, tags } lines the run produced
  reachedChoices?: Choice[];       // present if the run stopped at a choice point
  stop: "completed" | "choices" | "step-budget" | "line-budget";
  externals: { live: string[]; fallback: string[] };  // diagnostic: which externals fired vs fell back
  diagnostics: string[];           // non-empty ⇒ nothing ran (compile failure, or Tier-1 without projectSource)
}
```

**Always check `diagnostics` first** — when it's non-empty (a fragment that
didn't compile, or a Tier-1 source with no `projectSource`), the other fields
are meaningless.

### Options

```ts
interface EvaluateOptions {
  context?: "watch" | "eval";      // default "watch"
  liveEffects?: boolean;           // default false — see "Externals" below
  budget?: { steps?: number; lines?: number };  // defaults 100_000 / 1_000
  kinds?: Record<string, "query" | "effect">;   // external classification (see below)
  signal?: AbortSignal;            // abort → the promise rejects with AbortError
  projectSource?: ProjectSource;   // REQUIRED for a Tier-1 fragment (see below)
}
```

## Externals: the `@kind` policy

A speculation must not fire real side effects while you're just watching. So
externals are **tiered by kind**, which you supply as data (`opts.kinds`, a
`name → "query" | "effect"` map — typically derived from your host-capability
manifest; the runtime itself never sees a manifest):

- **`"query"`** (read-only — `get_variable`, `has_item`, …) → **runs live** in
  every context. These are load-bearing: most conditions branch on them.
- **`"effect"`** (state-changing — `grant`, `camera`, …) → **blocked** in
  `context: "watch"` (falls back to the ink fallback body, or stops). In
  `context: "eval"` it's still blocked **unless** you also pass
  `liveEffects: true` — an explicit, per-call arming for a debugger-console
  "run this for real" action.
- **Any external absent from `kinds`** is conservatively treated as `"effect"`.

`result.externals` reports which externals ran live vs fell back, for display.

Query externals may be async (a binding returning a `Promise`); `evaluate`
awaits them transparently.

## Values: `TypedValue`

Expression/function results marshal to a tagged `TypedValue`:

```ts
type TypedValue =
  | { type: "int"; value: number }   | { type: "float"; value: number }
  | { type: "bool"; value: boolean } | { type: "string"; value: string }
  | { type: "null" }
  | { type: "list"; items: ListMember[] }   // ink lists: { origin, name, ordinal }[]
  | { type: "divert"; path?: string };
```

Lists are **display-complete** (member origin + name + ordinal); diverts carry
the resolved target path. (Feeding a list *back into* another eval isn't
supported yet — display/compare only.)

## Tier 1: arbitrary fragments need `projectSource`

A `StoryRunner` holds only an already-linked program, not the source it was
compiled from. So to evaluate anything beyond a bare path or literal-arg call —
an arbitrary expression or content fragment — you must hand back the project's
current sources so the fragment can resolve against the live project's real
globals/knots/lists:

```ts
await runner.evaluate("has(sword) && reputation > 5", {
  projectSource: {
    entry: "main.ink",
    files: { "main.ink": mainSrc, "chars.ink": charsSrc /* keyed exactly as INCLUDE names them */ },
  },
});
```

Under the hood this recompiles the project with the fragment wrapped in a
synthetic symbol — **cached, so a given fragment compiles once per project
version**, then re-evaluations are cheap. You don't manage the cache; just pass
`projectSource` and re-call `evaluate` as state changes. (Omit it and a Tier-1
source returns a diagnostic rather than running.)

## `speculate()` — the composable verbs

When `evaluate`'s single-call shape doesn't fit (you want to drive line-by-line,
present-and-pick choices, or compose your own loop), use the primitive:

```ts
const spec = runner.speculate({ context: "watch", kinds });  // policy baked in here
try {
  spec.goToPath("cellar.intro");        // or omit — continues from the live position
  const line = await spec.advanceAsync(); // drive one line (awaits async externals)
  // … choose(), evalFunction(), transcript(), externalsReport(), etc.
} finally {
  spec.free();                          // discard — nothing survives
}
```

`SpeculationHandle` exposes the ordinary drive verbs — `goToPath`, `advance` /
`advanceAsync`, `choose`, `evalFunction` / `evalFunctionAsync`
(→ `SpeculationFunctionEval`), `resumeFunctionEval` / `resumeFunctionEvalAsync`,
`resolveExternal`, `pendingExternalName`, `transcript`, `externalsReport` — plus
`free()` to discard. The externals policy (`context`/`liveEffects`/`kinds`) and
budgets are baked in at `speculate(opts)`; the verbs take none. Same
`SpeculationOptions` as `evaluate`.

**Lifecycle:** a `SpeculationHandle` owns a snapshot; always `free()` it when
done (a `try/finally`). Dropping it discards everything — the live story is never
touched regardless.

## Guarantees & notes

- **Side-effect-proof:** a speculation runs over a sandboxed fork seeded from a
  *snapshot* of live state; the live runner is structurally a separate copy.
  Nothing a speculation does — globals, visit counts, RNG, effect externals —
  reaches the live story.
- **Reflects current state:** a speculation reads the live story's *current*
  globals/visits/RNG (as of the call), so a watch shows what the story would
  actually do next.
- **Caching** is automatic and keyed by `(program identity, fragment source)`;
  it invalidates when the project recompiles. Re-evaluating the same watch as
  state advances is cheap.
- **Abort:** pass `opts.signal`; aborting drops the in-flight speculation and
  rejects the promise with an `AbortError`.
