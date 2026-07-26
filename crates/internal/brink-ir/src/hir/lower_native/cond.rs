//! The annotated-brace family's conditional/alternation half → `Conditional`
//! / `Sequence` (`docs/b0-sequencing.md` §B0.7, charter §6).
//!
//! Native disambiguates block-level vs. inline placement **positionally**,
//! not heuristically: a `CONDITIONAL_BLOCK`/`ALTERNATION_BLOCK` reached as a
//! direct `Block`/`ChoiceBody`/arm item (never wrapped in a `CONTENT_LINE`)
//! occupied its own line and lowers to a block-level `Stmt`; the same node
//! kind reached while walking a `CONTENT_LINE`'s children (this module's
//! caller, `body::lower_content_run`) shared a line with other content and
//! lowers to a `ContentPart::InlineConditional`/`InlineSequence` instead.
//! Old ink's dual inline/promoted lowering paths (`lower/conditional/`,
//! `lower/block/promotion.rs`) exist only because ink's CST doesn't make
//! this distinction structurally — native's does, so this module has one
//! lowering function per construct, not two.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use brink_syntax_native::{SyntaxNode, SyntaxToken};

use crate::hir::FileId;
use crate::provenance::NodeClass;
use crate::{
    Block, CondBranch, CondKind, Conditional, Diagnostic, DiagnosticCode, Sequence, SequenceType,
    Stmt,
};

use super::body::{lower_block, lower_items};
use super::expr::lower_expr;
use super::provenance::native_provenance;

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// `{if cond {…} else {…}}` / `{if cond: … else: …}` / `{match subj {…}}`.
///
/// **`CondKind` judgment call** (flagged for the coordinator — a likely
/// D4-style tripwire): native `if`/`else` has exactly one condition and an
/// optional else arm (the grammar has no `else if` chain — `syntax_kind.rs`'s
/// `CONDITIONAL_BLOCK` doc). Cross-frontend differential testing
/// (`crates/internal/brink-ir/tests/b07_native_body.rs`) found that ink's
/// *own* natural spelling of this exact shape (`{cond: body - else: body2}`,
/// `ConditionalWithExpr` plus a branchless first body) lowers to
/// `CondKind::InitialCondition`, never `IfElse` — `IfElse` only appears for
/// ink's independently-chained multi-condition form (three or more `- cond:`
/// branches, no shared subject), a shape native's `if`/`else` grammar cannot
/// produce at all (no `else if`; a nested `{if}` inside the `else` arm is a
/// *different*, nested `Conditional`, not a flatter multi-branch one). So
/// `InitialCondition` — not `IfElse` — is the faithful choice here: it is
/// what the equivalent ink source actually compiles to, and `lir::CondKind`
/// preserves the distinction all the way to codegen (`lir/lower/mod.rs`), so
/// this is a real semantic choice, not cosmetic. (`match` still uses
/// `Switch`, unaffected by this finding.)
pub(super) fn lower_conditional(
    file_id: FileId,
    cb: &ast::ConditionalBlock,
    diags: &mut Vec<Diagnostic>,
) -> Conditional {
    let ptr = native_provenance(file_id, NodeClass::Conditional, cb.syntax());

    if cb.is_if() {
        let Some(cond_node) = cb.condition() else {
            diags.push(diag(
                file_id,
                cb.syntax().text_range(),
                DiagnosticCode::E020,
            ));
            return Conditional {
                ptr,
                kind: CondKind::InitialCondition,
                branches: Vec::new(),
            };
        };
        let condition = lower_expr(file_id, &cond_node, diags);
        // B1b (issue #1475): the template condition position of the `as`
        // binding — the same construct the statement form takes, so it
        // reuses the statement form's own lowering (and its E145
        // whole-condition check) verbatim rather than restating the rule.
        let binding = super::control_flow::lower_as_binding(
            file_id,
            cb.as_binding().as_ref(),
            &condition,
            diags,
        );
        let mut branches = Vec::new();
        let if_body = cb.if_arm().map_or_else(Block::default, |arm| {
            lower_arm_items(file_id, arm.syntax(), diags)
        });
        branches.push(CondBranch {
            condition: Some(condition),
            binding,
            body: if_body,
            container_id: None,
        });
        if let Some(eb) = cb.else_arm() {
            let else_body = lower_arm_items(file_id, eb.syntax(), diags);
            branches.push(CondBranch {
                // Scoped strictly to the success arm — the `else` never
                // sees the binding.
                condition: None,
                binding: None,
                body: else_body,
                container_id: None,
            });
        }
        return Conditional {
            ptr,
            kind: CondKind::InitialCondition,
            branches,
        };
    }

    if cb.is_match() {
        let subject = if let Some(n) = cb.condition() {
            lower_expr(file_id, &n, diags)
        } else {
            diags.push(diag(
                file_id,
                cb.syntax().text_range(),
                DiagnosticCode::E020,
            ));
            crate::Expr::Null
        };
        let branches: Vec<CondBranch> = cb
            .match_arms()
            .map(|arm| lower_match_arm(file_id, &arm, diags))
            .collect();
        return Conditional {
            ptr,
            kind: CondKind::Switch(subject),
            branches,
        };
    }

    // Neither `if` nor `match` — the parser already recorded an error
    // (`family.rs::conditional_block`'s own `p.error`); don't re-diagnose,
    // just hand back an empty, well-formed shape.
    Conditional {
        ptr,
        kind: CondKind::IfElse,
        branches: Vec::new(),
    }
}

