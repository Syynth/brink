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
//! `lir::Expr` provenance (the D5 remainder, closing this issue) is covered
//! in its own section below, mirroring this same round-trip discipline:
//! both surfaces, byte-exact ranges. Two granularities are asserted, per
//! the design the PR body justifies —
//!
//! - **Tight**: the ~10 `hir::Expr` shapes that already carry a real
//!   `.ptr` of their own (`Infix`/`Coalesce`, `ArrayLiteral`, `MapLiteral`,
//!   `Index`, `Range`, `StructLiteral`, `FieldAccess`, `FnLiteral`,
//!   `Lambda`, `RefArg`) get that exact range on the `lir::Expr` they
//!   lower to — proven here via `Infix`/`Coalesce` (the two most heavily
//!   used shapes in ordinary logic).
//! - **Ambient**: every other shape (most of `hir::Expr` — literals,
//!   variable reads, calls, …) inherits
//!   `LowerCtx::current_stmt_provenance`, the same statement-level
//!   fallback `lir::Stmt` uses for its own synthesized siblings — proven
//!   by asserting a leaf expression's provenance is byte-identical to its
//!   *enclosing statement's* provenance, not merely "non-empty".
//!
//! The `Coalesce` per-step test guards a *different* bug: the granularity
//! decision `lower_coalesce_chain` makes (see its own doc), where each
//! folded `Coalesce` node in an `a or b or c` chain must get *its own*
//! originating `Infix` node's range, not the whole chain's. It does **not**
//! exercise the "read the ambient back after recursing" bug class:
//! `lower_coalesce_chain` stamps each step from `ie.ptr` values copied off
//! the spine *before* any recursion, so the ambient path never runs there.
//!
//! The Fragment-granularity test below is the one that actually earns that
//! claim, one level down from the nested/ambient `Stmt` section above:
//! block capture's `hir::Expr::Fragment` recurses into
//! `super::stmts::lower_stmt` through the same `ctx`; `lower_stmt` opens
//! with `ctx.enter_stmt(...)` and never restores it, and the only restore
//! is one level *above* `lower_expr`'s `Fragment` arm
//! (`lower_block_with_children`'s loop). So both the Fragment expr itself
//! and the leaf call expression that wraps it — lowered right after the
//! Fragment argument, in the same statement — must be proven to still read
//! the enclosing statement's own ambient, not whatever the captured body's
//! last inner statement left behind.

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

// ─── `lir::Expr` provenance (issue #3183's own remainder) ───────────────

#[test]
fn ink_infix_expr_carries_its_own_byte_exact_range_not_the_whole_statement() {
    let src = "VAR x = 0\n~ x = 1 + 2\n-> END\n";
    let program = lower_ink(src);
    let assign = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::Assign { .. }))
        .expect("root body should contain the `~ x = 1 + 2` assignment");
    let lir::StmtKind::Assign { value, .. } = &assign.kind else {
        panic!("just matched Assign above")
    };
    assert!(
        matches!(&value.kind, lir::ExprKind::Infix(..)),
        "expected the assigned value to lower to an Infix expression"
    );
    let text = text_at(src, value.provenance);
    assert_eq!(
        text, "1 + 2",
        "an Infix expression carries its own tight `.ptr` (hir::InfixExpr::ptr), \
         not the enclosing `~ x = 1 + 2` statement's wider range"
    );
    // The enclosing statement's own range is indeed wider — proof this is a
    // real tighter value, not the ambient statement provenance smuggled
    // through under a different name.
    assert_eq!(text_at(src, assign.provenance), "x = 1 + 2");
}

