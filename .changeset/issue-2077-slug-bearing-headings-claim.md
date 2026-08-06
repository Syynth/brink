---
"@brink-lang/web": patch
---

Issue #2077: a scene heading's `@[convention(claims = "…")]` pattern now
matches even when the heading carries an explicit `[slug]` and/or trailing
`#tag`s — before this fix, the compiler's natural-notation claim dispatch
(`hir::lower_native::element::candidate`) declined a slug- or tag-bearing
heading outright, so a preset's `heading` handler could never claim any of
`docs/prose-dialect-spec.md`'s own worked-page examples (every one of them
spells an explicit slug).

- The pattern still only ever sees the heading's title text — the
  `[slug]`/`#tag`s are stripped before matching, not appended to it, so no
  existing preset pattern needs to change.
- The slug is now captured and delivered on `HirFile::element_matches` as
  a reserved capture (`ElementMatch::slug`) — tooling-visible, but not
  wired into the rewritten call (that remains heading→stitch promotion,
  issue #2078, a separate unowned issue).
- The heading's own trailing tags now reach `Content.tags`, the same
  channel any other tagged line already uses, instead of being silently
  dropped once a slug/tag-bearing heading became claimable.
- The built-in screenplay preset (`std/conventions/screenplay.brink`,
  mounted into every compiled project's `Environment` manifest since
  #2080) is directly affected: its `heading` handler can now claim a
  slugged heading end to end (`scene_entered`'s `slug` argument stays an
  empty string either way — wiring the captured slug into that call is
  #2078's territory, not this fix's).

This changeset is filed because the claim/decline behavior change is
compiler-level (`brink-ir`), and `@brink-lang/web` re-exports it through
every native compile that runs a project with `@[convention(claims = …)]`
handlers.