fn lower_match_arm(
    file_id: FileId,
    arm: &ast::MatchArm,
    diags: &mut Vec<Diagnostic>,
) -> CondBranch {
    let condition = arm.pattern_expr().map(|n| lower_expr(file_id, &n, diags));
    let body = if let Some(block) = arm.block() {
        lower_block(file_id, &block, diags)
    } else if let Some(expr_node) = arm.bare_expr() {
        // `pattern => expr` with no braces: the arm's "body" is a single
        // expression, not prose. `Stmt::ExprStmt` is the closest existing
        // HIR shape ("expression evaluated for side effects") — a judgment
        // call, since the native grammar doc itself flags this shape as
        // under-specified (`syntax_kind.rs`'s `MATCH_PATTERN` doc: "a bare
        // expression grammar reused, not a real pattern language"). No
        // block-level construct exists for "the value of one expression"
        // in the prose dialect, so this is the least-invented fit.
        let stmts = vec![Stmt::ExprStmt(lower_expr(file_id, &expr_node, diags))];
        let tail = crate::tail_from_stmts(&stmts);
        Block {
            label: None,
            stmts,
            container_id: None,
            tail,
        }
    } else {
        diags.push(diag(
            file_id,
            arm.syntax().text_range(),
            DiagnosticCode::E020,
        ));
        Block::default()
    };
    CondBranch {
        condition,
        // `match` arms are patterns, not conditions — no binding position.
        binding: None,
        body,
        container_id: None,
    }
}

