---
"@brink-lang/web": patch
---

Stdlib slice 1 completion: `char_at(s, i)` string-indexing primitive
(docs/t1b-surface-spec.md §5, issue #857) — a corpus finding that blocked
string-algorithm ports (levenshtein/tokenizers/edit-distance) with no way
to read a character out of a string.

- **Chars, not bytes**: `i` indexes Unicode scalar values (`str::chars`),
  not UTF-8 bytes — a byte-indexed read would panic or split a multi-byte
  sequence for any non-ASCII text. Returns the char at `i` as a
  single-character `String` (ink has no separate char type).
- **Turn-terminating fault** (value-model-spec §11c: no silent garbage) on
  `i` outside `[0, char_count)`, a non-`Int` `i`, or a non-`String` `s` —
  never a clamp, never a silently-empty result. New `RuntimeError`
  variants `CharAtOutOfBounds`/`CharAtIndexNotInt`.
- **VM-native** (`CharAt` opcode, `0xDD`), lowercase name, author-
  shadowable with a warning (`E035`) per the existing stdlib-slice-1
  ruling (`is_t1b_stdlib_name`).
- **Typing rule** declared at introduction: fixed `Ty::String` return
  (a char-as-1-string result), independent of argument types — the domain
  check is a runtime/gradual-mode concern at the `CharAt` op, matching the
  `int`/`float`/`string` conversion intrinsics' posture.
- `.inkt` text support lands with a reader in this same PR (writer +
  reader + round-trip test, matching the `#742`-adjacent discipline).

Observable through `@brink-lang/web`: new VM opcode/fault surface any
consumer executing compiled `.inkb` through the wasm runtime can now
encounter, so this ships as a patch per the wasm-observable-behavior
convention.
