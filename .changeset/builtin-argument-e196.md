---
"@brink-lang/web": patch
---

New compile error `E196`: a classic ink built-in called with arguments inklecate's compiler refuses, worded as inklecate words it (#3363, #3364). This covers the wrong number of arguments for any built-in, a bare knot/stitch/label name or a literal passed to `TURNS_SINCE`/`READ_COUNT` (write `-> name`), and a float or bool literal passed to `RANDOM`/`SEED_RANDOM`. Each of these also fails at runtime. Arguments only inklecate's runtime rejects are still left to the runtime fault.