/// Lower an `IF_ARM`/`ELSE_BRANCH` (conditional-family flavor)'s body: a
/// nested `BLOCK` (braced-arm form) or the node's own direct children
/// (colon form — `family.rs::colon_body` opens no wrapper node).
fn lower_arm_items(file_id: FileId, arm_syntax: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Block {
    if let Some(block_node) = arm_syntax.children().find(|n| n.kind() == N::BLOCK) {
        let items: Vec<SyntaxNode> = block_node.children().collect();
        let stmts = lower_items(file_id, &items, 0, diags);
        let tail = crate::tail_from_stmts(&stmts);
        Block {
            label: None,
            stmts,
            container_id: None,
            tail,
        }
    } else {
        let items: Vec<SyntaxNode> = arm_syntax.children().collect();
        let stmts = lower_items(file_id, &items, 0, diags);
        let tail = crate::tail_from_stmts(&stmts);
        Block {
            label: None,
            stmts,
            container_id: None,
            tail,
        }
    }
}

/// `{~ …}` shuffle / `{& …}` cycle / `{! …}` once / `{| …}` stopping.
/// `is_block_level`: whether this alternation occupies its own line (a
/// direct block/arm/choice-body item) — mirrors old ink's
/// `lower_block_sequence`'s leading-`EndOfLine`-per-branch convention,
/// which applies only to the block-promoted case, never the inline one
/// (`lower/conditional/sequence.rs`: `LowerSequence for
/// ast::SequenceWithAnnotation` inserts none; only the dedicated
/// `lower_block_sequence` does).
pub(super) fn lower_alternation(
    file_id: FileId,
    ab: &ast::AlternationBlock,
    diags: &mut Vec<Diagnostic>,
    is_block_level: bool,
) -> Sequence {
    let ptr = native_provenance(file_id, NodeClass::Sequence, ab.syntax());
    let kind = sequence_type(ab);
    let entries: Vec<ast::Entry> = ab.entries().collect();

    let branches: Vec<Block> = if entries.is_empty() {
        lower_inline_alternation_branches(file_id, ab.syntax(), diags, is_block_level)
    } else {
        entries
            .iter()
            .map(|e| {
                let items: Vec<SyntaxNode> = e.items().collect();
                let mut stmts = lower_items(file_id, &items, 0, diags);
                if is_block_level {
                    stmts.insert(0, Stmt::EndOfLine);
                }
                let tail = crate::tail_from_stmts(&stmts);
                Block {
                    label: None,
                    stmts,
                    container_id: None,
                    tail,
                }
            })
            .collect()
    };

    Sequence {
        ptr,
        kind,
        branches,
        container_id: None,
    }
}

fn sequence_type(ab: &ast::AlternationBlock) -> SequenceType {
    let marker: Option<SyntaxToken> = ab.marker_token();
    match marker.map(|t| t.kind()) {
        Some(N::TILDE) => SequenceType::SHUFFLE,
        Some(N::AMP) => SequenceType::CYCLE,
        Some(N::BANG) => SequenceType::ONCE,
        // `|` (stopping) and the "no marker recognized" fallback share the
        // same default — native has no combinator syntax (each block picks
        // exactly one marker char, unlike ink's `shuffle stopping` word
        // annotations), so there is no "empty mask" case to special-case.
        _ => SequenceType::STOPPING,
    }
}

/// Single-line, pipe-separated alternatives (`{~ red|blue|green}`). No
/// per-alternative wrapper node exists in the CST
/// (`family.rs::inline_alternatives`) — split `ALTERNATION_BLOCK`'s raw
/// children on top-level `PIPE` tokens ourselves.
fn lower_inline_alternation_branches(
    file_id: FileId,
    ab_syntax: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
    is_block_level: bool,
) -> Vec<Block> {
    let mut branches = Vec::new();
    let mut current: Vec<SyntaxNode> = Vec::new();
    let mut past_marker = false;

    for el in ab_syntax.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Node(n) if n.kind() == N::ALTERNATION_MARKER => {
                past_marker = true;
            }
            rowan::NodeOrToken::Node(n) if past_marker => current.push(n),
            rowan::NodeOrToken::Token(t) if past_marker && t.kind() == N::PIPE => {
                branches.push(finish_inline_branch(
                    file_id,
                    &current,
                    diags,
                    is_block_level,
                ));
                current.clear();
            }
            _ => {}
        }
    }
    branches.push(finish_inline_branch(
        file_id,
        &current,
        diags,
        is_block_level,
    ));
    branches
}

fn finish_inline_branch(
    file_id: FileId,
    items: &[SyntaxNode],
    diags: &mut Vec<Diagnostic>,
    is_block_level: bool,
) -> Block {
    // Never trailing-EOL: a pipe-separated alternative is a fragment, not a
    // whole line (see `body::lower_content_run`'s doc). Only the leading
    // `EndOfLine` (added below, for the block-level case) marks a line
    // boundary here.
    let mut stmts = super::body::lower_content_run(file_id, items, None, diags, false);
    if is_block_level {
        stmts.insert(0, Stmt::EndOfLine);
    }
    let tail = crate::tail_from_stmts(&stmts);
    Block {
        label: None,
        stmts,
        container_id: None,
        tail,
    }
}
