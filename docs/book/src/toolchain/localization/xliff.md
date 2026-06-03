# XLIFF Workflow

Localization source files use **XLIFF 2.0** — one file per locale. Lexical scopes (knots/stitches/root) map to `<file>` elements within the XLIFF document. Brink-specific metadata (content hashes for change tracking) uses XLIFF's custom namespace extension (`brink:`, see `BRINK_NS` in `brink-intl`), which conforming tools preserve across round-trips.

The workflow is shipped end-to-end: the `brink` CLI exposes `export-xliff`, `compile-locale`, and `regenerate-xliff`, and `brink-intl` exposes the same operations as a library (`generate_locale`, `compile_locale_xliff`, `regenerate_locale`).

## Why XLIFF

Every major translation management platform (Lokalise, Crowdin, etc.) natively imports/exports XLIFF, and the spec requires tools to preserve unknown extensions -- brink-specific metadata survives round-trips through external tooling.

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
