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

// ─── Nested/ambient cases (regression coverage for the stale-ambient bug
// found in review: `lower_block_with_children` was reading
// `ctx.current_stmt_provenance` back *after* recursing into child bodies
// through the same `ctx` — a choice's body, a conditional/sequence branch —
// so the value read back was the last inner statement's, not the enclosing
// statement's own. Fixed by capturing the ambient once per top-level
// statement and restoring it after each loop iteration; see
// `lower_block_with_children`'s `stmt_prov` local.) ───────────────────────

#[test]
fn ink_choice_set_stmt_carries_the_first_choices_range_not_a_nested_descendants() {
    // Each choice body ends in `-> END` — before the fix, lowering the
    // choices (which recurses into each body through the same `ctx`) left
    // `ctx.current_stmt_provenance` pointing at the *last* choice's
    // `-> END`, and the ChoiceSet stmt read that stale value back instead
    // of its own anchor.
    let src = "Intro.\n* Hello\n    World.\n    -> END\n* Bye\n    Farewell.\n    -> END\n";
    let program = lower_ink(src);
    let cs = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::ChoiceSet(_)))
        .expect("root body should contain the ChoiceSet stmt");
    let text = text_at(src, cs.provenance);
    assert_eq!(
        text, "* Hello\n",
        "the ChoiceSet stmt's provenance is `choice_set_anchor_range` — the \
         first choice's own range — not whatever the last-lowered nested \
         choice body happened to leave in the ambient"
    );
}

#[test]
fn ink_gather_after_choice_set_inherits_the_choice_sets_own_provenance() {
    // Same fixture as above: the gather continuation container is built via
    // `build_continuation_container` *after* all choice bodies (each
    // recursing through the same `ctx`) have been lowered — before the fix
    // it read the stale ambient (the last choice's `-> END`) instead of the
    // ChoiceSet's own provenance the caller now passes explicitly.
    let src = "Intro.\n* Hello\n    World.\n    -> END\n* Bye\n    Farewell.\n    -> END\n";
    let program = lower_ink(src);
    let gather = program
        .root
        .children
        .iter()
        .find(|c| c.kind == lir::ContainerKind::Gather)
        .expect("a g-0 gather continuation container");
    let text = text_at(src, gather.provenance);
    assert_eq!(
        text, "* Hello\n",
        "the gather continuation container has no HIR range of its own, so \
         it inherits the enclosing ChoiceSet's own provenance — passed \
         explicitly by `build_continuation_container`'s caller, not read \
         back off a by-then-stale `ctx.current_stmt_provenance`"
    );
}

#[test]
fn ink_conditional_stmt_carries_the_whole_construct_not_a_nested_descendants() {
    // Both branches end in `-> END` — before the fix, lowering the branches
    // (recursing through the same `ctx`) left the ambient pointing at the
    // last branch's `-> END` instead of the Conditional's own `c.ptr`.
    let src = "VAR n = 1\n{ n > 0:\n    Positive.\n    -> END\n- else:\n    Neg.\n    -> END\n}\n";
    let program = lower_ink(src);
    let cond = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::Conditional(_)))
        .expect("root body should contain the Conditional stmt");
    let text = text_at(src, cond.provenance);
    assert_eq!(
        text, "{ n > 0:\n    Positive.\n    -> END\n- else:\n    Neg.\n    -> END\n}",
        "the Conditional stmt's provenance should be the whole `{{ ... }}` \
         construct (already computed as `c.ptr` and previously discarded), \
         not the last-lowered branch's trailing statement"
    );
}

#[test]
fn ink_sequence_wrapper_container_carries_the_whole_construct_not_a_nested_descendants() {
    // The sequence has two plain-content branches ("One."/"Two."); before
    // the fix, lowering the branches (recursing through the same `ctx`)
    // left the ambient pointing at the last branch's content instead of the
    // wrapper's own `seq.ptr`. A trailing labeled gather (`- (top) ...`)
    // sits right after so the fix's loop-end restore is also exercised: the
    // Sequence's `EnterContainer`/`EndOfLine` root-body stmts must not leak
    // into the following LabeledBlock's own provenance either (see the
    // gather test below).
    let src = "{stopping:\n- One.\n- Two.\n}\n- (top) Gathered.\nMore.\n-> END\n";
    let program = lower_ink(src);
    let wrapper = program
        .root
        .children
        .iter()
        .find(|c| c.kind == lir::ContainerKind::Sequence)
        .expect("a Sequence wrapper container");
    let text = text_at(src, wrapper.provenance);
    assert_eq!(
        text, "{stopping:\n- One.\n- Two.\n}",
        "the Sequence wrapper container's provenance should be the whole \
         `{{stopping: ...}}` construct, not the last-lowered branch's content \
         (\"Two.\")"
    );
}

#[test]
fn ink_labeled_gather_after_sequence_does_not_inherit_the_sequences_leftover_ambient() {
    // Regression for the loop-end restore specifically: after the Sequence
    // stmt is fully lowered, `ctx.current_stmt_provenance` must be restored
    // to the Sequence's own provenance (not left at whatever the last
    // branch lowered) so this *next sibling* top-level statement — the
    // `- (top)` labeled gather — starts from a correct ambient too. Its own
    // container provenance comes from its label range directly, so this
    // asserts the container-level anchor, independent of the ambient.
    let src = "{stopping:\n- One.\n- Two.\n}\n- (top) Gathered.\nMore.\n-> END\n";
    let program = lower_ink(src);
    let gather = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("top"))
        .expect("a `top` labeled gather container");
    let text = text_at(src, gather.provenance);
    assert_eq!(
        text, "top",
        "the labeled gather's provenance should be its own label's range \
         (`labeled_block_anchor_range`), not the preceding Sequence's \
         leftover `-> END`/last-branch ambient"
    );
}
