---
"@brink-lang/web": patch
---

`E110` (the deprecated `#@effects(…)` tag-channel spelling) now offers a
`Safe`, batchable auto-fix that rewrites the tag to the `@[effects(…)]`
annotation spelling — translating the argument list from the legacy colon
grammar to the annotation's paren-clause grammar, so the definition's
inferred effect row is unchanged. Available through the Problems panel and
`brink fix`. No fix is offered for a dynamic tag, a bare tag with no
argument list, or a tag whose argument list fails to parse.
