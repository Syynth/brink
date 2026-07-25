use crate::support::*;

// ─── NS-A6 (issue #1112, docs/stdlib-spec.md §7): the rand-verb surface ──

/// The F4 arity split: nullary `float()` is the rand draw, unary `float(x)`
/// stays the conversion — both lower cleanly; any other arity is E031
/// naming both forms.
#[test]
fn float_arity_split_draw_vs_conversion() {
    let (program, diags) =
        lower_ink_with_warnings("~ temp u = float()\n~ temp v = float(1)\nDone.\n");
    assert!(
        find_code(&diags, brink_ir::DiagnosticCode::E031).is_none(),
        "both float arities are legal: {diags:?}"
    );
    let program = program.expect("should lower");
    assert!(!program.root.body.is_empty());

    let (_program, diags) = lower_ink_with_warnings("~ temp u = float(1, 2)\nDone.\n");
    let e031 = find_code(&diags, brink_ir::DiagnosticCode::E031)
        .expect("two-arg float must be an arity error");
    assert!(
        e031.message.contains("random draw") && e031.message.contains("conversion"),
        "the message names both forms: {}",
        e031.message
    );
}

/// `chance`/`pick`/`shuffled` lower as ordinary expressions.
#[test]
fn chance_pick_shuffled_lower_as_expressions() {
    let (program, diags) = lower_ink_with_warnings(
        "VAR a = #[1, 2, 3]\n~ temp c = chance(0.5)\n~ temp p = pick(a)\n~ temp s = shuffled(a)\nDone.\n",
    );
    assert!(
        find_code(&diags, brink_ir::DiagnosticCode::E031).is_none()
            && find_code(&diags, brink_ir::DiagnosticCode::E056).is_none(),
        "{diags:?}"
    );
    let program = program.expect("should lower");
    assert!(!program.root.body.is_empty());
}

/// `shuffle(a)` and `seed(n)` are statement-only: legal as statements
/// (both classic `~` lines and `~ { … }` blocks), E056 in expression
/// position.
#[test]
fn shuffle_and_seed_statement_forms_lower() {
    let (program, diags) = lower_ink_with_warnings(
        "VAR a = #[1, 2, 3]\n~ seed(42)\n~ shuffle(a)\n~ {\nseed(7)\nshuffle(a)\n}\nDone.\n",
    );
    assert!(
        find_code(&diags, brink_ir::DiagnosticCode::E056).is_none()
            && find_code(&diags, brink_ir::DiagnosticCode::E058).is_none(),
        "{diags:?}"
    );
    let program = program.expect("should lower");
    assert!(!program.root.body.is_empty());
}

#[test]
fn shuffle_and_seed_in_expression_position_are_e056() {
    for src in [
        "VAR a = #[1]\n~ temp x = shuffle(a)\n",
        "~ temp x = seed(4)\n",
    ] {
        let (_program, diags) = lower_ink_with_warnings(src);
        assert!(
            find_code(&diags, brink_ir::DiagnosticCode::E056).is_some(),
            "expected E056 for {src:?}: {diags:?}"
        );
    }
}

#[test]
fn shuffle_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("VAR a = #[1]\n~ shuffle(a, 1)\n");
    let e058 = find_e058(&diags).expect("expected E058 for shuffle with 2 arguments");
    assert!(
        e058.message.contains("shuffle(array)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

#[test]
fn seed_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("~ seed()\n");
    let e058 = find_e058(&diags).expect("expected E058 for seed with 0 arguments");
    assert!(
        e058.message.contains("seed(n)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

/// `shuffle`'s receiver must be an lvalue — an rvalue is the E055 "bind it
/// to a variable first" error, same as the other mutators.
#[test]
fn shuffle_rvalue_receiver_is_e055() {
    let (_program, diags) = lower_ink_with_warnings("~ shuffle(#[1, 2])\n");
    assert!(
        find_code(&diags, brink_ir::DiagnosticCode::E055).is_some(),
        "expected E055 for an rvalue receiver: {diags:?}"
    );
}
