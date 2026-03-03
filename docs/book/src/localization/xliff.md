# XLIFF Workflow

Localization source files use **XLIFF 2.0** — one file per locale. Containers are represented as `<file>` elements within the XLIFF document. Brink-specific metadata (content hashes, audio asset references) uses XLIFF's custom namespace extension mechanism.

## Why XLIFF

Every major translation management platform (Lokalise, Crowdin, etc.) natively imports/exports XLIFF, and the spec requires tools to preserve unknown extensions — brink-specific metadata survives round-trips through external tooling.

## The workflow

<!-- TODO: detail the four-step workflow:
  1. Generate: `brink generate-locale` reads .inkb → XLIFF with all translatable lines
     - Organized by container
     - Includes context annotations for translators
  2. Translate: work in XLIFF directly or import into TMS
     - Audio asset references via `brink:audio` extension attribute
     - Translation state tracking via XLIFF `state` attribute (initial/translated/reviewed/final)
  3. Compile: `brink compile-locale` reads translated XLIFF → binary .inkl overlay
  4. Regenerate: on source changes, diffs new .inkb against existing XLIFF by LineId
     - Preserves human-edited fields (translations, audio refs)
     - Updates machine-managed fields (original text, context)
     - Content hash detects changed lines → resets review status
-->

## CLI commands

<!-- TODO: document the localization CLI commands once implemented:
  - `brink generate-locale` — generate XLIFF from .inkb
  - `brink compile-locale` — compile translated XLIFF to .inkl
-->

## Implementation status

The localization architecture is fully specified but implementation is deferred to post-tier-3. Format types (`LineTemplate`, `PluralCategory`, etc.) are available in `brink-format`; the line resolver, `.inkl` loading, XLIFF tooling, and `brink-intl` come later.
