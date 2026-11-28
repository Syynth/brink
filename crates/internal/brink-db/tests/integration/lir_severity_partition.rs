//! Regression coverage for `lir_query`'s severity partition of
//! LIR-lowering-phase diagnostics (#672 lane G).
//!
//! The bug (caught while landing T1b-3, fixed in PR #579): LIR-lowering
//! diagnostics were dumped into `warnings` unconditionally, so an
//! Error-severity lowering diagnostic could never actually block
//! compilation — `lir_query` still reported `program: Some` and the
//! malformed statement silently vanished from the lowered bytecode.
//! `lower_to_program` is deliberately total (it returns `Some` even
//! alongside an E057), so this db-layer gate is the ONLY place the
//! "Error-severity LIR diagnostic ⇒ no program" decision is made; the
//! brink-ir layer tests cannot pin it.
//!
//! PR #656's review then re-threaded the same partition through
//! `brink_analyzer::effective_severity` (E063 policy-dependence), covered
//! e2e by `tm3_strict_policy.rs` — these tests pin the policy-independent
//! half at the layer the original bug lived in.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

/// An Error-severity LIR-lowering diagnostic (E057: `break` outside any
/// loop, emitted during LIR lowering — not analysis) must land in
/// `LirProduct::errors` and gate `program: None`. Before PR #579 it landed
/// in `warnings` and the program shipped with the statement silently
/// dropped.
#[test]
fn regression_error_severity_lir_diagnostic_gates_program_none() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", "Hello\n~ {\nbreak\n}\n".to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(brink_opts());

    let product = db.lir_product().expect("entry point is set");
    assert!(
        product
            .errors
            .iter()
            .any(|d| d.code == DiagnosticCode::E057),
        "E057 must be partitioned into errors, not warnings: errors={:?} warnings={:?}",
        product.errors,
        product.warnings
    );
    assert!(
        product.program.is_none(),
        "an Error-severity LIR diagnostic must gate program: None"
    );
}

/// The other half of the partition: a Warning-severity LIR-lowering
/// diagnostic (E030: string interpolation in a CONST initializer) must
/// stay in `warnings` and must NOT block compilation.
#[test]
fn regression_warning_severity_lir_diagnostic_keeps_program_some() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR name = \"world\"\nCONST greeting = \"hello {name}\"\n{greeting}\n".to_owned(),
    );
    db.set_entry("main.ink");

    let product = db.lir_product().expect("entry point is set");
    assert!(
        product
            .warnings
            .iter()
            .any(|d| d.code == DiagnosticCode::E030),
        "E030 must be partitioned into warnings: errors={:?} warnings={:?}",
        product.errors,
        product.warnings
    );
    assert!(
        product.errors.is_empty(),
        "a warning-only lowering must not produce errors: {:?}",
        product.errors
    );
    assert!(
        product.program.is_some(),
        "a Warning-severity LIR diagnostic must not block compilation"
    );
}
