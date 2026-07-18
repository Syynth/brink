---
"@brink-lang/web": patch
---

`load_state`/`load_journal` (and any other JSON deserialize boundary that
walks a `Value::Map`) now reject a crafted or corrupted save payload that
carries a duplicate map key with a decode error, instead of silently
keeping the last occurrence (#985, follow-up to #909's content-based
`OrderedMap` equality).

`OrderedMap`'s `Eq` is content-based and assumes every key appears at most
once. Before this fix, `serde`'s derived `Deserialize` for `OrderedMap`
walked the wire `entries` list verbatim, so a hand-crafted save/journal
JSON payload with a repeated key could construct a map that violated that
invariant. `OrderedMap` now has a hand-written `Deserialize` that rejects a
repeat with a decode error (never a panic) — the same duplicate-key
rejection the `.inkb`/`.inkt`/transcript binary codecs already apply on
their own `VAL_MAP` decode paths. A save/journal file with no duplicate
keys round-trips exactly as before; this only changes behavior for
already-invalid input.
