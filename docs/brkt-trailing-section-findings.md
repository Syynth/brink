# `.brkt` trailing-section readiness for v6 (findings, no design)

## Scope

#1519 ("v6 prep: needs a versioned/self-describing layout before the next
trailing section", `needs-design`) flagged the `.brkt` transcript codec's
hand-rolled "any bytes left?" backward-compat probe
(`crates/brink-runtime/src/transcript.rs`, `read_transcript`) as unextendable
for the trailing sections the v6 bump is expected to add (spans, element
data, block id, Choice captured environment — `docs/decision-log.md`,
"Choice captured environment... sequenced with the `.inkb` v6 bump...
transcript/wire codec territory, #1443 adjacency").

This document answers one question concretely: **what would a new trailing
section actually break, mechanically, in the current codec?** It does not
propose a replacement layout — that is #1519's own scope, ruled
`needs-design`. Read this alongside `docs/format-spec.md`'s "Versioning"
section, which already states the general principle for `.inkb`
("prefer... length-framed and append-only (TLV-style)... when the first
durable consumer appears") — `.brkt` *is* that durable consumer today (it
tolerates old files by design; `.inkb` hard-rejects any version mismatch and
regenerates instead), so the TLV gap already matters for `.brkt` in a way it
does not yet for `.inkb`.

## What is *not* broken

Two extension axes in this codec are already safe, because both are
self-describing (a tag byte precedes the payload) rather than positional:

- **New `OutputPart` variants** — `encode_part`/`decode_part` (the single
  shared codec landed by #1443/PR #1512) dispatch on a `u8` tag
  (`TAG_TEXT`, `TAG_LINE_REF`, ...). A new tag is one match arm in each
  function, reached identically from the top-level part list and every
  fragment's part list. No positional ambiguity.
- **New `Value` variants** — `encode_value`/`decode_value` use the same
  tag-byte dispatch (`VAL_INT`, `VAL_ARRAY`, ..., `VAL_WEIGHTED`). This is
  also how a *nested, per-value* extension already lands cleanly today: a
  `Closure`'s captured environment is a `u32`-counted list of
  `{ name, is_ref, payload }` entries inside the ordinary value codec
  (`VAL_CLOSURE`), not a new top-level section. If "Choice captured
  environment" rides as a value attached to an existing part (e.g. inside a
  `LineRef`'s slots, or a new `Value::Choice`-shaped payload), it inherits
  this same safe path and needs no section-layout change at all.

So the risk below is specific to genuinely new **top-level (or
per-fragment) trailing sections** — data that is not a part and not a
nested value, but a new positional block appended after the existing ones,
the same shape as the fragment list itself or the #953 fragment-tags
section.

## The mechanism that breaks

`read_transcript` gates exactly two trailing sections with the identical
idiom, hardcoded as two sequential `if` checks:

```rust
// 1) fragment section
let fragment_count = if off < bytes.len() {
    read_u32(bytes, &mut off)? as usize
} else {
    0 // backward compat: old transcripts without fragments
};
// ... decode `fragment_count` fragments ...

// 2) fragment-tags section
if off < bytes.len() {
    for fragment in &mut fragments {
        // ... decode this fragment's tags ...
    }
}
```

This works today only because two invariants hold, both true by convention
and enforced nowhere in the type system:

1. **Strict linear order.** Section 2 is only ever probed *after* section 1
   is fully consumed. There is no tag identifying which section follows —
   the reader's control flow *is* the schema.
2. **Monotonic joint presence.** `write_transcript` writes a section
   unconditionally once its writer-version knows about it (fragment count
   is always emitted, even `0`; each fragment's tag count is always
   emitted, even `0`, whenever there is at least one fragment). No writer
   in this codebase's history has ever emitted section 2 while omitting
   section 1, or vice versa. The probe only has to distinguish "this
   writer predates the section" from "this writer wrote it," never "this
   writer wrote section 2 but skipped section 1."

A third trailing section preserves both invariants *only if it is bolted on
as `if off < bytes.len() { ... }` #3, strictly after #2, and every writer
that emits it also emits #1 and #2 in lockstep forever.* Concretely, here
is what breaks when that assumption is violated — which the v6 roadmap's
own shape (four candidate sections, not one) makes likely:

### 1. Independently-optional sections are unrepresentable

The probe can express "sections 1..N present, N+1..end absent" (a single
cut point) and nothing else. It cannot express "section 1 present, section
2 absent, section 3 present" — content-dependent or feature-dependent
absence of an *earlier* section while a *later* one is present has no
encoding. `docs/format-spec.md`'s own precedent for `.inkb` shows this
exact shape already occurring in this codebase: the `Visibility` section
"is omitted entirely when empty" — an *optional-by-content*, not
optional-by-version, section. If any v6 trailing section follows that same
convenience (e.g. a Choice-captured-environment section that is omitted
entirely when no choice in the transcript ever captured a guard binding,
while a block-id section — present whenever fragments exist at all — comes
after it), the reader has no way to skip the absent earlier section and
correctly land on the present later one. It will instead try to interpret
the later section's bytes as the earlier section's payload.

### 2. The failure mode on misalignment is not guaranteed to be loud

Once section boundaries misalign, `read_transcript` reads whatever bytes
happen to be at the wrong offset as a `u32` count, then a payload of that
composition. Two outcomes are both live, and only one is safe:

- **Loud failure** — the misread count/tag doesn't match remaining bytes
  or hits an unrecognized tag: `UnexpectedEof`, `InvalidPartTag`, or
  `InvalidValueTag`. This is the "good" case but is not guaranteed; it
  depends on the misaligned bytes happening to violate a structural
  constraint.
- **Silent misdecode** — the misaligned bytes happen to satisfy every
  structural check the reader applies (a plausible small count, followed
  by well-formed-looking tag bytes) and the reader returns `Ok` with wrong
  data. Given how compact this wire format is (single-byte tags, `u32`
  counts), this is not a remote hypothetical: a `u32` fragment-tag count
  reinterpreted from four bytes of an unrelated section's payload has a
  wide range of small values that pass the "plausible count" bar.

### 3. There is no diagnostic for unconsumed trailing bytes today — this is a present-day gap, not a hypothetical one

`read_transcript` never checks, after decoding everything it knows about,
whether `off == bytes.len()`. It just returns `Ok(TranscriptData { .. })`
once its two hardcoded probes are exhausted. This means: **right now,
today, before any v6 section exists**, a `.brkt` file carrying *any*
trailing bytes this reader doesn't recognize — a genuinely new section from
a newer writer, or truncated/corrupted data that happens to survive the
CRC-32 check some other way — is silently, permanently dropped with zero
diagnostic. (The CRC-32 check does not help here: it is computed over the
*whole body* including the unrecognized trailing bytes, so a well-formed
newer file with a real v6 section passes the CRC check and then has that
section discarded anyway once the two known probes are exhausted.) This is
exactly the "flag silent data drops" pattern the project's rules call out
as a bug until proven otherwise (`CLAUDE.md` "Rules"): an older reader
(e.g. `bevy-brink` or a translation tool pinned to a pre-v6
`brink-runtime`) ingesting a v6-written transcript would silently discard
the new section — which, if that section is the Choice captured
environment, means silently dropping the guard-`as` binding data the
`.inkb` v6 bump exists to carry, with the reader reporting success.

### 4. No section is self-identifying, so a reader cannot skip what it does not need

Because sections are read positionally rather than tagged, an
older-than-N reader has no way to recognize "here is section K, which I
don't understand, skip `len(K)` bytes and continue to section K+1" — it can
only recognize "I am at the end of what I understand" (case 3, silently
drop everything after) or "I am mid-section, misaligned" (case 1/2 above).
A length-prefixed or tagged section layout would fix this by construction;
the current probe cannot approximate it without becoming exactly that
layout.

## Net assessment

The part-codec and value-codec extension paths (#1443's fix, and the
existing `VAL_*` tag surface) are ready for new `OutputPart`/`Value`
variants today — no further work needed there. The section-level layout is
not ready for a second *independently-optional* trailing section, and its
failure modes on misalignment range from a loud decode error to a silent,
undiagnosed data drop of exactly the kind the v6 bump's payloads (Choice
captured environment in particular) cannot afford. Whether the fix is a
length-prefixed section table, a tagged/TLV section list, or something
else is #1519's design question, not answered here.

## Oracle

This document makes no code change; the oracle ratchet is unaffected.
`RATCHET_EPISODE_COUNT` in
`crates/internal/brink-test-harness/tests/oracle_snapshots.rs` was 5,607 at
time of writing and held at 5,607 after this PR (see PR body for the gate
run).
