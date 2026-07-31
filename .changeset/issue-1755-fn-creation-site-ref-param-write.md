---
"@brink-lang/web": patch
---

Effects (#1755): a `ref` parameter bound at a `#fn` **creation** site
(`#fn(heal, player_hp)` — docs/t1c-spec.md §2) now records a write to the
bound cell in the creating definition's effect row. `ref` binds at two
grammar positions, and only the *call*-site one was recorded: the write a
creation-site binding causes was filed nowhere at all — not at the creation
site, not in the callee's body (where the target resolves as a parameter,
never a global), and not at the eventual value call (which knows the target
def but not the cell it was created against). That was an under-report, the
one direction docs/effects-spec.md §3 forbids a row to move.

Compile-behavior observable through `@brink-lang/web`: `@[effects(…)]`
exceedance (`E103`) now correctly fires on a definition whose declared bound
omits a cell it writes through a creation-site `ref` binding, where it was
previously silent — a false negative on the one diagnostic that surface
produces. Rows also widen for such definitions wherever a row is read (IDE
hover, `brink check`, the `.inkb` `EffectRows` section). No other diagnostic
changes; the oracle corpus is byte-identical.
