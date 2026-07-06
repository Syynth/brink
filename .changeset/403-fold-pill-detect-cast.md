---
"@brink-lang/editor": patch
---

Fix #403: the narrative fold pill's cast summary now routes through `detectCast` (the #366/#399 public dialect extractor) instead of reading the `speaker` attr raw off `LineInfo.dialect.attrs`. A custom dialect whose chain carries a differently-named attr (e.g. `narrator` instead of `speaker`) now surfaces correctly in the pill; the default at-cue dialect's pill output is unchanged (still shows the carried `speaker` value).
