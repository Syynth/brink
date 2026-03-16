//! Structural validation passes over the HIR.
//!
//! These passes walk the HIR statement tree and emit diagnostics for
//! structurally invalid patterns that the parser accepts but the language
//! semantics forbid.

use brink_ir::hir::{Block, Choice, ChoiceSet, ChoiceSetContext, Stmt};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

/// Run all structural validation passes on the given files.
pub fn validate(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for &(file_id, hir) in files {
        check_choices_in_inline_context(file_id, hir, &mut diagnostics);
        check_return_outside_function(file_id, hir, &mut diagnostics);
        check_unreachable_after_divert(file_id, hir, &mut diagnostics);
        check_all_fallback_choice_sets(file_id, hir, &mut diagnostics);
    }
    diagnostics
}

// ─── Choice-in-conditional/sequence validation ──────────────────────

/// Inklecate rejects choices nested inside conditionals or sequences when
/// the choice has no continuation path — no explicit divert on the choice
/// AND no statements after the conditional/sequence in the enclosing block
/// to fall through to.
///
/// Invalid: `{ true: * choice }` — dead end, no continuation.
/// Valid:   `{ true: * choice -> target }` — explicit divert.
/// Valid:   `{ true: + [Burn] \n Hello } \n - -> label` — gather after
///          the conditional provides a continuation path.
fn check_choices_in_inline_context(
    file_id: FileId,
    hir: &HirFile,
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_block(&hir.root_content, false, file_id, diagnostics);
    for knot in &hir.knots {
        walk_block(&knot.body, false, file_id, diagnostics);
        for stitch in &knot.stitches {
            walk_block(&stitch.body, false, file_id, diagnostics);
        }
    }
}

/// Walk a block's statements. `dead_end` is true when we're inside a
/// conditional/sequence that has no continuation after it — meaning
/// inline choices without diverts would be dead ends.
fn walk_block(block: &Block, dead_end: bool, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for (i, stmt) in block.stmts.iter().enumerate() {
        match stmt {
            Stmt::ChoiceSet(cs) => {
                if cs.context == ChoiceSetContext::Inline && dead_end {
                    check_choice_set_diverts(cs, file_id, diagnostics);
                }
                // Always recurse into choice bodies + continuation.
                walk_choice_set(cs, file_id, diagnostics);
            }
            Stmt::Conditional(cond) => {
                let has_continuation = has_meaningful_stmts_after(&block.stmts, i);
                for branch in &cond.branches {
                    walk_block(&branch.body, !has_continuation, file_id, diagnostics);
                }
            }
            Stmt::Sequence(seq) => {
                let has_continuation = has_meaningful_stmts_after(&block.stmts, i);
                for branch in &seq.branches {
                    walk_block(branch, !has_continuation, file_id, diagnostics);
                }
            }
            Stmt::LabeledBlock(inner) => {
                walk_block(inner, dead_end, file_id, diagnostics);
            }
            _ => {}
        }
    }
}

/// Check if there are meaningful (non-EOL) statements after position `i`.
fn has_meaningful_stmts_after(stmts: &[Stmt], i: usize) -> bool {
    stmts[i + 1..].iter().any(|s| !matches!(s, Stmt::EndOfLine))
}

/// Walk into a choice set's choices and continuation.
fn walk_choice_set(cs: &ChoiceSet, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for choice in &cs.choices {
        walk_block(&choice.body, false, file_id, diagnostics);
    }
    walk_block(&cs.continuation, false, file_id, diagnostics);
}

/// Check that every choice in the set has an explicit divert in its body.
/// Emit E029 for any choice that doesn't.
fn check_choice_set_diverts(cs: &ChoiceSet, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for choice in &cs.choices {
        if !choice_has_explicit_divert(choice) {
            diagnostics.push(Diagnostic {
                file: file_id,
                range: choice.ptr.text_range(),
                message: "choice in conditional or sequence must explicitly divert".into(),
                code: DiagnosticCode::E029,
            });
        }
    }
}

/// A choice has an explicit divert if its body contains a `Divert`,
/// `TunnelCall`, or `ThreadStart` statement (at any depth — the divert
/// could be inside nested content).
fn choice_has_explicit_divert(choice: &Choice) -> bool {
    block_has_divert(&choice.body)
}

