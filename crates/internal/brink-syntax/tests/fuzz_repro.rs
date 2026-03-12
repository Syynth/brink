#![allow(clippy::unwrap_used)]

/// Regression test for fuzzer timeout: block comments before `{` in
/// content caused `mixed_content` to loop forever because `text_content`
/// broke on `BLOCK_COMMENT` without consuming it, and the `L_BRACE` arm
/// had no progress check.
#[test]
fn fuzz_timeout_repro() {
    let data: &[u8] = &[
        126, 113, 113, 0, 1, 42, 123, 49, 33, 58, 47, 42, 42, 47, 123, 102, 42, 123, 58, 47, 42,
        126, 47, 126, 123, 0, 1, 42, 123, 49, 33, 58, 47, 42, 42, 42, 121, 42, 47, 32, 123, 42,
        123, 58, 47, 42, 126, 0, 40, 0, 7, 40, 74, 32, 44, 32, 113, 47, 42, 121, 42, 47, 32, 42,
        123, 113, 113, 113, 96, 63,
    ];
    let s = std::str::from_utf8(data).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let input = s.to_string();
    std::thread::spawn(move || {
        let _ = brink_syntax::parse(&input);
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(std::time::Duration::from_secs(3)).is_ok(),
        "Parser timed out on fuzzer input - infinite loop detected"
    );
}
