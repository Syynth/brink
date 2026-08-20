//! Container ID stamping pass.
//!
//! Assigns `DefinitionId`s to every HIR node that will become a synthetic
//! LIR container (choice targets, gathers, conditional branches, sequence
//! wrappers). Runs after analysis, before LIR lowering.
//!
//! This replaces the LIR planning pass by pushing structural identity
//! upstream: the LIR lowerer reads pre-stamped IDs directly from HIR
//! nodes instead of re-walking the tree with synchronized counters.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag};

use crate::FileId;
use crate::determinism::LookupMap;
use crate::hir;
use crate::symbols::{SymbolIndex, SymbolInfo, SymbolKind};

/// The structural scope path every *anonymous* container in `file_path`'s
/// root-level weave hangs off (#1504).
///
/// A knot scopes its children under the knot name, so two files' *distinctly
/// named* knots can never mint the same anonymous path. Root content has no
/// such prefix at all: with an empty root scope path, file A's first root
/// choice and file B's first root choice both hash `c-0` and — because
/// address allocation is a pure hash with no collision avoidance — receive
/// the **same** `DefinitionId`. That id is the linker's address key
/// (last-write-wins) and the save key for visit counts, so the collision
/// miscompiles: picking a choice from the included file runs the entry file's
/// choice body.
///
/// ⚠ "Two files' knots can never mint the same anonymous path" stops being
/// true the moment two files legitimately declare a **same-named** knot
/// (M-2d, #790: `native_module_path` always differs per file, so
/// `insert_symbol` lets the pair coexist rather than raising a
/// duplicate-definition diagnostic) — `stamp_container_ids`'s per-knot loop
/// used to qualify by the bare knot name alone and collided on every
/// unlabeled descendant container exactly like this function's own root
/// content used to (issue #2229, the 4th M-2d collision site; #2197/#2213/
/// #2215/#2226 are the other three). That loop now qualifies its own scope
/// path through this same function — the fix this doc's own false
/// invariant should have named from the start.
///
/// Qualifying by the *file* rather than by the owning module is deliberate.
/// An `INCLUDE`d file with no `#@module` of its own inherits its includer's
/// module (`docs/modules-spec.md` §1), so a module qualifier leaves exactly
/// the shape #1504 was filed against still colliding; two distinct files
/// always have distinct paths. See `docs/root-content-identity-findings.md`.
///
/// The `#` prefix is what makes the qualifier collision-proof against
/// authored scope paths: `#` is not legal in a knot, stitch or label name,
/// and the synthesized segments are all `c-N`/`g-N`/`b-N`/`s-N` — `-` not
/// being legal in an authored identifier either. (Choice segments only
/// actually spell `c-N` since #2229's review pass: `stamp_stmt` used to
/// write bare `c{n}`, contra this very sentence, which an authored knot
/// legally named `c0` could equal — colliding with a root anonymous
/// choice's subtree the moment knot interiors joined this shared `#file:`
/// namespace.)
///
/// `None` (a file whose path the caller did not supply — only in-crate test
/// harnesses do that) yields an empty qualifier, i.e. the pre-#1504 paths.
#[must_use]
pub fn root_content_scope_path(file_path: Option<&str>) -> String {
    match file_path {
        Some(path) if !path.is_empty() => format!("#file:{path}"),
        _ => String::new(),
    }
}

