#![allow(clippy::unwrap_used)]

//! Regression-test harness for brink-syntax-native fuzzer findings.
//!
//! Once the fuzzer finds an issue, its crash/timeout artifact will be pinned
//! here as a test case (data either inlined or included from
//! ../fuzz/artifacts/), allowing the regression to be caught by CI without
//! needing to run the fuzzer itself.
//!
//! **Status:** No fuzzer findings have been pinned yet. This harness is ready
//! to accept the first finding from #1191's native fuzzer work.

#[expect(
    dead_code,
    reason = "Used by regression tests when fuzzer findings are pinned"
)]
fn parse_with_timeout(data: &[u8], timeout_secs: u64) {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let input = s;
    std::thread::spawn(move || {
        let _ = brink_syntax_native::parse(&input);
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(std::time::Duration::from_secs(timeout_secs))
            .is_ok(),
        "Parser timed out on fuzzer input - infinite loop detected"
    );
}
