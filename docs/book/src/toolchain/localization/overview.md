# Localization

brink separates executable logic from localizable text. The bytecode is
locale-independent — all user-visible text is referenced by a
`(DefinitionId, u16)` pair: a scope-relative index into a lexical scope's
(knot / stitch / root) line table. Locale-specific content lives in `.inkl`
overlay files that replace line content per scope, without touching bytecode.

> **Status:** shipped end-to-end. Line templates, plural categories, and select
> keys live in `brink-format`; `.inkl` loading + plural-aware rendering are in
> `brink-runtime` (`apply_locale`, the `PluralResolver` trait); the CLI exposes
> `export-xliff` / `compile-locale` / `regenerate-xliff`; `brink-intl` provides
> the library API and `IcuPluralResolver`. `bevy-brink` adds runtime locale
> switching — see [Bevy › Localization & Saves](../../integrations/bevy/localization.md).

## Design principles

- **Bytecode is locale-independent.** `EmitLine(2)` always means "line 2 of this
  scope's table" — the VM never sees text directly.
- **Text lives in line tables, not the instruction stream**, so content can be
  replaced without recompiling bytecode.
- **`.inkl` overlays replace line content per scope**, never control flow.
- **Plural and gender logic lives in the line template**, not the VM —
  translators can restructure sentences, reorder slots, and change plural forms
  per locale.
- **Voice acting and text localization share one `LineId` addressing scheme.**

## How the pieces fit

| You want to… | See |
|--------------|-----|
| Extract, translate, and compile a locale | [XLIFF Workflow](./xliff.md) |
| Understand plural categories and resolvers | [Plurals](./plurals.md) |
| Know what a line template can express | [Reference › Line Templates](../reference/line-templates.md) |
| Read the `.inkl` byte layout | [Reference › Binary Format](../reference/format.md) |

The translation pipeline is always `.ink` → compile → `.inkb` →
`export-xliff` → `.xlf`. Never feed inklecate `.ink.json` into the intl
tooling — those files are kept only for oracle regeneration.
