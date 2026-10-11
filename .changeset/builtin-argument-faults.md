---
"@brink-lang/web": patch
---

Built-ins handed an argument ink rejects now fault, with ink's message, instead of playing on with a made-up value. This covers `TURNS_SINCE`/`READ_COUNT` on something that is not a divert target (#3363), `RANDOM` with a non-int bound or an inverted range, and `SEED_RANDOM` with a non-int seed (#3364). As in ink, a fault on the line after a finished one delivers the finished line first, and the fault follows on the next continue.
