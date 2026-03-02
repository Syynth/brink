#![no_main]

use libfuzzer_sys::fuzz_target;

// Feed arbitrary strings to the reader; if it produces a valid StoryData,
// re-encoding and decoding must yield the same value.
fuzz_target!(|data: &str| {
    let Ok(story) = brink_format::read_inkt(data) else {
        return;
    };

    let mut buf = String::new();
    brink_format::write_inkt(&story, &mut buf)
        .expect("write_inkt must succeed for valid StoryData");

    let recovered = brink_format::read_inkt(&buf)
        .expect("re-encoded .inkt must parse successfully");
    assert_eq!(story, recovered, "round-trip produced different StoryData");
});
