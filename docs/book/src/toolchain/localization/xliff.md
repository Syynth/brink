# XLIFF Workflow

Localization source files use **XLIFF 2.0** — one file per locale. Lexical scopes (knots/stitches/root) map to `<file>` elements within the XLIFF document. brink-specific metadata (content hashes for change tracking) uses XLIFF's custom namespace extension (`brink:`, see `BRINK_NS` in `brink-intl`), which conforming tools preserve across round-trips.

The workflow is shipped end-to-end: the `brink` CLI exposes `export-xliff`, `compile-locale`, `regenerate-xliff`, and `migrate-xliff`, and `brink-intl` exposes the same operations as a library (`generate_locale`, `compile_locale_xliff`, `regenerate_locale`, `migrate_unit_ids`).

`<unit id>` is keyed on the scope's `DefinitionId` (e.g. `0x0100000000000001:0`), not its display name — this is a canonical, NMTOKEN-safe identifier decoupled from the mutable, non-unique-across-scopes display name, matching the format `brink:scope-id` and `IntlError::InvalidUnitId` already documented. **Unit ids are not literally stable across renames**: a `DefinitionId` is itself a hash of the scope's (qualified) name/path, so renaming or moving a knot/stitch assigns it a new `DefinitionId` and every unit id beneath it changes. The human-readable scope name still rides along as the `name` attribute on `<unit>` (`{scope_name}:{line_index}`) and as the `id` attribute on the containing `<file>`, for translator context.

That churn no longer costs you translations, as long as the rename is **declared**. Annotate it with `#@was(old_name)` and both `regenerate-xliff` and `compile-locale` follow the compiled alias table to rebind the moved scope onto its old translations, instead of treating it as a brand-new scope (or, for `compile-locale`, failing outright). You do not need to run `migrate-xliff` after a rename — rebinding is automatic, and `migrate-xliff` exists only for the one-off migration of `.xlf` files exported before unit ids moved off display names.

One limit to know: `#@was` on a knot records an alias for **that knot only**. A *stitch* inside it is re-keyed too (its qualified name contains the knot's name) but has no alias of its own, so its translations are still orphaned — and adding `#@was(market)` to the stitch does not help, because the old name is qualified with the knot's *current* name and the edge collapses to a no-op. Carrying a whole renamed subtree across is tracked as issue #1671. See the scope-matching rules in `docs/intl-spec.md` for the full set.

## Why XLIFF

Every major translation management platform (Lokalise, Crowdin, etc.) natively imports/exports XLIFF, and the spec requires tools to preserve unknown extensions — brink-specific metadata survives round-trips through external tooling.

## Workflow

The translation pipeline is `.ink` → compile → `.inkb` → `export-xliff` → `.xlf`.
(Always start from a compiled `.inkb`; never feed inklecate's `.ink.json` into
the intl tooling.)

1. **Export**: extract every translatable line from a compiled story into an
   XLIFF file, organized by scope with context for translators.

   ```sh
   brink export-xliff story.inkb --src-lang en --trg-lang es -o story.es.xlf
   ```

2. **Translate**: work in the `.xlf` directly or import it into a TMS
   (Lokalise, Crowdin, …). Translation state rides XLIFF's `state` attribute
   (`initial`/`translated`/`reviewed`/`final`).

3. **Compile**: turn the translated XLIFF into a binary `.inkl` overlay.

   ```sh
   brink compile-locale --base story.inkb --xliff story.es.xlf --locale es -o story.es.inkl
   ```

4. **Regenerate**: after the source changes and you recompile, diff the new
   `.inkb` against the existing XLIFF — preserving human translations while
   updating machine-managed fields (original text, context). Content-hash
   changes flag entries whose source moved.

   ```sh
   brink regenerate-xliff --base story.inkb --existing story.es.xlf -o story.es.xlf
   ```

Load the resulting `.inkl` at runtime with `brink_runtime::apply_locale`, or in
Bevy via the locale-switching API (see the Bevy Integration section).

## Migrating archived `.xlf` files

`.xlf` files exported before the scope-id-based unit id scheme landed carry
display-name-based unit ids (e.g. `intro:0` instead of
`0x0100000000000001:0`). `brink regenerate-xliff` already re-keys them for
free the next time you recompile (it rebuilds the document from the fresh
export and overlays translations by content hash, never by unit id). If you
need to re-key an archived `.xlf` without recompiling — for example to push
it back through a TMS that indexes on unit id before your next source
change — use `migrate-xliff`:

```sh
brink migrate-xliff story.es.xlf -o story.es.xlf
```

This only rewrites the `id` attribute on each `<unit>`; `<source>`,
`<target>`, `state`, and every `brink:*` extension attribute are left
untouched, so no translation is lost. It's idempotent — running it on a file
that's already on the new scheme is a no-op.
