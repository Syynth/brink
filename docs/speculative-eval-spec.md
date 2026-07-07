# Speculative Evaluation — fragment compile + program overlay spec

**Status:** Design (issue #411). Implementation does not begin until this spec is
approved. Ships as **0.9.0** (new API surface across compiler + runtime + web).

> **Tier note (2026-07-07):** [`scoped-flow-state-spec.md`](scoped-flow-state-spec.md)
> re-derives watch/eval as two tiers on a shared core: **Tier 0** ("invoke
> existing" — a knot/stitch path or a literal-arg function call, over a
> sandboxed fork of live state) already shipped as F4.1–F4.3 —
> `brink_runtime::Speculation`/`KindTieredHandler` and the web
> `speculate()`/`evaluate()` surface. This document is now that plan's
> **Tier 1**: arbitrary author-typed expressions/content via fragment
> compilation + program overlay, for when Tier 0's two invoke-existing shapes
> aren't enough. Its `SpeculativeEval`/`SpeculativeExternalPolicy`/etc. below
> are this **future** tier's own types, distinct from (and not yet reconciled
> with) the already-shipped `Speculation`/`KindTieredHandler` — see #439/#440.

> Trust note: this feature never changes the semantics of compiled programs, the
> `.inkb` format on disk, or the oracle. The fragment/overlay artifacts are
> in-memory tooling constructs; the ratchet does not move.

## 1. Purpose

The celeris State panel's Watch/Eval section (celeris #913/#914) is a mini-REPL
over real ink: an expression returns a typed value; a divert or content fragment
returns the transcript it *would* produce from the current story state. Every
evaluation must be **side-effect-proof** — visit-count, global, and RNG
mutations must die with the evaluation.

The runtime's own factoring provides the mechanism: `Context` is `Clone` and was
explicitly designed as the fork/branch/rollback seam (story.rs `Context` doc).
`begin_function_eval` is **not** an alternative — it isolates output and call
stack but writes globals and draws RNG on the live `Context` in place (verified;
story.rs:1266–1414). Only clone-and-discard is side-effect-proof.

First consumer: celeris watches + transcript previews. Second consumer (design
constraint, not v1 deliverable): bevy-brink's live inspector — hence the runner
is **Rust-canonical** in `brink-runtime`, with `@brink-lang/web` as a thin
wrapper (decision-log 2026-07-06, "#411 deep-layer build").

## 2. Design rulings (decision-log 2026-07-06)

1. **Proper deep-layer build** — a true fragment-compilation entrypoint plus
   program-overlay linking, not the recompile+state-migration shortcut.
2. **Externals policy is a harness-built handler** — the VM stays
   manifest-blind. Tiering by `@kind`: query → live in both contexts;
   effect/presentation/unclassified → fallback-or-stop in watch context,
   armable (`liveEffects`) in eval context. Presentation is an effect for
   speculative purposes (the kind exists for client/server authority routing, not
   purity).
3. **Async pending supported in v1** — the isolated speculative flow awaits via the
   existing `Pending`/`resolve_external` machinery; `evaluate` is async,
   with cancellation and a concurrency cap.
4. **Version 0.9.0.**

## 3. Architecture at a glance

```
author-typed source ("doors_open > 2", "-> cellar", "Hello {name}")
        │
        ▼
┌─ fragment compiler (brink-syntax / brink-ir / brink-analyzer / brink-codegen-inkb)
│    parse_expression / parse_content_block          (new pub entrypoints)
│    HIR lower fragment → resolve against live SymbolIndex → LIR fragment → emit chunk
│    ⇒ CompiledFragment { containers, extra names/list-literals/line-table,
│                         entry id, base_checksum, diagnostics }
▼
┌─ program overlay (brink-runtime)
│    OverlayProgram<'p> { base: &'p Program, extra_* }  via ProgramLike trait
│    checksum guard (locale-overlay precedent)
▼
┌─ speculative runner (brink-runtime)
│    SpeculativeEval { cloned Context, fresh FlowInstance at overlay entry }
│    run → value + transcript + choices | AwaitingExternal | budget stop
│    Drop = discard (nothing to undo)
▼
┌─ policy handler + web surface (brink-web / packages/wasm)
│    SpeculativeExternalPolicy built from analyzer external_meta (name → kind tier)
│    StoryRunnerHandle.evaluate(source, opts) → Promise<SpeculativeResult>
```

