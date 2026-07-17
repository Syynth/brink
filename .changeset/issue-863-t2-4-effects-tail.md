---
"@brink-lang/web": patch
---

T2-4: effects tail — book "Effects" chapter, IDE hover, `brink ide
effects-diff`, tier1-brink corpus wing (docs/effects-spec.md §10, issue
#863, tracked from #859). Builds on T2-1..3's inference/assertion/emission
substrate (#860–#862).

- **Hover** shows a knot/stitch's inferred effect row alongside its
  signature and docs — `reads …; writes …; calls …`, `pure`, or `opaque`
  (a call through a function value, or an unresolved callee) — resolved
  through the wasm analysis pipeline `@brink-lang/web`'s editor consumes,
  so this is editor-observable.
- **`brink ide effects-diff --base REV [--head REV]`** (CLI-only, not
  wasm-exposed): diffs every def's inferred row between two git revisions,
  or a revision and the working tree — the sitting-2 ruling's answer to
  "what about drift?" (no lockfile; the shipped rows already are the
  frozen record). Exit `0` on no drift, `1` on any change (unix-`diff`
  shaped), `2` on usage error; `--format json` for CI comments.
- **Book**: a new "Effects" dialect chapter (compile-checked `ink`/`text`
  examples, same convention the Types/Function-Values chapters
  established), documented in `brink ide`'s CLI reference.
- **Corpus**: a `tests/tier1-brink/effects-assertions-clean` case plus
  full-pipeline (`compile_with_options`) tests for a satisfied `calls:`
  assertion and the `E103` exceedance error, exercising inference +
  assertions + exceedance end to end (not just through `ProjectDb`
  directly, the way T2-2's own test suite already does).

Oracle byte-identical (5,577 episodes unmoved) — nothing here touches
codegen, format, or runtime; this is hover/CLI/docs/test surface only.