fn block_has_divert(block: &Block) -> bool {
    block.stmts.iter().any(|stmt| match stmt {
        Stmt::Divert(_) | Stmt::TunnelCall(_) | Stmt::ThreadStart(_) => true,
        Stmt::Conditional(cond) => cond.branches.iter().all(|b| block_has_divert(&b.body)),
        Stmt::LabeledBlock(inner) => block_has_divert(inner),
        _ => false,
    })
}

// ─── Return outside function (E032) ─────────────────────────────────

/// Flag explicit `~ return` statements in non-function knots.
/// Tunnel returns (`ptr: None`) are not flagged — they're valid anywhere.
fn check_return_outside_function(
    file_id: FileId,
    hir: &HirFile,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Check root content (not inside any knot — definitely not a function).
    walk_block_for_returns(&hir.root_content, file_id, diagnostics);

    for knot in &hir.knots {
        if knot.is_function {
            continue;
        }
        walk_block_for_returns(&knot.body, file_id, diagnostics);
        for stitch in &knot.stitches {
            walk_block_for_returns(&stitch.body, file_id, diagnostics);
        }
    }
}

fn walk_block_for_returns(block: &Block, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Return(ret) if ret.ptr.is_some() => {
                // SAFETY: just checked is_some()
                let range = ret
                    .ptr
                    .map_or(rowan::TextRange::default(), |p| p.text_range());
                diagnostics.push(Diagnostic {
                    file: file_id,
                    range,
                    message: DiagnosticCode::E032.title().to_string(),
                    code: DiagnosticCode::E032,
                });
            }
            Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    walk_block_for_returns(&choice.body, file_id, diagnostics);
                }
                walk_block_for_returns(&cs.continuation, file_id, diagnostics);
            }
            Stmt::Conditional(cond) => {
                for branch in &cond.branches {
                    walk_block_for_returns(&branch.body, file_id, diagnostics);
                }
            }
            Stmt::Sequence(seq) => {
                for branch in &seq.branches {
                    walk_block_for_returns(branch, file_id, diagnostics);
                }
            }
            Stmt::LabeledBlock(inner) => {
                walk_block_for_returns(inner, file_id, diagnostics);
            }
            _ => {}
        }
    }
}

// ─── Unreachable code after divert (E033) ────────────────────────────

/// Flag statements that follow a terminal statement (`Divert`, `Return`,
/// `TunnelCall`) in the same block. Only the first unreachable statement
/// per block is flagged. `ThreadStart` is NOT terminal — threads fork
/// execution, they don't end the current flow.
fn check_unreachable_after_divert(
    file_id: FileId,
    hir: &HirFile,
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_block_for_unreachable(&hir.root_content, file_id, diagnostics);
    for knot in &hir.knots {
        walk_block_for_unreachable(&knot.body, file_id, diagnostics);
        for stitch in &knot.stitches {
            walk_block_for_unreachable(&stitch.body, file_id, diagnostics);
        }
    }
}

fn walk_block_for_unreachable(block: &Block, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    let mut saw_terminal = false;
    let mut flagged = false;

    for stmt in &block.stmts {
        if saw_terminal
            && !flagged
            && !matches!(stmt, Stmt::EndOfLine)
            && let Some(range) = stmt_range(stmt)
        {
            diagnostics.push(Diagnostic {
                file: file_id,
                range,
                message: DiagnosticCode::E033.title().to_string(),
                code: DiagnosticCode::E033,
            });
            flagged = true;
        }

        match stmt {
            Stmt::Divert(_) | Stmt::Return(_) | Stmt::TunnelCall(_) => {
                saw_terminal = true;
            }
            _ => {}
        }

        // Recurse into nested structures regardless.
        match stmt {
            Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    walk_block_for_unreachable(&choice.body, file_id, diagnostics);
                }
                walk_block_for_unreachable(&cs.continuation, file_id, diagnostics);
            }
            Stmt::Conditional(cond) => {
                for branch in &cond.branches {
                    walk_block_for_unreachable(&branch.body, file_id, diagnostics);
                }
            }
            Stmt::Sequence(seq) => {
                for branch in &seq.branches {
                    walk_block_for_unreachable(branch, file_id, diagnostics);
                }
            }
            Stmt::LabeledBlock(inner) => {
                walk_block_for_unreachable(inner, file_id, diagnostics);
            }
            _ => {}
        }
    }
}

