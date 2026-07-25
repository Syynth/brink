use crate::support::*;

// ─── #581: collection mutator arity mismatch is a targeted compile error ──
//
// `push`/`insert`/`remove` called with the wrong argument count used to
// share the generic warning-severity E031 with ordinary function-call arity
// checking, and — because E031 never blocked compilation — the malformed
// mutator statement silently vanished from the lowered bytecode (nothing
// pushed to `out`, `try_lower_mutator_stmt` still returned `true`). E058 is
// Error-severity and names the expected signature.

#[test]
fn push_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("VAR a = #[1]\n~ push(a)\n");
    let e058 = find_e058(&diags).expect("expected E058 for push with 1 argument");
    assert_eq!(e058.code.severity(), brink_ir::Severity::Error);
    assert!(
        e058.message.contains("push(container, value)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

#[test]
fn insert_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("VAR a = #[1]\n~ insert(a, 0)\n");
    let e058 = find_e058(&diags).expect("expected E058 for insert with 2 arguments");
    assert!(
        e058.message
            .contains("insert(container, key_or_index, value)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

#[test]
fn remove_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("VAR a = #[1]\n~ remove(a, 0, 1)\n");
    let e058 = find_e058(&diags).expect("expected E058 for remove with 3 arguments");
    assert!(
        e058.message.contains("remove(container, key_or_index)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

#[test]
fn mutator_correct_arity_no_e058() {
    let (program, diags) = lower_ink_with_warnings(
        "VAR a = #[1]\n~ {\npush(a, 2)\ninsert(a, 0, 9)\nremove(a, 0)\n}\n",
    );
    assert!(find_e058(&diags).is_none(), "unexpected E058: {diags:?}");
    let program = program.expect("should lower to a real program");
    assert!(!program.root.body.is_empty());
}

#[test]
fn pure_function_call_arity_still_uses_e031_not_e058() {
    // Ordinary (non-mutator) function-call arity checking is untouched by
    // #581 — only push/insert/remove route through E058.
    let (_program, diags) =
        lower_ink_with_warnings("~ temp x = f(1)\n== function f(a, b) ==\n~ return a + b\n");
    assert!(
        find_e058(&diags).is_none(),
        "pure function arity mismatch must not use E058: {diags:?}"
    );
}
