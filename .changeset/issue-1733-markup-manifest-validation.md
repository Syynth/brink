---
"@brink-lang/web": patch
---

Compiler: host-manifest validation of the inline markup vocabulary — the
second half of `docs/prose-dialect-spec.md` §4.2 (issue #1733), completing
what PR #1732 landed as freeform-only.

The host capability manifest gains a `markup` section: an array of
`{ name, attrs }` span kinds declaring which `<tag attr="v">…</tag>` names
a project may use and which attributes each accepts. It sits beside
`externals` because the markup vocabulary is host-authored and can be
generated from engine code (a text-effect plugin declaring its own tags),
by §3.4's authorship test — element conventions are project-authored and
live on a different surface.

**Freeform stays the default.** With no manifest — and with a manifest
that declares only `externals`/`types` — markup is never diagnosed, exactly
as before. Declaring at least one span kind is the only thing that turns
checking on. Two new codes then reach a project's diagnostics: `E164` for
an undeclared tag, `E165` for an undeclared attribute on a declared kind.
Both default to `Warning`, which is what makes their severity configurable
(`[lints] E164 = "deny"` to make a vocabulary binding, `@[allow(E164)]` or
`// brink-disable E164` to silence it locally) — a hard-error code would be
neither overridable nor suppressible.

Web-observable through `EditorHandle.setHostManifest(json)`: a manifest
JSON carrying a `markup` key now takes effect, and the resulting `E164`/
`E165` warnings appear in the background analysis the editor renders and in
`compile()`'s `warnings` array. Attribute *values* are unchecked — they are
static text by construction, so only attribute names are vocabulary.
