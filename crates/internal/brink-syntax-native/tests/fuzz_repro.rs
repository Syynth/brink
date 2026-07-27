#![allow(clippy::unwrap_used)]

//! Regression-test harness for brink-syntax-native fuzzer findings.
//!
//! This module pins crash/timeout artifacts from libfuzzer, allowing discovered
//! regressions to be caught by CI in the regression-test suite without needing
//! to run the fuzzer itself. As the fuzzer finds new issues, they are added
//! as test cases with their artifact data either inlined or included from
//! ../fuzz/artifacts/.
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
