#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let parsed = brink_syntax::parse(s);
        let roundtrip = parsed.syntax().text().to_string();
        assert_eq!(roundtrip, s, "lossless round-trip violated");
    }
});
