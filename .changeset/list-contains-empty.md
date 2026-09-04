---
"@brink-lang/web": patch
---

`list ? other` and `list !? other` now match ink when either operand
is empty: `l ? ()` is `false` and `l !? ()` is `true` (ink's
`InkList.Contains` returns false for an empty list on either side,
where brink answered the vacuous subset test) (#3531).
