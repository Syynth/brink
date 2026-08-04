---
"@brink-lang/web": patch
---

`brink-ir`: a labeled choice/gather/block nested inside a
content-embedded inline conditional/sequence (`{if …}`/`{~ …}` etc.
sharing a line with prose, not on its own line) now keeps the same
same-file-preferred label lookup issue #2197/#2213 already gave the
primary weave walk (issue #2215). This also covers the same shape
reached transitively through a block-capture's own captured plain
content line — a top-level labeled container can never itself be
absorbed into a block-capture (issue #1839's `is_plain_content_line`
terminator stops the captured run at any `CONTENT_LINE` carrying a
`LABEL`/`CHOICE_POINT`/`DIVERT_STMT`/`TUNNEL_CALL`), so the only way a
label reaches `Expr::Fragment` is via a captured line's own mid-line
inline conditional/sequence — the same shape as above, just nested one
level deeper.

`stamp_lambdas_in_expr`'s `Fragment` arm and
`stamp_lambdas_in_content_part`'s `InlineConditional`/`InlineSequence`
arms used to call `lookup_label_id` with `file: None` — the pre-#2197
unscoped lookup. When two declared modules legitimately coexist with a
same-named flow (M-2d, e.g. the stdlib mount alongside a project's own
declarations) and each nests an identically-labeled choice inside such a
construct, the unscoped lookup could silently prefer the wrong file's
`DefinitionId`, colliding two distinct containers onto one id — the same
`[E060] internal codegen error: duplicate DefinitionId` class #2197 fixed
elsewhere, reachable this time only through the lambda-stamping
traversal. `brink-web` transitively depends on `brink-ir`, so this is
wasm-observable for any `.brink`/`.ink` source reaching this shape.