## 4. Fragment compilation

### 4.1 Parse entrypoints (`brink-syntax`)

The grammar rules already exist as `pub(crate)` (`parser::expression::expression`,
`parser::content::content_line`/`mixed_content`). Two new public entrypoints wrap
them with a fresh parser:

- `parse_expression(source) -> Parse` — a bare expression.
- `parse_content_block(source) -> Parse` — one or more content lines, including
  diverts, glue, tags, inline logic. A lone `-> knot.stitch` is just the
  degenerate content block; no special-cased divert path.

Fragment kind is chosen by the caller (the panel knows whether the user is in
the watch box or typed content); `evaluate` defaults to: try expression,
fall back to content block if expression parsing fails (mirrors ink's own
ambiguity conventions — TBD by implementer with tests, see §10).

### 4.2 Lowering + resolution (`brink-ir` / `brink-analyzer`)

HIR expression lowering already defers name resolution (it stamps
`Expr::Path`/`Expr::Call` and records unresolved refs via `LowerSink`), so a
fragment lowers with only a `LowerScope` anchor and a sink. Resolution then runs
against the **live project's `AnalysisResult.index`** (`SymbolIndex.by_name` →
`DefinitionId`) — the same universe the real program was compiled from.
Unresolved names become **diagnostics in the result, never panics** (issue
requirement).

What's genuinely new is a fragment-scoped `lir` entrypoint: today
`lower_to_program` re-collects globals/lists/externals from all files. The
fragment variant skips decl collection entirely — the symbol universe is given —
and lowers a single synthetic root container holding the fragment.

**Scope anchor (v1):** fragments resolve at **root scope with qualified names**
(`cellar.wine_rack`, not a bare `wine_rack` relative to the current stitch).
Position-relative resolution requires mapping the live flow's current position
back to a `LowerScope`; deferred as a follow-up (§10). Likewise, the paused
flow's **temps/locals are not readable** — they live on the flow's value stack,
not in `Context`; the clone can't see them. v1 speculative evals see globals, visit
counts, list values, and callable knots/functions.

### 4.3 Identity and collisions

`DefinitionId` is a content hash of `(tag, qualified path)`. The fragment's own
synthetic containers are stamped under a reserved scope prefix — `$speculative.<n>`
— which cannot collide with real ink paths (`$` is not a valid ink identifier
character). Everything the fragment *references* uses the live ids verbatim.

### 4.4 Chunk codegen (`brink-codegen-inkb`)

`emit()` is already continuation-friendly (it seeds its name-table from the
input and appends). The fragment path exploits this: codegen is seeded with the
**live program's `name_table` and `list_literals`**, so every pre-existing
constant keeps its exact index and new entries append past the end. The
`CompiledFragment` then carries **only the appended tails** (`extra_names`,
`extra_list_literals`) — indices `>= base.len()` resolve into them (§5).

Guard: `NameId` is `u16`. If the base table is near 65,536 entries, appending
can overflow — this is a fragment-compile **diagnostic**, not a runtime error.

Content fragments emit `EmitLine` ops referencing a scope-relative line table;
the fragment gets its **own synthetic scope line table**, carried in the
artifact and resolved overlay-first at read time (§5). This slots into the
existing "line tables live beside `Program`, swappable" split from the locale
work — no new concept.

### 4.5 The artifact

