//! Structural validation passes over the HIR.
//!
//! These passes walk the HIR statement tree and emit diagnostics for
//! structurally invalid patterns that the parser accepts but the language
//! semantics forbid.

use brink_ir::hir::{Block, Choice, ChoiceSet, HirVisitor, Knot, ReturnKind, Stmt};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

/// Run all structural validation passes on the given files.
pub fn validate(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for &(file_id, hir) in files {
        // E029 is positional — it depends on the statements *after* a
        // conditional/sequence in the enclosing block — so it keeps its own
        // contextual walk. The remaining checks are node-local or per-block and
        // share a single traversal via the shared HIR visitor.
        check_choices_in_inline_context(file_id, hir, &mut diagnostics);

        let mut v = StructuralChecks::new(file_id);
        brink_ir::hir::visit::visit(hir, &mut v);
        // Append per-check buckets in the original pass order; each bucket is
        // already in DFS order, so overall diagnostic ordering is unchanged.
        diagnostics.extend(v.returns);
        diagnostics.extend(v.unreachable);
        diagnostics.extend(v.fallbacks);
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
                if dead_end {
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
                    walk_block(&branch.body, !has_continuation, file_id, diagnostics);
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

// ─── Combined node-local / per-block checks (E032, E033, E034) ───────
//
// One shared-visitor traversal drives three checks that the old code ran as
// three separate full walks:
//   - E032: an explicit `~ return` outside a function knot.
//   - E033: the first statement after a terminal (`Divert`/`Return`) in a block.
//   - E034: a choice set consisting entirely of fallback choices.
// Diagnostics are bucketed per check so `validate` can append them in the
// original pass order.

/// Per-block state for the E033 unreachable-after-terminal check. Pushed on
/// `enter_block`, popped on `exit_block`, so nested blocks don't interfere.
#[derive(Default)]
struct UnreachableState {
    saw_terminal: bool,
    flagged: bool,
}

struct StructuralChecks {
    file_id: FileId,
    /// True while inside a function knot's body/stitches — suppresses E032.
    in_function: bool,
    /// Per-block E033 state, one frame per enclosing block.
    unreachable_stack: Vec<UnreachableState>,
    returns: Vec<Diagnostic>,
    unreachable: Vec<Diagnostic>,
    fallbacks: Vec<Diagnostic>,
}

impl StructuralChecks {
    fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            in_function: false,
            unreachable_stack: Vec::new(),
            returns: Vec::new(),
            unreachable: Vec::new(),
            fallbacks: Vec::new(),
        }
    }
}

impl HirVisitor for StructuralChecks {
    fn enter_knot(&mut self, knot: &Knot) {
        // E032 is suppressed inside function knots (bodies and stitches). Knots
        // don't nest, so a single flag reset in exit_knot suffices.
        self.in_function = knot.is_function;
    }

    fn exit_knot(&mut self, _knot: &Knot) {
        self.in_function = false;
    }

    fn enter_block(&mut self, _block: &Block) {
        self.unreachable_stack.push(UnreachableState::default());
    }

    fn exit_block(&mut self, _block: &Block) {
        self.unreachable_stack.pop();
    }

    fn enter_stmt(&mut self, stmt: &Stmt) {
        // E033: the first non-EOL statement after a terminal, per block. Check
        // against the current block's state before updating it, mirroring the
        // old per-block walk order.
        let flag_unreachable = self
            .unreachable_stack
            .last()
            .is_some_and(|s| s.saw_terminal && !s.flagged)
            && !matches!(stmt, Stmt::EndOfLine);
        if flag_unreachable && let Some(range) = stmt_range(stmt) {
            self.unreachable.push(Diagnostic {
                file: self.file_id,
                range,
                message: DiagnosticCode::E033.title().to_string(),
                code: DiagnosticCode::E033,
            });
            if let Some(s) = self.unreachable_stack.last_mut() {
                s.flagged = true;
            }
        }
        // `Divert`/`Return` are terminal; `TunnelCall`/`ThreadStart` are not.
        if matches!(stmt, Stmt::Divert(_) | Stmt::Return(_))
            && let Some(s) = self.unreachable_stack.last_mut()
        {
            s.saw_terminal = true;
        }

        // E032: explicit `~ return` outside a function. Keys off
        // `ReturnKind`, never `ptr` presence — provenance on a `Return` is
        // uniform carrying-or-not metadata with no semantic load (a
        // provenance-carrying tunnel return is legal and clean).
        if let Stmt::Return(ret) = stmt
            && ret.kind == ReturnKind::Explicit
            && !self.in_function
        {
            let range = ret
                .ptr
                .map_or(rowan::TextRange::default(), |p| p.text_range());
            self.returns.push(Diagnostic {
                file: self.file_id,
                range,
                message: DiagnosticCode::E032.title().to_string(),
                code: DiagnosticCode::E032,
            });
        }

        // E034: a choice set that is entirely fallback choices.
        if let Stmt::ChoiceSet(cs) = stmt
            && !cs.choices.is_empty()
            && cs.choices.iter().all(|c| c.is_fallback)
        {
            self.fallbacks.push(Diagnostic {
                file: self.file_id,
                range: cs.choices[0].ptr.text_range(),
                message: DiagnosticCode::E034.title().to_string(),
                code: DiagnosticCode::E034,
            });
        }
    }
}

/// Extract a source range from a statement, if available.
fn stmt_range(stmt: &Stmt) -> Option<rowan::TextRange> {
    match stmt {
        Stmt::Content(c) => c.ptr.as_ref().map(brink_ir::Provenance::text_range),
        Stmt::Divert(d) => d.ptr.as_ref().map(brink_ir::Provenance::text_range),
        Stmt::TunnelCall(t) => Some(t.ptr.text_range()),
        Stmt::ThreadStart(t) => Some(t.ptr.text_range()),
        Stmt::TempDecl(t) => Some(t.ptr.text_range()),
        Stmt::Assignment(a) => Some(a.ptr.text_range()),
        Stmt::Return(r) => r.ptr.as_ref().map(brink_ir::Provenance::text_range),
        Stmt::ChoiceSet(cs) => cs.choices.first().map(|c| c.ptr.text_range()),
        Stmt::Conditional(c) => Some(c.ptr.text_range()),
        Stmt::Sequence(s) => Some(s.ptr.text_range()),
        Stmt::LabeledBlock(b) => b.label.as_ref().map(|l| l.range),
        // Issue #2108: `AttachElement`/`EndElementRun` carry no
        // `Provenance`/`ptr` of their own.
        Stmt::ExprStmt(_) | Stmt::EndOfLine | Stmt::AttachElement(_) | Stmt::EndElementRun => None,
        Stmt::LogicBlock(lb) => Some(lb.ptr.text_range()),
        Stmt::Await(a) => Some(a.ptr.text_range()),
    }
}

#[cfg(test)]
mod tests {
    use brink_ir::hir::*;
    use brink_ir::provenance::{NodeClass, Provenance};
    use brink_ir::{DiagnosticCode, FileId, HirFile};
    use rowan::{TextRange, TextSize};

    use super::*;

    /// Guards the combined `StructuralChecks` pass against the one real risk of
    /// sharing a walker: the shared walk descends inline conditional/sequence
    /// branches inside content (which the old per-check walks never did). By
    /// grammar those branches can hold a divert (always last) but never a
    /// return, a choice set, or a terminal-then-statement — so no new
    /// `E032`/`E033`/`E034` may fire. (Verified: HIR lowering puts the divert
    /// last, e.g. `{cond: -> a text}` lowers to `[Content, Divert]`.)
    #[test]
    fn inline_branch_diverts_produce_no_spurious_structural_diagnostics() {
        let cases = [
            "A {cond: -> away} B\n=== away ===\n-> END\n",
            "{cond: -> a | -> b}\n=== a ===\n-> END\n=== b ===\n-> END\n",
            "{shuffle: -> a | -> b}\n=== a ===\n-> END\n=== b ===\n-> END\n",
            "{cond: -> a text after divert}\n=== a ===\n-> END\n",
            "Line {cond: -> a} {other: -> b}\n=== a ===\n-> END\n=== b ===\n-> END\n",
        ];
        for src in cases {
            let parsed = brink_syntax::parse(src);
            let tree = parsed.tree();
            let (hir, _, _) = brink_ir::hir::lower(FileId(0), &tree);
            let diags = validate(&[(FileId(0), &hir)]);
            let structural: Vec<_> = diags
                .iter()
                .map(|d| d.code)
                .filter(|c| {
                    matches!(
                        c,
                        DiagnosticCode::E032 | DiagnosticCode::E033 | DiagnosticCode::E034
                    )
                })
                .collect();
            assert!(
                structural.is_empty(),
                "inline-branch diverts must not produce structural diagnostics: {src:?} -> {structural:?}"
            );
        }
    }

    fn empty_hir() -> HirFile {
        HirFile {
            root_content: Block::default(),
            knots: Vec::new(),
            variables: Vec::new(),
            constants: Vec::new(),
            lists: Vec::new(),
            structs: Vec::new(),
            externals: Vec::new(),
            includes: Vec::new(),
            module: None,
            imports: Vec::new(),
            visibility: Vec::new(),
            was_directives: Vec::new(),
            allow_scopes: Vec::new(),
            element_matches: Vec::new(),
            cue_names: Vec::new(),
            native: false,
            claim_handlers: Vec::new(),
        }
    }

    fn dummy_range() -> TextRange {
        TextRange::new(TextSize::new(0), TextSize::new(1))
    }

    fn dummy_knot_ptr() -> Provenance {
        Provenance::synthetic(NodeClass::Knot, dummy_range())
    }

    fn dummy_choice_ptr() -> Provenance {
        Provenance::synthetic(NodeClass::Choice, dummy_range())
    }

    fn dummy_return_ptr() -> Provenance {
        Provenance::synthetic(NodeClass::Return, dummy_range())
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
            body: Block::from_stmts(vec![Stmt::Return(Return {
                ptr: Some(dummy_return_ptr()),
                kind: ReturnKind::Explicit,
                value: None,
                onwards_args: Vec::new(),
            })]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E032);
    }

    /// Issue #1973's own scope note, pinned: parsing/lowering a
    /// value-carrying `return <expr>` at content-ground/prose-body position
    /// (a real native-**grammar** fix) is deliberately independent of
    /// whether a non-function `flow` may *semantically* carry a return
    /// value — that stays an open design question, unchanged here. A
    /// value-carrying `Explicit` return in a non-function knot must still
    /// raise E032 exactly as a bare one does — `fixup_return_kind`
    /// (`lower_native::body`) only demotes a *bare* (`value.is_none()`)
    /// return to `TunnelRedirect`, so this shape reaches `validate` intact.
    #[test]
    fn value_carrying_return_in_non_function_still_emits_e032() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "my_flow".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block::from_stmts(vec![Stmt::Return(Return {
                ptr: Some(dummy_return_ptr()),
                kind: ReturnKind::Explicit,
                value: Some(Expr::Int(5)),
                onwards_args: Vec::new(),
            })]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
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
            body: Block::from_stmts(vec![Stmt::Return(Return {
                ptr: Some(dummy_return_ptr()),
                kind: ReturnKind::Explicit,
                value: Some(Expr::Int(42)),
                onwards_args: Vec::new(),
            })]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
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
            body: Block::from_stmts(vec![Stmt::Return(Return {
                ptr: None,
                kind: ReturnKind::TunnelRedirect,
                value: None,
                onwards_args: Vec::new(),
            })]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        assert!(
            diags.is_empty(),
            "tunnel return should not trigger E032: {diags:?}"
        );
    }

    /// The D5/F-I#6 trap this slice kills: a tunnel return that *carries*
    /// provenance must still classify as a tunnel return — E032 keys off
    /// `ReturnKind`, never `ptr` presence. No ink surface syntax produces
    /// this shape today; a provenance-stamping frontend (native) will.
    #[test]
    fn provenance_carrying_tunnel_return_no_e032() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "my_knot".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block::from_stmts(vec![Stmt::Return(Return {
                ptr: Some(dummy_return_ptr()),
                kind: ReturnKind::TunnelRedirect,
                value: None,
                onwards_args: Vec::new(),
            })]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        assert!(
            diags.is_empty(),
            "provenance-carrying tunnel return must not trigger E032: {diags:?}"
        );
    }

    /// The converse direction: an explicit return synthesized *without*
    /// provenance still errors outside a function — the kind alone decides.
    #[test]
    fn pointerless_explicit_return_still_emits_e032() {
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "my_knot".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block::from_stmts(vec![Stmt::Return(Return {
                ptr: None,
                kind: ReturnKind::Explicit,
                value: None,
                onwards_args: Vec::new(),
            })]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E032);
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
            body: Block::from_stmts(vec![
                Stmt::Divert(Divert {
                    ptr: None,
                    target: DivertTarget {
                        path: DivertPath::Done,
                        args: Vec::new(),
                    },
                }),
                Stmt::Content(Content {
                    ptr: Some(Provenance::synthetic(NodeClass::Content, dummy_range())),
                    parts: vec![ContentPart::Text("unreachable".into())],
                    tags: Vec::new(),
                }),
            ]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
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
            body: Block::from_stmts(vec![
                Stmt::Divert(Divert {
                    ptr: None,
                    target: DivertTarget {
                        path: DivertPath::Done,
                        args: Vec::new(),
                    },
                }),
                Stmt::EndOfLine,
            ]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
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
            body: Block::from_stmts(vec![
                Stmt::ThreadStart(ThreadStart {
                    ptr: Provenance::synthetic(NodeClass::ThreadStart, dummy_range()),
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
                    ptr: Some(Provenance::synthetic(NodeClass::Content, dummy_range())),
                    parts: vec![ContentPart::Text("still reachable".into())],
                    tags: Vec::new(),
                }),
            ]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
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

    #[test]
    fn content_after_tunnel_call_no_warning() {
        // `-> wave ->` returns control to the next statement, so content
        // following a tunnel call is reachable and must not trigger E033.
        let mut hir = empty_hir();
        hir.knots.push(Knot {
            ptr: dummy_knot_ptr(),
            name: Name {
                text: "greet".into(),
                range: dummy_range(),
            },
            is_function: false,
            params: Vec::new(),
            body: Block::from_stmts(vec![
                Stmt::TunnelCall(TunnelCall {
                    ptr: Provenance::synthetic(NodeClass::TunnelCall, dummy_range()),
                    targets: vec![DivertTarget {
                        path: DivertPath::Path(Path {
                            segments: vec![Name {
                                text: "wave".into(),
                                range: dummy_range(),
                            }],
                            range: dummy_range(),
                        }),
                        args: Vec::new(),
                    }],
                }),
                Stmt::Content(Content {
                    ptr: Some(Provenance::synthetic(NodeClass::Content, dummy_range())),
                    parts: vec![ContentPart::Text("and we're off".into())],
                    tags: Vec::new(),
                }),
            ]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
        });

        let files = vec![(FileId(0), &hir)];
        let diags = validate(&files);
        let e033s: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E033)
            .collect();
        assert!(
            e033s.is_empty(),
            "TunnelCall is not terminal — content after it is reachable: {e033s:?}"
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
            body: Block::from_stmts(vec![Stmt::ChoiceSet(Box::new(ChoiceSet {
                choices: vec![Choice {
                    ptr: dummy_choice_ptr(),
                    is_sticky: false,
                    is_fallback: true,
                    label: None,
                    condition: None,
                    binding: None,
                    start_content: None,
                    bracket_content: None,
                    inner_content: None,
                    tags: Vec::new(),
                    body: Block::default(),
                    container_id: None,
                }],
                continuation: Block::default(),
                context: ChoiceSetContext::Weave,
                depth: 1,
                gather_id: None,
            }))]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
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
            body: Block::from_stmts(vec![Stmt::ChoiceSet(Box::new(ChoiceSet {
                choices: vec![
                    Choice {
                        ptr: dummy_choice_ptr(),
                        is_sticky: false,
                        is_fallback: true,
                        label: None,
                        condition: None,
                        binding: None,
                        start_content: None,
                        bracket_content: None,
                        inner_content: None,
                        tags: Vec::new(),
                        body: Block::default(),
                        container_id: None,
                    },
                    Choice {
                        ptr: dummy_choice_ptr(),
                        is_sticky: false,
                        is_fallback: false,
                        label: None,
                        condition: None,
                        binding: None,
                        start_content: None,
                        bracket_content: None,
                        inner_content: None,
                        tags: Vec::new(),
                        body: Block::default(),
                        container_id: None,
                    },
                ],
                continuation: Block::default(),
                context: ChoiceSetContext::Weave,
                depth: 1,
                gather_id: None,
            }))]),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            element_annotation: None,
            convention_annotation: None,
            style_annotation: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
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
