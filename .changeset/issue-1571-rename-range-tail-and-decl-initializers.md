---
"@brink-lang/web": patch
---

IDE: qualified references no longer collapse when their target is renamed,
and references written inside `VAR`/`CONST` initializers are narrowed like
every other reference (issue #1571, variants 4–5 of the whole-path
`ResolvedRef` corruption class started by #1539/#1550/#1560).

- **Tail-segment corruption.** When a reference's resolved target is a
  stitch, a list item or a label, the segment naming it is the path's *last*
  one (`market` in `-> hub.market`, `Red` in `Colors.Red`). Rewriting the
  whole-path range collapsed the reference to `-> newname` / `Crimson`,
  silently dropping the qualifier. `rename`, `find_references` and
  `prepare_rename` now narrow to the tail segment, in every path-bearing
  position (diverts, tunnels, threads, divert-target values, list literals
  and plain expressions).
- **Declaration initializers.** The HIR walk behind every one of these
  narrowings covered only the block tree, so a reference written in
  `VAR n = p.x` or `CONST k = Colors.Red` never matched and was rewritten at
  its whole-path range. The walk now covers declaration initializers too.
- **`prepare_rename`** applies the same narrowing as `rename`, so pressing
  F2 on the head of `p.x.y` (or the receiver of `recv.verb(…)`) highlights
  only that segment instead of the whole path.
- **Semantic tokens** no longer paint a dotted path one uniform colour: the
  field segments of `p.x.y` are reported as `property` (a new, appended
  entry in the token-type legend), and a qualified list-item/stitch/label
  reference colours the segment that actually names the symbol.
