#![no_main]

use libfuzzer_sys::fuzz_target;

// Feed arbitrary bytes to the reader; if it produces a valid StoryData,
// re-encoding and decoding must yield the same value.
fuzz_target!(|data: &[u8]| {
    let Ok(story) = brink_format::read_inkb(data) else {
        return;
    };

    let mut buf = Vec::new();
    brink_format::write_inkb(&story, &mut buf);

    let mut recovered = brink_format::read_inkb(&buf)
        .expect("re-encoded .inkb must decode successfully");
    // `source_checksum` is a CRC-32 of the *physical* `.inkb` payload bytes
    // (set from the binary header on read, recomputed from the freshly
    // written buffer on write — see `crates/internal/brink-format/tests/inkb.rs`,
    // which resets it the same way before comparing). Arbitrary/mutated
    // input isn't guaranteed to already be in `write_inkb`'s canonical byte
    // layout, so re-encoding a semantically-identical `StoryData` can
    // legitimately produce different bytes and thus a different checksum
    // even though every other field round-trips exactly (#745 CI wiring
    // found this on the very first PR run).
    recovered.source_checksum = story.source_checksum;
    assert_eq!(story, recovered, "round-trip produced different StoryData");
});
