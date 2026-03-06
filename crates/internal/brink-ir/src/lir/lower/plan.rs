use std::collections::HashMap;

use brink_format::DefinitionId;

use crate::FileId;
use crate::hir;
use crate::symbols::{SymbolIndex, SymbolKind};

use super::context::IdAllocator;
use super::lir;

/// The result of container planning: maps HIR constructs to container shells.
pub struct ContainerPlan {
    /// Ordered list of container shells (id, path, kind).
    pub shells: Vec<ContainerShell>,
    /// Map from choice index key to the target container id.
    pub choice_targets: HashMap<ChoiceKey, DefinitionId>,
    /// Map from gather key to the gather container id.
    pub gather_targets: HashMap<GatherKey, DefinitionId>,
    /// The root container id.
    pub root_id: DefinitionId,
}

pub struct ContainerShell {
    pub id: DefinitionId,
    pub path: String,
    pub kind: lir::ContainerKind,
    pub scope_root: Option<DefinitionId>,
}

/// Identifies a choice within the HIR structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChoiceKey {
    pub file: FileId,
    /// Path prefix of the containing scope (e.g. "knot" or "knot.stitch").
    pub scope: String,
    /// Sequential choice index within the scope.
    pub index: usize,
}

/// Identifies a gather within the HIR structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GatherKey {
    pub file: FileId,
    pub scope: String,
    pub index: usize,
}

/// Walk all HIR files and create container shells.
pub fn plan_containers(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    ids: &mut IdAllocator,
) -> ContainerPlan {
    let mut plan = ContainerPlan {
        shells: Vec::new(),
        choice_targets: HashMap::new(),
        gather_targets: HashMap::new(),
        root_id: ids.alloc_container(""),
    };

    // Root container
    plan.shells.push(ContainerShell {
        id: plan.root_id,
        path: String::new(),
        kind: lir::ContainerKind::Root,
        scope_root: None,
    });

    for &(file_id, hir_file) in files {
        // Root content gets choice/gather containers
        plan_block_choices(
            &hir_file.root_content,
            file_id,
            "",
            plan.root_id,
            ids,
            &mut plan,
        );

        for knot in &hir_file.knots {
            let knot_path = &knot.name.text;
            let knot_id = lookup_container_id(index, knot_path)
                .unwrap_or_else(|| ids.alloc_container(knot_path));

            plan.shells.push(ContainerShell {
                id: knot_id,
                path: knot_path.clone(),
                kind: lir::ContainerKind::Knot,
                scope_root: None,
            });

            plan_block_choices(&knot.body, file_id, knot_path, knot_id, ids, &mut plan);

            for stitch in &knot.stitches {
                let stitch_path = format!("{knot_path}.{}", stitch.name.text);
                let stitch_id = lookup_container_id(index, &stitch_path)
                    .unwrap_or_else(|| ids.alloc_container(&stitch_path));

                plan.shells.push(ContainerShell {
                    id: stitch_id,
                    path: stitch_path.clone(),
                    kind: lir::ContainerKind::Stitch,
                    scope_root: Some(knot_id),
                });

                plan_block_choices(&stitch.body, file_id, &stitch_path, knot_id, ids, &mut plan);
            }
        }
    }

    plan
}

fn plan_block_choices(
    block: &hir::Block,
    file: FileId,
    scope_path: &str,
    scope_root: DefinitionId,
    ids: &mut IdAllocator,
    plan: &mut ContainerPlan,
) {
    let mut choice_counter = 0usize;
    let mut gather_counter = 0usize;

    for stmt in &block.stmts {
        plan_stmt_choices(
            stmt,
            file,
            scope_path,
            scope_root,
            ids,
            plan,
            &mut choice_counter,
            &mut gather_counter,
        );
    }
}

#[expect(clippy::too_many_arguments)]
fn plan_stmt_choices(
    stmt: &hir::Stmt,
    file: FileId,
    scope_path: &str,
    scope_root: DefinitionId,
    ids: &mut IdAllocator,
    plan: &mut ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) {
    match stmt {
        hir::Stmt::ChoiceSet(choice_set) => {
            // Plan gather container first (if present)
            if let Some(ref gather) = choice_set.gather {
                let gather_path = if let Some(ref label) = gather.label {
                    format!("{scope_path}.{}", label.text)
                } else {
                    let path = format!("{scope_path}.g{gather_counter}");
                    *gather_counter += 1;
                    path
                };

                let gather_id = if gather.label.is_some() {
                    lookup_label_id(
                        &plan.shells,
                        scope_path,
                        gather.label.as_ref().map(|n| n.text.as_str()),
                        ids,
                        &gather_path,
                    )
                } else {
                    ids.alloc_container(&gather_path)
                };

                plan.shells.push(ContainerShell {
                    id: gather_id,
                    path: gather_path.clone(),
                    kind: super::lir::ContainerKind::Gather,
                    scope_root: Some(scope_root),
                });

                plan.gather_targets.insert(
                    GatherKey {
                        file,
                        scope: scope_path.to_string(),
                        index: *gather_counter - usize::from(gather.label.is_none()),
                    },
                    gather_id,
                );
            }

            // Plan each choice target
            for choice in &choice_set.choices {
                let choice_path = format!("{scope_path}.c{choice_counter}");
                let choice_id = ids.alloc_container(&choice_path);
                *choice_counter += 1;

                plan.shells.push(ContainerShell {
                    id: choice_id,
                    path: choice_path,
                    kind: super::lir::ContainerKind::ChoiceTarget,
                    scope_root: Some(scope_root),
                });

                plan.choice_targets.insert(
                    ChoiceKey {
                        file,
                        scope: scope_path.to_string(),
                        index: *choice_counter - 1,
                    },
                    choice_id,
                );

                // Recursively plan nested choices within choice bodies
                let mut nested_choice_counter = 0usize;
                let mut nested_gather_counter = 0usize;
                for body_stmt in &choice.body.stmts {
                    plan_stmt_choices(
                        body_stmt,
                        file,
                        &format!("{scope_path}.c{}", *choice_counter - 1),
                        scope_root,
                        ids,
                        plan,
                        &mut nested_choice_counter,
                        &mut nested_gather_counter,
                    );
                }
            }
        }
        hir::Stmt::Conditional(cond) => {
            for branch in &cond.branches {
                let mut bc = 0;
                let mut bg = 0;
                for s in &branch.body.stmts {
                    plan_stmt_choices(s, file, scope_path, scope_root, ids, plan, &mut bc, &mut bg);
                }
            }
        }
        hir::Stmt::Sequence(seq) => {
            for branch in &seq.branches {
                let mut bc = 0;
                let mut bg = 0;
                for s in &branch.stmts {
                    plan_stmt_choices(s, file, scope_path, scope_root, ids, plan, &mut bc, &mut bg);
                }
            }
        }
        _ => {}
    }
}

fn lookup_container_id(index: &SymbolIndex, name: &str) -> Option<DefinitionId> {
    index.by_name.get(name).and_then(|ids| {
        ids.iter()
            .find(|&&id| {
                index
                    .symbols
                    .get(&id)
                    .is_some_and(|info| matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch))
            })
            .copied()
    })
}

fn lookup_label_id(
    _shells: &[ContainerShell],
    _scope_path: &str,
    _label: Option<&str>,
    ids: &mut IdAllocator,
    gather_path: &str,
) -> DefinitionId {
    // Labels are in the symbol index, but we may not find them there
    // if the gather label wasn't declared as a separate symbol.
    // Fall back to allocating a new id.
    ids.alloc_container(gather_path)
}
