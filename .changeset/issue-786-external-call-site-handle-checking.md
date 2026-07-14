---
"@brink-lang/web": patch
---

Issue #786 (T1d follow-up): extends the strict call-checking machinery to
`EXTERNAL` binding call sites — a manifest-registered binding declared to
take `handle<AudioInstance>` now rejects a `handle<Timer>` argument at
compile time, closing the last disclosed gap from T1d-2 (#767, PR #769)
and T1d-2b (#774, PR #779): those two slices covered a *local-vs-local*
handle-kind mismatch found by body-usage inference, but not a *binding's
own declared param* vs. a call-site argument.

Mechanism: `infer::collect_external_sigs` resolves each manifest-registered
`EXTERNAL`'s declared parameter/return types to `Ty` (handle kinds via the
same `declared_handle_kinds` vocabulary `handle<K>` annotations already
resolve against) and seeds them into `known_sigs` before body inference
runs — a call to the binding now types its arguments through the exact
same `known_sigs`/`observe`/`unify` path an ordinary knot/stitch call
already uses. A cross-kind argument folds to the pre-existing `Ty::Conflicted`
lattice point and reports through the existing `E066` (Conflicted-escape)
diagnostic — no new diagnostic code.

Observable through `@brink-lang/web`: under `types = strict` (`IdeSession
.set_type_policy("strict")`) with a registered `HostManifest` (`setHostManifest`)
declaring two or more handle kinds and at least one `EXTERNAL` binding whose
manifest entry declares a handle-kinded param, a call site passing an
argument of a *different* declared handle kind now reports `E066` where it
previously reported nothing. `types = gradual` is unaffected — the existing
runtime fault at the binding boundary stays the only enforcement there,
byte-identical. An `EXTERNAL` with no matching registered manifest entry
(inline-doc-only) stays unchecked, same as before this issue.

Oracle ratchet unchanged (5,577 episodes, byte-identical) — analyzer/
diagnostic surface only, no compiler/codegen change reachable by vanilla
ink (no handles by construction), so this is oracle-inert by construction.
