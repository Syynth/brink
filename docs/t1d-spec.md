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

**PROPOSED**: the type form is spelled `handle<AudioInstance>`
(lowercase `handle`, manifest-declared kind name as the parameter) —
this extends typed-mode-spec §3's type-name list and should be
ratified as that spec's first amendment. Under `types = strict`, a
binding declared to take `handle<AudioInstance>` rejects a
`handle<Timer>` argument at compile time; under gradual, kind
mismatch is a runtime fault at the binding boundary (the §11c
posture).

## 4. Rehydration and dead handles — RULED (hook API shape PROPOSED)

- A **rehydration hook** runs at load: saved tokens → live resources
  or **dead**. bevy-brink's native implementation is
  `EntityMapper`-based for entity kinds; other kinds get a per-kind
  mapping callback.
- **Dead handles are never UB and never a turn fault**: a binding
  dereferencing one returns its **declared failure value** (each
  binding declares this in the manifest — the same place its
  capability row lives). Optional `is_valid(h)` ships as a standard
  world-query binding in bevy-brink, not a language intrinsic.
- **PROPOSED hook shape** (bevy-brink): a per-kind
  `HandleRehydrator` registration — `fn rehydrate(kind, saved_id) ->
  Option<live_id>`; unregistered kinds rehydrate as dead (safe
  default, load never fails on a missing mapper).

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

## 7. Effects interaction — PROPOSED (restating the T2 skeleton position)

Handle-typed arguments add **the binding's declared access, nothing
more** — dereference happens host-side, so the handle itself
contributes no cells or kinds to a row beyond what the called
binding's manifest entry already declares. (Entity-granular
capability refinement — "reads Transform *of this handle's entity*" —
remains reserved manifest syntax space, explicitly not designed.)

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
   manifest, `handle<K>` type form (typed-mode amendment), strict
   kind-checking + gradual fault.
3. **T1d-3 bevy-brink**: rehydrator registration + EntityMapper
   implementation, dead-handle failure values, `is_valid` binding,
   snapshot-retention dev metric, book section.
