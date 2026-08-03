---
"@brink-lang/web": patch
---

Analyzer: a UFCS-shaped call into an `EXTERNAL`/`extern` target is now
argument-checked through the db-backed inference path, not just the pure
`infer_project` path (issue #1921).

`brink_analyzer::solve_scc`'s `known_sigs` was already seeded with each
`EXTERNAL`'s declaration-derived signature (issue #786), but externals are
never members of any SCC `batch`, so the `signatures` map `solve_scc`
*returned* was filtered down to exactly `batch`'s own members — an
external's seeded signature never survived past that one call.
`brink-db`'s `type_inference_query` aggregates every SCC's own
`SolvedScc::signatures` into the project-wide `InferenceResult::signatures`
that `ufcs::check_ufcs_arg_types` reads, so on the db-backed path (the CLI,
LSP, and `@brink-lang/web` all run through this) that lookup always missed
for an `EXTERNAL` target, and a UFCS call's argument types went completely
unchecked there — even though the identical mismatch, through the pure
`infer_project` path, was already caught (that path's `solve_batches`
sibling returns `known_sigs` wholesale, no batch filter). The direct-call
spelling of the same call was unaffected either way (it reads
`ctx.known_sigs`, not `InferenceResult::signatures`).

```brink
extern set_volume(level)

fn get_name() {
  return "loud";
}

fn total() {
  let n = get_name();
  n.set_volume();
}
```

used to report zero diagnostics under `types = strict` on the db-backed
path; it now reports `E063` there too, matching the pure path and the
direct-call spelling `set_volume(n)` already did.
