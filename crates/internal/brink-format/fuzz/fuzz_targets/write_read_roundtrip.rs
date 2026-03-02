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

    let recovered = brink_format::read_inkb(&buf)
        .expect("re-encoded .inkb must decode successfully");
    assert_eq!(story, recovered, "round-trip produced different StoryData");
});
