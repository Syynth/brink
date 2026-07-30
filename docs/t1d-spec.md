# T1d spec — handles & the host boundary

Status: **draft for ratification** (light spec pass, 2026-07-14 — the
design was ruled in the value-model round; this document transcribes
§8 of `docs/value-model-spec.md` into implementable surface and marks
the few genuinely open details). Companions: `docs/format-v4-rfc.md`
(VAL_HANDLE, reserved), `docs/tier1-roadmap.md` §T1d,
`docs/effects-spec.md` (capability manifest), `docs/bevy-brink.md`.
Sections marked **RULED** transcribe the ratified value model;
**PROPOSED** items ratify at this PR's review.

## 1. The model — RULED

Host resources (entities, audio instances, assets, timers) enter the
script world as **`Handle` tokens**: opaque `{kind, id}` scalars with
value semantics — copied like ints, serializable, compared by token.
**No live pointer ever lives in a `Value`.** Dereferencing happens
only host-side, against the host's registry, inside bindings. The
summary dogma applies verbatim: the script world holds only values
and names; a handle is a *name*, re-bound at a defined seam
(rehydration).

## 2. Runtime and wire — RULED

- `Value::Handle { kind: NameId, id: u64 }` in brink-runtime.
- Wire form is V4's reserved `VAL_HANDLE` (kind NameId, u64 id) —
  encoding frozen since the RFC; T1d is its first emission.
- **No new opcodes** (RFC §3: handles are values; bindings do the
  work). `.inkt` gains the matching atom (dump parity rule).
- Handles appear in saves, journals, and speculation snapshots as
  ordinary values.

## 3. Kinds and the manifest — RULED (type-form spelling PROPOSED)

Handle *kinds* live in the **external manifest** — the existing host
semantic-type vocabulary the analyzer already polices — not in the
format. This gives the capability manifest its nouns (effects-spec
§9) and the typed dialect its checking surface.

