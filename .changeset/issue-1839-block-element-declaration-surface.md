---
"@brink-lang/web": patch
---

Compiler: `@[element(…, block)]` declaration-surface parsing and
validation (issue #1839, `docs/decision-log.md` 2026-07-31 "Conventions
are annotated handlers").

`@[element(args = "…", block)]` declares that the annotated handler
captures the run **following** its matched line into a trailing
`content`-typed parameter — the ruled block-capture contract. This PR
delivers only the declaration surface, matching the precedent #1719 set
for `element`/`style`: `ElementAnnotation` gains a `block: bool` field,
and a `block`-flagged declaration with no qualifying trailing
`content`-typed parameter (or one that collides with a named capture)
raises the new `E166` (`Error` by default, so not `[lints]`-configurable
or `@[allow]`-suppressible, matching `E159`/`E160`).

**Not delivered here:** the `!name`/natural-notation dispatch rewrite that
actually matches a line, finds a block's terminator (a blank line or any
element-level line), captures the following run as a `Value::FragmentRef`,
and calls the handler — that is issue #1838's natural-notation dispatch,
not yet landed, and this PR does not invent it. See the tracked remainder
on issue #1839.

**Not usable end-to-end yet, even for the declaration surface alone:**
`content` is not a recognized annotation type name
(`brink_analyzer::annotations::is_known_leaf`), so under `dialect = brink`
(the dialect brink-lsp and brink-web resolve from `brink.toml`) the
qualifying trailing parameter `E166` requires — literally annotated
`content` — itself raises `E061` on the same compile. A `block`-flagged
declaration parses and validates cleanly (no `E166`), but compiling it
under the brink dialect still fails, on `E061`, until `content` joins the
annotation vocabulary (a separate, not-yet-filed ruling). See
`docs/diagnostics/E166.md`'s note and the regression test
`e166_block_declaration_surface_parses_but_content_param_still_trips_e061`
(`crates/brink-compiler/tests/e0xx_diagnostics.rs`).

Web-observable through `EditorHandle.compile()`/background analysis: a
`.brink` file with a `block`-flagged `@[element(…)]` annotation now
lowers `block` onto its `ElementAnnotation` instead of falling through
unrecognized, and a malformed one surfaces the new `E166` diagnostic
alongside the existing `E159`/`E160` codes on that same channel.