```rust
/// In-memory only in v1 — never serialized into .inkb.
pub struct CompiledFragment {
    pub kind: FragmentKind,            // Expression | Content
    pub entry: DefinitionId,           // $speculative.<n> root container
    pub containers: Vec<ContainerDef>, // usually 1–few
    pub addresses: Vec<AddressDef>,
    pub extra_names: Vec<String>,      // name_table tail (indices >= base len)
    pub extra_list_literals: Vec<ListValue>,
    pub line_table: Vec<LineEntry>,    // fragment's synthetic scope
    pub base_checksum: u32,            // live program's source_checksum
    pub diagnostics: Vec<Diagnostic>,  // unresolved names, overflow, parse errors
}
```

Compilation is **cached** by `(base_checksum, source, kind)` — a watch
expression compiles once per program version, then every per-step re-evaluation
is just clone-run-drop.

Where the compile entrypoint lives: `brink-compiler` grows
`compile_fragment(source, kind, &AnalysisResult, &StoryData/&Program-tables) ->
CompiledFragment` as the public driver (exact table-handoff shape TBD by
implementer; the inputs are the symbol index + the base constant pools).

## 5. Program overlay (`brink-runtime`)

### 5.1 `ProgramLike`

The VM already accesses `Program` exclusively through accessor methods
(`container`, `resolve_target`, `resolve_global`, `name`, `list_literal`,
`list_item`, `list_def`, `scope_table_idx`, …) — no raw field access in
`vm.rs`/`value_ops.rs`/`list_ops.rs`. Extract that surface into a
`ProgramLike` trait, implemented by `Program` (trivially) and by:

```rust
pub struct OverlayProgram<'p> {
    base: &'p Program,
    containers: Vec<LinkedContainer>,     // idx >= base.containers.len()
    address_map: HashMap<DefinitionId, (u32, usize)>, // checked first, then base
    extra_names: Vec<String>,             // NameId >= base len
    extra_list_literals: Vec<ListValue>,
    // fragment line-table scope, resolved overlay-first
}
```

`vm::step` and the `FlowInstance` drive methods become generic over
`P: ProgramLike` (monomorphized — zero overhead for the base `Program` case;
the concrete `Story` API stays `&Program` and is untouched). This is the
mechanical-but-wide part of the change: every `&Program` threading site in
`vm.rs`, `story.rs`, `value_ops.rs`, `list_ops.rs` generalizes. It is also the
"proper fix we don't recreate later": hot-reload, A/B patching, and future
tooling all want the same seam.

### 5.2 Construction + guard

`OverlayProgram::link(base: &Program, fragment: &CompiledFragment) ->
Result<OverlayProgram, RuntimeError>`:

- **Checksum guard** (locale precedent, `apply_locale`): error if
  `fragment.base_checksum != base.source_checksum()`. New
  `RuntimeError::FragmentChecksumMismatch`.
- Link the fragment containers exactly as `linker::link` does, assigning
  container indices starting at `base.containers.len()`.
- Coarse guard only: the checksum detects "compiled against a different
  program"; finer divergence surfaces naturally as
  `UnresolvedDefinition`/`UnresolvedGlobal` at resolve time — reported as an
  eval diagnostic, never a panic.

## 6. Speculative runner (`brink-runtime`)

