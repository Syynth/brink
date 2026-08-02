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

Migration: a `.brink` file whose tag text contains a recognized escape
(`\#`, `\{`, `\<`, or a bare `\\`) and depends on the backslash surviving
into the rendered tag will see it disappear (a bare pair collapses from
two backslashes to one). To keep one literal backslash immediately before
a literal `#`/`{`/`<` in the *same* tag, use three backslashes (e.g.
`\\\#`, not `\\#` or `\\\\#`): the odd count is what keeps the following
character from ending the tag early (unchanged structural parity,
#1738/#1852), and the materialized text then collapses the leading pair to
one backslash while the trailing backslash escapes the final character —
`#tag \\\#more` renders as `tag \#more`. An even count (`\\#`, `\\\\#`, …)
still ends the tag at that unescaped character exactly as before this fix,
splitting into a new sibling tag instead. This matches what ordinary
content already requires for the same effect.