/// Stamp container IDs on all HIR files.
///
/// Must be called after analysis (needs `SymbolIndex` for labeled containers)
/// and before LIR lowering.
///
/// `file_paths` supplies each file's registered project path, which
/// qualifies both the root-content scope path (see
/// [`root_content_scope_path`]) and, since issue #2229, every knot's own
/// interior scope path the same way (root-scope-prefixed via [`qualify`]).
/// Name-based lookups (`label_scope`) stay unqualified throughout: an
/// author's label — root-level or knot-scoped — is addressed by its bare
/// (optionally knot/stitch-qualified) name from anywhere in the project,
/// and the analyzer's `SymbolIndex` keys it that way.
#[expect(
    clippy::implicit_hasher,
    reason = "internal API, no need to generalize"
)]
pub fn stamp_container_ids(
    files: &mut [(FileId, hir::HirFile)],
    index: &SymbolIndex,
    file_paths: &LookupMap<FileId, String>,
) {
    for (file_id, hir_file) in files {
        // Root content — scoped by the owning file (#1504), counters start
        // at 0. The *label* scope stays empty: root labels are addressed by
        // bare name.
        let mut seq = 0;
        let root_scope = root_content_scope_path(file_paths.get(file_id).map(String::as_str));
        stamp_block(
            &mut hir_file.root_content,
            *file_id,
            &root_scope,
            "",
            index,
            &mut seq,
        );

        // File-scope `VAR`/`CONST` initializers (issue #1727) — these are
        // flat, non-recursive `HirFile` vecs, not part of `root_content`'s
        // block tree, so `stamp_block` above never reaches them. A default
        // that is itself a lambda literal (issue #1774,
        // `lir::lower::decls::eval_const_lambda`) is lowered with an
        // **empty** `ctx.scope_path` qualified by the same file's
        // `root_scope` prefix (`GlobalLambdaCtx`'s `set_path_prefix` call in
        // `collect_globals`) — exactly `root_scope` itself, the same
        // qualifier root content's own anonymous containers use.
        for cst in &mut hir_file.constants {
            stamp_lambdas_in_expr(&mut cst.value, *file_id, &root_scope, "", index);
        }
        for var in &mut hir_file.variables {
            stamp_lambdas_in_expr(&mut var.value, *file_id, &root_scope, "", index);
        }

        for knot in &mut hir_file.knots {
            // `knot_path` (the *label* scope) stays bare — it must match
            // the unqualified `{knot}.{label}` naming
            // `insert_symbol`/`lookup_label_id` key `SymbolIndex.by_name`
            // by (`manifest.rs`'s `format!("{k}.{s}.", …)`), addressed by
            // bare name from anywhere in the project. `knot_scope` (the
            // *anonymous-container hashing* scope) gets the same per-file
            // `#file:{path}` qualifier root content already carries
            // ([`root_content_scope_path`], #1504) — this is the 4th M-2d
            // collision site (#2229): two files legitimately declaring a
            // same-named knot (their `native_module_path`s always differ,
            // so `insert_symbol` lets them coexist, #790) previously
            // stamped every *unlabeled* descendant container at the same
            // structural position (`start.0.c-0`) to the identical
            // `DefinitionId`, tripping the #1673 duplicate-id `E060`
            // codegen guard the moment both files' container trees were
            // walked. `root_scope` is already file-qualified and empty for
            // a caller that supplied no file path (in-crate test
            // harnesses), so `qualify` degrades to the pre-#2229 bare
            // `knot_path` in that case — byte-identical single-file/no-path
            // behavior.
            let knot_path = &knot.name.text;
            let knot_scope = qualify(&root_scope, knot_path);
            let mut seq = 0;
            stamp_block(
                &mut knot.body,
                *file_id,
                &knot_scope,
                knot_path,
                index,
                &mut seq,
            );

            for stitch in &mut knot.stitches {
                let stitch_path = format!("{knot_path}.{}", stitch.name.text);
                let stitch_scope = qualify(&root_scope, &stitch_path);
                let mut seq = 0;
                stamp_block(
                    &mut stitch.body,
                    *file_id,
                    &stitch_scope,
                    &stitch_path,
                    index,
                    &mut seq,
                );
            }
        }
    }
}

/// Stamp container IDs on all structural statements in a block.
fn stamp_block(
    block: &mut hir::Block,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
    seq_counter: &mut usize,
) {
    let mut choice_counter = 0usize;
    let mut gather_counter = 0usize;

    for stmt in &mut block.stmts {
        stamp_stmt(
            stmt,
            file,
            scope_path,
            label_scope,
            index,
            seq_counter,
            &mut choice_counter,
            &mut gather_counter,
        );
    }
}

