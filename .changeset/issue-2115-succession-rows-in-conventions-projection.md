---
"@brink-lang/web": patch
---

Issue #2115 (NS-T seam 5/6, backported design from #2111–#2115's
2026-08-03 "Conventions × the editor" ruling): `DialogueDialect`
(#368)'s surviving `transitions`/`templates` fields — Tab/Enter/Shift-Tab
succession rows and template/picker metadata, the editing-time dual of
chain rules — now **re-key against declared convention kinds instead of
carrying an independent element list**, and `brink_ir::ConventionsProjection`
(the compiler's `@[convention]`-handler projection) gains a
`with_succession` method plus `transitions`/`templates` fields to carry
them through to a serialized wire shape. The compiler transports these
rows; it never interprets them (§5 of `docs/prose-dialect-spec.md`,
"ignored by the compiler").

- **Observable behavior change, `set_dialect`:** `brink-web`'s
  `set_dialect(json)` calls the same `brink_ir::dialect::validate` this
  slice extends — a `DialogueDialect` JSON payload whose `templates`
  array names a `kind` that `elements` never declared (and that isn't a
  reserved structural kind) is now rejected with a `JsError`
  (`DialectError::TemplateUndeclaredKind`), where it previously validated
  silently. `transitions` was already checked this way (reported as
  `DialectError::TransitionUndeclaredKind`); `templates` was not — this
  closes that gap for both callers of the shared `validate_succession`
  helper at once, each kind of row now reported under its own error
  variant.
- **New API surface (brink-ir):** `ConventionsProjection::with_succession`,
  `dialect::validate_succession`, and `dialect::reserved_structural_kinds`
  are now exported from the crate root alongside `Templates`,
  `TemplateEntry`, `TransitionAction`, `TransitionRow`.
- **Wire shape (brink-format):** `ConventionsProjectionDef` gains
  `transitions`/`templates` fields (`TransitionRowDef`, `TransitionActionDef`,
  `TemplatesDef`, `TemplateEntryDef`), and `CONVENTIONS_PROJECTION_WIRE_VERSION`
  bumps `1` → `2`. Nothing has emitted a version-`1` payload into a real
  `.inkb`/`StoryData` file yet (this section is still not wired into
  `crate::StoryData` — see `brink_format::conventions`'s own module doc),
  so the bump orphans no on-disk data; it exists so a future reader can
  tell the two shapes apart.

Scope fence held: this is transport only. Actually wiring Tab/Enter
succession in CM6 stays held as editor-frontend work (NS-T hold,
2026-08-01 sequencing ruling).