/// Extract a source range from a statement, if available.
fn stmt_range(stmt: &Stmt) -> Option<rowan::TextRange> {
    match stmt {
        Stmt::Content(c) => c
            .ptr
            .as_ref()
            .map(brink_syntax::ast::SyntaxNodePtr::text_range),
        Stmt::Divert(d) => d
            .ptr
            .as_ref()
            .map(brink_syntax::ast::SyntaxNodePtr::text_range),
        Stmt::TunnelCall(t) => Some(t.ptr.text_range()),
        Stmt::ThreadStart(t) => Some(t.ptr.text_range()),
        Stmt::TempDecl(t) => Some(t.ptr.text_range()),
        Stmt::Assignment(a) => Some(a.ptr.text_range()),
        Stmt::Return(r) => r.ptr.as_ref().map(brink_syntax::ast::AstPtr::text_range),
        Stmt::ChoiceSet(cs) => cs.choices.first().map(|c| c.ptr.text_range()),
        Stmt::Conditional(c) => Some(c.ptr.text_range()),
        Stmt::Sequence(s) => Some(s.ptr.text_range()),
        Stmt::LabeledBlock(b) => b.label.as_ref().map(|l| l.range),
        Stmt::ExprStmt(_) | Stmt::EndOfLine => None,
    }
}

// ─── All-fallback choice set (E034) ─────────────────────────────────

/// Warn when a choice set consists entirely of fallback choices.
fn check_all_fallback_choice_sets(
    file_id: FileId,
    hir: &HirFile,
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_block_for_fallbacks(&hir.root_content, file_id, diagnostics);
    for knot in &hir.knots {
        walk_block_for_fallbacks(&knot.body, file_id, diagnostics);
        for stitch in &knot.stitches {
            walk_block_for_fallbacks(&stitch.body, file_id, diagnostics);
        }
    }
}