```rust
impl Story<'_, R> {
    /// Clone the flow's Context, spawn a speculative flow at the fragment entry.
    pub fn begin_speculative_eval(
        &self,
        fragment: &CompiledFragment,
        opts: SpeculativeOpts,        // step_budget, line_budget, flow (default flow by default)
    ) -> Result<SpeculativeEval<'_>, RuntimeError>;
}

pub struct SpeculativeEval<'p> { /* OverlayProgram<'p>, cloned Context, FlowInstance, budgets */ }

impl SpeculativeEval<'_> {
    /// Drive to completion or the next pause point.
    pub fn run(&mut self, handler: &dyn ExternalFnHandler) -> Result<SpeculativeStep, RuntimeError>;
    pub fn resolve_external(&mut self, value: Value) -> Result<(), RuntimeError>;
    pub fn pending_external_name(&self) -> Option<&str>;
    pub fn finish(self) -> SpeculativeOutcome;   // Drop also just discards
}

pub enum SpeculativeStep { Complete, AwaitingExternal }

pub struct SpeculativeOutcome {
    pub value: Option<Value>,          // Expression fragments
    pub transcript: Vec<Line>,         // Content fragments (and any output an expression's calls produce)
    pub reached_choices: Vec<Choice>,  // stopped at a choice point
    pub stop: SpeculativeStop,             // Completed | ChoicesReached | StepBudget(u64) | LineBudget(usize) | ExternalBlocked(String) | Diverged(Diagnostic)
    pub externals: ExternalsReport,    // invoked_live / fallback / blocked, by name
}
```

Semantics:

- The clone is taken at call time from the target flow's `Context` — globals,
  visit/turn counts, **RNG state** (a previewed shuffle shows what the live
  story would actually produce next). Everything the speculative run mutates dies
  with the `SpeculativeEval`.
- Runs to `Done`/`END`, a **choice point** (choices are collected and reported,
  never picked), or budget exhaustion. Budgets default far below the VM's
  `STEP_LIMIT` (proposed: 100k steps / 1k lines, both overridable) — a panel
  watch must not be able to camp the UI thread. This honors the standing
  "guard against unbounded growth" rule.