/// Stamp container IDs on a single statement and recurse into children.
///
/// `file`: the file whose declarations this statement belongs to — see
/// [`lookup_label_id`]'s doc. Threaded down from [`stamp_block`] for the
/// primary weave walk and from every lambda-stamping call site (issue
/// #2215) so a labeled gather/choice/block reached through a
/// content-embedded inline conditional/sequence
/// (`stamp_lambdas_in_content_part`'s `InlineConditional`/
/// `InlineSequence` arms, or transitively via `stamp_lambdas_in_expr`'s
/// `Fragment` arm when a block-capture's own captured content line
/// carries one of these mid-line) gets the same same-file-preferred label
/// lookup the primary walk already gets — every call site has a real
/// `FileId` in scope, so there is no longer an unscoped case to fall back
/// to.
#[expect(
    clippy::too_many_lines,
    reason = "structural match over all statement types"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "issue #2197 added `file` for label self-identity; a context struct isn't worth it \
              for one more threaded parameter"
)]
fn stamp_stmt(
    stmt: &mut hir::Stmt,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
    seq_counter: &mut usize,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) {
    match stmt {
        hir::Stmt::ChoiceSet(cs) => {
            // Gather container ID — from label lookup or scope path.
            let gather_id = if let Some(ref label) = cs.continuation.label {
                let label_path = qualify(label_scope, &label.text);
                lookup_label_id(index, file, &label_path)
                    .unwrap_or_else(|| alloc_address(&format!("{scope_path}.g-{gather_counter}")))
            } else {
                alloc_address(&format!("{scope_path}.g-{gather_counter}"))
            };
            cs.gather_id = Some(gather_id);
            cs.continuation.container_id = Some(gather_id);
            *gather_counter += 1;

            // Choice target container IDs. The synthesized segment is
            // `c-{n}` — dash-separated like `g-N`/`b-N`/`s-N`, exactly as
            // [`root_content_scope_path`]'s doc always claimed — NOT the
            // bare `c{n}` this used to spell. The dash is load-bearing
            // (issue #2229 review): `c0` is a perfectly legal authored
            // knot name, and once knot interiors share the root `#file:`
            // namespace, an authored knot `c0` hashes the same scope as a
            // root-level anonymous choice's subtree (`{root}.c0`),
            // colliding every same-position descendant (`E060`). `-` is
            // not legal in any authored identifier, so a dashed segment
            // can never equal one.
            for choice in &mut cs.choices {
                let choice_id = if let Some(ref label) = choice.label {
                    let label_path = qualify(label_scope, &label.text);
                    lookup_label_id(index, file, &label_path).unwrap_or_else(|| {
                        alloc_address(&format!("{scope_path}.c-{choice_counter}"))
                    })
                } else {
                    alloc_address(&format!("{scope_path}.c-{choice_counter}"))
                };
                choice.container_id = Some(choice_id);
                *choice_counter += 1;

                // A lambda in the choice's own condition/content (issue
                // #1727) is lowered *before* `ctx.scope_path` narrows to the
                // choice's own scope (`lir::lower::mod.rs`'s
                // `lower_choice`: the block scope opens, then
                // condition/content lower, and only the *body* lowering
                // below gets the narrowed `.c-{n}` scope) — so these stamp
                // at the parent `scope_path`, not `child_scope`.
                if let Some(cond) = &mut choice.condition {
                    stamp_lambdas_in_expr(cond, file, scope_path, label_scope, index);
                }
                for c in [
                    &mut choice.start_content,
                    &mut choice.bracket_content,
                    &mut choice.inner_content,
                ]
                .into_iter()
                .flatten()
                {
                    stamp_lambdas_in_content(c, file, scope_path, label_scope, index);
                }
                for tag in &mut choice.tags {
                    for part in &mut tag.parts {
                        stamp_lambdas_in_content_part(part, file, scope_path, label_scope, index);
                    }
                }

                // Recurse into choice body with narrowed scope.
                let child_scope = format!("{scope_path}.c-{}", *choice_counter - 1);
                let mut child_choice_counter = 0;
                let mut child_gather_counter = 0;
                for body_stmt in &mut choice.body.stmts {
                    stamp_stmt(
                        body_stmt,
                        file,
                        &child_scope,
                        label_scope,
                        index,
                        seq_counter,
                        &mut child_choice_counter,
                        &mut child_gather_counter,
                    );
                }
            }

            // Recurse into continuation — shares parent scope and counters.
            for cont_stmt in &mut cs.continuation.stmts {
                stamp_stmt(
                    cont_stmt,
                    file,
                    scope_path,
                    label_scope,
                    index,
                    seq_counter,
                    choice_counter,
                    gather_counter,
                );
            }
        }

        hir::Stmt::LabeledBlock(block) => {
            if block.label.is_some() {
                let label_path = block
                    .label
                    .as_ref()
                    .map(|l| qualify(label_scope, &l.text))
                    .unwrap_or_default();
                let label_id = lookup_label_id(index, file, &label_path)
                    .unwrap_or_else(|| alloc_address(&label_path));
                block.container_id = Some(label_id);

                // Register as gather target for the lowerer.
                *gather_counter += 1;
            }

            for s in &mut block.stmts {
                stamp_stmt(
                    s,
                    file,
                    scope_path,
                    label_scope,
                    index,
                    seq_counter,
                    choice_counter,
                    gather_counter,
                );
            }
        }

        hir::Stmt::Conditional(cond) => {
            let cond_idx = *seq_counter;
            *seq_counter += 1;
            let cond_scope = format!("b-{cond_idx}");

            // The switch expression (`{expr: - val: …}`) and each branch's
            // own condition lower *before* `ctx.scope_path` narrows to that
            // branch (`lir::lower::mod.rs`'s `Stmt::Conditional` arm: the
            // condition/`as`-binding lowers, then `ctx.scope_path =
            // branch_scope` is set) — issue #1727 stamps a lambda there at
            // the parent `scope_path`, matching that order.
            if let hir::CondKind::Switch(e) = &mut cond.kind {
                stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
            }

            for (branch_idx, branch) in cond.branches.iter_mut().enumerate() {
                let branch_scope = if scope_path.is_empty() {
                    format!("{cond_scope}.{branch_idx}")
                } else {
                    format!("{scope_path}.{cond_scope}.{branch_idx}")
                };
                let branch_id = alloc_address(&branch_scope);
                branch.container_id = Some(branch_id);

                if let Some(bc) = &mut branch.condition {
                    stamp_lambdas_in_expr(bc, file, scope_path, label_scope, index);
                }

                // Recurse into branch body — shares parent choice/gather counters.
                for s in &mut branch.body.stmts {
                    stamp_stmt(
                        s,
                        file,
                        &branch_scope,
                        label_scope,
                        index,
                        seq_counter,
                        choice_counter,
                        gather_counter,
                    );
                }
            }
        }

        hir::Stmt::Sequence(seq) => {
            let seq_idx = *seq_counter;
            *seq_counter += 1;
            let display_name = format!("s-{seq_idx}");
            let child_scope = if scope_path.is_empty() {
                display_name.clone()
            } else {
                format!("{scope_path}.{display_name}")
            };
            let wrapper_id = alloc_address(&child_scope);
            seq.container_id = Some(wrapper_id);

            // Each branch gets its own container ID.
            for (branch_idx, branch) in seq.branches.iter_mut().enumerate() {
                let branch_path = if child_scope.is_empty() {
                    format!("{branch_idx}")
                } else {
                    format!("{child_scope}.{branch_idx}")
                };
                let branch_id = alloc_address(&branch_path);
                branch.body.container_id = Some(branch_id);

                // Sequence branches get fresh counters.
                let mut bc = 0;
                let mut gc = 0;
                for s in &mut branch.body.stmts {
                    stamp_stmt(
                        s,
                        file,
                        &child_scope,
                        label_scope,
                        index,
                        seq_counter,
                        &mut bc,
                        &mut gc,
                    );
                }
            }
        }

        // None of these statement types ever produce a *structural*
        // container (choice/gather/branch/sequence-wrapper) — but every one
        // of them can carry an embedded `Expr::Lambda` (issue #1727), so
        // each still needs a scan at the current `scope_path`. T1b `~ { … }`
        // blocks (docs/t1b-surface-spec.md §2): `BlockStmt` is a closed set
        // with no variant for any *weave* concept, so nothing inside a
        // logic block can ever need a synthetic LIR container — the seam
        // rule enforces that by construction, not by a check here — but a
        // logic block's statements are exactly where a `let f = |x| …;`
        // most commonly lives, so `LogicBlock` gets the fullest walk below.
        hir::Stmt::Content(content) => {
            stamp_lambdas_in_content(content, file, scope_path, label_scope, index);
        }
        hir::Stmt::Divert(d) => {
            for a in &mut d.target.args {
                stamp_lambdas_in_expr(a, file, scope_path, label_scope, index);
            }
        }
        hir::Stmt::TunnelCall(t) => {
            for target in &mut t.targets {
                for a in &mut target.args {
                    stamp_lambdas_in_expr(a, file, scope_path, label_scope, index);
                }
            }
        }
        hir::Stmt::ThreadStart(t) => {
            for a in &mut t.target.args {
                stamp_lambdas_in_expr(a, file, scope_path, label_scope, index);
            }
        }
        hir::Stmt::TempDecl(t) => {
            if let Some(e) = &mut t.value {
                stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
            }
        }
        hir::Stmt::Assignment(a) => {
            stamp_lambdas_in_expr(&mut a.target, file, scope_path, label_scope, index);
            stamp_lambdas_in_expr(&mut a.value, file, scope_path, label_scope, index);
        }
        hir::Stmt::Return(r) => {
            if let Some(e) = &mut r.value {
                stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
            }
            for a in &mut r.onwards_args {
                stamp_lambdas_in_expr(a, file, scope_path, label_scope, index);
            }
        }
        // Issue #2108: the attach handler's call expression could embed a
        // lambda the same way any other call argument can — scan it like
        // `ExprStmt` does. `EndElementRun` carries no expression, like
        // `EndOfLine`.
        hir::Stmt::ExprStmt(e) | hir::Stmt::AttachElement(e) => {
            stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
        }
        hir::Stmt::EndOfLine | hir::Stmt::EndElementRun => {}
        hir::Stmt::LogicBlock(lb) => {
            stamp_lambdas_in_block_stmts(&mut lb.stmts, file, scope_path, label_scope, index);
        }
        // `await` (docs/flow-suspension-spec.md §3): the resume-container
        // synthesis (§3, a synthetic container id + tunnel-return stack) is
        // FS-2's later step, gated behind the FS-3 runtime; the construct is
        // fenced at LIR lowering (E052) here and stamps no container yet —
        // but its condition expression is still ordinary HIR that could
        // embed a lambda, so it still gets scanned.
        hir::Stmt::Await(a) => {
            if let Some(c) = &mut a.condition {
                stamp_lambdas_in_expr(c, file, scope_path, label_scope, index);
            }
        }
    }
}

