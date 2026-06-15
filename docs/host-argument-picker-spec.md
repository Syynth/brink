# Host-aware argument picker spec

**Status:** Design (Phase 9, issues #176 epic, #174 manifest, #175 studio extension). This is the
**umbrella spec** for host-aware authoring — it composes the manifest hook (#174) and the studio
extension surface (#175) into the call-site argument picker (#176). At implementation time the
schema delta lands in `docs/host-capability-manifest.md` (Tier 3) and the extension surface in
`docs/studio-shell-spec.md` §8; this doc is the cohesive design reviewed first.

> Trust note: **author-time tooling only.** Nothing here touches the runtime, the compiled program,
> codegen, or the oracle. A `switch_id` literal still compiles as its base `int`; host-provided
> values are advisory. This is Tier 3 of the host-capability manifest, whose Tiers 1–2 already ship.

## 1. Goal

Authoring a call to a host `EXTERNAL` should let the author **pick an argument from the host's own
vocabulary** instead of typing a magic number. In celeris (RPG Maker MZ): `set_switch(‹pick›, true)`
offers the game's *named* switches; `give_item(‹pick›, 1)` the item database; `play_se(‹pick›)` the
audio folder. The values are the host's, current to the project, and the picker degrades to plain
literal entry when no host is attached.

## 2. The join point already exists

The hard part — knowing an argument's semantic type at a call site — is **already built** (Tiers
1–2):

- `signature_help` ([`brink-ide/src/signature.rs`](../crates/internal/brink-ide/src/signature.rs))
  + `find_call_context` ([`brink-ide/src/text.rs`](../crates/internal/brink-ide/src/text.rs)) map a
  call-site argument position → its `ManifestParam.ty` (the semantic type, e.g. `switch_id`).
- The manifest schema (`brink-ir/host_manifest.rs`, mirrored in `@brink/wasm-types`) already carries
  `SemanticTypeDef { name, base, constraint? }` and per-param `ty: TypeRef`.

So when the cursor is on `set_switch(‹here›, true)`, the studio already knows arg 0 is a `switch_id`.
The picker is a new **consumer** of that fact: given the type, produce values + labels.

## 3. The pieces

Three parties, the same separation Tiers 1–2 use (manifest declares the hook, studio brokers, host
owns the dynamic data):

| # | Piece | Role |
|---|---|---|
| **#174** | Manifest hook | A semantic type declares *where its values come from* — static labels or `host`. |
| **#175** | Studio extension surface | `argumentProviders` — the host (embedder) supplies the values as **data**; the studio renders. |
| **#176** | The picker | Composition: at a call-site arg of a value-bearing type, offer the values as completions + inlay-label the literal. |

### 3.1 #174 — manifest value sources

`SemanticTypeDef` gains an optional `values` field — *where the picker's value/label list comes
from* — orthogonal to `constraint` (which is for **checking**):

```ts
/** Where a semantic type's pickable values + labels come from (Tier 3). */
type ValueSource =
  | { source: "static"; items: { value: string; label: string; detail?: string }[] }
  | { source: "host" };   // enumerate/resolveLabel answered by the attached host (§4)

interface SemanticTypeDef {
  name: string;
  base: BaseType;
  constraint?: Constraint | null;   // Tier 2 — checking (enum/regex/range)
  values?: ValueSource | null;      // Tier 3 — the picker's value list (NEW)
}
```

- **`static`** — a closed, labelled set baked into the manifest (`direction`, `difficulty`). Drives
  the picker **with no host attached** (the Phase-9 static slice). Distinct from `constraint: enum`,
  which has values but *no labels* and is for validation; a type may carry both (enum to check,
  static `values` to label/pick), or just one.
- **`host`** — the values are dynamic and project-specific (`switch_id`, `item_id`); the studio
  asks the attached host (§4). With no host, the param degrades to plain literal entry.

Rust mirror: `SemanticTypeDef.values: Option<ValueSource>` in `brink-ir/host_manifest.rs`;
`ValueSource` a serde enum (`#[serde(tag = "source")]`).

**Checking posture (unchanged philosophy):** `values` is **advisory** — it never hardens a
diagnostic. A literal outside a `host` set is at most informational (the running game is source of
truth; ids legitimately appear/disappear between sessions). Enforcement stays with `constraint`
(E041/E042). So `values: host` ⇒ no closed-domain error, only the picker + (optionally) a *warning*
if the studio later gains `Warning` severity (deferred, see manifest doc).

### 3.2 #175 — studio `argumentProviders`

`StudioExtensions` (mount-time, `docs/studio-shell-spec.md` §8.1) gains a `TypeRef`-keyed surface.
**Data-only**, like the rest of the extension API — the provider returns values; the studio owns all
rendering (no React/CodeMirror coupling):

```ts
interface ArgumentValue {
  value: string;     // the literal inserted into source (e.g. "5")
  label: string;     // display (e.g. "HarborGate")
  detail?: string;   // secondary (e.g. "Switch #5")
}

interface ArgumentContext {
  external: string;      // the EXTERNAL being called, e.g. "set_switch"
  paramIndex: number;    // 0-based argument position
  type: TypeRef;         // the param's semantic type, e.g. "switch_id"
  currentText: string;   // what the author has typed so far in the slot
}

interface ArgumentProvider {
  /** The semantic type this provides values for. */
  type: TypeRef;
  /** Pickable values for this argument (sync or async; see §4 push-cache). */
  enumerate(ctx: ArgumentContext): ArgumentValue[] | Promise<ArgumentValue[]>;
  /** Optional label for an existing literal — drives inlay hints. */
  resolveLabel?(value: string): string | undefined;
}

interface StudioExtensions {
  // …toolWindows / commands / statusBarItems (existing)…
  argumentProviders?: ArgumentProvider[];   // ids/types must be host.<vendor>.* or a known type
}
```

A host registers providers for the types its manifest marks `values: host`. The provider is the
*supply*; #174's manifest flag is the *declaration that a supply exists*. They wire together on the
**type name** (`switch_id`).

### 3.3 #176 — the picker (composition)

At a call-site argument the studio:

1. **Resolves the arg's type** — the existing join point (§2): cursor inside `(…)` → `paramIndex` →
   `ManifestParam.ty`.
2. **Finds a value source**, in precedence order: a registered `argumentProvider` for that type →
   else the manifest's `values` (`static` items, or `host` → the pushed cache, §4) → else none.
3. **Completions** — if a source exists, offer its `{value, label, detail}` through the **existing
   completion UI** (`ink-editor/completions.ts`): `label` shown, `value` inserted, `detail` as
   secondary text. No new rendering model.
4. **Inlay labels** — for an *existing* literal whose param has a value source, render
   `set_switch(5 /* HarborGate */)` via the source's `resolveLabel`/`static` map, through the
   existing inlay-hint pipeline.
5. **Degrade** — no source (detached host, no provider) → plain literal entry; the type still checks
   at the base level (Tier 1) and via any `constraint` (Tier 2).

## 4. Transport — when a host is attached

The dynamic (`host`) source needs author-time host data. **Resolved fork (was open in the manifest
doc §"Remaining design forks"):** **push-cache, not async-per-query.**

- The host **pushes** value snapshots into the studio session — `{ type, items: ArgumentValue[] }` —
  at attach and whenever its data changes (a switch renamed, an item added). The studio holds a
  cache keyed by `TypeRef`.
- Completions + inlay labels are served **synchronously from the cache** — no await on a keystroke,
  no editor jank. `resolveLabel` is a cache lookup.
- Rationale: value sets (item DB, switches) change rarely and the host knows *when*; the host already
  pushes session events over the live bridge (Phase 8, #127); sync completions are the right UX. The
  async-call-mid-query alternative pays latency on every interaction for data that's effectively
  static between edits.

**Wiring:** a new author-time entry alongside the manifest registration —
`EditorSession.set_host_values(json)` / `clear_host_values` (mirrors `set_host_manifest`), or
equivalently the `StudioApi`/embedder pushes them; the studio updates the cache and invalidates open
editors' completions. This is a **distinct message set from the runtime SessionProvider** (Phase 8) —
it may ride the *same* host connection (RMMZ/Bevy embed) but answers author-time `enumerate`, not
runtime stepping. When detached, the cache is empty → `host`-source params fall back to literal entry;
`static`-source params still work.

The `ArgumentProvider.enumerate` returning a `Promise` is supported (an embedder may compute lazily),
but the **expected** path is sync-from-push.

## 5. Staging (when we implement — not now)

Each step is independently shippable; the static slice de-risks the editor UI before any transport.

1. **Schema** (#174) — `ValueSource` in `brink-ir/host_manifest.rs` + `@brink/wasm-types` mirror.
   Pure additive serde; no behavior.
2. **Arg-source query** (brink-ide/brink-web) — a query `argument_value_source(source, offset)` →
   `{ type, source } | null`, built on the existing `find_call_context` + manifest lookup, so the
   editor knows *when* to offer a picker and what type. Inlay-hint pass gains value labels.
3. **Static slice** (no host) — `values: static` → completion dropdown + inlay labels. **Builds the
   whole call-site picker UI + plumbing** against static data; locally exercisable in the brink
   studio with a static manifest. This is the first concrete, testable increment.
4. **Extension surface** (#175) — `argumentProviders` in `StudioExtensions` +
   `installStudioExtensions`; the editor consults providers ahead of the manifest `values`.
5. **Host transport** (#174 dynamic) — `set_host_values` push + the cache; `values: host` served
   from it. Exercised end-to-end only with a real host (celeris RMMZ) — same "mechanism now, consumer
   later" pattern as Phase 8's degraded mode / remote provider.

## 6. Out of scope (Tier 3+, not these issues)

- **Host-rendered editors** — the map-point/path picker that the studio *cannot* render (RMMZ map
  editor needing tilesets) and its request-host-UI/return-structured-value invocation protocol. Fully
  designed already in `docs/host-capability-manifest.md` §Tier-3; a heavier, separate increment than
  the value-provider picker these issues cover.
- **Arg-group / inter-arg-context widgets** (one widget spanning `[x,y]`, context from another arg) —
  part of the host-editor design above, deferred with it.
- **Manifest generation** from host source (an RMMZ plugin's command list) — a nice-to-have.
- **Insert-`EXTERNAL` code action** and **regex/`Warning` severity** — pre-existing Tier-1/2
  follow-ups, tracked in the manifest doc.

## 7. Consumer

celeris `@codetta/brink-host` (Syynth/celeris epic #71): `switch_id` / `var_id` / `item_id` /
`audio_name` become `values: host`; the host pushes `$dataSystem.switches`, `$dataItems`, … — the
data already exists host-side; only the schema hook (#174) + the push transport (§4) + the extension
surface (#175) are missing. Separate repo; not built here.

## 8. Open questions for the regroup

1. **`values: host` vs reusing `constraint`** — do we want `values` as a *separate* field (this spec)
   or fold the host source into the `Constraint` union (`{ kind: "host" }`)? Separate keeps
   *checking* (constraint) and *picking* (values) orthogonal — recommended — but it's a schema-shape
   call.
2. **Where the push lives** — a dedicated `EditorSession.set_host_values` (parallel to
   `set_host_manifest`) vs threading values through the existing manifest push vs a `StudioApi`
   method. Leaning a dedicated entry (values change independently of the manifest shape).
3. **Static slice as a standalone ship** — worth landing steps 1–3 (static picker) as its own PR for
   immediate value, or hold until the dynamic path is also ready? (The static slice's value is real
   but narrower than the host-dynamic killer case.)
