---
"@brink-lang/web": patch
---

Editor: folding, inlay/color hints, argument widgets, and line conversion no
longer read a native (`.brink`) file's editor state from ink's mis-parse of
its source text (issue #2291, same defect class as #2280/#2286).

`EditorSession::folding_ranges` now routes the machinery/narrative fold-run
pass through `IdeSession::syntax_root_native` +
`brink_ide::line_context::line_contexts_native`/
`line_contexts_with_dialect_native` for a native file, instead of
`syntax_root`'s always-ink parse.

`inlay_hints`, `color_hints_doc`, and `argument_widgets_doc` walk
`root.descendants()` casting to ink-only typed AST nodes
(`ast::FunctionCall`, `ast::DivertTargetWithArgs`, `ast::TempDecl`) — there
is no native-CST equivalent of that pass yet, so a native file now returns
no hints/widgets rather than ones computed from a mis-cast tree (verified:
ink's parse of a native `-> target(args)` divert-with-args produces a real,
wrong `DIVERT_TARGET_WITH_ARGS` node when the callee happens to resolve in
the project's real symbol index, rendering a plausible-looking but
ink-computed parameter/color/argument-widget hint).

`convert_element`/`convert_element_doc` now return no edit for a native
file: the feature rewrites bare-line `*`/`+`/`-` ink choice/gather sigils,
which have no native equivalent at all (native choices only exist inside an
explicit `{? ... }` choice point) — applying it to a `.brink` file would
write invalid native syntax. `format_document`/`format_document_doc` return
the source unchanged for a native file rather than relying on
`sort_knots_in_source`'s ink-knot-header search coincidentally finding
nothing.
