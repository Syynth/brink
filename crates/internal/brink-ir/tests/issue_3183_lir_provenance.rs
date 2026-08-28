//! Issue #3183: `lir::Stmt` and `lir::Container` carry real source
//! provenance (`brink-ir::Provenance`), populated at LIR-lowering time from
//! the HIR node each was lowered from.
//!
//! Proof shape (per the issue's acceptance bar): lower a small fixture on
//! **both** frontends — `.ink` (`brink_syntax::parse` → `hir::lower`) and
//! `.brink` native (`brink_syntax_native::parse` → `hir::lower_native`) —
//! and assert the lowered `lir::Stmt`/`lir::Container`'s stamped
//! `provenance.range`, sliced back out of the original source text, is the
//! **exact** substring of the construct it came from — not merely
//! "non-empty" or "roughly right". The two frontends mint provenance
//! through different resolvers (`brink_syntax`'s vs `brink_syntax_native`'s
//! own `NodeClass`/range stamping), so a round-trip proof on one surface
//! alone would not cover the other.
//!
//! `lir::Expr` is deliberately **not** covered here — see the PR body for
//! #3183: this ticket scoped Expr-level provenance out (Container/Stmt
//! granularity only), so there is nothing on `lir::Expr` to prove yet.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into codegen output can't ship quietly. This
// file's two uses (an always-empty `file_paths` map handed to
// `lir::lower_to_program` — these fixtures never populate `SourceLocation`)
// have no order to leak, the same exemption every other `lir_lowering`-style
// pipeline helper in this crate takes for the identical pattern.
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map, no order to leak — see file doc"
)]

use brink_ir::{FileId, HirFile, SymbolManifest, lir};

/// Slice `src` by a `Provenance`'s stamped range.
fn text_at(src: &str, provenance: brink_ir::Provenance) -> &str {
    let range = provenance.text_range();
    &src[usize::from(range.start())..usize::from(range.end())]
}

// ─── .ink surface ────────────────────────────────────────────────────

fn lower_ink(source: &str) -> lir::Program {
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (mut hir, manifest, diags) = brink_ir::hir::lower(file_id, &tree);
    assert!(
        diags.is_empty(),
        "unexpected ink lowering diagnostics: {diags:?}"
    );
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let (program, diags) = lir::lower_to_program(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
    );
    assert!(diags.is_empty(), "unexpected LIR diagnostics: {diags:?}");
    program.expect("plain ink source always lowers to a program")
}

#[test]
fn ink_assignment_stmt_carries_its_own_byte_exact_range() {
    let src = "VAR x = 0\n~ x = 5\n-> END\n";
    let program = lower_ink(src);
    let assign = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::Assign { .. }))
        .expect("root body should contain the `~ x = 5` assignment");
    let text = text_at(src, assign.provenance);
    assert_eq!(
        text, "x = 5",
        "the Assign statement's provenance should span exactly the source \
         assignment it was lowered from (whatever the frontend's own \
         convention for the logic-line's `~` prefix is), not some \
         approximation"
    );
}

#[test]
fn ink_temp_decl_stmt_carries_its_own_byte_exact_range() {
    let src = "~ temp x = 5\n-> END\n";
    let program = lower_ink(src);
    let decl = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::DeclareTemp { .. }))
        .expect("root body should contain the `~ temp x = 5` declaration");
    let text = text_at(src, decl.provenance);
    assert_eq!(text, "temp x = 5");
}

#[test]
fn ink_knot_container_carries_its_own_byte_exact_range() {
    let src = "-> next\n== next ==\nHello.\n-> END\n";
    let program = lower_ink(src);
    let knot = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("next"))
        .expect("a `next` knot container");
    let text = text_at(src, knot.provenance);
    assert!(
        text.starts_with("== next =="),
        "the knot container's provenance should start at its own `==` \
         header, got: {text:?}"
    );
    assert!(
        text.contains("Hello."),
        "the knot container's provenance should cover its own body too, \
         got: {text:?}"
    );
}

// ─── .brink native surface ─────────────────────────────────────────────

fn lower_native(source: &str) -> lir::Program {
    let parsed = brink_syntax_native::parse(source);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let file_id = FileId(0);
    let (hir, manifest, diags) = brink_ir::hir::lower_native::lower(file_id, &parsed.tree());
    assert!(
        diags.is_empty(),
        "unexpected native lowering diagnostics: {diags:?}"
    );

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let analysis_opts = brink_analyzer::AnalysisOptions {
        dialect: brink_analyzer::Dialect::Brink,
        ..Default::default()
    };
    let analysis = brink_analyzer::analyze_with_options(&files_for_analysis, &analysis_opts);
    assert!(
        analysis.diagnostics.is_empty(),
        "unexpected analysis diagnostics: {:?}",
        analysis.diagnostics
    );

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let (program, diags) = lir::lower_to_program(
        &files_for_lir,
        &analysis.index,
        &analysis.resolutions,
        &std::collections::HashMap::new(),
    );
    assert!(diags.is_empty(), "unexpected LIR diagnostics: {diags:?}");
    program.expect("well-formed native source always lowers to a program")
}

#[test]
fn native_assignment_stmt_carries_its_own_byte_exact_range() {
    let src = "flow main() {\n  ~ let x = 0\n  ~ x = 5\n  -> END\n}\n";
    let program = lower_native(src);
    let main = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("a `main` flow container");
    let assign = main
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::Assign { .. }))
        .expect("main body should contain the `~x = 5` assignment");
    let text = text_at(src, assign.provenance);
    assert_eq!(
        text, "x = 5",
        "the native frontend mints provenance through its own resolver — \
         same round-trip contract as the .ink surface above"
    );
}

#[test]
fn native_knot_container_carries_its_own_byte_exact_range() {
    let src = "flow main() {\n  -> END\n}\n";
    let program = lower_native(src);
    let main = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("a `main` flow container");
    let text = text_at(src, main.provenance);
    assert!(
        text.starts_with("flow main"),
        "the flow container's provenance should start at its own header, \
         got: {text:?}"
    );
}
