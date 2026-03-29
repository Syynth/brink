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

use crate::hir;
use crate::symbols::{SymbolIndex, SymbolKind};
use crate::FileId;

/// Stamp container IDs on all HIR files.
///
/// Must be called after analysis (needs `SymbolIndex` for labeled containers)
/// and before LIR lowering.
pub fn stamp_container_ids(
    files: &mut [(FileId, hir::HirFile)],
    index: &SymbolIndex,
) {
    for (file_id, hir_file) in files {
        // Root content — scope is empty, counters start at 0.
        let mut seq = 0;
        stamp_block(
            &mut hir_file.root_content,
            *file_id,
            "",
            "",
            index,
            &mut seq,
        );

        for knot in &mut hir_file.knots {
            let knot_path = &knot.name.text;
            let mut seq = 0;
            stamp_block(&mut knot.body, *file_id, knot_path, knot_path, index, &mut seq);

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
#[expect(clippy::too_many_arguments)]
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
                let mut nested_cc = 0;
                let mut nested_gc = 0;
                for body_stmt in &mut choice.body.stmts {
                    stamp_stmt(
                        body_stmt,
                        &child_scope,
                        label_scope,
                        index,
                        seq_counter,
                        &mut nested_cc,
                        &mut nested_gc,
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
                branch.container_id = Some(branch_id);

                // Sequence branches get fresh counters.
                let mut bc = 0;
                let mut gc = 0;
                for s in &mut branch.stmts {
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
        hir::Stmt::Content(_)
        | hir::Stmt::Divert(_)
        | hir::Stmt::TunnelCall(_)
        | hir::Stmt::ThreadStart(_)
        | hir::Stmt::TempDecl(_)
        | hir::Stmt::Assignment(_)
        | hir::Stmt::Return(_)
        | hir::Stmt::ExprStmt(_)
        | hir::Stmt::EndOfLine => {}
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