// ─── Lambda stamping (issue #1727) ─────────────────────────────────────
//
// A lifted lambda's `DefinitionId` used to be minted independently in LIR
// lowering (`IdAllocator::alloc_lambda_address`), hashing a path built from
// the *live*, mutated `ctx.scope_path` — the same value the structural
// stamping above mirrors, one statement at a time, as it descends. That
// made a lambda nested inside a `Conditional`/`Sequence`/`ChoiceSet` body
// unreproducible from a fresh HIR-time walk, because nothing walked
// *expressions* at all.
//
// RULED 2026-08-02 (`docs/decision-log.md`): invert the direction. HIR
// mints the id here — using the exact same `scope_path` values the
// structural stamping above already tracks for exactly this reason — and
// `lir::lower::lambda::lower_lambda` only *consumes* `LambdaExpr::
// container_id`, never re-derives it. The functions below extend every
// `stamp_stmt`/`stamp_block` recursion site with an expression walk
// (mirroring `lir::lower::lambda::FreeScan`'s descent, minus binder
// tracking — a lambda's identity depends only on structural scope, never on
// which names are in scope) that finds and stamps every `Expr::Lambda`,
// including ones nested inside another lambda's own body.

/// Recursively stamp every `Expr::Lambda` reachable from `expr` — including
/// a lambda nested inside another lambda's body — with a content-derived
/// container id: `{scope_path}.#lambda-{source start offset}`, the exact
/// scheme `IdAllocator::alloc_lambda_address` used to derive independently.
fn stamp_lambdas_in_expr(
    expr: &mut hir::Expr,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
) {
    match expr {
        hir::Expr::Lambda(l) => stamp_lambda(l, file, scope_path, label_scope, index),
        hir::Expr::Prefix(_, inner) | hir::Expr::Postfix(inner, _) => {
            stamp_lambdas_in_expr(inner, file, scope_path, label_scope, index);
        }
        hir::Expr::Infix(ie) => {
            stamp_lambdas_in_expr(&mut ie.lhs, file, scope_path, label_scope, index);
            stamp_lambdas_in_expr(&mut ie.rhs, file, scope_path, label_scope, index);
        }
        hir::Expr::Call(_, args) => {
            for a in args {
                stamp_lambdas_in_expr(a, file, scope_path, label_scope, index);
            }
        }
        hir::Expr::ArrayLiteral(a) => {
            for e in &mut a.elements {
                stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
            }
        }
        hir::Expr::MapLiteral(m) => {
            for (k, v) in &mut m.entries {
                stamp_lambdas_in_expr(k, file, scope_path, label_scope, index);
                stamp_lambdas_in_expr(v, file, scope_path, label_scope, index);
            }
        }
        hir::Expr::Index(idx) => {
            stamp_lambdas_in_expr(&mut idx.base, file, scope_path, label_scope, index);
            stamp_lambdas_in_expr(&mut idx.index, file, scope_path, label_scope, index);
        }
        hir::Expr::Range(r) => {
            stamp_lambdas_in_expr(&mut r.start, file, scope_path, label_scope, index);
            stamp_lambdas_in_expr(&mut r.end, file, scope_path, label_scope, index);
        }
        hir::Expr::StructLiteral(sl) => {
            for (_, v) in &mut sl.fields {
                stamp_lambdas_in_expr(v, file, scope_path, label_scope, index);
            }
        }
        hir::Expr::FieldAccess(fa) => {
            stamp_lambdas_in_expr(&mut fa.base, file, scope_path, label_scope, index);
        }
        hir::Expr::FnLiteral(fl) => {
            for a in &mut fl.args {
                stamp_lambdas_in_expr(a, file, scope_path, label_scope, index);
            }
        }
        hir::Expr::RefArg(ra) => {
            stamp_lambdas_in_expr(&mut ra.operand, file, scope_path, label_scope, index);
        }
        hir::Expr::String(s) => {
            for part in &mut s.parts {
                if let hir::StringPart::Interpolation(inner) = part {
                    stamp_lambdas_in_expr(inner, file, scope_path, label_scope, index);
                }
            }
        }
        // Block capture (issue #1839): a captured run's statements keep
        // their own `Stmt::Content`/`EndOfLine` shape and lower through the
        // ordinary per-statement path at the *current* `ctx.scope_path`,
        // unchanged (`lir::lower::expr`'s `Fragment` arm reuses `ctx`
        // as-is, never pushing a new scope) — so a lambda inside one stamps
        // at this same `scope_path`, with fresh local counters exactly like
        // a choice body gets (this fragment is its own isolated statement
        // list, not sharing the enclosing frame's structural counters).
        // `file` is threaded through (issue #2215) so that traversal still
        // gets the same same-file-preferred [`lookup_label_id`] lookup the
        // primary weave walk gets — see that function's doc for the
        // collision this closes. Note a top-level labeled
        // gather/choice/block is never itself one of `stmts` here:
        // `capture_block`'s terminator (`is_plain_content_line`,
        // `hir::lower_native::element`) stops the captured run at any
        // `CONTENT_LINE` bearing a `LABEL`/`CHOICE_POINT`/`DIVERT_STMT`/
        // `TUNNEL_CALL`, specifically so it can never be absorbed into a
        // `Stmt::LabeledBlock`/`Stmt::ChoiceSet`. A label only reaches this
        // arm transitively, through a captured plain content line's own
        // mid-line inline conditional/sequence — the same
        // `InlineConditional`/`InlineSequence` shape handled below, just
        // one level deeper.
        hir::Expr::Fragment(stmts) => {
            let mut seq = 0;
            let mut cc = 0;
            let mut gc = 0;
            for s in stmts {
                stamp_stmt(
                    s,
                    file,
                    scope_path,
                    label_scope,
                    index,
                    &mut seq,
                    &mut cc,
                    &mut gc,
                );
            }
        }
        hir::Expr::Path(_)
        | hir::Expr::DivertTarget(_)
        | hir::Expr::ListLiteral(_)
        | hir::Expr::Int(_)
        | hir::Expr::Float(_)
        | hir::Expr::Bool(_)
        | hir::Expr::Null => {}
    }
}

