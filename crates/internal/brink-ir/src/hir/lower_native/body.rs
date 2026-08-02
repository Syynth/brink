//! Prose-dialect body lowering: content lines, tags, glue, `{expr}`
//! interpolation, diverts/tunnels/return, and the label-absorption
//! algorithm that dissolves both content-line labels (G-1) and choice-point
//! gathers into `Stmt::LabeledBlock`/`ChoiceSet.continuation`
//! (`docs/b0-sequencing.md` §B0.7).
//!
//! # The dissolved gather, mechanically
//!
//! Native has no gather-dash token; charter §5 says "after the choices
//! rejoin is simply the next line after the block." [`lower_items`]
//! implements this literally: when it meets a `{?}` choice point, it does
//! **not** keep iterating siblings — it recursively lowers everything that
//! follows in the same item stream as the choice set's own
//! `continuation: Block` (via [`lower_continuation`]), then returns. This
//! is exactly old ink's own weave-fold behavior once a gather is reached
//! (`lower/block/weave.rs::flush_choices`: "Gather after choices ... fold
//! them recursively, and nest everything into the continuation") — native
//! just never needs the depth-matching machinery that surrounds it there,
//! because a `{?}` block's extent is never ambiguous.
//!
//! The same absorption shape handles G-1 labeled content lines: a `(name)`
//! label is not itself a gather, but ink's own "standalone labeled gather"
//! is the same concept applied to a labeled *content line* with no
//! preceding choice block — see `weave.rs`'s
//! `last_standalone_label`/`gather_stmts_start` retroactive
//! `Stmt::LabeledBlock` wrap, which [`lower_items`] mirrors directly. One
//! refinement, also lifted from old ink: a label immediately following a
//! closed `{?}` block attaches to `continuation.label` directly rather than
//! wrapping in a nested `LabeledBlock` (`weave.rs`'s `WeaveItem::Continuation`
//! handling, built from `lower_gather_to_block`'s `label: gather.label()`)
//! — see [`lower_continuation`].

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::SyntaxNode;
use brink_syntax_native::ast::{self, AstNode as _};

use crate::Provenance;
use crate::hir::FileId;
use crate::provenance::NodeClass;
use crate::{
    Assignment, Block, BlockStmt, Content, ContentPart, Diagnostic, DiagnosticCode, Divert,
    DivertPath, DivertTarget, ElseBranch, Expr, IfStmt, LogicBlock, Name, Return, ReturnKind,
    SpanPart, Stmt, StringPart, Tag, TempDecl, TunnelCall,
};

use super::choice::lower_choice_point;
use super::cond::{lower_alternation, lower_conditional};
use super::element::Elements;
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

fn name_from(tok: Option<brink_syntax_native::SyntaxToken>) -> Option<Name> {
    tok.map(|t| Name {
        text: t.text().to_string(),
        range: t.text_range(),
    })
}

