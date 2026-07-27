---
"@brink-lang/web": patch
---

Removed `LineFlags::STARTS_WITH_WS`/`ENDS_WITH_WS` (`brink-format`). A grep
audit found zero production consumers: `STARTS_WITH_WS` had none at all, and
the only `ENDS_WITH_WS` reader (`OutputBuffer::ends_in_whitespace`) was
`#[cfg(test)]`-only. Live whitespace-only/empty suppression in
`brink-runtime` uses `ALL_WS`/`EMPTY` exclusively, which this does not
touch.

Traced against the C# reference runtime before removing: its output-stream
whitespace handling (`PushToOutputStreamIndividual`, `TrimNewlinesFromOutputStream`,
`TrimWhitespaceFromFunctionEnd`) always operates on whole tokens
(`isNewline`/`isNonWhitespace`/`isInlineWhitespace`), never on whether a
mixed-content token merely starts or ends with whitespace. There is no
sub-token leading/trailing whitespace concept in ink's reference semantics,
so these flags encoded a distinction the runtime never needed — this is a
dead-code removal, not a conformance gap.

`LineFlags` is derived at `.inkb` decode time, not stored on the wire there,
so this has no `.inkb` format-version impact. `.brkt` (the transcript save
format) does persist `LineFlags` bits (`transcript.rs`'s `encode_part`/
`decode_part`), so the bit values for the surviving `ALL_WS`/`EMPTY` flags
are left unchanged from their prior positions to avoid reinterpreting
existing `.brkt` files. No observable rendering effect, since neither
removed flag had a live consumer.
