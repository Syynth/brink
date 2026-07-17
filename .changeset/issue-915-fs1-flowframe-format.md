---
"@brink-lang/web": patch
---

FS-1 (#915, tracking #889): the FlowFrame suspended-flow section in
`SaveState` — format only (`docs/flow-suspension-spec.md` §2/§9). No
compiler `await` support and no runtime spill/restore land in this slice;
`Story::save_state`/`load_state` always produce/consume `None`.

- `SaveState` grows an optional `suspended: Option<SuspendedFlow>` field
  behind `#[serde(default)]`/`skip_serializing_if` — an older save missing
  the key still deserializes, and an unsuspended save's wire form is
  byte-identical to before (no `"suspended": null` noise).
- `SuspendedFlow` (section-locally versioned via
  `SUSPENDED_FLOW_SECTION_VERSION`, independent of `SAVE_FORMAT_VERSION`):
  the parked flow's current container `DefinitionId`, its tunnel-return
  stack (`Vec<DefinitionId>`), a name-keyed frame record (an ordinary
  `Value`, so no new wire representation), and a `WakePolicy` (await-site
  id + optional condition fn token + a `WakeSource` host-source
  discriminant). All identity rides name-stable `DefinitionId`s, never
  instruction offsets — the same recompile-stability contract as the rest
  of `SaveState`.
- Round-trip tests per `docs/flow-suspension-spec.md` §7: both
  `WakeSource` variants, the absent/backward-compat case, and a
  frame-shape-drift case proving the name-keyed encoding survives a
  missing/extra/renamed crossing-local between save and load (the
  tolerant *decode* itself is FS-3 scope).

Inert wire growth: this is purely additive surface with no producer yet,
so no existing save or story's observable behavior changes.
