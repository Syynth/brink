---
"@brink-lang/web": patch
---

The debug info section's file table now carries each file's `source_hash`
and line index (#3261).

`source_hash` lets a consumer detect that the source it is measuring against
is not the source the program was compiled from — the debounced-recompile
window, or an edited file on disk — and answer "stale" instead of a
confidently wrong address. Per-file, so one dirty file no longer degrades
debugging everywhere.

The line index lets `file:line` resolve with no source text supplied at all,
which is what a remote debugger frontend needs and what keeps line-to-byte
conversion in one place instead of one per consumer.

Also makes `content_hash` a specified stable hash (FNV-1a 64) rather than
`std`'s `DefaultHasher`, whose algorithm Rust documents as unspecified
between releases. Hashes are now written into artifacts and compared later,
so the algorithm is part of the wire contract.