/// Stamp `l.container_id` from its own source position, then recurse into
/// its body to stamp any lambda nested inside it. A nested lambda's own
/// scope path is the qualified path just minted for `l` — matching
/// `lir::lower::lambda::lower_lambda`'s child `LowerCtx::scope_path`, which
/// is set to exactly that string before lowering the outer lambda's body.
fn stamp_lambda(
    l: &mut hir::LambdaExpr,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
) {
    let offset = u32::from(l.ptr.text_range().start());
    let own_path = qualify(scope_path, &format!("#lambda-{offset}"));
    l.container_id = Some(alloc_address(&own_path));

    match &mut l.body {
        hir::LambdaBody::Expr(e) => stamp_lambdas_in_expr(e, file, &own_path, label_scope, index),
        hir::LambdaBody::Block { stmts, tail } => {
            stamp_lambdas_in_block_stmts(stmts, file, &own_path, label_scope, index);
            if let Some(t) = tail {
                stamp_lambdas_in_expr(t, file, &own_path, label_scope, index);
            }
        }
    }
}

/// Walk a T1b `~ { … }` block-statement list for embedded lambdas.
/// `BlockStmt` never mutates `ctx.scope_path` in LIR lowering (its `If`/
/// `While`/`For` bodies are lexical scopes only, never LIR containers — see
/// the seam rule at `hir::Stmt::LogicBlock`'s doc), so every statement here
/// stamps at the *same* `scope_path` the caller passed in.
fn stamp_lambdas_in_block_stmts(
    stmts: &mut [hir::BlockStmt],
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
) {
    for s in stmts {
        stamp_lambdas_in_block_stmt(s, file, scope_path, label_scope, index);
    }
}