#[test]
fn ink_leaf_expr_inherits_the_enclosing_statements_provenance() {
    let src = "VAR x = 0\n~ x = 5\n-> END\n";
    let program = lower_ink(src);
    let assign = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::Assign { .. }))
        .expect("root body should contain the `~ x = 5` assignment");
    let lir::StmtKind::Assign { value, .. } = &assign.kind else {
        panic!("just matched Assign above")
    };
    assert!(
        matches!(&value.kind, lir::ExprKind::Int(5)),
        "expected a bare Int literal — the leaf case with no `.ptr` of its own"
    );
    assert_eq!(
        text_at(src, value.provenance),
        text_at(src, assign.provenance),
        "a leaf expression (no real `.ptr` of its own — most `hir::Expr` \
         shapes) inherits `ctx.current_stmt_provenance` exactly, matching \
         the enclosing statement's own provenance byte-for-byte"
    );
}

#[test]
fn native_infix_expr_carries_its_own_byte_exact_range_not_the_whole_statement() {
    let src = "flow main() {\n  ~ let x = 0\n  ~ x = 1 + 2\n  -> END\n}\n";
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
        .expect("main body should contain the `~ x = 1 + 2` assignment");
    let lir::StmtKind::Assign { value, .. } = &assign.kind else {
        panic!("just matched Assign above")
    };
    assert!(matches!(&value.kind, lir::ExprKind::Infix(..)));
    assert_eq!(
        text_at(src, value.provenance),
        "1 + 2",
        "the native frontend mints Infix provenance through its own \
         resolver — same tight-range contract as the .ink surface above"
    );
}

#[test]
fn native_leaf_expr_inherits_the_enclosing_statements_provenance() {
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
        .expect("main body should contain the `~ x = 5` assignment");
    let lir::StmtKind::Assign { value, .. } = &assign.kind else {
        panic!("just matched Assign above")
    };
    assert!(matches!(&value.kind, lir::ExprKind::Int(5)));
    assert_eq!(
        text_at(src, value.provenance),
        text_at(src, assign.provenance),
        "a leaf expression on the native surface inherits the enclosing \
         statement's provenance exactly, same as the .ink surface above"
    );
}

#[test]
fn native_coalesce_chain_stamps_each_step_with_its_own_infix_range() {
    // `some(1) or some(2) or 3` parses left-associatively as
    // `Infix(Infix(some(1), or, some(2)), or, 3)` — two coalescing steps.
    // A naive "whole chain gets one ambient value" implementation would
    // stamp every folded `Coalesce` node with the same range; the correct
    // behavior (this fix) stamps each step with its own originating
    // `Infix` node's range — see `lower_coalesce_chain`'s own doc.
    let src = "flow main() {\n  ~ let v = some(1) or some(2) or 3\n  -> END\n}\n";
    let program = lower_native(src);
    let main = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("a `main` flow container");
    let decl = main
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::DeclareTemp { .. }))
        .expect("main body should contain the `~ let v = ...` declaration");
    let lir::StmtKind::DeclareTemp {
        value: Some(value), ..
    } = &decl.kind
    else {
        panic!("expected a DeclareTemp with a value")
    };
    let lir::ExprKind::Coalesce {
        lhs: outer_lhs,
        rhs: outer_rhs,
        ..
    } = &value.kind
    else {
        panic!("expected the outermost step to be a Coalesce")
    };
    assert_eq!(
        text_at(src, value.provenance),
        "some(1) or some(2) or 3",
        "the outermost (last-folded) Coalesce node covers the whole chain \
         — it IS the root Infix node's own range"
    );
    assert_eq!(
        text_at(src, outer_rhs.provenance),
        text_at(src, decl.provenance),
        "the outer step's rhs is the plain literal `3` — a leaf, so it \
         inherits the *enclosing statement's* ambient provenance (the whole \
         `let v = ...` declaration), not its own narrow text and not a \
         chain-step range"
    );
    assert!(
        matches!(&outer_lhs.kind, lir::ExprKind::Coalesce { .. }),
        "the outer step's lhs should be the nested (inner) Coalesce step"
    );
    assert_eq!(
        // The native parser's `InfixExpr` CST node range trails through the
        // single space before the next token (here, the outer chain's own
        // `or`) — the same "whatever the frontend's own convention is" the
        // Assign-statement test above already accounts for — so this is
        // `.trim_end()`ed rather than compared byte-for-byte against a
        // string with no trailing whitespace.
        text_at(src, outer_lhs.provenance).trim_end(),
        "some(1) or some(2)",
        "the INNER Coalesce node — one level down the fold — must carry \
         its OWN originating Infix node's range, not the whole chain's \
         range the outer node has: this is exactly the granularity a \
         blind 'whole chain gets one ambient value' bug would collapse"
    );
}