/// Lower a `flow`/`fn`/nested-`flow` (stitch) body — the B0.7 entry point
/// `container.rs` calls in place of B0.6's `Block::default()` stub.
pub(super) fn lower_block(
    file_id: FileId,
    block: &ast::Block,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> Block {
    let items: Vec<SyntaxNode> = block.items().collect();
    let stmts = lower_items(file_id, &items, 0, elements, diags);
    let tail = crate::tail_from_stmts(&stmts);
    Block {
        label: None,
        stmts,
        container_id: None,
        tail,
    }
}

/// Lower a `flow`/`fn` body selected as **code**-ground — `fn`'s default,
/// or a `flow`'s `~{ }` "Compound guard" override (charter §4, #1309) — to
/// the HIR `Block` a `Knot`/`Stitch` carries.
///
/// The `STMT_BLOCK`'s own statements already have a real lowering target
/// (`control_flow::lower_block_item`, B0.8 Waves A/B/B-tail): the T1b
/// closed `BlockStmt` set. Each maximal **run** of those gets wrapped as one
/// `Stmt::LogicBlock` rather than inventing a flattened `Stmt`-level
/// mapping: `BlockStmt` carries `If`/`While`/`For`/`Break`/`Continue`, none
/// of which have a top-level `Stmt` counterpart to flatten into (only
/// `TempDecl`/`Assignment`/`Return`/`ExprStmt`/`Await` do), so a uniform
/// wrap is both the simplest rule and the one that already has a
/// fully-wired LIR lowering (`lir::lower::blocks::lower_logic_block`
/// splices a `LogicBlock`'s statements directly into the enclosing
/// container's flat sequence) — the exact shape a brink-dialect container
/// whose entire body is one `~ { … }` block already produces, see
/// `crates/internal/brink-ir/tests/b08_native_control_flow.rs`'s
/// `ink_block_stmts` differential helper.
///
/// **`> text` (charter §8.2, issue #1992)** is the one `STMT_BLOCK` item
/// that *isn't* folded into a `LogicBlock` run: a `PROSE_LINE` splits the
/// run there and lowers through the same content-emission path a
/// content-ground body's own `CONTENT_LINE` uses
/// ([`lower_content_line_body`]), producing ordinary `Stmt::Content`/
/// `Stmt::EndOfLine` siblings — content is a weave concept, out of the
/// closed `BlockStmt` set by design (`docs/t1b-surface-spec.md` §2's seam
/// rule), so it can never live *inside* a `LogicBlock`, only *beside* one.
/// A code-ground body with no `> text` line in it (every body before this
/// issue) still lowers to exactly one `LogicBlock`, byte-for-byte the prior
/// shape — this is a strict generalization, not a behavior change for the
/// existing case. Splitting is scoped to *this* function only: a
/// `PROSE_LINE` nested inside an `if`/`while`/`for` body or a lambda's
/// braced body still reaches [`control_flow::lower_block_item`] directly
/// (via [`control_flow::lower_stmt_block`]/`lower_stmt_block_stmts`) and
/// falls to its default `E129` arm — see that function's doc.
///
/// An empty `STMT_BLOCK` (`{}`/`~{}`) produces an empty `Block` — no
/// `LogicBlock` wrapper with zero statements, matching `lower_block`'s own
/// empty-body shape (`Block::default()`-equivalent) rather than a
/// degenerate non-empty-looking node.
pub(super) fn lower_stmt_block_as_body(
    file_id: FileId,
    block: &ast::StmtBlock,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> Block {
    let items: Vec<SyntaxNode> = block.items().collect();
    let mut stmts = lower_code_ground_items(file_id, &items, elements, diags);
    // A body with no `> text` prose-line split still lowers to exactly one
    // `Stmt::LogicBlock` — re-anchor its provenance on the whole
    // `STMT_BLOCK` node (the pre-#1992 shape) rather than
    // `flush_code_ground_run`'s `run_start` anchor (the first statement
    // inside it), which is only a meaningful choice once a split has
    // actually happened (review finding F4).
    if let [Stmt::LogicBlock(lb)] = stmts.as_mut_slice() {
        lb.ptr = native_provenance(file_id, NodeClass::LogicBlock, block.syntax());
    }
    let tail = crate::tail_from_stmts(&stmts);
    Block {
        label: None,
        stmts,
        container_id: None,
        tail,
    }
}

/// Lower a code-ground item stream, splitting runs of ordinary `BlockStmt`
/// items (wrapped as `Stmt::LogicBlock`) around `PROSE_LINE` items (lowered
/// to `Stmt::Content`/`Stmt::EndOfLine` directly) — see
/// [`lower_stmt_block_as_body`]'s doc for why the split lives here rather
/// than inside `BlockStmt` itself. A G-1 `(name)` label on one of those
/// `PROSE_LINE`s has no absorption target in this loop (unlike
/// [`lower_items`]'s weave-ground stream) and is reported loudly (`E129`)
/// rather than silently dropped (review finding F3).
fn lower_code_ground_items(
    file_id: FileId,
    items: &[SyntaxNode],
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let mut stmts = Vec::new();
    let mut run: Vec<&SyntaxNode> = Vec::new();
    let mut run_start: Option<SyntaxNode> = None;

    for item in items {
        if item.kind() == N::PROSE_LINE {
            flush_code_ground_run(file_id, &mut run, &mut run_start, diags, &mut stmts);
            if let Some(pl) = ast::ProseLine::cast(item.clone()) {
                if let Some(cl) = pl.content_line() {
                    // A G-1 `(name)` label on a code-ground `> text` line
                    // has no absorption target here — unlike weave-ground
                    // `lower_items`, which uses a label to decide how much
                    // of the item stream a labeled content line/gather
                    // swallows, this split-run loop has no such stream to
                    // absorb into. Report it loudly (E129) rather than
                    // silently dropping it and leaving a later `-> again`
                    // to fail to resolve elsewhere with no explanation
                    // (review finding F3).
                    if let Some(label) = cl.label() {
                        diags.push(diag(
                            file_id,
                            label.syntax().text_range(),
                            DiagnosticCode::E129,
                        ));
                    }
                    stmts.extend(lower_content_line_body(file_id, &cl, elements, diags));
                } else {
                    diags.push(diag(file_id, item.text_range(), DiagnosticCode::E129));
                }
            } else {
                diags.push(diag(file_id, item.text_range(), DiagnosticCode::E129));
            }
            continue;
        }
        if run.is_empty() {
            run_start = Some(item.clone());
        }
        run.push(item);
    }
    flush_code_ground_run(file_id, &mut run, &mut run_start, diags, &mut stmts);
    mark_split_logic_block_scopes(&mut stmts);
    stmts
}

/// After a `> text` prose-line escape has split one code-ground body into
/// more than one `Stmt::LogicBlock` (review finding F1), those runs must
/// still share **one** T1b lexical scope: a `let`/`temp` declared in an
/// earlier run has to stay visible, for both reads and writes, in every
/// run after it — including any trailing `Stmt::Content` after the *last*
/// split run, since a `> text` prose line can be the final item in the
/// body — not be popped the moment its own run's `LogicBlock` ends, which
/// would otherwise send a later write silently to a phantom global
/// (`lir::lower::stmts::lower_assign_target`'s `resolve_path` fallback
/// finds no block-scoped slot and emits `AssignTarget::Global`) and a
/// later read to a wrong-block-blaming E082 (`lir::lower::expr`'s
/// `block_scoped_temp_names` arm). Tags the first split run `Opens`
/// (pushes the shared scope) and every other one `Continues` (neither
/// pushes nor pops) — see [`crate::LogicBlockScope`]'s doc for why the
/// matching pop lives one level up, in
/// `lir::lower::lower_block_with_children`, rather than on any particular
/// run here. A body with fewer than two `LogicBlock`s (no split at all,
/// the overwhelmingly common case) is left untouched at the default
/// `LogicBlockScope::Standalone` — byte-for-byte the original
/// push-and-pop-per-block shape.
fn mark_split_logic_block_scopes(stmts: &mut [Stmt]) {
    let count = stmts
        .iter()
        .filter(|s| matches!(s, Stmt::LogicBlock(_)))
        .count();
    if count < 2 {
        return;
    }
    let mut seen = 0usize;
    for stmt in stmts.iter_mut() {
        if let Stmt::LogicBlock(lb) = stmt {
            lb.scope = if seen == 0 {
                crate::LogicBlockScope::Opens
            } else {
                crate::LogicBlockScope::Continues
            };
            seen += 1;
        }
    }
}

/// Flush a buffered run of ordinary (non-`PROSE_LINE`) `STMT_BLOCK` items
/// into a single `Stmt::LogicBlock`, appended to `stmts` — the helper
/// [`lower_code_ground_items`]'s loop calls both mid-stream (on hitting a
/// `PROSE_LINE`) and once more after the loop, to flush any trailing run.
/// A no-op when `run` is empty (two `PROSE_LINE`s back to back, or one at
/// either edge of the item stream).
fn flush_code_ground_run(
    file_id: FileId,
    run: &mut Vec<&SyntaxNode>,
    run_start: &mut Option<SyntaxNode>,
    diags: &mut Vec<Diagnostic>,
    stmts: &mut Vec<Stmt>,
) {
    if run.is_empty() {
        return;
    }
    let block_stmts: Vec<_> = run
        .drain(..)
        .filter_map(|item| super::control_flow::lower_block_item(file_id, item, diags))
        .collect();
    if !block_stmts.is_empty()
        && let Some(anchor) = run_start.take()
    {
        stmts.push(Stmt::LogicBlock(LogicBlock {
            ptr: native_provenance(file_id, NodeClass::LogicBlock, &anchor),
            stmts: block_stmts,
            scope: crate::LogicBlockScope::Standalone,
        }));
    }
    *run_start = None;
}

/// The shared item-stream lowering algorithm: dispatches each item, but a
/// labeled content line or a `{?}` choice point **absorbs every item after
/// it** (the dissolved-gather / G-1 mechanism — see the module doc) rather
/// than being folded as an ordinary sibling. Used for every body-shaped
/// item list: knot/fn/stitch bodies, choice bodies, conditional/alternation
/// arm bodies.
pub(super) fn lower_items(
    file_id: FileId,
    items: &[SyntaxNode],
    start: usize,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let mut stmts = Vec::new();
    let mut i = start;
    while i < items.len() {
        let node = &items[i];

        if node.kind() == N::CONTENT_LINE
            && let Some(cl) = ast::ContentLine::cast(node.clone())
            && let Some(label) = cl.label().and_then(|l| name_from(l.name_token()))
        {
            let mut inner = lower_content_line_body(file_id, &cl, elements, diags);
            inner.extend(lower_items(file_id, items, i + 1, elements, diags));
            let inner_tail = crate::tail_from_stmts(&inner);
            stmts.push(Stmt::LabeledBlock(Box::new(Block {
                label: Some(label),
                stmts: inner,
                container_id: None,
                tail: inner_tail,
            })));
            return stmts;
        }

        if node.kind() == N::CHOICE_POINT {
            if let Some(cp) = ast::ChoicePoint::cast(node.clone()) {
                let continuation = lower_continuation(file_id, items, i + 1, elements, diags);
                stmts.extend(lower_choice_point(
                    file_id,
                    &cp,
                    continuation,
                    elements,
                    diags,
                ));
            }
            return stmts;
        }

        stmts.extend(lower_one_item(file_id, node, elements, diags));
        i += 1;
    }
    stmts
}

/// Build a `{?}` choice point's continuation `Block` from whatever follows
/// it in the item stream. If the very next item is a labeled content line,
/// its label attaches directly to `continuation.label` (the gather-label
/// convention — see the module doc) instead of nesting a `LabeledBlock`
/// one level in.
fn lower_continuation(
    file_id: FileId,
    items: &[SyntaxNode],
    start: usize,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> Block {
    if let Some(node) = items.get(start)
        && node.kind() == N::CONTENT_LINE
        && let Some(cl) = ast::ContentLine::cast(node.clone())
        && let Some(label) = cl.label().and_then(|l| name_from(l.name_token()))
    {
        let mut stmts = lower_content_line_body(file_id, &cl, elements, diags);
        stmts.extend(lower_items(file_id, items, start + 1, elements, diags));
        let tail = crate::tail_from_stmts(&stmts);
        return Block {
            label: Some(label),
            stmts,
            container_id: None,
            tail,
        };
    }
    let stmts = lower_items(file_id, items, start, elements, diags);
    let tail = crate::tail_from_stmts(&stmts);
    Block {
        label: None,
        stmts,
        container_id: None,
        tail,
    }
}

/// Dispatch a single body item that is neither a labeled content line nor a
/// choice point (both handled by [`lower_items`] itself, since they can
/// absorb the rest of the stream).
fn lower_one_item(
    file_id: FileId,
    node: &SyntaxNode,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    // Natural-notation element dispatch (issue #1838): a claiming
    // `@[element(claims = "…")]` handler takes the line before any ordinary
    // reading of it. Declines for every file with no claiming handler and
    // for every line no pattern matches, so the fall-through below is the
    // pre-#1838 behavior byte for byte.
    if let Some(claimed) = super::element::try_claim(file_id, node, elements) {
        return claimed;
    }
    // `!name` sigil dispatch (issue #2004): tried right after claiming,
    // same "harmless when it isn't its own node kind" posture — see
    // `element::try_dispatch`'s own doc for what "not dispatched" falls
    // through to (this function's own loud-`E129` default arm below, for a
    // `BANG_DISPATCH` node kind no other arm here claims).
    if let Some(dispatched) = super::element::try_dispatch(file_id, node, elements) {
        return dispatched;
    }
    match node.kind() {
        N::CONTENT_LINE => {
            let Some(cl) = ast::ContentLine::cast(node.clone()) else {
                return Vec::new();
            };
            lower_content_line_body(file_id, &cl, elements, diags)
        }
        // A header-scoped scene stitch (§8b.2): the heading line is offered
        // to the dispatcher, and the braceless body it opens lowers in
        // place. Only reachable once something claims the heading — an
        // unclaimed heading falls to the loud-`E129` arm below exactly as
        // it did before, since promoting a heading to a real HIR stitch is
        // issue #1717's slice, not this one's.
        N::SCENE_STITCH => {
            let heading = node.children().find(|n| n.kind() == N::SCENE_HEADING);
            let Some(claimed) = heading
                .as_ref()
                .and_then(|h| super::element::try_claim(file_id, h, elements))
            else {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
                return Vec::new();
            };
            let mut stmts = claimed;
            if let Some(body) = node.children().find(|n| n.kind() == N::SCENE_BODY) {
                let items: Vec<SyntaxNode> = body.children().collect();
                stmts.extend(lower_items(file_id, &items, 0, elements, diags));
            }
            stmts
        }
        N::TAG_LINE => {
            let Some(tl) = ast::TagLine::cast(node.clone()) else {
                return Vec::new();
            };
            let tags: Vec<Tag> = tl
                .tags()
                .map(|t| lower_tag(file_id, &t, diags))
                .collect();
            if tags.is_empty() {
                Vec::new()
            } else {
                vec![
                    Stmt::Content(Content {
                        ptr: None,
                        parts: Vec::new(),
                        tags,
                    }),
                    Stmt::EndOfLine,
                ]
            }
        }
        N::DIVERT_STMT | N::TUNNEL_CALL => lower_divert_like(file_id, node, diags)
            .into_iter()
            .collect(),
        // `~ stmt` — the content-ground logic-line escape into code
        // (charter §8.2, RULED 2026-07-23, issue #1991: ink's logic line,
        // kept). See `lower_logic_line`'s doc.
        N::LOGIC_LINE => {
            let Some(ll) = ast::LogicLine::cast(node.clone()) else {
                return Vec::new();
            };
            lower_logic_line(file_id, &ll, diags)
        }
        // `return <expr>` (issue #1973) — see `lower_return_value`'s doc.
        N::RETURN_STMT => vec![Stmt::Return(Return {
            ptr: Some(native_provenance(file_id, NodeClass::Return, node)),
            kind: ReturnKind::Explicit,
            value: lower_return_value(file_id, node, diags),
            onwards_args: Vec::new(),
        })],
        N::RETURN_REDIRECT => lower_return_redirect(file_id, node, diags),
        N::CONDITIONAL_BLOCK => {
            let Some(cb) = ast::ConditionalBlock::cast(node.clone()) else {
                return Vec::new();
            };
            vec![Stmt::Conditional(lower_conditional(file_id, &cb, elements, diags))]
        }
        N::ALTERNATION_BLOCK => {
            let Some(ab) = ast::AlternationBlock::cast(node.clone()) else {
                return Vec::new();
            };
            vec![Stmt::Sequence(lower_alternation(file_id, &ab, elements, diags, true))]
        }
        // Declarations reachable at body position are handled by other
        // passes: `flow`/`fn` become stitches (`container.rs`), `var`/
        // `const`/`flags` are hoisted flat by `lower_native::lower`'s
        // whole-tree walk, and `struct`/`extern`/`use`/`import`/`module`
        // nested here are already diagnosed E129 by that same function's
        // out-of-position pass. Re-emitting statements or diagnostics for
        // any of them here would double up, not fill a gap.
        N::FLOW_DECL
        | N::FN_DECL
        | N::VAR_DECL
        | N::CONST_DECL
        | N::FLAGS_DECL
        | N::STRUCT_DECL
        | N::EXTERN_DECL
        | N::USE_DECL
        | N::IMPORT_DECL
        | N::MODULE_DECL
        // Already diagnosed by the parser itself.
        | N::ERROR
        // A container's own inner `//!` doc comment (B0.6b) — already
        // consumed by `container.rs::container_doc` reading `body.doc()`
        // directly off the AST before this item-stream lowering ever runs;
        // not a body statement, not an error, just skipped here so it
        // doesn't fall into the loud-E129 default arm below.
        | N::DOC_COMMENT => Vec::new(),
        // `@[…]` annotations at body position (issue #1563): an
        // `@[effects(…)]` above a nested `flow` is consumed by
        // `container::lower_stitch`; anything else is diagnosed by the
        // channel's own chokepoint. Either way an annotation line never
        // lowers to content.
        N::ANNOTATION_LINE => {
            super::annotation::handle_line(file_id, node, diags);
            Vec::new()
        }
        _ => {
            diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
            Vec::new()
        }
    }
}

/// `~ stmt` — the content-ground logic-line escape into code (charter
/// §8.2, RULED 2026-07-23 `docs/decision-log.md` "Native interleaving &
/// body-dialect spelling", issue #1991: ink's logic line, kept). Targets
/// the top-level `Stmt::TempDecl`/`Stmt::Assignment`/`Stmt::ExprStmt`
/// variants — an already-proven HIR/LIR/codegen/runtime path, since those
/// are exactly what the ink-dialect's own logic line already lowers to
/// (`hir::lower::content::logic_line::LogicLineOutput`) — rather than
/// inventing a new one. `TempDecl` (`~ let name = expr`) is issue #1972's
/// addition to the two shapes #1991 originally wired; only the three
/// `LogicLine` shapes `parser/stmt.rs::logic_line` can produce are matched
/// — a `LOGIC_LINE` with none of them (should not happen given that
/// parser, but CST nodes from a malformed parse are never assumed
/// well-formed) is diagnosed loudly (E129) rather than silently dropped.
fn lower_logic_line(
    file_id: FileId,
    ll: &ast::LogicLine,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let range = ll.syntax().text_range();
    if let Some(let_stmt) = ll.let_stmt() {
        return lower_logic_line_temp_decl(file_id, &let_stmt, diags).map_or_else(Vec::new, |td| {
            let needs_eol = td.value.as_ref().is_some_and(expr_contains_call);
            let mut out = vec![Stmt::TempDecl(td)];
            if needs_eol {
                out.push(Stmt::EndOfLine);
            }
            out
        });
    }
    if let Some(assign) = ll.assign_stmt() {
        return lower_logic_line_assignment(file_id, &assign, diags).map_or_else(Vec::new, |a| {
            let needs_eol = expr_contains_call(&a.value);
            let mut out = vec![Stmt::Assignment(a)];
            if needs_eol {
                out.push(Stmt::EndOfLine);
            }
            out
        });
    }
    if let Some(expr_stmt) = ll.expr_stmt() {
        return lower_logic_line_expr_stmt(file_id, &expr_stmt, diags);
    }
    diags.push(diag(file_id, range, DiagnosticCode::E129));
    Vec::new()
}

/// `~ let name: type = expr` — the content-ground `Stmt::TempDecl` shares
/// its name/value/annotation handling verbatim with
/// `lower_native::control_flow::lower_temp_decl` (same `LET_STMT` node
/// shape, reused unmodified by the parser — see [`lower_logic_line`]'s
/// doc), so it delegates there directly rather than duplicating the logic;
/// only the wrapper differs (`Stmt::TempDecl` here vs. that function's own
/// `StmtBlock` call site, which wraps as `BlockStmt::TempDecl`).
fn lower_logic_line_temp_decl(
    file_id: FileId,
    temp: &ast::LetStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<TempDecl> {
    super::control_flow::lower_temp_decl(file_id, temp, diags)
}

/// `~ x = expr` / `~ x += expr` / `~ x -= expr` — the content-ground
/// `Stmt::Assignment` shares its place/value/op-token handling verbatim
/// with `lower_native::control_flow::lower_assignment` (same `ASSIGN_STMT`
/// node shape, reused verbatim by the parser — see `SyntaxKind::LOGIC_LINE`'s
/// doc), so it delegates there directly rather than duplicating the logic;
/// only the wrapper differs (`Stmt::Assignment` here vs. that function's
/// own `BlockStmt::Assignment` at its `StmtBlock` call site).
fn lower_logic_line_assignment(
    file_id: FileId,
    assign: &ast::AssignStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<Assignment> {
    super::control_flow::lower_assignment(file_id, assign, diags)
}

/// `~ expr` — an expression evaluated for its side effect (a function call
/// being the overwhelmingly common case). Appends `Stmt::EndOfLine` when
/// the expression contains a call, matching inklecate's behavior for a
/// call-only logic line — the same rule the ink-dialect frontend already
/// applies (`hir::lower::content::logic_line::LogicLineOutput::has_call`);
/// this is the identical semantic construct ("ink's logic line, kept"), so
/// it needs the identical trailing-boundary behavior on the same runtime.
fn lower_logic_line_expr_stmt(
    file_id: FileId,
    stmt: &ast::ExprStmt,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let range = stmt.syntax().text_range();
    let Some(expr_node) = stmt.expr() else {
        diags.push(diag(file_id, range, DiagnosticCode::E015));
        return Vec::new();
    };
    let expr = lower_expr(file_id, &expr_node, diags);
    let needs_eol = expr_contains_call(&expr);
    let mut out = vec![Stmt::ExprStmt(expr)];
    if needs_eol {
        out.push(Stmt::EndOfLine);
    }
    out
}

/// Whether `expr`'s tree contains a function call. Deliberately mirrors
/// `hir::lower::helpers::expr_contains_call` (that module is private to the
/// ink-dialect's own `lower` tree, so this is a small, intentional
/// duplication rather than a cross-dialect reach-through) — see
/// [`lower_logic_line_expr_stmt`]'s doc for why the two dialects need the
/// identical answer here.
fn expr_contains_call(expr: &Expr) -> bool {
    match expr {
        Expr::Call(..) => true,
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => expr_contains_call(inner),
        Expr::Infix(ie) => expr_contains_call(&ie.lhs) || expr_contains_call(&ie.rhs),
        Expr::String(s) => s
            .parts
            .iter()
            .any(|p| matches!(p, StringPart::Interpolation(e) if expr_contains_call(e))),
        _ => false,
    }
}

/// The `return`/tunnel-return container-exit unification (charter §11,
/// `tests/tier1-brink-respell/basic-tunnel/manifest.toml`): a bare `return`
/// (no redirect target) means two different things in ink — `~ return`
/// inside a function, bare `->->` (tunnel return) everywhere else — that
/// native's single `return` keyword cannot distinguish syntactically. The
/// `N::RETURN_STMT` arm above always stamps `ReturnKind::Explicit` (it has
/// no access to the enclosing container's `is_function` flag at the point
/// a single item is dispatched), so this post-lowering fixup corrects
/// every bare return reachable in a non-function container's body to
/// `ReturnKind::TunnelRedirect` — `brink-analyzer`'s E032 check keys off
/// `kind`, never syntax (B0.2), so this is the one seam that needs
/// correcting for `flow`s reached via tunnel call to lower cleanly.
///
/// Called once per top-level `Knot`/`Stitch` by `container.rs` right after
/// its body is built (`is_function` is known there, not down in the
/// per-item dispatch), walking every reachable structural nesting point —
/// choice bodies/continuations, conditional branches, sequence branches,
/// labeled blocks, and code-ground `Stmt::LogicBlock` (`~{ }` bodies, plus
/// nested `if`/`while`/`for` bodies within one, via
/// [`fixup_return_kind_in_block_stmts`]) — and recomputing each touched
/// `Block`'s `tail` (S1, docs/block-effect-model.md §10 row j), since
/// mutating a tail-position `Return` in place leaves a stale `tail`
/// otherwise. `LogicBlock`/`BlockStmt` carry no `tail` field of their own
/// (S1 only populates `Block`), so no recompute is needed on that side.
///
/// **Known scope gap, flagged rather than silently mishandled**: a
/// `return` reached only through a content-embedded inline conditional/
/// sequence (`ContentPart::InlineConditional`/`InlineSequence`, e.g.
/// `Hi {if x { return }}`) is not walked here — inline positions are
/// content, not a realistic return site, and this fixup strictly improves
/// the already-broken baseline (every bare return, everywhere, previously
/// stamped `Explicit` unconditionally) without regressing that corner.
pub(super) fn fixup_return_kind(is_function: bool, block: &mut Block) {
    for stmt in &mut block.stmts {
        match stmt {
            Stmt::Return(r) => {
                if !is_function && r.kind == ReturnKind::Explicit && r.value.is_none() {
                    r.kind = ReturnKind::TunnelRedirect;
                }
            }
            Stmt::ChoiceSet(cs) => {
                for choice in &mut cs.choices {
                    fixup_return_kind(is_function, &mut choice.body);
                }
                fixup_return_kind(is_function, &mut cs.continuation);
            }
            Stmt::LabeledBlock(b) => fixup_return_kind(is_function, b),
            Stmt::Conditional(c) => {
                for branch in &mut c.branches {
                    fixup_return_kind(is_function, &mut branch.body);
                }
            }
            Stmt::Sequence(s) => {
                for branch in &mut s.branches {
                    fixup_return_kind(is_function, &mut branch.body);
                }
            }
            Stmt::LogicBlock(lb) => fixup_return_kind_in_block_stmts(is_function, &mut lb.stmts),
            Stmt::Content(_)
            | Stmt::Divert(_)
            | Stmt::TunnelCall(_)
            | Stmt::ThreadStart(_)
            | Stmt::TempDecl(_)
            | Stmt::Assignment(_)
            | Stmt::ExprStmt(_)
            | Stmt::EndOfLine
            | Stmt::Await(_) => {}
        }
    }
    block.recompute_tail();
}

/// [`fixup_return_kind`]'s recursion into a code-ground `~{ }` body
/// (`LogicBlock::stmts`/`BlockStmt`, B0.8's closed statement set — see that
/// enum's doc: no weave concept reaches this side, so there is no
/// `ChoiceSet`/`Conditional`/`Sequence`/`LabeledBlock` arm to mirror here).
/// Walks `if`/`while`/`for` bodies (and `else`/`else if` chains) to reach
/// every `BlockStmt::Return` a logic block can nest, applying the same
/// non-function-bare-return → `TunnelRedirect` correction
/// [`fixup_return_kind`] applies at weave level.
fn fixup_return_kind_in_block_stmts(is_function: bool, stmts: &mut [BlockStmt]) {
    for stmt in stmts {
        match stmt {
            BlockStmt::Return(r) => {
                if !is_function && r.kind == ReturnKind::Explicit && r.value.is_none() {
                    r.kind = ReturnKind::TunnelRedirect;
                }
            }
            BlockStmt::If(i) => fixup_return_kind_in_if_stmt(is_function, i),
            BlockStmt::While(w) => fixup_return_kind_in_block_stmts(is_function, &mut w.body),
            BlockStmt::For(f) => fixup_return_kind_in_block_stmts(is_function, &mut f.body),
            BlockStmt::TempDecl(_)
            | BlockStmt::Assignment(_)
            | BlockStmt::Break(_)
            | BlockStmt::Continue(_)
            | BlockStmt::ExprStmt(_)
            | BlockStmt::Await(_) => {}
        }
    }
}

/// [`fixup_return_kind_in_block_stmts`]'s `if`/`else if`/`else` walk —
/// split out since `ElseBranch::ElseIf` recurses on `IfStmt` itself, not on
/// a `[BlockStmt]` slice.
fn fixup_return_kind_in_if_stmt(is_function: bool, i: &mut IfStmt) {
    fixup_return_kind_in_block_stmts(is_function, &mut i.body);
    match &mut i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => fixup_return_kind_in_if_stmt(is_function, inner),
        Some(ElseBranch::Else(stmts)) => fixup_return_kind_in_block_stmts(is_function, stmts),
        None => {}
    }
}

/// Grant a native `flow`/`stitch` body ink's ROOT-content implicit-end
/// grace: a body that falls off the end (`Tail::Unit` — no trailing divert
/// or return) gets a synthesized `-> DONE` terminator so the VM does not
/// raise "ran out of content" (RULED 2026-07-22, `docs/decision-log.md` →
/// native implicit end; charter §15).
///
/// **Native-only, and deliberately mirroring the ink pipeline's own
/// root-content terminator** in [`crate::lir::lower::assemble_program`]
/// (which appends `lir::DivertTarget::Done` when the assembled root body
/// lacks a trailing divert). Ink grants literal ROOT content that grace but
/// not a knot reached by an ordinary divert; native's `flow main()` entry
/// convention (charter §15) moves former root content into exactly such a
/// divert-reached knot, which would otherwise lose it. We restore it here at
/// the flow level rather than the LIR root level so it rides the same HIR
/// `DivertPath::Done` representation every other native `-> DONE` uses — no
/// new opcode.
///
/// `-> DONE`, never `-> END`: the flow's turn completes; the story is not
/// permanently ended. Applied to the **top-level** body block only (mirroring
/// `assemble_program`, which appends to `root_body` alone) — a nested block
/// that falls through rejoins its gather/continuation and must not be
/// terminated. Caller excludes functions: a `fn` that runs off the end does
/// an implicit *return*, not a DONE.
pub(super) fn apply_implicit_done(block: &mut Block) {
    if !matches!(block.tail, crate::Tail::Unit) {
        return;
    }
    block.stmts.push(Stmt::Divert(Divert {
        ptr: None,
        target: DivertTarget {
            path: DivertPath::Done,
            args: Vec::new(),
        },
    }));
    block.recompute_tail();
}

/// Lower one `CONTENT_LINE`'s own content (its `LABEL` child, if any, is
/// skipped here — [`lower_items`]/[`lower_continuation`] already consumed
/// it for the absorption decision before calling this).
fn lower_content_line_body(
    file_id: FileId,
    cl: &ast::ContentLine,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let line_prov = native_provenance(file_id, NodeClass::Content, cl.syntax());
    let children: Vec<SyntaxNode> = cl
        .syntax()
        .children()
        .filter(|n| n.kind() != N::LABEL)
        .collect();
    lower_content_run(file_id, &children, Some(line_prov), elements, diags, true)
}

/// The shared "run of content-shaped items" lowering engine — used for a
/// content line's own body-item lowering and for alternation branches
/// (`cond.rs`). Handles inline `{expr}` interpolation, `<>` glue, embedded
/// diverts/tunnels (N-1: a `->` mid-run is a real node, not swallowed as
/// text), inline conditional/alternation (`ContentPart::InlineConditional`/
/// `InlineSequence`), and — uniquely among the content-lowering helpers —
/// an embedded `{?}` choice point, which absorbs the remainder of `items`
/// as its continuation exactly like [`lower_items`] does at body-item
/// granularity (the same dissolved-gather mechanism, one level down).
///
/// `line_prov` becomes the `ptr` of the (possibly only) `Content` statement
/// this run's trailing flush produces; interior flushes (before an embedded
/// divert/choice-point) always carry `ptr: None`, matching old ink's own
/// accumulator convention (`content/accumulator.rs::flush` uses `ptr: None`
/// for every flush except the line's own top-level one).
///
/// `trailing_eol`: whether the run's *final* flush may append
/// `Stmt::EndOfLine` (when the content doesn't end with glue). `true` for a
/// genuine content line (and for a `{?}` continuation, which behaves like
/// ordinary subsequent lines). `false` for a synthesized fragment that
/// isn't a whole line — an inline alternation branch (`cond.rs`'s
/// `finish_inline_branch`): `{& a cat|a dog}.` must not force a line break
/// after "a cat" before the trailing "." resolves on the same line.
pub(super) fn lower_content_run(
    file_id: FileId,
    items: &[SyntaxNode],
    line_prov: Option<Provenance>,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
    trailing_eol: bool,
) -> Vec<Stmt> {
    let mut out = Vec::new();
    let mut parts: Vec<ContentPart> = Vec::new();
    let mut tags: Vec<Tag> = Vec::new();
    let mut i = 0;

    while i < items.len() {
        let node = &items[i];
        match node.kind() {
            N::TEXT => {
                push_text(&mut parts, node);
                i += 1;
            }
            N::ESCAPE => {
                push_escape(&mut parts, node);
                i += 1;
            }
            N::INTERPOLATION => {
                parts.push(lower_interpolation(file_id, node, diags));
                i += 1;
            }
            N::SPAN => {
                parts.push(lower_span(file_id, node, elements, diags));
                i += 1;
            }
            N::GLUE_NODE => {
                parts.push(ContentPart::Glue);
                i += 1;
            }
            N::TAG => {
                if let Some(t) = ast::Tag::cast(node.clone()) {
                    tags.push(lower_tag(file_id, &t, diags));
                }
                i += 1;
            }
            N::DIVERT_STMT | N::TUNNEL_CALL => {
                flush_content(&mut parts, &mut tags, &mut out, None, false);
                out.extend(lower_divert_like(file_id, node, diags));
                i += 1;
            }
            N::CHOICE_POINT => {
                flush_content(&mut parts, &mut tags, &mut out, None, false);
                if let Some(cp) = ast::ChoicePoint::cast(node.clone()) {
                    let stmts = lower_content_run(
                        file_id,
                        &items[i + 1..],
                        line_prov,
                        elements,
                        diags,
                        true,
                    );
                    let tail = crate::tail_from_stmts(&stmts);
                    let continuation = Block {
                        label: None,
                        stmts,
                        container_id: None,
                        tail,
                    };
                    out.extend(lower_choice_point(
                        file_id,
                        &cp,
                        continuation,
                        elements,
                        diags,
                    ));
                }
                return out;
            }
            N::CONDITIONAL_BLOCK => {
                if let Some(cb) = ast::ConditionalBlock::cast(node.clone()) {
                    parts.push(ContentPart::InlineConditional(lower_conditional(
                        file_id, &cb, elements, diags,
                    )));
                }
                i += 1;
            }
            N::ALTERNATION_BLOCK => {
                if let Some(ab) = ast::AlternationBlock::cast(node.clone()) {
                    parts.push(ContentPart::InlineSequence(lower_alternation(
                        file_id, &ab, elements, diags, false,
                    )));
                }
                i += 1;
            }
            N::ERROR => {
                i += 1;
            }
            _ => {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
                i += 1;
            }
        }
    }

    flush_content(&mut parts, &mut tags, &mut out, line_prov, trailing_eol);
    out
}

fn flush_content(
    parts: &mut Vec<ContentPart>,
    tags: &mut Vec<Tag>,
    out: &mut Vec<Stmt>,
    ptr: Option<Provenance>,
    allow_eol: bool,
) {
    if parts.is_empty() && tags.is_empty() {
        return;
    }
    let ends_glue = matches!(parts.last(), Some(ContentPart::Glue));
    out.push(Stmt::Content(Content {
        ptr,
        parts: std::mem::take(parts),
        tags: std::mem::take(tags),
    }));
    if allow_eol && !ends_glue {
        out.push(Stmt::EndOfLine);
    }
}

pub(super) fn push_text(parts: &mut Vec<ContentPart>, node: &SyntaxNode) {
    push_literal(parts, &node.text().to_string());
}

/// `ESCAPE` (`BACKSLASH` + the one escaped token, §8d.6) → the literal
/// character it produces. No unescape table needed the way
/// `expr::unescape_string_token` needs one for `STRING_ESCAPE`'s `\n`/`\t`:
/// every token this escape set can carry (`LT`/`L_BRACE`/`HASH`/
/// `BACKSLASH`) already spells exactly the one character it produces —
/// `<`/`{`/`#`/`\` respectively — as its own source text.
pub(super) fn push_escape(parts: &mut Vec<ContentPart>, node: &SyntaxNode) {
    if let Some(escaped) = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .nth(1)
    {
        push_literal(parts, escaped.text());
    }
}

/// Append literal text to `parts`, merging into a trailing `Text` part
/// instead of always starting a new one. Before `ESCAPE`/`SPAN` existed,
/// `content_items_until`'s CST-level scanner only ever produced one maximal
/// `TEXT` run per structural gap, so two adjacent `ContentPart::Text`s were
/// never possible and this merge was a no-op by construction; `Hello \<
/// world` (`TEXT`, `ESCAPE`, `TEXT`) is now a real case where NOT merging
/// would fragment one plain line into three parts, defeating
/// `try_recognize`'s Phase-1 "exactly one `Text` part" `Plain` recognition
/// for no reason (the line has no actual dynamic content).
fn push_literal(parts: &mut Vec<ContentPart>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(ContentPart::Text(last)) = parts.last_mut() {
        last.push_str(s);
    } else {
        parts.push(ContentPart::Text(s.to_string()));
    }
}

/// Lower a `SPAN` node (§4, issue #1716) into `ContentPart::Span`,
/// recursively. Shared by [`lower_content_run`] (a span at content-line top
/// level or nested inside another span) and `choice::lower_choice_region`
/// (a span inside a choice's display/bracket/inner text) — one lowering,
/// every content-scanning context.
///
/// A span's own fragment scope (§4.3) admits text, interpolation, glue,
/// escapes, nested spans, and — logic nesting freely inside markup —
/// conditional/alternation blocks. A `DIVERT_STMT`/`TUNNEL_CALL`/
/// `CHOICE_POINT`/`TAG` inside a span has no `ContentPart` shape to hold
/// it: loud `E129`, the same posture `lower_choice_region`'s own fallback
/// arm takes for a nested `{?}` it cannot represent either — never a
/// silent drop.
pub(super) fn lower_span(
    file_id: FileId,
    node: &SyntaxNode,
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> ContentPart {
    let mut name = String::new();
    let mut attrs = Vec::new();
    let mut children: Vec<ContentPart> = Vec::new();
    for child in node.children() {
        match child.kind() {
            N::SPAN_NAME => name = child.text().to_string(),
            N::SPAN_ATTR => attrs.push(lower_span_attr(&child)),
            N::TEXT => push_text(&mut children, &child),
            N::ESCAPE => push_escape(&mut children, &child),
            N::INTERPOLATION => children.push(lower_interpolation(file_id, &child, diags)),
            N::GLUE_NODE => children.push(ContentPart::Glue),
            N::SPAN => children.push(lower_span(file_id, &child, elements, diags)),
            N::CONDITIONAL_BLOCK => {
                if let Some(cb) = ast::ConditionalBlock::cast(child) {
                    children.push(ContentPart::InlineConditional(lower_conditional(
                        file_id, &cb, elements, diags,
                    )));
                }
            }
            N::ALTERNATION_BLOCK => {
                if let Some(ab) = ast::AlternationBlock::cast(child) {
                    children.push(ContentPart::InlineSequence(lower_alternation(
                        file_id, &ab, elements, diags, false,
                    )));
                }
            }
            N::ERROR => {}
            _ => diags.push(diag(file_id, child.text_range(), DiagnosticCode::E129)),
        }
    }
    ContentPart::Span(SpanPart {
        ptr: native_provenance(file_id, NodeClass::Span, node),
        name,
        attrs,
        children,
    })
}

/// One `SPAN_ATTR` (`name="value"`, static text only — see
/// `SyntaxKind::SPAN_ATTR_VALUE`'s doc) → `(name, value)`.
fn lower_span_attr(node: &SyntaxNode) -> (String, String) {
    let mut name = String::new();
    let mut value = String::new();
    for el in node.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(t) if t.kind() == N::IDENT => {
                name = t.text().to_string();
            }
            rowan::NodeOrToken::Node(n) if n.kind() == N::SPAN_ATTR_VALUE => {
                value = attr_value_text(&n);
            }
            _ => {}
        }
    }
    (name, value)
}

/// A `SPAN_ATTR_VALUE`'s decoded text — reuses `expr::unescape_string_token`
/// for `STRING_ESCAPE`, exactly `expr::lower_string_lit`'s own decode loop
/// minus the `INTERPOLATION` arm (attribute values don't support it).
fn attr_value_text(node: &SyntaxNode) -> String {
    let mut s = String::new();
    for tok in node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
    {
        match tok.kind() {
            N::STRING_TEXT => s.push_str(tok.text()),
            N::STRING_ESCAPE => s.push_str(super::expr::unescape_string_token(tok.text())),
            _ => {}
        }
    }
    s
}

pub(super) fn lower_interpolation(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> ContentPart {
    if let Some(inner) = node.children().next() {
        ContentPart::Interpolation(lower_expr(file_id, &inner, diags))
    } else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        ContentPart::Interpolation(Expr::Null)
    }
}

fn lower_tag(file_id: FileId, t: &ast::Tag, diags: &mut Vec<Diagnostic>) -> Tag {
    let mut text = String::new();
    // Skip only the tag's own *leading* `HASH` (the `#` `tag()` consumes via
    // `p.expect(HASH)` as its very first token) — not every `HASH` in the
    // node. Before issue #1738's escape fix this distinction never mattered
    // (a `HASH` anywhere else in the node was structurally impossible:
    // `tag()`'s free-text scan always stopped *at* an interior `#`, ending
    // the node right there). Now that `\#` lets a literal `#` survive inside
    // a tag's own text (parser::content::tag's doc comment), an
    // unconditional `HASH => continue` here would silently strip that
    // escaped hash back out during lowering — dropping the very character
    // the escape exists to preserve and leaving its paired backslash
    // dangling with nothing after it.
    let mut skipped_leading_hash = false;
    for tok in t
        .syntax()
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
    {
        if !skipped_leading_hash && tok.kind() == N::HASH {
            skipped_leading_hash = true;
            continue;
        }
        text.push_str(tok.text());
    }
    let trimmed = text.trim().to_string();
    if let Some(rest) = trimmed.strip_prefix('@') {
        diags.push(directive_like_tag_diagnostic(
            file_id,
            t.syntax().text_range(),
            rest,
        ));
    }
    let parts = if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![ContentPart::Text(trimmed)]
    };
    Tag {
        parts,
        ptr: native_provenance(file_id, NodeClass::Tag, t.syntax()),
    }
}

/// `E172` (issue #1835): a native tag whose text starts with `@` has the
/// *shape* of an ink-dialect compiler directive, but native's tag lowering
/// has no directive channel: `#` is already the runtime-tag sigil in native
/// content position, which is exactly why `#@…` parses as an ordinary tag
/// rather than a directive here too. Four message shapes, by name:
///
/// - `was`/`effects` — real ink directive names
///   (`hir::lower::directive::parse_directive_tag`'s recognized set) that
///   *do* have a native `@[name(…)]` annotation counterpart
///   (`hir::lower_native::annotation`) — the message names it directly.
/// - `module`/`public`/`private`/`local` — real ink directive names with
///   no native annotation-channel equivalent yet.
/// - `allow` — not an ink directive name at all (ink's own recognizer
///   only knows the six names above; `#@allow` is an *unknown* directive
///   there too, per review of #1953). Gets its own wording naming the
///   unrelated native `@[allow(…)]` diagnostic-suppression channel, rather
///   than being folded in with `was`/`effects` as if ink recognized it.
/// - anything else — an unrecognized name. The message only asserts the
///   tag has the *shape* of a directive (leading `@`), never that ink
///   itself would recognize it — a project may deliberately tag content
///   with its own `@`-led runtime convention (e.g. `#@narrator`), and this
///   arm must not tell that author their tag "is an ink compiler directive"
///   when it verifiably isn't one.
fn directive_like_tag_diagnostic(file: FileId, range: rowan::TextRange, rest: &str) -> Diagnostic {
    let name: String = rest
        .chars()
        .take_while(|c| *c != '(' && !c.is_whitespace())
        .collect();
    let message = match name.as_str() {
        "was" | "effects" => format!(
            "`#@{name}` is the ink-dialect directive-tag spelling; native's equivalent is the \
             `@[{name}(…)]` annotation — this tag has no directive effect here and compiles as \
             literal runtime tag content"
        ),
        "module" | "public" | "private" | "local" => format!(
            "`#@{name}` is an ink-dialect compiler-directive spelling (`module`/`public`/\
             `private`/`local`/`was`/`effects`); native has no directive channel and no \
             `{name}` equivalent — this tag has no directive effect here and compiles as \
             literal runtime tag content"
        ),
        "allow" => "`#@allow` has no directive meaning in either dialect — ink's directive \
             recognizer only knows `module`/`public`/`private`/`local`/`was`/`effects`; \
             native's `@[allow(…)]` annotation is an unrelated diagnostic-suppression channel \
             — this tag compiles as literal runtime tag content"
            .to_string(),
        _ => format!(
            "`#@{name}` has the shape of an ink-dialect compiler-directive tag, but native has \
             no directive channel and ink has no `{name}` directive either — this tag compiles \
             as literal runtime tag content"
        ),
    };
    Diagnostic {
        file,
        range,
        message,
        code: DiagnosticCode::E172,
    }
}

/// `DIVERT_STMT` → `Stmt::Divert`, `TUNNEL_CALL` → `Stmt::TunnelCall`.
pub(super) fn lower_divert_like(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Option<Stmt> {
    match node.kind() {
        N::DIVERT_STMT => {
            let target = ast::DivertStmt::cast(node.clone())
                .and_then(|d| d.target())
                .and_then(|t| lower_divert_target(file_id, &t, diags))?;
            Some(Stmt::Divert(Divert {
                ptr: Some(native_provenance(file_id, NodeClass::Divert, node)),
                target,
            }))
        }
        N::TUNNEL_CALL => {
            let target = ast::TunnelCall::cast(node.clone())
                .and_then(|t| t.target())
                .and_then(|t| lower_divert_target(file_id, &t, diags))?;
            Some(Stmt::TunnelCall(TunnelCall {
                ptr: native_provenance(file_id, NodeClass::TunnelCall, node),
                targets: vec![target],
            }))
        }
        _ => None,
    }
}

fn lower_divert_target(
    file_id: FileId,
    t: &ast::DivertTarget,
    diags: &mut Vec<Diagnostic>,
) -> Option<DivertTarget> {
    let path = if t.is_end() {
        DivertPath::End
    } else if t.is_done() {
        DivertPath::Done
    } else if let Some(p) = t.path() {
        DivertPath::Path(super::expr::lower_path(&p))
    } else {
        diags.push(diag(file_id, t.syntax().text_range(), DiagnosticCode::E012));
        return None;
    };
    // Native's `DIVERT_TARGET` grammar (`parser/divert.rs::divert_target`)
    // now captures `-> knot(args)` call args as an `ARG_LIST` sibling of
    // `PATH` (bug #1196's fix, read back via `DivertTarget::call_args()`).
    // This pass doesn't wire them into `DivertPath`/codegen yet — that's a
    // follow-up's job, not this one's — so a present arg list is diagnosed
    // loudly (E129, "parses but has no HIR lowering yet") instead of being
    // silently dropped.
    if let Some(args) = t.call_args() {
        diags.push(diag(
            file_id,
            args.syntax().text_range(),
            DiagnosticCode::E129,
        ));
    }
    Some(DivertTarget {
        path,
        args: Vec::new(),
    })
}

/// `RETURN_STMT`'s optional value expression (issue #1973): `None` for a
/// bare content-ground `return` (unchanged) and `Some` once
/// `parser/divert.rs::return_stmt` parses a trailing value expression —
/// mirrors `lower_native::control_flow::lower_return_stmt`'s identical
/// `ret.value().map(...)` for the code-ground `return expr?;` form, the two
/// grammars' shared `RETURN_STMT` node shape. A value-carrying `Explicit`
/// return outside a `fn` is still caught downstream by `brink-analyzer`'s
/// E032 ("explicit return outside function") — [`fixup_return_kind`] only
/// demotes a *bare* (`value.is_none()`) return to `TunnelRedirect`, so this
/// stays a pure grammar/lowering fix, not a change to whether a
/// non-function `flow` may carry a return value (an open design question
/// per issue #1973's own scope note, not decided here).
fn lower_return_value(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Option<Expr> {
    ast::ReturnStmt::cast(node.clone())
        .and_then(|n| n.value())
        .map(|v| lower_expr(file_id, &v, diags))
}

/// `return -> x` (charter §11's tunnel-return respelling, B0.2's payoff).
/// `END`/`DONE` targets lower as a plain `Stmt::Divert` — matching old
/// ink's own `->-> DONE`/`->-> END` treatment
/// (`lower/divert.rs::LowerDivert`) exactly, because `Expr::DivertTarget`
/// only carries a `Path` and cannot represent either sentinel. A named-path
/// target lowers as `Stmt::Return { kind: TunnelRedirect, .. }`.
fn lower_return_redirect(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Stmt> {
    let Some(target) = ast::ReturnRedirect::cast(node.clone())
        .and_then(|r| r.target())
        .and_then(|t| lower_divert_target(file_id, &t, diags))
    else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E012));
        return Vec::new();
    };
    match target.path {
        DivertPath::Path(p) => vec![Stmt::Return(Return {
            ptr: Some(native_provenance(file_id, NodeClass::Return, node)),
            kind: ReturnKind::TunnelRedirect,
            value: Some(Expr::DivertTarget(p)),
            onwards_args: target.args,
        })],
        DivertPath::Done => vec![Stmt::Divert(Divert {
            ptr: Some(native_provenance(file_id, NodeClass::Divert, node)),
            target: DivertTarget {
                path: DivertPath::Done,
                args: Vec::new(),
            },
        })],
        DivertPath::End => vec![Stmt::Divert(Divert {
            ptr: Some(native_provenance(file_id, NodeClass::Divert, node)),
            target: DivertTarget {
                path: DivertPath::End,
                args: Vec::new(),
            },
        })],
    }
}
