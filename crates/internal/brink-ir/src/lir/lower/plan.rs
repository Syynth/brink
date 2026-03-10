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
        plan_block_choices(&hir_file.root_content, file_id, "", index, ids, &mut plan);

        for knot in &hir_file.knots {
            let knot_path = &knot.name.text;
            let knot_id = lookup_container_id(index, knot_path)
                .unwrap_or_else(|| ids.alloc_container(knot_path));

            plan.knot_ids.insert(knot_path.clone(), knot_id);

            plan_block_choices(&knot.body, file_id, knot_path, index, ids, &mut plan);

            for stitch in &knot.stitches {
                let stitch_path = format!("{knot_path}.{}", stitch.name.text);
                let stitch_id = lookup_container_id(index, &stitch_path)
                    .unwrap_or_else(|| ids.alloc_container(&stitch_path));

                plan.stitch_ids.insert(stitch_path.clone(), stitch_id);

                plan_block_choices(&stitch.body, file_id, &stitch_path, index, ids, &mut plan);
            }
        }
    }

    plan
}

fn plan_block_choices(
    block: &hir::Block,
    file: FileId,
    scope_path: &str,
    index: &SymbolIndex,
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
            index,
            ids,
            plan,
            &mut choice_counter,
            &mut gather_counter,
        );
    }
}

#[expect(clippy::too_many_arguments, clippy::too_many_lines)]
fn plan_stmt_choices(
    stmt: &hir::Stmt,
    file: FileId,
    scope_path: &str,
    index: &SymbolIndex,
    ids: &mut IdAllocator,
    plan: &mut ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) {
    match stmt {
        hir::Stmt::ChoiceSet(choice_set) => {
            // Always plan a gather container for the continuation — even
            // without an explicit gather in the source, both backends need
            // a convergence point (inklecate always emits g-0).
            let gather_path = if let Some(ref label) = choice_set.continuation.label {
                if scope_path.is_empty() {
                    label.text.clone()
                } else {
                    format!("{scope_path}.{}", label.text)
                }
            } else {
                format!("{scope_path}.g-{gather_counter}")
            };

            let gather_id = lookup_container_id(index, &gather_path)
                .unwrap_or_else(|| ids.alloc_container(&gather_path));

            plan.gather_targets.insert(
                GatherKey {
                    file,
                    scope: scope_path.to_string(),
                    index: *gather_counter,
                },
                gather_id,
            );
            *gather_counter += 1;

            // Plan each choice target
            for choice in &choice_set.choices {
                let choice_id = if let Some(ref label) = choice.label {
                    let label_path = if scope_path.is_empty() {
                        label.text.clone()
                    } else {
                        format!("{scope_path}.{}", label.text)
                    };
                    lookup_container_id(index, &label_path).unwrap_or_else(|| {
                        ids.alloc_container(&format!("{scope_path}.c{choice_counter}"))
                    })
                } else {
                    ids.alloc_container(&format!("{scope_path}.c{choice_counter}"))
                };
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
                        index,
                        ids,
                        plan,
                        &mut nested_choice_counter,
                        &mut nested_gather_counter,
                    );
                }
            }

            // Recursively plan nested choices within the continuation block
            for cont_stmt in &choice_set.continuation.stmts {
                plan_stmt_choices(
                    cont_stmt,
                    file,
                    scope_path,
                    index,
                    ids,
                    plan,
                    choice_counter,
                    gather_counter,
                );
            }
        }
        hir::Stmt::LabeledBlock(block) => {
            // A labeled block wrapping a choice set (opening gather pattern).
            // Allocate a container for the label, then recurse into its stmts.
            if let Some(ref label) = block.label {
                let label_path = if scope_path.is_empty() {
                    label.text.clone()
                } else {
                    format!("{scope_path}.{}", label.text)
                };
                let label_id = lookup_container_id(index, &label_path)
                    .unwrap_or_else(|| ids.alloc_container(&label_path));
                plan.gather_targets.insert(
                    GatherKey {
                        file,
                        scope: scope_path.to_string(),
                        index: *gather_counter,
                    },
                    label_id,
                );
                *gather_counter += 1;
            }
            for s in &block.stmts {
                plan_stmt_choices(
                    s,
                    file,
                    scope_path,
                    index,
                    ids,
                    plan,
                    choice_counter,
                    gather_counter,
                );
            }
        }
        hir::Stmt::Conditional(cond) => {
            for branch in &cond.branches {
                let mut bc = 0;
                let mut bg = 0;
                for s in &branch.body.stmts {
                    plan_stmt_choices(s, file, scope_path, index, ids, plan, &mut bc, &mut bg);
                }
            }
        }
        hir::Stmt::Sequence(seq) => {
            for branch in &seq.branches {
                let mut bc = 0;
                let mut bg = 0;
                for s in &branch.stmts {
                    plan_stmt_choices(s, file, scope_path, index, ids, plan, &mut bc, &mut bg);
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
