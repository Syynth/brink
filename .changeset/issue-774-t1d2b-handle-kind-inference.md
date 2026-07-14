---
"@brink-lang/web": patch
---

T1d-2b (#774): threads the registered `HostManifest`'s handle-kind
vocabulary through `infer_project`/`solve_scc` (and `brink-db`'s FG-2
`signature_query`/`solve_scc_query` salsa substrate) into inference —
`docs/t1d-spec.md` §3's remaining gap, disclosed as deferred in T1d-2
(#767, PR #769). `handle<K>` param/return/temp annotations now resolve to
`Ty::Handle(K)` during body-usage inference, not just at the
`signature()`/annotation-firewall seam.

Observable through `@brink-lang/web`: under `types = strict` with a
registered `HostManifest` declaring two or more handle kinds, a genuine
cross-kind handle mismatch detected purely from body-usage inference (e.g.
two locals of different declared handle kinds compared or reassigned
together, with neither side's slot independently exempted by its own
annotation) now reports `E066` (Conflicted-escape) — reusing the existing
TM-3 machinery, no new diagnostic code. This is the #767 acceptance
criterion ("binding declared `handle<AudioInstance>` rejects
`handle<Timer>` at compile time") becoming reachable end-to-end. `types =
gradual` is unaffected — TM-1 inference stays advisory-only there,
byte-identical.

Oracle ratchet unchanged (5,577 episodes, byte-identical) — vanilla ink has
no handles by construction, so this is oracle-inert.
