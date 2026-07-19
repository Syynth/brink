---
"@brink-lang/web": patch
---

F34 + F35 (ruled 2026-07-19): the comparator write-guard, keyed on
`ExecMode`, plus bevy-brink's profile-defaulted mode. Observable through
`@brink-lang/web`, brink dialect only (vanilla ink cannot reach a
`sort_by`/`sorted_by` comparator):

- **F34 — comparator write-guard.** In the re-entrant comparator runner
  (the `sort_by`/`sorted_by` value-call boundary), a WORLD-WRITE performed
  by a comparator mid-sort now faults under `ExecMode::Dev` with the new
  tracked fault `ComparatorWroteState` (sibling to
  `ComparatorNotAFunction`/`ComparatorReturnType`/`ComparatorEscaped`).
  Under `ExecMode::Prod` the check is skipped — the write executes,
  defined and deterministic, because the stable merge-sort's comparison
  sequence is fixed (the mode changes WHERE execution stops, never WHAT
  the sort produces). World-write = global-var writes (direct, or through
  a `ref`-parameter pointer / path projection) and every RNG-cell advance:
  a `rand` draw inside a comparator IS a world-write and dev-faults — a
  random comparator is exactly the nondeterminism the pure·silent contract
  bans. Reads stay legal at runtime (E119's static bound owns the read
  posture — no runtime read-guard), and visit-count increments from the
  comparator's own in-story dispatch are NOT world-writes (explicitly
  exempt). This is the gradual-mode runtime residual of the E119 gate,
  reached only by an opaque comparator whose origin the checker cannot
  prove.

- **F35 — bevy-brink profile-defaulted `ExecMode`.** Core `brink-runtime`
  keeps `ExecMode::default() == Dev`. Where `bevy-brink` spawns a flow it
  now stamps a host-selected mode whose default keys off the build
  profile: `Dev` under `debug_assertions`, `Prod` in a release build — so
  a shipped game defaults to keep-moving and an in-editor session to
  fault-loud. Carried by the new `BrinkExecMode<M>` resource; a host pins
  a mode regardless of profile via `BrinkPlugin::with_exec_mode`, with a
  per-flow runtime override still available through
  `FlowInstance::set_exec_mode`.

Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.
