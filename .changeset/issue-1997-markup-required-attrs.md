---
"@brink-lang/web": patch
---

#1997 (ruled 2026-08-01, closing #1780): the host capability manifest's
`markup` section gains a required-attribute flag, and its `attrs` schema
widens to make room for typed attribute values later without another
breaking change.

- **(a) Required attributes.** Each declared attribute
  (`ManifestSpanKind.attrs`) can now carry `required: true`. A span of a
  declared kind that omits one of that kind's required attributes reports
  the new `E173`, gated the same way `E164`/`E165` already are (only for a
  span whose name the manifest declares, one diagnostic per missing
  attribute) and defaulting to `Warning` for the same `[lints]`-
  configurability reason.
- **(b) Widened attribute schema — headroom, not typing.**
  `ManifestSpanKind.attrs` moves from `Vec<String>` (bare attribute names)
  to `Vec<ManifestSpanAttr>` (`{ name, required }`, plus a reserved,
  currently-inert `ty` slot). **This is schema headroom only — attribute-
  value typing is NOT implemented.** Span attribute values stay static text
  by construction; the reserved slot exists only so a future PR that adds
  typing needs a new check, not another manifest shape change.

**This is a breaking wire-format change to the `markup` section itself**,
observable through `@brink-lang/web`'s `EditorHandle.setHostManifest` /
`ManifestSpanKind`/`ManifestSpanAttr` TS types: a bare attribute-name array
(`"attrs": ["amount"]`) is no longer accepted — hosts must migrate to
`"attrs": [{ "name": "amount" }]`. See `docs/host-capability-manifest.md`
§ "Markup vocabulary" for the updated shape.

Oracle ratchet unaffected (tooling/author-time manifest validation only,
never consumed by the runtime or codegen).
