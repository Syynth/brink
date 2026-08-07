---
"@brink-lang/web": patch
---

Editor: native (`.brink`) files now get inlay hints, color hints, and
argument widgets (issue #2359).

#2291 (PR #2358) fixed folding and `line_contexts` to route `.brink` files
through the native CST instead of always ink-parsing the source text
regardless of extension, but left `inlay_hints_impl`/`color_hints_impl`/
`argument_widgets_impl` returning `[]`/`null` for a native file — there was
no native-CST equivalent of the underlying `brink-ide` passes yet. This adds
`brink_ide::inlay_hints::inlay_hints_native`,
`brink_ide::color::color_hints_native`, and
`brink_ide::argument_widgets::argument_widgets_native` (mirroring the ink
passes' shape over `brink_syntax_native::SyntaxKind`) and wires the
`is_native` dispatch in `crates/brink-web`, the same pattern #2358 used for
folding. A `.brink` file now gets real inlay hints, color-picker hints, and
argument-widget slot data instead of silently none at all.
