#![allow(clippy::unwrap_used)]

fn parse_with_timeout(data: &[u8], timeout_secs: u64) {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let input = s;
    std::thread::spawn(move || {
        let _ = brink_syntax::parse(&input);
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(std::time::Duration::from_secs(timeout_secs))
            .is_ok(),
        "Parser timed out on fuzzer input - infinite loop detected"
    );
}

/// Regression: block comments before `{` in content caused `mixed_content`
/// to loop forever.
#[test]
fn fuzz_timeout_block_comment_before_brace() {
    let data: &[u8] = &[
        126, 113, 113, 0, 1, 42, 123, 49, 33, 58, 47, 42, 42, 47, 123, 102, 42, 123, 58, 47, 42,
        126, 47, 126, 123, 0, 1, 42, 123, 49, 33, 58, 47, 42, 42, 42, 121, 42, 47, 32, 123, 42,
        123, 58, 47, 42, 126, 0, 40, 0, 7, 40, 74, 32, 44, 32, 113, 47, 42, 121, 42, 47, 32, 42,
        123, 113, 113, 113, 96, 63,
    ];
    parse_with_timeout(data, 3);
}

/// Regression: repeated `{:\t|` conditional + gather patterns caused timeout.
#[test]
fn fuzz_timeout_repeated_conditional_gathers() {
    let data: &[u8] = include_bytes!(
        "../fuzz/artifacts/parse_no_panic/timeout-d61ecb5c817836cad4a69a71416ccba908596daf"
    );
    parse_with_timeout(data, 5);
}
