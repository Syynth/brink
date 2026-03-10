use super::*;

// ── BranchContent ────────────────────────────────────────────────────

#[test]
fn branch_content_texts() {
    let tree = parse_tree("=== k ===\n{x: hello}\n");
    let bc: BranchContent = first(tree.syntax());
    assert!(bc.texts().next().is_some());
}

#[test]
fn branch_content_inline_logic() {
    let tree = parse_tree("=== k ===\n{x: hello {y}}\n");
    let bc: BranchContent = first(tree.syntax());
    assert!(bc.inline_logics().next().is_some());
}

#[test]
fn branch_content_glue() {
    let tree = parse_tree("=== k ===\n{x: hello<>world}\n");
    let bc: BranchContent = first(tree.syntax());
    assert!(bc.glue_nodes().next().is_some());
}

#[test]
fn branch_content_divert_some() {
    let tree = parse_tree("=== k ===\n{x: -> target}\n");
    let bc: BranchContent = first(tree.syntax());
    assert!(bc.divert().is_some());
}

#[test]
fn branch_content_divert_none() {
    let tree = parse_tree("=== k ===\n{x: just text}\n");
    let bc: BranchContent = first(tree.syntax());
    assert!(bc.divert().is_none());
}

// ── MultilineBranchBody ──────────────────────────────────────────────

#[test]
fn multiline_branch_body_texts() {
    let tree = parse_tree("=== k ===\n{\n- x > 0:\n    Hello\n}\n");
    let body: MultilineBranchBody = first(tree.syntax());
    // The branch body contains either content_lines or direct text nodes.
    // Verify at least one text-bearing child exists.
    let has_content = body.content_lines().next().is_some() || body.texts().next().is_some();
    assert!(has_content, "branch body should have text content");
}

#[test]
fn multiline_branch_body_logic_lines() {
    let tree = parse_tree("=== k ===\n{\n- x > 0:\n    ~ temp y = 1\n}\n");
    let body: MultilineBranchBody = first(tree.syntax());
    assert!(body.logic_lines().next().is_some());
}

#[test]
fn multiline_branch_body_divert() {
    let tree = parse_tree("=== k ===\n{\n- x > 0:\n    -> target\n}\n");
    let body: MultilineBranchBody = first(tree.syntax());
    assert!(body.divert().is_some());
}

// ── MixedContent ─────────────────────────────────────────────────────

#[test]
fn mixed_content_texts() {
    let mc = parse_first::<MixedContent>("Hello world\n");
    assert!(mc.texts().next().is_some());
}

#[test]
fn mixed_content_inline_logics() {
    let mc = parse_first::<MixedContent>("Hello {x}\n");
    assert!(mc.inline_logics().next().is_some());
}

#[test]
fn mixed_content_glue_nodes() {
    let mc = parse_first::<MixedContent>("Hello<>world\n");
    assert!(mc.glue_nodes().next().is_some());
}

#[test]
fn mixed_content_escapes() {
    let mc = parse_first::<MixedContent>("Hello\\#world\n");
    assert!(mc.escapes().next().is_some());
}

// ── ContentLine ──────────────────────────────────────────────────────

#[test]
fn content_line_mixed_content() {
    let cl = parse_first::<ContentLine>("Hello world\n");
    assert!(cl.mixed_content().is_some());
}

#[test]
fn content_line_divert() {
    let cl = parse_first::<ContentLine>("-> target\n");
    assert!(cl.divert().is_some());
}

#[test]
fn content_line_no_divert() {
    let cl = parse_first::<ContentLine>("Hello world\n");
    assert!(cl.divert().is_none());
}

// ── GatherDashes ─────────────────────────────────────────────────────

#[test]
fn gather_dashes_depth_one() {
    let dashes = parse_first::<GatherDashes>("- text\n");
    assert_eq!(dashes.depth(), 1);
}

#[test]
fn gather_dashes_depth_two() {
    let dashes = parse_first::<GatherDashes>("-- deeper\n");
    assert_eq!(dashes.depth(), 2);
}

#[test]
fn gather_dashes_depth_three() {
    let dashes = parse_first::<GatherDashes>("--- deepest\n");
    assert_eq!(dashes.depth(), 3);
}

// ── Gather ───────────────────────────────────────────────────────────

#[test]
fn gather_label() {
    let gather = parse_first::<Gather>("- (end) The end\n");
    let label = gather.label().unwrap();
    assert_eq!(label.name().as_deref(), Some("end"));
}

#[test]
fn gather_no_label() {
    let gather = parse_first::<Gather>("- Just text\n");
    assert!(gather.label().is_none());
}

#[test]
fn gather_mixed_content() {
    let gather = parse_first::<Gather>("- Some text\n");
    assert!(gather.mixed_content().is_some());
}

#[test]
fn gather_divert() {
    let gather = parse_first::<Gather>("- -> target\n");
    assert!(gather.divert().is_some());
}

#[test]
fn gather_no_divert() {
    let gather = parse_first::<Gather>("- Just text\n");
    assert!(gather.divert().is_none());
}

#[test]
fn gather_tags() {
    let gather = parse_first::<Gather>("- text #tagged\n");
    assert!(gather.tags().is_some());
}

// ── AuthorWarning ────────────────────────────────────────────────────

