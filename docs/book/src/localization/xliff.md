# XLIFF Workflow

> **Implementation status:** The XLIFF workflow is designed but not yet implemented. The format-level types (`LineTemplate`, `PluralCategory`, etc.) exist in `brink-format`; the CLI commands and runtime integration are planned.

Localization source files will use **XLIFF 2.0** -- one file per locale. Containers are represented as `<file>` elements within the XLIFF document. Brink-specific metadata (content hashes, audio asset references) uses XLIFF's custom namespace extension mechanism.

## Why XLIFF

Every major translation management platform (Lokalise, Crowdin, etc.) natively imports/exports XLIFF, and the spec requires tools to preserve unknown extensions -- brink-specific metadata survives round-trips through external tooling.

## Planned workflow

1. **Generate**: `brink generate-locale` reads `.inkb` and extracts all translatable lines into an XLIFF file, organized by container with context annotations for translators.

2. **Translate**: Work in the XLIFF directly or import into a TMS. Audio asset references can be attached via the `brink:audio` extension attribute. Translation state is tracked via XLIFF's `state` attribute (initial/translated/reviewed/final).

3. **Compile**: `brink compile-locale` reads translated XLIFF and produces a binary `.inkl` overlay file.

4. **Regenerate**: On source changes, diffs the new `.inkb` against the existing XLIFF by `LineId`. Preserves human-edited fields (translations, audio refs) and updates machine-managed fields (original text, context). Content hash changes reset the review status.
