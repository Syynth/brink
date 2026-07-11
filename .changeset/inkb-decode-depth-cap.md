---
"@brink-lang/web": patch
---

Added a recursion-depth cap (128 levels, `MAX_DECODE_DEPTH`) to the
`VAL_ARRAY`/`VAL_MAP` decoder in both the `.inkb` reader
(`brink_format::read_inkb`, reachable from `@brink-lang/web`) and the
runtime transcript reader (`.brkt`). Previously a crafted file of deeply
nested single-element arrays (~5 bytes/level) could recurse unboundedly and
stack-overflow the reader (#553). Nesting beyond the cap now returns a
proper decode error (`DecodeError::MaxDepthExceeded` /
`TranscriptError::MaxDepthExceeded`) instead of crashing the wasm module.
Valid data — including hand-built collections nested exactly at the cap —
decodes byte-identical to before; the oracle corpus is unaffected (still
5,577 passing episodes).
