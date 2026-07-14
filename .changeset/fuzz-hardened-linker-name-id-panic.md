---
"@brink-lang/web": patch
---

Fixed a fuzzer-discovered linker panic (PR #672 workstream C's new
`vm_no_panic` fuzz target for malformed `.inkb`, previously masked by
a CI job structure that never actually ran it — see the accompanying
CI fix — now caught on its first real run). `link()` indexed
`StoryData::name_table` with a container/address-path `NameId` taken
straight from the input bytecode with no bounds check; an out-of-range
`NameId` panicked with `index out of bounds`.

`link()` now returns `RuntimeError::InvalidNameId` on an out-of-range
`NameId` instead of panicking. Observable through `@brink-lang/web`:
`new StoryRunner(story_bytes)` (and every other entry point that links
caller-supplied `.inkb` bytes) no longer panics/traps the wasm module
on malformed/corrupted input — it returns a normal error result, like
any other malformed input.
