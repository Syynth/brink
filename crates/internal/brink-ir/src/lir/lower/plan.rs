use std::collections::HashMap;

use brink_format::{DefinitionId, DefinitionTag};

use crate::FileId;
use crate::hir;
use crate::symbols::{SymbolIndex, SymbolKind};

use super::context::IdAllocator;

/// The result of container planning: pre-allocates IDs for all containers.
pub struct ContainerPlan {
    /// Map from choice index key to the target container id.
    pub choice_targets: HashMap<ChoiceKey, DefinitionId>,
    /// Map from gather key to the gather container id.
    pub gather_targets: HashMap<GatherKey, DefinitionId>,
    /// The root container id.
    pub root_id: DefinitionId,
    /// Knot name → `DefinitionId`.
    pub knot_ids: HashMap<String, DefinitionId>,
    /// Stitch path ("knot.stitch") → `DefinitionId`.
    pub stitch_ids: HashMap<String, DefinitionId>,
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

impl ContainerPlan {
    /// An empty plan for inline lowering contexts where no choice/gather
    /// containers exist.
    pub fn empty() -> Self {
        Self {
            choice_targets: HashMap::new(),
            gather_targets: HashMap::new(),
            root_id: DefinitionId::new(DefinitionTag::Container, 0),
            knot_ids: HashMap::new(),
            stitch_ids: HashMap::new(),
        }
    }
}

/// Walk all HIR files and pre-allocate container IDs.
pub fn plan_containers(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    ids: &mut IdAllocator,
) -> ContainerPlan {
    let mut plan = ContainerPlan {
        choice_targets: HashMap::new(),
        gather_targets: HashMap::new(),
        root_id: ids.alloc_container(""),
        knot_ids: HashMap::new(),
        stitch_ids: HashMap::new(),
    };

    for &(file_id, hir_file) in files {
        // Root content gets choice/gather containers
        plan_block_choices(&hir_file.root_content, file_id, "", ids, &mut plan);

        for knot in &hir_file.knots {
            let knot_path = &knot.name.text;
            let knot_id = lookup_container_id(index, knot_path)
                .unwrap_or_else(|| ids.alloc_container(knot_path));

            plan.knot_ids.insert(knot_path.clone(), knot_id);

            plan_block_choices(&knot.body, file_id, knot_path, ids, &mut plan);

            for stitch in &knot.stitches {
                let stitch_path = format!("{knot_path}.{}", stitch.name.text);
                let stitch_id = lookup_container_id(index, &stitch_path)
                    .unwrap_or_else(|| ids.alloc_container(&stitch_path));

                plan.stitch_ids.insert(stitch_path.clone(), stitch_id);

                plan_block_choices(&stitch.body, file_id, &stitch_path, ids, &mut plan);
            }
        }
    }

    plan
}

fn plan_block_choices(
    block: &hir::Block,
    file: FileId,
    scope_path: &str,
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
            ids,
            plan,
            &mut choice_counter,
            &mut gather_counter,
        );
    }
}

fn plan_stmt_choices(
    stmt: &hir::Stmt,
    file: FileId,
    scope_path: &str,
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

                let gather_id = ids.alloc_container(&gather_path);

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
                    plan_stmt_choices(s, file, scope_path, ids, plan, &mut bc, &mut bg);
                }
            }
        }
        hir::Stmt::Sequence(seq) => {
            for branch in &seq.branches {
                let mut bc = 0;
                let mut bg = 0;
                for s in &branch.stmts {
                    plan_stmt_choices(s, file, scope_path, ids, plan, &mut bc, &mut bg);
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
