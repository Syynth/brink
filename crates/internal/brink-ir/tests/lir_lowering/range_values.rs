use crate::support::*;

// ─── NS-A5 (issue #1111, docs/stdlib-spec.md §7): range values ──────────

/// Range literals (both forms) and the `non_empty` validator lower as
/// ordinary expressions; `int(range)` rides the existing `ConvertInt`
/// lowering (one value-directed verb — no new arm, no diagnostic).
#[test]
fn range_literals_and_verbs_lower_as_expressions() {
    let (program, diags) = lower_ink_with_warnings(
        "~ temp r = 1..=6\n\
         ~ temp s = 0..10\n\
         ~ temp o = non_empty(r)\n\
         ~ temp x = int(r)\n\
         ~ temp p = pick(s)\n\
         Done.\n",
    );
    assert!(
        find_code(&diags, brink_ir::DiagnosticCode::E031).is_none()
            && find_code(&diags, brink_ir::DiagnosticCode::E056).is_none(),
        "{diags:?}"
    );
    let program = program.expect("should lower");
    assert!(!program.root.body.is_empty());
}

/// A range literal's bounds are ordinary expressions — nested calls,
/// arithmetic, negatives all lower.
#[test]
fn range_bounds_are_expressions_at_lowering() {
    let (program, diags) = lower_ink_with_warnings(
        "VAR a = #[1, 2, 3]\n\
         ~ temp r = -3..len(a) + 1\n\
         Done.\n",
    );
    assert!(
        find_code(&diags, brink_ir::DiagnosticCode::E031).is_none(),
        "{diags:?}"
    );
    let program = program.expect("should lower");
    assert!(!program.root.body.is_empty());
}

/// `non_empty` demands exactly one argument (E031 otherwise).
#[test]
fn non_empty_wrong_arity_is_e031() {
    let (_program, diags) = lower_ink_with_warnings("~ temp o = non_empty()\nDone.\n");
    assert!(
        find_code(&diags, brink_ir::DiagnosticCode::E031).is_some(),
        "nullary non_empty must be an arity error: {diags:?}"
    );
}
