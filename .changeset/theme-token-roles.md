---
"@brink-lang/web": patch
---

Three new semantic token types, split out of the operator/keyword buckets so themes can color marks by what they do (theme ruling 2026-08-25): `marker` (choice bullets, gather dashes, weave brackets — position-checked, so expression-position `*`/`+`/`-` stay operators), `divert` (`->`, `->->`, `<-`, glue), and `halt` (`END`/`DONE`). Header equals-runs now classify with their definition (namespace/function) instead of as operators, so a knot header reads as one mark.