#[test]
fn author_warning_text() {
    let aw = parse_first::<AuthorWarning>("TODO: fix this\n");
    assert_eq!(aw.text(), "fix this");
}

// ── InlineLogic ──────────────────────────────────────────────────────

#[test]
fn inline_logic_inner_expression() {
    let tree = parse_tree("=== k ===\nHello {x}\n");
    let il: InlineLogic = first(tree.syntax());
    assert!(il.inner_expression().is_some());
}

#[test]
fn inline_logic_conditional() {
    let tree = parse_tree("=== k ===\n{x: hello}\n");
    let il: InlineLogic = first(tree.syntax());
    assert!(il.conditional().is_some());
}

// ── ConditionalWithExpr ──────────────────────────────────────────────

#[test]
fn conditional_with_expr_condition() {
    let tree = parse_tree("=== k ===\n{x: hello}\n");
    let cond: ConditionalWithExpr = first(tree.syntax());
    let expr = cond.condition().unwrap();
    assert!(matches!(expr, Expr::Path(_)));
}

#[test]
fn conditional_with_expr_condition_infix() {
    let tree = parse_tree("=== k ===\n{x > 0: big}\n");
    let cond: ConditionalWithExpr = first(tree.syntax());
    let expr = cond.condition().unwrap();
    assert!(matches!(expr, Expr::Infix(_)));
}

// ── MultilineBranchCond condition ────────────────────────────────────

#[test]
fn multiline_branch_cond_condition() {
    let tree = parse_tree("=== k ===\n{\n- x > 0:\n    Hello\n}\n");
    let branch: MultilineBranchCond = first(tree.syntax());
    let cond = branch.condition().unwrap();
    assert!(matches!(cond, Expr::Infix(_)));
}

#[test]
fn multiline_branch_cond_else_no_condition() {
    let tree = parse_tree("=== k ===\n{\n- x > 0:\n    Hello\n- else:\n    Bye\n}\n");
    let branches: Vec<MultilineBranchCond> = tree
        .syntax()
        .descendants()
        .filter_map(MultilineBranchCond::cast)
        .collect();
    let else_branch = branches.iter().find(|b| b.is_else()).unwrap();
    assert!(else_branch.condition().is_none());
}

// ── BranchlessCondBody ───────────────────────────────────────────────

#[test]
fn branchless_cond_body_texts() {
    let tree = parse_tree("{\n  x:\n  Content here.\n}\n");
    let body: BranchlessCondBody = first(tree.syntax());
    assert!(body.texts().next().is_some());
}

#[test]
fn branchless_cond_body_else_branch() {
    let tree = parse_tree("{\n  x:\n  Content.\n- else:\n  Other.\n}\n");
    let body: BranchlessCondBody = first(tree.syntax());
    assert!(body.else_branch().is_some());
}

#[test]
fn branchless_cond_body_else_after_logic_line() {
    // When a logic line consumes the trailing NEWLINE, the parser lands
    // directly on MINUS — must still parse as an else branch.
    let tree = parse_tree("{\n  x > 0:\n  ~ y = 1\n- else:\n  ~ y = 2\n}\n");
    let body: BranchlessCondBody = first(tree.syntax());
    assert!(body.else_branch().is_some());
}

// ── Sequences ────────────────────────────────────────────────────────

#[test]
fn sequence_symbol_annotation_amp() {
    let tree = parse_tree("=== k ===\n{&a|b|c}\n");
    let ann: SequenceSymbolAnnotation = first(tree.syntax());
    assert!(ann.amp_token().is_some());
    assert!(ann.bang_token().is_none());
}

#[test]
fn sequence_symbol_annotation_bang() {
    let tree = parse_tree("=== k ===\n{!a|b|c}\n");
    let ann: SequenceSymbolAnnotation = first(tree.syntax());
    assert!(ann.bang_token().is_some());
}

#[test]
fn sequence_symbol_annotation_tilde() {
    let tree = parse_tree("=== k ===\n{~a|b|c}\n");
    let ann: SequenceSymbolAnnotation = first(tree.syntax());
    assert!(ann.tilde_token().is_some());
}

#[test]
fn sequence_symbol_annotation_dollar() {
    let tree = parse_tree("=== k ===\n{$a|b|c}\n");
    let ann: SequenceSymbolAnnotation = first(tree.syntax());
    assert!(ann.dollar_token().is_some());
}

#[test]
fn sequence_word_annotation_stopping() {
    let tree = parse_tree("=== k ===\n{stopping:a|b|c}\n");
    let ann: SequenceWordAnnotation = first(tree.syntax());
    assert!(ann.stopping_kw().is_some());
}

#[test]
fn sequence_word_annotation_cycle() {
    let tree = parse_tree("=== k ===\n{cycle:a|b|c}\n");
    let ann: SequenceWordAnnotation = first(tree.syntax());
    assert!(ann.cycle_kw().is_some());
}

#[test]
fn sequence_word_annotation_shuffle() {
    let tree = parse_tree("=== k ===\n{shuffle:a|b|c}\n");
    let ann: SequenceWordAnnotation = first(tree.syntax());
    assert!(ann.shuffle_kw().is_some());
}

#[test]
fn sequence_word_annotation_once() {
    let tree = parse_tree("=== k ===\n{once:a|b|c}\n");
    let ann: SequenceWordAnnotation = first(tree.syntax());
    assert!(ann.once_kw().is_some());
}
