---
"@brink-lang/web": patch
---

brink-format: `read_inkb`'s container decoder now rejects a `.inkb` whose
declared `param_count` disagrees with the number of per-param name/mode
metadata entries that actually follow it (#954, sibling of the `.inkt`
reader's same guard, #745).

`ContainerDef::params`'s documented invariant is that `params.len()` always
equals `param_count` whenever per-param metadata is present at all. Before
this fix, `decode_container` built a `ContainerDef` from the two
independently-read counts with no consistency check, so a mutated/corrupt
`.inkb` could construct exactly the inconsistent state the `.inkt` reader
now rejects. Fixed by validating the invariant at decode time and returning
a new `DecodeError::ParamCountMismatch` on mismatch — a defined decode
error, never a panic (the format fuzz lanes wired up in #948 exercise this
exact path).

Observable through `@brink-lang/web`: `read_inkb` is called unconditionally
(not feature-gated) from `brink-web`'s session/story-runner/compile paths,
so a corrupted `.inkb` payload with this specific inconsistency now surfaces
as a clean decode error instead of constructing invariant-violating data.
