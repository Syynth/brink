---
"@brink-lang/web": patch
"@brink-lang/editor": minor
"@brink-lang/studio": patch
---

`[project] indent` is now the single source for indentation width, and the
default when it is unset is **4** (ruled 2026-08-27).

- `brink-fmt` no longer keeps a default of its own — it defaulted to two
  spaces while the editor indented by four, which is exactly the
  disagreement this setting exists to prevent.
- `brink fmt` discovers the `brink.toml` for each file it formats.
- The language server reads the project's width and ignores the client's
  `tabSize`, which would otherwise be a silent second source.
- The editor's `indentUnit` reads the configured width instead of
  hardcoding four spaces; the indent guides follow it automatically.

New: `EditorSessionHandle.getConfiguredIndent()`, and `DEFAULT_INDENT` from
`@brink-lang/editor`.

Also: the status bar no longer says "— file not analyzed" for a draft
(#3145), matching the out-of-scope banner it accompanies.
