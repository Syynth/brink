---
"@brink-lang/web": patch
---

Issue #1719's remaining scope: a native `@[style(...)]` declaration is now
readable through the shared `brink_ide::hover::hover` query — hovering a
knot/stitch that carries one appends a `**style**` line rendering its
entries (`key = "value"`, built-in tokens spelled from the closed
vocabulary, `Custom`/color values shown as-written). `StyleToken` was
previously produced by `hir::lower_native::annotation` and read by
nothing; this is the compiler-side query half only — no CSS class, no
semantic-token modifier, no buffer decoration is produced. Observable
through `@brink-lang/web`'s editor hover, brink dialect only (`.ink` files
never populate `style_annotation`).