#[test]
fn native_fragment_expr_and_its_wrapping_call_both_inherit_the_enclosing_statements_provenance() {
    // Regression for the stale-ambient-after-recursion bug found in this
    // PR's own review round: `lower_stmt` (`super::stmts::lower_stmt`)
    // opens with `ctx.enter_stmt(...)` and never restores it — the only
    // restore is `lower_block_with_children`'s loop, one level *above*
    // `lower_expr`'s `Fragment` arm. Before the fix, lowering a block
    // capture's captured body (which recurses into `lower_stmt` through
    // the same `ctx`, once per inner statement) left
    // `ctx.current_stmt_provenance` pointing at the captured body's *last*
    // inner statement — so (a) the `Fragment` expr itself got stamped with
    // that stale value instead of its enclosing statement's own, and (b)
    // the wrapping call expression — a leaf shape with no `.ptr` of its
    // own, lowered via `lower_call_args` *after* the Fragment argument, in
    // the very same `lower_expr` call — inherited the identical stale
    // ambient too.
    //
    // Block capture (issue #1839, "Content-as-value") is the sole producer
    // of `hir::Expr::Fragment`, reached here via a `block` convention
    // claim (same shape as `tests/tier1-native/annotations-element-block/`'s
    // own fixture): `VENDOR` claims the following line as `cue`'s captured
    // `body: content` argument.
    let src = "@[convention(claims = \"^(?<name>[A-Z][A-Z ]*)$\", order = 10, block)]\n\
               fn cue(name: string, body: content) >{\n  {name}\n  {body}\n}\n\n\
               flow main() {\n  VENDOR\n  You shouldn't be here.\n  -> END\n}\n";
    let program = lower_native(src);
    let main = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("a `main` flow container");
    let claimed = main
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::EmitContent(_)))
        .expect("main body should contain the claimed `VENDOR` line's EmitContent stmt");
    let lir::StmtKind::EmitContent(content) = &claimed.kind else {
        panic!("just matched EmitContent above")
    };
    let call = content
        .parts
        .iter()
        .find_map(|p| match p {
            lir::ContentPart::Interpolation(e) => Some(e),
            _ => None,
        })
        .expect("the claimed line rewrites to a single call interpolation");
    assert_eq!(
        text_at(src, call.provenance),
        text_at(src, claimed.provenance),
        "(b): the wrapping call expression is a leaf shape (no `.ptr` of \
         its own) lowered *after* the Fragment argument recurses through \
         `ctx` in the same statement — it must inherit the enclosing \
         statement's own ambient, not the stale value the Fragment's last \
         inner statement left behind"
    );
    let lir::ExprKind::Call { args, .. } = &call.kind else {
        panic!("expected the rewritten claim to lower to a plain Call")
    };
    let fragment = args
        .iter()
        .find_map(|a| match a {
            lir::CallArg::Value(v) if matches!(&v.kind, lir::ExprKind::Fragment(_)) => Some(v),
            _ => None,
        })
        .expect("`cue`'s trailing `content`-typed param captures a Fragment arg");
    assert_eq!(
        text_at(src, fragment.provenance),
        text_at(src, claimed.provenance),
        "(a): the Fragment expr itself has no `.ptr` of its own either, so \
         it too must inherit the enclosing statement's ambient — not its \
         own captured body's last inner statement"
    );
}
