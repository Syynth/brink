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
use crate::symbols::{SymbolIndex, SymbolKind};

/// The structural scope path every *anonymous* container in `file_path`'s
/// root-level weave hangs off (#1504).
///
/// A knot scopes its children under the knot name, so two files' knots can
/// never mint the same anonymous path. Root content has no such prefix: with
/// an empty root scope path, file A's first root choice and file B's first
/// root choice both hash `c-0` and — because address allocation is a pure
/// hash with no collision avoidance — receive the
/// **same** `DefinitionId`. That id is the linker's address key
/// (last-write-wins) and the save key for visit counts, so the collision
/// miscompiles: picking a choice from the included file runs the entry file's
/// choice body.
///
/// Qualifying by the *file* rather than by the owning module is deliberate.
/// An `INCLUDE`d file with no `#@module` of its own inherits its includer's
/// module (`docs/modules-spec.md` §1), so a module qualifier leaves exactly
/// the shape #1504 was filed against still colliding; two distinct files
/// always have distinct paths. See `docs/root-content-identity-findings.md`.
///
/// The `#` prefix is what makes the qualifier collision-proof against
/// authored scope paths: `#` is not legal in a knot, stitch or label name,
/// and the synthesized segments are all `c-N`/`g-N`/`b-N`/`s-N`.
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
/// `file_paths` supplies each file's registered project path, which qualifies
/// its root-content scope path (see [`root_content_scope_path`]). Name-based
/// lookups (`label_scope`) stay unqualified: an author's root-level label is
/// addressed by its bare name from anywhere in the project, and the analyzer's
/// `SymbolIndex` keys it that way.
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

        for knot in &mut hir_file.knots {
            let knot_path = &knot.name.text;
            let mut seq = 0;
            stamp_block(
                &mut knot.body,
                *file_id,
                knot_path,
                knot_path,
                index,
                &mut seq,
            );

            for stitch in &mut knot.stitches {
                let stitch_path = format!("{knot_path}.{}", stitch.name.text);
                let mut seq = 0;
                stamp_block(
                    &mut stitch.body,
                    *file_id,
                    &stitch_path,
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
    _file: FileId,
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
#[expect(
    clippy::too_many_lines,
    reason = "structural match over all statement types"
)]
fn stamp_stmt(
    stmt: &mut hir::Stmt,
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
                lookup_label_id(index, &label_path)
                    .unwrap_or_else(|| alloc_address(&format!("{scope_path}.g-{gather_counter}")))
            } else {
                alloc_address(&format!("{scope_path}.g-{gather_counter}"))
            };
            cs.gather_id = Some(gather_id);
            cs.continuation.container_id = Some(gather_id);
            *gather_counter += 1;

            // Choice target container IDs.
            for choice in &mut cs.choices {
                let choice_id = if let Some(ref label) = choice.label {
                    let label_path = qualify(label_scope, &label.text);
                    lookup_label_id(index, &label_path).unwrap_or_else(|| {
                        alloc_address(&format!("{scope_path}.c{choice_counter}"))
                    })
                } else {
                    alloc_address(&format!("{scope_path}.c{choice_counter}"))
                };
                choice.container_id = Some(choice_id);
                *choice_counter += 1;

                // Recurse into choice body with narrowed scope.
                let child_scope = format!("{scope_path}.c{}", *choice_counter - 1);
                let mut child_choice_counter = 0;
                let mut child_gather_counter = 0;
                for body_stmt in &mut choice.body.stmts {
                    stamp_stmt(
                        body_stmt,
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
                let label_id = lookup_label_id(index, &label_path)
                    .unwrap_or_else(|| alloc_address(&label_path));
                block.container_id = Some(label_id);

                // Register as gather target for the lowerer.
                *gather_counter += 1;
            }

            for s in &mut block.stmts {
                stamp_stmt(
                    s,
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

            for (branch_idx, branch) in cond.branches.iter_mut().enumerate() {
                let branch_scope = if scope_path.is_empty() {
                    format!("{cond_scope}.{branch_idx}")
                } else {
                    format!("{scope_path}.{cond_scope}.{branch_idx}")
                };
                let branch_id = alloc_address(&branch_scope);
                branch.container_id = Some(branch_id);

                // Recurse into branch body — shares parent choice/gather counters.
                for s in &mut branch.body.stmts {
                    stamp_stmt(
                        s,
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

        // These statement types never produce containers.
        // T1b `~ { … }` blocks (docs/t1b-surface-spec.md §2): `BlockStmt` is
        // a closed set with no variant for any weave concept, so nothing
        // inside a logic block can ever need a synthetic LIR container —
        // the seam rule enforces this by construction, not by a check here.
        hir::Stmt::Content(_)
        | hir::Stmt::Divert(_)
        | hir::Stmt::TunnelCall(_)
        | hir::Stmt::ThreadStart(_)
        | hir::Stmt::TempDecl(_)
        | hir::Stmt::Assignment(_)
        | hir::Stmt::Return(_)
        | hir::Stmt::ExprStmt(_)
        | hir::Stmt::EndOfLine
        | hir::Stmt::LogicBlock(_)
        // `await` (docs/flow-suspension-spec.md §3): the resume-container
        // synthesis (§3, a synthetic container id + tunnel-return stack) is
        // FS-2's later step, gated behind the FS-3 runtime; the construct is
        // fenced at LIR lowering (E052) here and stamps no container yet.
        | hir::Stmt::Await(_) => {}
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
fn lookup_label_id(index: &SymbolIndex, name: &str) -> Option<DefinitionId> {
    index.by_name.get(name).and_then(|ids| {
        ids.iter()
            .find(|&&id| {
                index.symbols.get(&id).is_some_and(|info| {
                    matches!(
                        info.kind,
                        SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label
                    )
                })
            })
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
