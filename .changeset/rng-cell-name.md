---
"@brink-lang/web": patch
---

Hovering a function that calls `RANDOM` no longer shows a raw internal
handle. The effects row printed `writes: GlobalVar(0x5eed0000d1ce)`; it now
reads `writes: rng`.

The compiler-owned RNG state cell has no symbol-index entry, so the hover
row's name lookup fell through to the id's debug form. Naming now goes
through one shared authority (`brink_analyzer::effect_atom_name`) used by
both surfaces that print effect atoms — the hover row and the `E103`
exceedance message — so an author reads the same name in both, and the same
one they would write in `@[effects(writes rng)]`.
