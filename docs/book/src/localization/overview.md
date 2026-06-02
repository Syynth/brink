# Localization Overview

Brink separates executable logic from localizable text. The bytecode is locale-independent -- all user-visible text is referenced via a `(DefinitionId, u16)` pair: a scope-relative index into the lexical scope's (knot / stitch / root) line table. Locale-specific content lives in `.inkl` overlay files that replace line content per scope.

> **Implementation status:** Shipped end-to-end. Line templates, plural categories, and select keys live in `brink-format`; `.inkl` loading + plural-aware rendering are in `brink-runtime` (`apply_locale`, the `PluralResolver` trait); the CLI tooling is `export-xliff` / `compile-locale` / `regenerate-xliff`; `brink-intl` provides the library API and the `IcuPluralResolver`. The `bevy-brink` integration adds runtime locale switching (see the Bevy Integration section).

## Design principles

- **Bytecode is locale-independent.** `EmitLine(2)` always means "line 2 of this scope's table" -- the VM never sees text directly.
- **Text lives in line tables, not in the instruction stream.** This allows line content to be replaced without recompiling bytecode.
- **`.inkl` overlays replace line content per-scope** without touching bytecode or control flow.
- **Plural and gender logic lives in the line template**, not the VM. Translators can restructure sentences, reorder interpolation slots, and alter plural forms per locale.
- **Voice acting and text localization share a single `LineId` addressing scheme.**

## The `.inkl` overlay format

A decoded `.inkl` is a `LocaleData { locale_tag, base_checksum, line_tables }`:

- BCP 47 locale tag and the base `.inkb` checksum (so a mismatched overlay is rejected before it can render garbage).
- Per-scope line tables (`LocaleScopeTable`) keyed by scope `DefinitionId`.
- Application mode is the caller's choice (`LocaleMode::Overlay` falls back to base text for untranslated scopes; `LocaleMode::Strict` requires a full translation) — only scopes present in the overlay are replaced.
