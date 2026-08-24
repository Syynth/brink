---
"@brink-lang/web": patch
---

Ink lowering no longer lowers every knot and declaration twice per edit (#3088): the db road's assembler harvests the declaration surface from a decl-only composition instead of a discarded whole-file lowering. Large-file keystroke re-analysis drops ~35% (the HIR-lower stage 24 → 14 ms on the 5.9k-line bench fixture). Behavior fix riding along: the file-level `#@module`/`#@was` arbitration diagnostics (E095 self-alias, E049 `#@was` without `#@module`) were silently dropped with that discarded sink and now reach editor diagnostics; E049 is error-severity, so an orphaned `#@was` now fails compilation loudly instead of being ignored.
