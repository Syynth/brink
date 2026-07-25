use crate::support::*;
use brink_ir::FileId;

// ─── Externals ──────────────────────────────────────────────────────

#[test]
fn external_declaration() {
    let p = lower_ink("EXTERNAL multiply(a, b)\n");
    assert_eq!(p.externals.len(), 1);
    assert_eq!(p.externals[0].arg_count, 2);
}

#[test]
fn multiple_externals() {
    let p = lower_ink("EXTERNAL foo(x)\nEXTERNAL bar(a, b, c)\n");
    assert_eq!(p.externals.len(), 2);
    let arg_counts: Vec<u8> = p.externals.iter().map(|e| e.arg_count).collect();
    assert!(arg_counts.contains(&1));
    assert!(arg_counts.contains(&3));
}

/// Ink keywords are contextual — an external may be named after an operator
/// keyword (e.g. `has`, the `Has` list operator). This must lower to a proper
/// external symbol named "has", not be dropped with a misleading "missing name"
/// diagnostic (E010). Regression for keyword-named externals.
#[test]
fn external_keyword_name() {
    // HIR lowering must not emit E010 ("missing name") for the keyword name.
    let parsed = brink_syntax::parse("EXTERNAL has(item)\n");
    let (_hir, _manifest, hir_diags) = brink_ir::hir::lower(FileId(0), &parsed.tree());
    assert!(
        hir_diags.is_empty(),
        "keyword-named external should lower without diagnostics, got: {hir_diags:?}"
    );

    // …and the external survives end-to-end with the correct name and arity.
    let p = lower_ink("EXTERNAL has(item)\n");
    assert_eq!(p.externals.len(), 1);
    assert_eq!(p.externals[0].arg_count, 1);
    let name = &p.name_table[p.externals[0].name.0 as usize];
    assert_eq!(name, "has");
}

/// External parameters may likewise be named after contextual keywords.
#[test]
fn external_keyword_params() {
    let p = lower_ink("EXTERNAL combine(and, or)\n");
    assert_eq!(p.externals.len(), 1);
    assert_eq!(p.externals[0].arg_count, 2);
}
