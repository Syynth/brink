---
"@brink-lang/web": patch
---

A `claims` clause inside `@[element(…)]`, the spelling from before
`@[convention]` existed, still reports E159, but the message now says where
it moved: `@[convention(claims = "…", order = N)]` for a whole-line claim,
or `@[element(args = "…")]` for a called element (#2181).