- Expression fragments run in eval mode and produce `value`; if the expression
  calls an ink function that emits output, that output lands in `transcript`
  (reported, not discarded — it's free information).
- `AwaitingExternal` uses the existing frozen-external-frame machinery
  unchanged; the harness resolves and re-runs. Dropping a frozen `SpeculativeEval`
  is the cancellation path — nothing to unwind.

## 7. Externals policy (harness layer)

The VM stays manifest-blind (host-capability-manifest charter: "the runtime
never sees the manifest"). The harness constructs the speculative flow's handler:

```rust
pub struct SpeculativeExternalPolicy<'h> {
    inner: &'h dyn ExternalFnHandler,     // the real host bindings
    kinds: HashMap<String, ExternalKind>, // from analyzer external_meta — a name→tier map, NOT the manifest
    context: EvalContext,                 // Watch | Eval
    live_effects: bool,                   // arming flag, Eval only
}
```

| kind | Watch | Eval (disarmed) | Eval (armed) |
|---|---|---|---|
| `Query` | live | live | live |
| `Effect` / `Presentation` / `Plain` (unclassified) | fallback-or-stop | fallback-or-stop | live |

- **fallback-or-stop:** if the external has an ink fallback body →
  `ExternalResult::Fallback`; otherwise the eval stops with
  `SpeculativeStop::ExternalBlocked(name)`. Which path each external took is in
  `ExternalsReport`.
- A live query returning `Pending` → `AwaitingExternal`; allowed in **both**
  contexts (watch cadence/staleness is the consumer's concern — see §8
  cancellation).
- An *unbound* query-kind external follows the session's existing
  lenient-unbound setting, same as live flows.

`SpeculativeExternalPolicy` lives in `brink-runtime` as a plain composable handler
(it's just a name→tier map — no manifest types cross the boundary); the *data*
comes from the analyzer at the web/bevy layer.

## 8. Web surface (`brink-web` / `packages/wasm`)

```ts
interface SpeculativeOptions {
  context?: "watch" | "eval";      // default "watch"
  liveEffects?: boolean;           // default false; only meaningful with context: "eval"
  stepBudget?: number;
  signal?: AbortSignal;            // cancel: drops the speculative flow, promise rejects AbortError
}

interface SpeculativeResult {
  value?: TypedValue;
  transcript: Line[];
  reachedChoices?: Choice[];
  stop: "completed" | "choices" | "step-budget" | "line-budget"
      | { externalBlocked: string };
  externals: { live: string[]; fallback: string[]; blocked: string[] };
  diagnostics: Diagnostic[];       // compile diagnostics; non-empty ⇒ nothing ran
}

class StoryRunnerHandle {
  evaluate(source: string, opts?: SpeculativeOptions): Promise<SpeculativeResult>;
}
```

- **Async by construction**: a live query external bound to a
  Promise-returning JS function parks the speculative flow; the wrapper awaits and
  resumes — same plumbing as the existing `Pending` path.
- **Cancellation**: `AbortSignal`; aborting destroys the speculative flow
  immediately (discard is free). The panel cancels the in-flight eval when a
  newer step lands.
- **Concurrency cap**: at most N speculative evals in flight per runner (proposed
  default 8); excess calls queue. Prevents frozen-flow leaks from
  never-settling promises, together with the abort path.
- **Kind map**: the wrapper pulls the external→kind classification from the
  project's analysis (the same `external_meta` that feeds hover/signature) and
  builds the policy handler. `StorySessionHandle` gets a passthrough
  `evaluate` that targets its underlying runner; speculative evals are
  **not journaled** (they never touch session state by construction).
- Fragment-compile caching keyed by `(program checksum, source, kind)` lives in
  the wrapper.

## 9. What this does NOT do

- No `.inkb` format change; `CompiledFragment` is never serialized in v1.
- No oracle impact — speculative evals are pure tooling on top of the VM; episode
  behavior is untouched; the ratchet stays at its current value.
- No manifest-to-runtime coupling — the runtime receives a name→tier map built
  by the harness.
- No mutation escape hatch: there is deliberately **no** "apply speculative result
  to live state" in v1. Eval-context writes that should stick are what
  `setVar`/session mutation APIs are for.

## 10. Deferred / open

- **Position-relative name resolution** (a watch referencing `wine_rack`
  relative to the flow's current stitch) — needs current-position →
  `LowerScope` mapping; follow-up.
- **Reading the paused flow's temps/locals** — they live on the flow's value
  stack, not `Context`; would need a stack-inspection seam; follow-up if
  celeris asks.
- **Expression-vs-content auto-detection heuristics** — implementer decides
  parse-fallback order with tests; the API also lets the caller force a kind.
- **Async query externals in high-frequency watches** — supported, but if it
  proves janky in practice, celeris can down-tier (treat pending-capable
  watches as manual-refresh); no runtime change needed.
- **Persisting fragments / breakpoint-condition reuse** — a future debugger
  wants compiled-fragment reuse; the artifact is shaped to allow serialization
  later, but it's out of scope now.

## 11. Milestones

1. **Fragment front-end** — `parse_expression`/`parse_content_block`,
   fragment HIR lowering + resolution against a live `SymbolIndex`,
   fragment-scoped LIR entrypoint. Tests: resolution, diagnostics, no-panic on
   garbage input.
2. **Chunk codegen** — seeded-pool emission, `$speculative` stamping, synthetic
   line table, `CompiledFragment`. Tests: constant-index alignment against the
   base program, NameId overflow diagnostic.
3. **`ProgramLike` + `OverlayProgram`** — trait extraction (pure refactor,
   zero behavior change, oracle green), overlay linking + checksum guard.
4. **Speculative runner** — `begin_speculative_eval`/`SpeculativeEval`, budgets,
   choice/done/blocked outcomes, async pause/resume, discard semantics. Tests:
   side-effect-proofness (globals/visits/RNG identical before/after), shuffle
   preview matches live continuation, budget stops.
5. **Policy + web** — `SpeculativeExternalPolicy`, a wasm binding for it, and
   the `evaluate` wrapper with cache/cap/abort, TS types, docs
   (editor-consumer-guide section). Then the celeris-side wave consumes it.

Steps 1–3 are independent of each other's landing order except 4 needs all
three; each step is a separately reviewable PR in the usual train.
