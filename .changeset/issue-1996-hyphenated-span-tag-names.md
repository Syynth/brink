---
"@brink-lang/web": patch
---

Native `.brink` prose-dialect markup: span tag names may now contain `-` as
an internal separator (`<fade-in>`), issue #1996 (RULED 2026-08-01,
`docs/prose-dialect-spec.md` §4.1). Both the open (`<fade-in>`) and close
(`</fade-in>`) forms are supported; a leading or trailing hyphen (`<-x>`,
`<x->`) is still a parse error. This is scoped to span-tag position only —
plain identifier lexing elsewhere in the language is unchanged. Before this
fix, a hyphenated tag name failed to parse (`expected GT, found MINUS`).