fn walk_block_for_fallbacks(block: &Block, file_id: FileId, diagnostics: &mut Vec<Diagnostic>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::ChoiceSet(cs) => {
                if !cs.choices.is_empty() && cs.choices.iter().all(|c| c.is_fallback) {
                    diagnostics.push(Diagnostic {
                        file: file_id,
                        range: cs.choices[0].ptr.text_range(),
                        message: DiagnosticCode::E034.title().to_string(),
                        code: DiagnosticCode::E034,
                    });
                }
                for choice in &cs.choices {
                    walk_block_for_fallbacks(&choice.body, file_id, diagnostics);
                }
                walk_block_for_fallbacks(&cs.continuation, file_id, diagnostics);
            }
            Stmt::Conditional(cond) => {
                for branch in &cond.branches {
                    walk_block_for_fallbacks(&branch.body, file_id, diagnostics);
                }
            }
            Stmt::Sequence(seq) => {
                for branch in &seq.branches {
                    walk_block_for_fallbacks(branch, file_id, diagnostics);
                }
            }
            Stmt::LabeledBlock(inner) => {
                walk_block_for_fallbacks(inner, file_id, diagnostics);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use brink_ir::hir::*;
    use brink_ir::{DiagnosticCode, FileId, HirFile};
    use brink_syntax::ast::{self, AstPtr, SyntaxNodePtr};
    use rowan::{TextRange, TextSize};

    use super::*;

    fn empty_hir() -> HirFile {
        HirFile {
            root_content: Block::default(),
            knots: Vec::new(),
            variables: Vec::new(),
            constants: Vec::new(),
            lists: Vec::new(),
            externals: Vec::new(),
            includes: Vec::new(),
        }
    }

    fn dummy_range() -> TextRange {
        TextRange::new(TextSize::new(0), TextSize::new(1))
    }

    fn dummy_knot_ptr() -> ContainerPtr {
        ContainerPtr::Knot(AstPtr::from_range(dummy_range()))
    }

    fn dummy_choice_ptr() -> AstPtr<ast::Choice> {
        AstPtr::from_range(dummy_range())
    }

    fn dummy_return_ptr() -> AstPtr<ast::ReturnStmt> {
        AstPtr::from_range(dummy_range())
    }

    // ── E032: return outside function ────────────────────────────

    #[test]
    fn return_in_non_function_emits_e032() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "my_knot".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![Stmt::Return(Return {
                    ptr: Some(dummy_return_ptr()),
                    value: None,
                    onwards_args: Vec::new(),
                })],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E032);
    }

    #[test]
    fn return_in_function_no_error() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "my_func".into(),
                range: dummy_range(),
            },
            is_function: true,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![Stmt::Return(Return {
                    ptr: Some(dummy_return_ptr()),
                    value: Some(Expr::Int(42)),
                    onwards_args: Vec::new(),
                })],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        assert!(
            diags.is_empty(),
            "return in function should not trigger E032: {diags:?}"
        );
    }

    #[test]
    fn tunnel_return_in_non_function_no_error() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "my_knot".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![Stmt::Return(Return {
                    ptr: None, // tunnel return
                    value: None,
                    onwards_args: Vec::new(),
                })],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        assert!(
            diags.is_empty(),
            "tunnel return (ptr=None) should not trigger E032: {diags:?}"
        );
    }

    // ── E033: unreachable code after divert ──────────────────────

    #[test]
    fn content_after_divert_emits_e033() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "test".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![
                    Stmt::Divert(Divert {
                        ptr: None,
                        target: DivertTarget {
                            path: DivertPath::Done,
                            args: Vec::new(),
                        },
                    }),
                    Stmt::Content(Content {
                        ptr: Some(SyntaxNodePtr::from_range(dummy_range())),
                        parts: vec![ContentPart::Text("unreachable".into())],
                        tags: Vec::new(),
                    }),
                ],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        let e033s: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E033)
            .collect();
        assert_eq!(e033s.len(), 1);
    }

    #[test]
    fn eol_after_divert_no_warning() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "test".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![
                    Stmt::Divert(Divert {
                        ptr: None,
                        target: DivertTarget {
                            path: DivertPath::Done,
                            args: Vec::new(),
                        },
                    }),
                    Stmt::EndOfLine,
                ],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        let e033s: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E033)
            .collect();
        assert!(
            e033s.is_empty(),
            "EndOfLine after divert should not trigger E033"
        );
    }

    #[test]
    fn content_after_thread_start_no_warning() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "test".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![
                    Stmt::ThreadStart(ThreadStart {
                        ptr: AstPtr::from_range(dummy_range()),
                        target: DivertTarget {
                            path: DivertPath::Path(Path {
                                segments: vec![Name {
                                    text: "other".into(),
                                    range: dummy_range(),
                                }],
                                range: dummy_range(),
                            }),
                            args: Vec::new(),
                        },
                    }),
                    Stmt::Content(Content {
                        ptr: Some(SyntaxNodePtr::from_range(dummy_range())),
                        parts: vec![ContentPart::Text("still reachable".into())],
                        tags: Vec::new(),
                    }),
                ],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        let e033s: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E033)
            .collect();
        assert!(
            e033s.is_empty(),
            "ThreadStart is not terminal — content after it is reachable"
        );
    }

    // ── E034: all-fallback choice set ────────────────────────────

    #[test]
    fn all_fallback_choice_set_emits_e034() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "test".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![Stmt::ChoiceSet(Box::new(ChoiceSet {
                    choices: vec![Choice {
                        ptr: dummy_choice_ptr(),
                        is_sticky: false,
                        is_fallback: true,
                        label: None,
                        condition: None,
                        start_content: None,
                        bracket_content: None,
                        inner_content: None,
                        tags: Vec::new(),
                        body: Block::default(),
                    }],
                    continuation: Block::default(),
                    context: ChoiceSetContext::Weave,
                }))],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        let e034s: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E034)
            .collect();
        assert_eq!(e034s.len(), 1);
    }

    #[test]
    fn mixed_fallback_and_normal_no_warning() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "test".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block {
                label: None,
                stmts: vec![Stmt::ChoiceSet(Box::new(ChoiceSet {
                    choices: vec![
                        Choice {
                            ptr: dummy_choice_ptr(),
                            is_sticky: false,
                            is_fallback: true,
                            label: None,
                            condition: None,
                            start_content: None,
                            bracket_content: None,
                            inner_content: None,
                            tags: Vec::new(),
                            body: Block::default(),
                        },
                        Choice {
                            ptr: dummy_choice_ptr(),
                            is_sticky: false,
                            is_fallback: false,
                            label: None,
                            condition: None,
                            start_content: None,
                            bracket_content: None,
                            inner_content: None,
                            tags: Vec::new(),
                            body: Block::default(),
                        },
                    ],
                    continuation: Block::default(),
                    context: ChoiceSetContext::Weave,
                }))],
            },
            stitches: Vec::new(),
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        let e034s: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E034)
            .collect();
        assert!(e034s.is_empty(), "mixed set should not trigger E034");
    }
}
