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
`with_succession` method plus `transitions`/`templates` fields for
validating succession rows against the projection's declared convention
kinds. The compiler never interprets them (§5 of
`docs/prose-dialect-spec.md`, "ignored by the compiler"); per the
2026-08-05 ruling *"Succession is EDITOR-OWNED and externally defined"*
(PR #2304), they stay in-process validator state and are never carried
into a serialized wire shape.

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

Scope fence held: this is validator-only, in-process state — it never
travels beyond tooling. Actually wiring Tab/Enter succession in CM6 stays
held as editor-frontend work (NS-T hold, 2026-08-01 sequencing ruling).