**RATIFIED** (issue #1552, `docs/decision-log.md` 2026-07-27 "Type-name
surface ruled"): the type form is spelled `Handle<AudioInstance>`
(Uppercase `Handle`, manifest-declared kind name as the parameter) —
this extends typed-mode-spec §3's type-name list as that spec's first
amendment. Under `types = strict`, a
binding declared to take `Handle<AudioInstance>` rejects a
`Handle<Timer>` argument at compile time; under gradual, kind
mismatch is a runtime fault at the binding boundary (the §11c
posture).

## 4. Rehydration and dead handles — RULED (2026-07-14 mechanics round)

Rehydration is **two-halved**, because the knowledge lives on
opposite sides of the save boundary: **save-side keying** (live
resource → durable `SaveKey`) and **load-side resolution**
(`SaveKey` → new resource). One per-kind trait carries both:

```rust
trait HandleKind: 'static {
    const KIND: &'static str;                 // manifest name
    type Resource;                            // Entity, AudioInstanceId, …
    type SaveKey: Serialize + DeserializeOwned;
    fn save_key(&self, world: &World, res: &Self::Resource) -> Option<Self::SaveKey>;
    fn resolve(&self, world: &mut World, key: &Self::SaveKey) -> Option<Self::Resource>;
}
```

- **`SaveKey` is a reconstruction recipe, not just a foreign key.**
  The spectrum, chosen per kind by its implementor: identity lookup
  (an NPC GUID, an asset path), reconstruction (a timer saves its
  remaining duration and `resolve` spawns a fresh one — timers ARE
  resumable), or deliberate ephemerality (`save_key → None`: "this
  resource is meaningless across sessions" — an implementor CHOICE,
  never a spec-assigned category).
- bevy-brink owns opaque token ids and the per-kind registries,
  persists the `token → SaveKey` table beside the ink `SaveState`,
  and rebinds registries at load **keeping token ids stable** (ink
  state is untouched; only the registry's right-hand side rebinds).
  `EntityMapper` integrates for scene-based games.
- **Dead handles are never UB and never a turn fault**: a binding
  dereferencing one returns its **declared failure value** (manifest,
  beside its capability row). Optional `is_valid(h)` ships as a
  standard world-query binding, not a language intrinsic. An
  optional per-binding **dead-deref host event** feeds telemetry.
- **Never-fail-load is the invariant** (player saves are sacred),
  refined for developers: load produces a **rehydration report** —
  rebound / dead-by-resolve (normal) / dead-ephemeral (chosen) /
  **dead-by-unregistered-kind** (suspicious: integration drift) —
  and a host policy knob: `Lenient` (production default) vs
  `StrictKinds` (dev/CI: unregistered kinds fail the load loudly).
- **Registry GC at quiescent points**: script state is enumerable,
  so the host computes the live token set at `-> DONE` sweeps
  (value-model §6 license) and drops unreachable registry entries —
  no script-side destructors exist or are needed.

## 5. Journal and replay — RULED

The journal records returned tokens; replay returns the recorded
token. Determinism holds at the token level; *rebinding* to live
resources happens at the boundary, per §4. No handle-specific journal
record shape is needed — tokens are values.

## 6. Equality, display, restrictions — PROPOSED

- Equality is token equality (`kind == kind && id == id`) — trivially
  consistent with structural value equality. No ordering. **Not a
  legal map key** (keys stay int/string/bool, ruled — same as fn
  values).
- `string(h)` stays total; display form is deliberately boring and
  stable: `handle AudioInstance#42` (kind name + id). Same
  observable-surface-forever reasoning as the fn-value display
  ruling.

## 7. Effects interaction — RULED (2026-07-14, reverses the earlier position)

**Capability atoms may be handle-parameterized**: a binding's
manifest entry may declare `reads Transform(@arg0)` — the capability
attaches to *the resource passed in that parameter*, not the
component class globally. The factored `EffectRows` encoding
**reserves the parameter slot now** (the flat-rows lesson); T2 v1
populates every atom as `(any)` — component-granular, exactly the
pre-amendment design — and **instance resolution ships later as a
narrowing rung** (token comparison at schedule-commit; the existing
selection-not-inference machinery). What this buys, when populated:
per-entity reactive-sleep subscriptions (the flagship ambient-flow
case), token-disjoint parallel scheduling, and **possession-bounded
capabilities as the tier-2 security model** — handles are true
object-capability tokens (no literal syntax exists; only bindings
mint them; possession is authority).

## 8. Snapshot economics note — RULED (carried, no new surface)

The §8 snapshot contract (Arc-bump crossings, bounded retention,
`ptr_eq` as host-side change-detection hint only) is already locked
into the binding contract; T1d adds no new rules. The **dev-build
snapshot-retention metric** rides the bevy-brink slice as a
diagnostics feature, not a semantic.

## 9. Testing — PROPOSED

- Oracle ratchet byte-identical (vanilla ink has no handles by
  construction).
- tier1-brink corpus wing: handle creation via binding, storage in
  collections/structs, save → rehydrate-live → deref, save →
  rehydrate-dead → declared failure value, `is_valid` both ways,
  replay returns recorded tokens.
- Property tests: token round-trip through inkb/inkt/SaveState;
  display-form stability; equality/sharing-unobservable law extension
  (arb_value gains Handle — closing the generator-coverage gap class
  #746 flagged for List).

## 10. Sequencing — PROPOSED (single reviewed agents, oracle-gated)

1. **T1d-1 runtime + format**: `Value::Handle`, VAL_HANDLE emission,
   `.inkt` atom, equality/display, wasm marshal leg (exhaustiveness —
   the #667 wildcard-arm hazard applies verbatim).
2. **T1d-2 manifest kinds + analyzer**: kind vocabulary in the
   manifest, `Handle<K>` type form (typed-mode amendment), strict
   kind-checking + gradual fault.
3. **T1d-3 bevy-brink**: rehydrator registration + EntityMapper
   implementation, dead-handle failure values, `is_valid` binding,
   snapshot-retention dev metric, book section.