fn stamp_lambdas_in_block_stmt(
    stmt: &mut hir::BlockStmt,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
) {
    match stmt {
        hir::BlockStmt::TempDecl(t) => {
            if let Some(e) = &mut t.value {
                stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
            }
        }
        hir::BlockStmt::Assignment(a) => {
            stamp_lambdas_in_expr(&mut a.target, file, scope_path, label_scope, index);
            stamp_lambdas_in_expr(&mut a.value, file, scope_path, label_scope, index);
        }
        hir::BlockStmt::Return(r) => {
            if let Some(e) = &mut r.value {
                stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
            }
            for a in &mut r.onwards_args {
                stamp_lambdas_in_expr(a, file, scope_path, label_scope, index);
            }
        }
        hir::BlockStmt::If(i) => stamp_lambdas_in_if_stmt(i, file, scope_path, label_scope, index),
        hir::BlockStmt::While(w) => {
            stamp_lambdas_in_expr(&mut w.condition, file, scope_path, label_scope, index);
            stamp_lambdas_in_block_stmts(&mut w.body, file, scope_path, label_scope, index);
        }
        hir::BlockStmt::For(f) => {
            stamp_lambdas_in_expr(&mut f.iterable, file, scope_path, label_scope, index);
            stamp_lambdas_in_block_stmts(&mut f.body, file, scope_path, label_scope, index);
        }
        hir::BlockStmt::ExprStmt(e) => {
            stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
        }
        hir::BlockStmt::Await(a) => {
            if let Some(c) = &mut a.condition {
                stamp_lambdas_in_expr(c, file, scope_path, label_scope, index);
            }
        }
        hir::BlockStmt::Break(_) | hir::BlockStmt::Continue(_) => {}
    }
}

