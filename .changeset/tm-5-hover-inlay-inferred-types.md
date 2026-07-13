---
"@brink-lang/web": patch
---

TM-5 (#621, docs/typed-mode-spec.md §9 step 5): hover and inlay hints now
surface *inferred* types, not just declared/annotated ones, through the
FG-narrowed per-def seam (`ProjectDb::inferred_signature`/`infer_body` —
never the whole-project `type_inference()`).

Hover: a `temp` or parameter with no annotation now shows its inferred
type (`` `x: int` ``) instead of nothing; an unannotated knot/stitch
header falls back to its inferred signature for any param/return position
a TM-2 inline annotation or doc-tag doesn't already cover. A declared
annotation (TM-2 `name: type`, or a `///` doc-tag/host-manifest type for
externals) always wins over inference — the firewall rule — and an
`Unknown`/unresolvable inferred type shows nothing rather than noise.

Inlay hints: a new `InferredType` kind renders an inferred-type ghost
label (`: int`) right after an unannotated `~ temp name = …` declaration;
an explicit `: type` ascription suppresses it (already visible in the
source). Exposed through `@brink-lang/web`'s `inlay_hints`/`hover` JSON as
`"inferred_type"` and the existing hover content string respectively; the
LSP maps it to the standard `TYPE` inlay-hint kind (previously every hint
defaulted to `PARAMETER`).

`brink_ide::hover::hover` and `brink_ide::inlay_hints::inlay_hints` both
gained a `&ProjectDb` (plus `FileId` for `inlay_hints`) parameter to reach
the per-def queries — an internal API change to `brink-ide`, `brink-lsp`,
and `brink-web`'s wasm bridge, not a `.inkb`/runtime change. Boundary-
annotation quick-fix is explicitly out of scope (#657, parked).
