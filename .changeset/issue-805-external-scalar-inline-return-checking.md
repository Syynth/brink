---
"@brink-lang/web": patch
---

Issue #805 (PR #794 / issue #786 lineage): widens `EXTERNAL` call-site
checking under `types = strict` to three cases the wave-10 reconciliation
flagged as missing:

1. **Scalar semantic types from the manifest vocabulary** — a binding
   declared to take a manifest-registered scalar semantic type (e.g.
   `switch_id`, `base: int`) now rejects a mismatched literal type
   (`string`) at compile time, not just `handle<K>` kinds.
2. **Inline-doc-only externals** — an `EXTERNAL` documented purely via an
   inline `///` `@param`/`@returns` doc comment, with no matching
   `ManifestExternal` entry in the registered manifest, now gets a checked
   signature too (previously disclosed as out of scope by PR #794).
3. **Return-position kind checking** — a binding's own declared *return*
   type (handle or scalar) now flows into and is checked at its call site's
   usage, not just its declared param types.

Mechanism: `infer::collect_external_sigs` now merges a binding's declared
signature from both sources (inline doc wins by param name, else the
registered manifest entry wins by position — the same merge order
`external_check::analyze_externals` already uses for its own enrichment),
and resolves every param/return `TypeRef` against the full registered
`SemanticTypeDef` table (scalar bases in addition to `handle<K>` kinds).
Mismatches fold to the pre-existing `Ty::Conflicted` lattice point and
report through the existing `E066` diagnostic — no new diagnostic code.

Observable through `@brink-lang/web`: under `types = strict`
(`IdeSession.set_type_policy("strict")`) with a registered `HostManifest`
(`setHostManifest`), a call site now reports `E066` for (1) a literal
mismatching a binding's declared scalar semantic type, (2) a cross-kind
argument to an inline-doc-only binding, or (3) a caller local receiving a
binding's declared return kind and later used against a conflicting kind —
none of which previously reported anything. `types = gradual` is
unaffected — byte-identical.

Oracle ratchet unchanged (5,577 episodes, byte-identical) — analyzer/
diagnostic surface only, no compiler/codegen change reachable by vanilla
ink, so this is oracle-inert by construction, same as #786.