fn stamp_lambdas_in_if_stmt(
    i: &mut hir::IfStmt,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
) {
    stamp_lambdas_in_expr(&mut i.condition, file, scope_path, label_scope, index);
    stamp_lambdas_in_block_stmts(&mut i.body, file, scope_path, label_scope, index);
    match &mut i.else_branch {
        Some(hir::ElseBranch::ElseIf(nested)) => {
            stamp_lambdas_in_if_stmt(nested, file, scope_path, label_scope, index);
        }
        Some(hir::ElseBranch::Else(stmts)) => {
            stamp_lambdas_in_block_stmts(stmts, file, scope_path, label_scope, index);
        }
        None => {}
    }
}

/// Walk a `Content` line's interpolations/inline conditionals/inline
/// sequences/spans and tags for embedded lambdas.
fn stamp_lambdas_in_content(
    content: &mut hir::Content,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
) {
    for part in &mut content.parts {
        stamp_lambdas_in_content_part(part, file, scope_path, label_scope, index);
    }
    for tag in &mut content.tags {
        for part in &mut tag.parts {
            stamp_lambdas_in_content_part(part, file, scope_path, label_scope, index);
        }
    }
}

/// A content-embedded inline conditional/sequence (`ContentPart::
/// InlineConditional`/`InlineSequence`) leaves `ctx.scope_path` unchanged in
/// LIR lowering (`lir::lower::content::lower_content_part` never pushes a
/// scope for these, unlike the weave-statement `Stmt::Conditional`/
/// `Stmt::Sequence` forms that reuse the same `hir::Conditional`/`Sequence`
/// structs) — so their branch bodies stamp at the *same* `scope_path`,
/// with fresh local structural counters since these bodies are not part of
/// the enclosing frame's own folded weave.
///
/// `file` is threaded through the branch bodies' `stamp_stmt` calls (issue
/// #2215): native's annotated-brace family shares one grammar rule for a
/// `{if …}`/alternation block regardless of whether it sits on its own line
/// or is embedded mid-line in a `CONTENT_LINE` (`hir::lower_native::cond`'s
/// module doc), so a branch body reached only through this content-embedded
/// path can still contain a full `Stmt::ChoiceSet`/`Stmt::LabeledBlock` with
/// its own `(label)` — collision-prone exactly like the primary weave
/// walk's, see [`lookup_label_id`]'s doc.
fn stamp_lambdas_in_content_part(
    part: &mut hir::ContentPart,
    file: FileId,
    scope_path: &str,
    label_scope: &str,
    index: &SymbolIndex,
) {
    match part {
        hir::ContentPart::Interpolation(e) => {
            stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
        }
        hir::ContentPart::InlineConditional(cond) => {
            if let hir::CondKind::Switch(e) = &mut cond.kind {
                stamp_lambdas_in_expr(e, file, scope_path, label_scope, index);
            }
            for b in &mut cond.branches {
                if let Some(c) = &mut b.condition {
                    stamp_lambdas_in_expr(c, file, scope_path, label_scope, index);
                }
                let mut seq = 0;
                let mut cc = 0;
                let mut gc = 0;
                for s in &mut b.body.stmts {
                    stamp_stmt(
                        s,
                        file,
                        scope_path,
                        label_scope,
                        index,
                        &mut seq,
                        &mut cc,
                        &mut gc,
                    );
                }
            }
        }
        hir::ContentPart::InlineSequence(seq) => {
            for b in &mut seq.branches {
                let mut sc = 0;
                let mut cc = 0;
                let mut gc = 0;
                for s in &mut b.body.stmts {
                    stamp_stmt(
                        s,
                        file,
                        scope_path,
                        label_scope,
                        index,
                        &mut sc,
                        &mut cc,
                        &mut gc,
                    );
                }
            }
        }
        hir::ContentPart::Span(span) => {
            for child in &mut span.children {
                stamp_lambdas_in_content_part(child, file, scope_path, label_scope, index);
            }
        }
        hir::ContentPart::Text(_) | hir::ContentPart::Glue | hir::ContentPart::Spring => {}
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────

/// Create a `DefinitionId` for a synthetic container from its scope path.
///
/// Uses the same `DefaultHasher` scheme as the LIR planner's `IdAllocator`.
fn alloc_address(path: &str) -> DefinitionId {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    DefinitionId::new(DefinitionTag::Address, hasher.finish())
}

/// Look up a labeled container in the analyzer's `SymbolIndex`.
///
/// Returns the analyzer-assigned `DefinitionId` for labels so that
/// diverts resolved by the analyzer point to the same container.
///
/// **File-scoped** (issue #2197 — see `lir::lower::lookup_container_id`'s
/// doc for the full collision this mirrors): M-2d
/// (`is_cross_declared_module_collision`) lets a same-name label/gather/
/// choice in two different *declared* modules coexist in `index.by_name`,
/// so an unscoped `.find()` can pick either one for *both* files stamping
/// a container of that name — silently minting the same `DefinitionId`
/// for two distinct containers. Preferring the entry declared in `file`
/// is the correct self-identity semantic regardless of module-visibility
/// policy.
///
/// Every call site (issue #2215) now has a real `FileId` in scope: the
/// primary weave walk (`stamp_block`/`stamp_stmt`) always did, and the
/// separate lambda-stamping traversal (`stamp_lambdas_in_expr`'s
/// `Fragment` arm, `stamp_lambdas_in_content_part`'s `InlineConditional`/
/// `InlineSequence` arms) now threads one through too — those call sites
/// share the exact same collision this function's file-scoped lookup
/// fixes, reachable through an explicitly *labeled* gather/choice/block
/// nested inside a content-embedded inline conditional/sequence (or, one
/// level deeper, inside such a construct's own mid-line inline
/// conditional/sequence when it sits within a block-capture's captured
/// content line — see the `Fragment` arm's own doc for why a labeled
/// container can never be a top-level statement of a capture), declared
/// identically in two *coexisting* declared modules (confirmed live via
/// the real production compile path, not merely theoretical — see the
/// `brink-test-harness` regression this issue adds).
fn lookup_label_id(index: &SymbolIndex, file: FileId, name: &str) -> Option<DefinitionId> {
    fn is_container(info: &SymbolInfo) -> bool {
        matches!(
            info.kind,
            SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label
        )
    }
    index.by_name.get(name).and_then(|ids| {
        if let Some(id) = ids.iter().find(|&&id| {
            index
                .symbols
                .get(&id)
                .is_some_and(|info| is_container(info) && info.file == file)
        }) {
            return Some(*id);
        }
        ids.iter()
            .find(|&&id| index.symbols.get(&id).is_some_and(is_container))
            .copied()
    })
}

/// Qualify a name with a scope path prefix.
fn qualify(scope_path: &str, name: &str) -> String {
    if scope_path.is_empty() {
        name.to_string()
    } else {
        format!("{scope_path}.{name}")
    }
}
