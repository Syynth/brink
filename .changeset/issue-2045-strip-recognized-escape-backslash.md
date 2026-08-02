---
"@brink-lang/web": patch
---

Issue #2045: a *recognized* inline escape (`\< \{ \# \\`, §8d.6) now
strips its backslash from a tag's rendered text, in parity with ordinary
content — this is a **breaking change** for any `.brink` file relying on
the backslash surviving into rendered tag text.

`content::tag()`'s raw free-text scan already gave `\#`/`\{` *structural*
recognition (#1738/#1852: an escaped `#`/`{` doesn't end the tag early),
but never stripped the backslash from the tag's own materialized text —
so `Hello \# world #a \#b` produced `Hello # world` for the content line
(backslash stripped, via `markup::escape`) but `a \#b` for the trailing
tag (backslash retained) — two different treatments of the same escape on
one line. Fixed at the materialization point (`ast::Tag::text()`, shared
by `hir::lower_native::body::lower_tag`): a recognized escape's backslash
is now stripped there too, so the tag observed through `Line::Text`'s
`tags` field reads `a #b`.

Migration: a `.brink` file whose tag text contains `\#`, `\{`, `\<`, or
`\\` and depends on the backslash surviving into the rendered tag will see
that backslash disappear. Rewrite `\\#` (a literal backslash you want to
keep) as `\\\\#` if you need the backslash itself to survive, matching
what ordinary content already requires.
