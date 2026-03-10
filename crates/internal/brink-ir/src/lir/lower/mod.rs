mod content;
mod context;
mod decls;
mod expr;
mod plan;
mod stmts;
mod temps;

use brink_format::CountingFlags;

use crate::FileId;
use crate::hir;
use crate::symbols::{ResolutionMap, SymbolIndex};

use super::types as lir;
use context::{LowerCtx, NameTable, ResolutionLookup, TempMap};

/// Lower analyzed HIR into a resolved LIR `Program`.
///
/// All references are resolved — the returned `Program` is self-contained
/// and does not need the `SymbolIndex` or `ResolutionMap`.
pub fn lower_to_program(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> lir::Program {
    let resolutions = ResolutionLookup::build(resolutions);
    let mut names = NameTable::new();
    let mut ids = context::IdAllocator::new();

    // ── Step 1: Plan containers (pre-allocate IDs) ─────────────────
    let plan = plan::plan_containers(files, index, &mut ids);

    // ── Step 2: Collect declarations ────────────────────────────────
    let globals = decls::collect_globals(files, index, &mut names, &resolutions);
    let (lists, list_items) = decls::collect_lists(files, index, &mut names);
    let externals = decls::collect_externals(files, index, &mut names);

    // ── Step 3: Lower containers as a tree ──────────────────────────
    let root = lower_root(files, &resolutions, index, &mut names, &plan);

    // ── Step 4: Counting flags ──────────────────────────────────────
    let mut root = root;
    apply_counting_flags(&mut root);

    lir::Program {
        root,
        globals,
        lists,
        list_items,
        externals,
        name_table: names.into_entries(),
    }
}

// ─── Tree-building lowering ─────────────────────────────────────────

fn lower_root(
    files: &[(FileId, &hir::HirFile)],
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    plan: &plan::ContainerPlan,
) -> lir::Container {
    let mut body = Vec::new();
    let mut children = Vec::new();

    // Allocate temp slots for root content (top-level ~ temp declarations).
    let root_blocks: Vec<&hir::Block> = files.iter().map(|(_, hir)| &hir.root_content).collect();
    let temp_map = temps::alloc_temps(&[], &root_blocks);

    for &(file_id, hir_file) in files {
        let mut ctx = make_ctx(file_id, resolutions, index, &temp_map, names, String::new());
        let mut cc = 0;
        let mut gc = 0;
        let (stmts, mut block_children) =
            lower_block_with_children(&hir_file.root_content, &mut ctx, plan, &mut cc, &mut gc);
        body.extend(stmts);
        children.append(&mut block_children);

        // Add knots as children of root
        for knot in &hir_file.knots {
            children.push(lower_knot(
                file_id,
                hir_file,
                knot,
                resolutions,
                index,
                names,
                plan,
            ));
        }
    }

    // Implicit DONE at end of root
    let ends_with_divert = body
        .last()
        .is_some_and(|s| matches!(s, lir::Stmt::Divert(_)));
    if !ends_with_divert {
        body.push(lir::Stmt::Divert(lir::Divert {
            target: lir::DivertTarget::Done,
            args: Vec::new(),
        }));
    }

    lir::Container {
        id: plan.root_id,
        name: None,
        kind: lir::ContainerKind::Root,
        params: Vec::new(),
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        label_id: None,
        inline: false,
    }
}

fn lower_knot(
    file_id: FileId,
    _hir_file: &hir::HirFile,
    knot: &hir::Knot,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    plan: &plan::ContainerPlan,
) -> lir::Container {
    let knot_name = &knot.name.text;
    let knot_id = plan
        .knot_ids
        .get(knot_name.as_str())
        .copied()
        .unwrap_or(plan.root_id);

    let mut scope_blocks: Vec<&hir::Block> = vec![&knot.body];
    for stitch in &knot.stitches {
        scope_blocks.push(&stitch.body);
    }

    let temp_map = temps::alloc_temps(&knot.params, &scope_blocks);
    let temp_count = temp_map.total_slots();
    let params = lower_params(&knot.params, names, &temp_map);

    let mut ctx = make_ctx(
        file_id,
        resolutions,
        index,
        &temp_map,
        names,
        knot_name.clone(),
    );
    let mut cc = 0;
    let mut gc = 0;
    let (body, mut children) =
        lower_block_with_children(&knot.body, &mut ctx, plan, &mut cc, &mut gc);

    // Add stitches as children
    for stitch in &knot.stitches {
        children.push(lower_stitch(
            file_id,
            knot,
            stitch,
            &temp_map,
            resolutions,
            index,
            names,
            plan,
        ));
    }

    // First-stitch auto-enter: if knot body is empty, divert to first stitch
    let mut final_body = body;
    if final_body.is_empty()
        && !knot.stitches.is_empty()
        && let Some(first_stitch) = children
            .iter()
            .find(|c| c.kind == lir::ContainerKind::Stitch)
    {
        final_body.push(lir::Stmt::Divert(lir::Divert {
            target: lir::DivertTarget::Container(first_stitch.id),
            args: Vec::new(),
        }));
    }

    lir::Container {
        id: knot_id,
        name: Some(knot_name.clone()),
        kind: lir::ContainerKind::Knot,
        params,
        body: final_body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: temp_count,
        label_id: None,
        inline: false,
    }
}

#[expect(clippy::too_many_arguments)]
fn lower_stitch(
    file_id: FileId,
    knot: &hir::Knot,
    stitch: &hir::Stitch,
    temp_map: &TempMap,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    plan: &plan::ContainerPlan,
) -> lir::Container {
    let stitch_name = &stitch.name.text;
    let stitch_path = format!("{}.{stitch_name}", knot.name.text);
    let stitch_id = plan
        .stitch_ids
        .get(stitch_path.as_str())
        .copied()
        .unwrap_or(plan.root_id);
    let params = lower_params(&stitch.params, names, temp_map);

    let mut ctx = make_ctx(file_id, resolutions, index, temp_map, names, stitch_path);
    let mut cc = 0;
    let mut gc = 0;
    let (body, children) =
        lower_block_with_children(&stitch.body, &mut ctx, plan, &mut cc, &mut gc);

    lir::Container {
        id: stitch_id,
        name: Some(stitch_name.clone()),
        kind: lir::ContainerKind::Stitch,
        params,
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        label_id: None,
        inline: false,
    }
}

/// Lower a block, returning both statements and any child containers
/// (choice targets, gathers) produced by choice sets within the block.
///
/// When a `ChoiceSet` with a gather is encountered, remaining statements
/// go into the gather's body (not the current block).
#[expect(clippy::too_many_lines)]
fn lower_block_with_children(
    block: &hir::Block,
    ctx: &mut LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> (Vec<lir::Stmt>, Vec<lir::Container>) {
    let mut stmts = Vec::new();
    let mut children = Vec::new();
    let mut pos = 0;

    while pos < block.stmts.len() {
        let stmt = &block.stmts[pos];
        match stmt {
            hir::Stmt::ChoiceSet(cs) if cs.opening_gather.is_some() => {
                // Gather-choice chain (- * hello\n- * world pattern).
                // Process the entire chain producing flat sibling containers.
                let (chain_children, consumed) = lower_gather_choice_chain(
                    block,
                    pos,
                    ctx,
                    plan,
                    choice_counter,
                    gather_counter,
                );
                // Divert to first container in the chain so execution reaches it.
                if let Some(first) = chain_children.first() {
                    stmts.push(lir::Stmt::Divert(lir::Divert {
                        target: lir::DivertTarget::Container(first.id),
                        args: Vec::new(),
                    }));
                }
                children.extend(chain_children);
                pos += consumed;
            }
            hir::Stmt::ChoiceSet(cs) => {
                // Every choice set gets a gather target — explicit or implicit.
                let gather_target = find_gather_target(ctx, plan, gather_counter);

                // Build choice target children
                let mut choice_children = Vec::new();
                let choices: Vec<lir::Choice> = cs
                    .choices
                    .iter()
                    .map(|choice| {
                        let (lir_choice, child) = lower_choice_with_child(
                            choice,
                            ctx,
                            plan,
                            choice_counter,
                            gather_target,
                        );
                        if let Some(c) = child {
                            choice_children.push(c);
                        }
                        lir_choice
                    })
                    .collect();

                stmts.push(lir::Stmt::ChoiceSet(lir::ChoiceSet {
                    choices,
                    gather_target,
                }));
                children.append(&mut choice_children);

                if let Some(ref gather) = cs.gather {
                    // Explicit gather — build it with trailing statements
                    let gather_container = build_gather_container(
                        gather,
                        block,
                        pos,
                        ctx,
                        plan,
                        gather_target,
                        *gather_counter - 1,
                    );
                    children.push(gather_container);
                    // Trailing statements went into the gather — stop here
                    break;
                }

                // No explicit gather — build an implicit one.
                // Remaining statements in this block go into the implicit
                // gather's body (not the parent) so that choice targets that
                // divert to the gather execute the trailing code.
                let implicit_gather_id = gather_target.unwrap_or(plan.root_id);
                pos += 1;
                let mut trailing = Vec::new();
                while pos < block.stmts.len() {
                    let s = &block.stmts[pos];
                    if let Some(ls) =
                        stmts::lower_stmt(s, ctx, plan, choice_counter, gather_counter)
                    {
                        trailing.push(ls);
                    }
                    pos += 1;
                }
                children.push(build_implicit_gather_with_body(
                    implicit_gather_id,
                    gather_counter,
                    trailing,
                ));
            }
            hir::Stmt::Conditional(cond) => {
                // Lower conditional branches with lower_block_with_children
                // so ChoiceSets inside branches produce child containers.
                let kind = match &cond.kind {
                    hir::CondKind::InitialCondition => lir::CondKind::InitialCondition,
                    hir::CondKind::IfElse => lir::CondKind::IfElse,
                    hir::CondKind::Switch(expr) => {
                        lir::CondKind::Switch(expr::lower_expr(expr, ctx))
                    }
                };
                let branches = cond
                    .branches
                    .iter()
                    .map(|b| {
                        let condition = b.condition.as_ref().map(|e| expr::lower_expr(e, ctx));
                        let mut bc = 0;
                        let mut gc = 0;
                        let (body, branch_children) =
                            lower_block_with_children(&b.body, ctx, plan, &mut bc, &mut gc);
                        children.extend(branch_children);
                        lir::CondBranch { condition, body }
                    })
                    .collect();
                stmts.push(lir::Stmt::Conditional(lir::Conditional { kind, branches }));
                pos += 1;
            }
            hir::Stmt::Sequence(seq) => {
                // Lower sequence branches with lower_block_with_children
                // so ChoiceSets inside branches produce child containers.
                let branches = seq
                    .branches
                    .iter()
                    .map(|b| {
                        let mut bc = 0;
                        let mut gc = 0;
                        let (body, branch_children) =
                            lower_block_with_children(b, ctx, plan, &mut bc, &mut gc);
                        children.extend(branch_children);
                        body
                    })
                    .collect();
                stmts.push(lir::Stmt::Sequence(lir::Sequence {
                    kind: seq.kind,
                    branches,
                }));
                pos += 1;
            }
            _ => {
                if let Some(s) = stmts::lower_stmt(stmt, ctx, plan, choice_counter, gather_counter)
                {
                    stmts.push(s);
                }
                pos += 1;
            }
        }
    }

    (stmts, children)
}

/// Process a gather-choice chain (`- * hello\n- * world`) into flat sibling
/// gather containers. Each container wraps one `ChoiceSet` and its choice
/// targets. Returns `(children, stmts_consumed)`.
fn lower_gather_choice_chain(
    block: &hir::Block,
    start: usize,
    ctx: &mut LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> (Vec<lir::Container>, usize) {
    let mut containers = Vec::new();
    let mut pos = start;
    let mut prev_convergence_target: Option<brink_format::DefinitionId> = None;
    // Track the gather_counter value at the previous convergence allocation,
    // so subsequent containers can compute the correct wrapper name.
    let mut prev_convergence_counter: usize = 0;

    while pos < block.stmts.len() {
        let hir::Stmt::ChoiceSet(cs) = &block.stmts[pos] else {
            break;
        };

        let is_first = pos == start;

        // For the first CS, allocate the opening wrapper ID.
        // For subsequent CSes, the previous convergence IS the wrapper ID.
        let wrapper_target = if is_first {
            find_gather_target(ctx, plan, gather_counter) // opening wrapper (g-0)
        } else {
            prev_convergence_target
        };

        // Allocate the convergence target (what choices divert to = NEXT container).
        let convergence_counter = *gather_counter;
        let convergence_target = find_gather_target(ctx, plan, gather_counter);

        // Build choice targets
        let mut choice_children = Vec::new();
        let choices: Vec<lir::Choice> = cs
            .choices
            .iter()
            .map(|choice| {
                let (lir_choice, child) =
                    lower_choice_with_child(choice, ctx, plan, choice_counter, convergence_target);
                if let Some(c) = child {
                    choice_children.push(c);
                }
                lir_choice
            })
            .collect();

        let cs_body = vec![lir::Stmt::ChoiceSet(lir::ChoiceSet {
            choices,
            gather_target: convergence_target,
        })];

        // Determine the container name
        let wrapper_id = wrapper_target.unwrap_or(plan.root_id);
        let wrapper_name = if is_first {
            // First container: use opening_gather label or "g-N"
            cs.opening_gather
                .as_ref()
                .and_then(|g| g.label.as_ref())
                .map_or_else(|| format!("g-{}", *gather_counter - 2), |l| l.text.clone())
        } else {
            // Previous convergence allocation was at prev_convergence_counter
            format!("g-{prev_convergence_counter}")
        };

        let label_id = if is_first {
            cs.opening_gather.as_ref().and_then(|g| g.label.as_ref())
        } else {
            None
        }
        .and_then(|label| {
            let qualified = if ctx.scope_path.is_empty() {
                label.text.clone()
            } else {
                format!("{}.{}", ctx.scope_path, label.text)
            };
            ctx.index
                .by_name
                .get(&qualified)
                .and_then(|ids| ids.first())
                .copied()
        });

        containers.push(lir::Container {
            id: wrapper_id,
            name: Some(wrapper_name),
            kind: lir::ContainerKind::Gather,
            params: Vec::new(),
            body: cs_body,
            children: choice_children,
            counting_flags: CountingFlags::empty(),
            temp_slot_count: 0,
            label_id,
            inline: is_first,
        });

        prev_convergence_target = convergence_target;
        prev_convergence_counter = convergence_counter;
        pos += 1;

        // If this CS has no gather, we're at the end of the chain
        if cs.gather.is_none() {
            // Build terminal gather
            let terminal_id = convergence_target.unwrap_or(plan.root_id);
            containers.push(build_implicit_gather_with_body(
                terminal_id,
                gather_counter,
                Vec::new(),
            ));
            break;
        }

        // If next stmt is not a ChoiceSet, the chain ends and the gather
        // wraps the remaining trailing stmts (normal gather behavior).
        if pos >= block.stmts.len() || !matches!(&block.stmts[pos], hir::Stmt::ChoiceSet(_)) {
            let gather = cs.gather.as_ref().unwrap_or_else(|| unreachable!());
            let gather_container = build_gather_container(
                gather,
                block,
                pos - 1,
                ctx,
                plan,
                convergence_target,
                *gather_counter - 1,
            );
            containers.push(gather_container);
            pos = block.stmts.len(); // build_gather_container consumed trailing stmts
            break;
        }
    }

    (containers, pos - start)
}

#[expect(clippy::too_many_lines)]
fn lower_choice_with_child(
    choice: &hir::Choice,
    ctx: &mut LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    choice_counter: &mut usize,
    gather_target: Option<brink_format::DefinitionId>,
) -> (lir::Choice, Option<lir::Container>) {
    let key = plan::ChoiceKey {
        file: ctx.file,
        scope: ctx.scope_path.clone(),
        index: *choice_counter,
    };
    *choice_counter += 1;

    let target = plan
        .choice_targets
        .get(&key)
        .copied()
        .unwrap_or(plan.root_id);

    // Preserve the three-part content split for codegen backends.
    let start_content = choice
        .start_content
        .as_ref()
        .map(|c| content::lower_content(c, ctx));
    let choice_only_content = choice
        .bracket_content
        .as_ref()
        .map(|c| content::lower_content(c, ctx));
    let inner_content = choice
        .inner_content
        .as_ref()
        .map(|c| content::lower_content(c, ctx));

    let condition = choice.condition.as_ref().map(|e| expr::lower_expr(e, ctx));
    let tags = choice.tags.iter().map(|t| t.text.clone()).collect();

    // Lower choice body into a child container.
    // Update scope_path to match the planner's convention so nested
    // choice/gather keys resolve to the correct container IDs.
    let old_scope = ctx.scope_path.clone();
    ctx.scope_path = format!("{}.c{}", old_scope, *choice_counter - 1);
    let mut cc = 0;
    let mut gc = 0;
    let (body_stmts, mut children) =
        lower_block_with_children(&choice.body, ctx, plan, &mut cc, &mut gc);
    ctx.scope_path = old_scope;

    // Build the choice target container body. The output after selecting
    // a choice is: start_content + inner_content + newline + body.
    // ChoiceOutput bundles content + optional inline divert + newline
    // into a single statement so codegen backends can handle it atomically.
    let mut body: Vec<lir::Stmt> = Vec::new();

    // 1. Choice output preamble: start+inner content, inline divert, newline
    let mut output_parts = Vec::new();
    if let Some(ref sc) = start_content {
        output_parts.extend(sc.parts.clone());
    }
    if let Some(ref ic) = inner_content {
        output_parts.extend(ic.parts.clone());
    }
    if !output_parts.is_empty() {
        let inline_divert = choice.divert.as_ref().map(|d| lower_hir_divert(d, ctx));
        body.push(lir::Stmt::ChoiceOutput {
            content: lir::Content {
                parts: output_parts,
                tags: Vec::new(),
            },
            inline_divert,
        });
    } else if let Some(ref divert) = choice.divert {
        // No output content but has inline divert — emit divert standalone
        body.push(lir::Stmt::Divert(lower_hir_divert(divert, ctx)));
    }

    // 2. Body statements from the choice's indented block
    body.extend(body_stmts);

    // 5. Auto-gather divert when the body doesn't end with Done/End.
    let ends_with_terminal = body.last().is_some_and(|s| {
        matches!(
            s,
            lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Done | lir::DivertTarget::End)
        )
    });
    if !ends_with_terminal && let Some(gather_id) = gather_target {
        let body_ends_with_choice_set = body
            .last()
            .is_some_and(|s| matches!(s, lir::Stmt::ChoiceSet(_)));

        let divert = lir::Divert {
            target: lir::DivertTarget::Container(gather_id),
            args: Vec::new(),
        };

        if body_ends_with_choice_set {
            // The body ends with a ChoiceSet → `done` stops execution,
            // so a divert appended to the body would be dead code.
            // Instead, append the outer gather divert to the last child
            // gather container (implicit or explicit) so that after the
            // inner gather's content, execution flows to the outer gather.
            if let Some(gather) = children
                .last_mut()
                .filter(|c| c.kind == lir::ContainerKind::Gather)
            {
                let gather_body_ends_terminal = gather.body.last().is_some_and(|s| {
                    matches!(
                        s,
                        lir::Stmt::Divert(d)
                            if matches!(
                                d.target,
                                lir::DivertTarget::End
                                    | lir::DivertTarget::Done
                                    | lir::DivertTarget::Container(_)
                            )
                    )
                });
                if gather_body_ends_terminal {
                    // Replace the terminal (e.g., Done) with the outer gather divert
                    let last_idx = gather.body.len() - 1;
                    gather.body[last_idx] = lir::Stmt::Divert(divert);
                } else {
                    // Append the divert after the gather's content
                    gather.body.push(lir::Stmt::Divert(divert));
                }
            } else {
                body.push(lir::Stmt::Divert(divert));
            }
        } else {
            body.push(lir::Stmt::Divert(divert));
        }
    }

    // Look up the label's DefinitionId if the choice has a label.
    let label_id = choice.label.as_ref().and_then(|label| {
        let qualified = if ctx.scope_path.is_empty() {
            label.text.clone()
        } else {
            format!("{}.{}", ctx.scope_path, label.text)
        };
        ctx.index
            .by_name
            .get(&qualified)
            .and_then(|ids| ids.first())
            .copied()
    });

    let child_name = format!("c-{}", *choice_counter - 1);
    let child = lir::Container {
        id: target,
        name: Some(child_name),
        kind: lir::ContainerKind::ChoiceTarget,
        params: Vec::new(),
        body,
        children,
        counting_flags: if choice.is_sticky {
            CountingFlags::empty()
        } else {
            CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY
        },
        temp_slot_count: 0,
        label_id,
        inline: false,
    };

    let lir_choice = lir::Choice {
        is_sticky: choice.is_sticky,
        is_fallback: choice.is_fallback,
        condition,
        start_content,
        choice_only_content,
        inner_content,
        target,
        tags,
        has_inline_divert: choice.divert.is_some(),
    };

    (lir_choice, Some(child))
}

fn build_gather_container(
    gather: &hir::Gather,
    parent_block: &hir::Block,
    choice_set_pos: usize,
    ctx: &mut LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    gather_id: Option<brink_format::DefinitionId>,
    gather_index: usize,
) -> lir::Container {
    let id = gather_id.unwrap_or(plan.root_id);
    let display_name = gather
        .label
        .as_ref()
        .map_or_else(|| format!("g-{gather_index}"), |l| l.text.clone());

    let mut body = Vec::new();

    // Emit gather's inline content
    if let Some(ref c) = gather.content {
        body.push(lir::Stmt::EmitContent(content::lower_content(c, ctx)));
        body.push(lir::Stmt::EndOfLine);
    }
    if let Some(ref d) = gather.divert {
        body.push(lir::Stmt::Divert(lower_hir_divert(d, ctx)));
    }

    // Lower trailing statements from the parent block after the ChoiceSet
    let mut cc = 0;
    let mut gc = 0;
    let trailing_block = hir::Block {
        stmts: parent_block.stmts[choice_set_pos + 1..].to_vec(),
    };
    let (trailing, children) =
        lower_block_with_children(&trailing_block, ctx, plan, &mut cc, &mut gc);
    body.extend(trailing);

    // Look up the gather label's DefinitionId if it has one.
    let label_id = gather.label.as_ref().and_then(|label| {
        let qualified = if ctx.scope_path.is_empty() {
            label.text.clone()
        } else {
            format!("{}.{}", ctx.scope_path, label.text)
        };
        ctx.index
            .by_name
            .get(&qualified)
            .and_then(|ids| ids.first())
            .copied()
    });

    lir::Container {
        id,
        name: Some(display_name),
        kind: lir::ContainerKind::Gather,
        params: Vec::new(),
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        label_id,
        inline: false,
    }
}

/// Build an implicit gather container with optional trailing body.
///
/// If `trailing` is empty, the gather contains just `Done`.
/// If `trailing` has statements (e.g., from remaining content after the
/// choice set), they are included so that choice targets can reach them.
fn build_implicit_gather_with_body(
    id: brink_format::DefinitionId,
    gather_counter: &mut usize,
    trailing: Vec<lir::Stmt>,
) -> lir::Container {
    let name = format!("g-{}", *gather_counter - 1);
    let body = if trailing.is_empty() {
        vec![lir::Stmt::Divert(lir::Divert {
            target: lir::DivertTarget::Done,
            args: Vec::new(),
        })]
    } else {
        trailing
    };
    lir::Container {
        id,
        name: Some(name),
        kind: lir::ContainerKind::Gather,
        params: Vec::new(),
        body,
        children: Vec::new(),
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        label_id: None,
        inline: false,
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

fn make_ctx<'a>(
    file: FileId,
    resolutions: &'a ResolutionLookup,
    index: &'a SymbolIndex,
    temps: &'a TempMap,
    names: &'a mut NameTable,
    scope_path: String,
) -> LowerCtx<'a> {
    LowerCtx {
        file,
        resolutions,
        index,
        temps,
        names,
        scope_path,
    }
}

fn lower_params(
    params: &[hir::Param],
    names: &mut NameTable,
    temp_map: &TempMap,
) -> Vec<lir::Param> {
    params
        .iter()
        .map(|p| {
            let name = names.intern(&p.name.text);
            let slot = temp_map.get(&p.name.text).unwrap_or(0);
            lir::Param {
                name,
                slot,
                is_ref: p.is_ref,
                is_divert: p.is_divert,
            }
        })
        .collect()
}

fn lower_hir_divert(divert: &hir::Divert, ctx: &mut LowerCtx<'_>) -> lir::Divert {
    let args = divert
        .target
        .args
        .iter()
        .map(|a| lir::CallArg::Value(expr::lower_expr(a, ctx)))
        .collect();

    let target = match &divert.target.path {
        hir::DivertPath::Done => lir::DivertTarget::Done,
        hir::DivertPath::End => lir::DivertTarget::End,
        hir::DivertPath::Path(path) => {
            if let Some(info) = ctx.resolve_path(path.range) {
                match info.kind {
                    crate::symbols::SymbolKind::Variable | crate::symbols::SymbolKind::Constant => {
                        lir::DivertTarget::Variable(info.id)
                    }
                    _ => lir::DivertTarget::Container(info.id),
                }
            } else {
                lir::DivertTarget::Done
            }
        }
    };

    lir::Divert { target, args }
}

fn find_gather_target(
    ctx: &LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    gather_counter: &mut usize,
) -> Option<brink_format::DefinitionId> {
    let key = plan::GatherKey {
        file: ctx.file,
        scope: ctx.scope_path.clone(),
        index: *gather_counter,
    };
    *gather_counter += 1;
    plan.gather_targets.get(&key).copied()
}

// ─── Counting flags ─────────────────────────────────────────────────

fn apply_counting_flags(root: &mut lir::Container) {
    let mut visit_ids = Vec::new();
    let mut turns_ids = Vec::new();

    // Collect phase: walk entire tree
    collect_counting_refs_tree(root, &mut visit_ids, &mut turns_ids);

    // Apply phase: walk entire tree
    apply_counting_flags_tree(root, &visit_ids, &turns_ids);
}

fn collect_counting_refs_tree(
    container: &lir::Container,
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    collect_counting_refs(&container.body, visit_ids, turns_ids);
    for child in &container.children {
        collect_counting_refs_tree(child, visit_ids, turns_ids);
    }
}

fn apply_counting_flags_tree(
    container: &mut lir::Container,
    visit_ids: &[brink_format::DefinitionId],
    turns_ids: &[brink_format::DefinitionId],
) {
    if visit_ids.contains(&container.id) {
        container.counting_flags |= CountingFlags::VISITS;
    }
    if turns_ids.contains(&container.id) {
        container.counting_flags |= CountingFlags::TURNS;
    }
    for child in &mut container.children {
        apply_counting_flags_tree(child, visit_ids, turns_ids);
    }
}

fn collect_counting_refs(
    stmts: &[lir::Stmt],
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    for stmt in stmts {
        match stmt {
            lir::Stmt::EmitContent(content) | lir::Stmt::ChoiceOutput { content, .. } => {
                collect_counting_refs_content(content, visit_ids, turns_ids);
            }
            lir::Stmt::Assign { value: e, .. }
            | lir::Stmt::DeclareTemp { value: Some(e), .. }
            | lir::Stmt::Return { value: Some(e), .. }
            | lir::Stmt::ExprStmt(e) => {
                collect_counting_refs_expr(e, visit_ids, turns_ids);
            }
            lir::Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    if let Some(ref cond) = choice.condition {
                        collect_counting_refs_expr(cond, visit_ids, turns_ids);
                    }
                    if let Some(ref c) = choice.start_content {
                        collect_counting_refs_content(c, visit_ids, turns_ids);
                    }
                    if let Some(ref c) = choice.choice_only_content {
                        collect_counting_refs_content(c, visit_ids, turns_ids);
                    }
                    if let Some(ref c) = choice.inner_content {
                        collect_counting_refs_content(c, visit_ids, turns_ids);
                    }
                }
            }
            lir::Stmt::Conditional(cond) => {
                for branch in &cond.branches {
                    if let Some(ref e) = branch.condition {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                    collect_counting_refs(&branch.body, visit_ids, turns_ids);
                }
            }
            lir::Stmt::Sequence(seq) => {
                for branch in &seq.branches {
                    collect_counting_refs(branch, visit_ids, turns_ids);
                }
            }
            _ => {}
        }
    }
}

fn collect_counting_refs_content(
    content: &lir::Content,
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    for part in &content.parts {
        match part {
            lir::ContentPart::Interpolation(e) => {
                collect_counting_refs_expr(e, visit_ids, turns_ids);
            }
            lir::ContentPart::InlineConditional(cond) => {
                for branch in &cond.branches {
                    if let Some(ref e) = branch.condition {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                    collect_counting_refs(&branch.body, visit_ids, turns_ids);
                }
            }
            lir::ContentPart::InlineSequence(seq) => {
                for branch in &seq.branches {
                    collect_counting_refs(branch, visit_ids, turns_ids);
                }
            }
            _ => {}
        }
    }
}

fn collect_counting_refs_expr(
    expr: &lir::Expr,
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    match expr {
        lir::Expr::VisitCount(id) => visit_ids.push(*id),
        lir::Expr::CallBuiltin {
            builtin: lir::BuiltinFn::TurnsSince,
            args,
        } => {
            for a in args {
                if let lir::Expr::DivertTarget(id) = a {
                    turns_ids.push(*id);
                }
                collect_counting_refs_expr(a, visit_ids, turns_ids);
            }
        }
        lir::Expr::Prefix(_, inner) | lir::Expr::Postfix(inner, _) => {
            collect_counting_refs_expr(inner, visit_ids, turns_ids);
        }
        lir::Expr::Infix(lhs, _, rhs) => {
            collect_counting_refs_expr(lhs, visit_ids, turns_ids);
            collect_counting_refs_expr(rhs, visit_ids, turns_ids);
        }
        lir::Expr::Call { args, .. } | lir::Expr::CallExternal { args, .. } => {
            for arg in args {
                if let lir::CallArg::Value(e) = arg {
                    collect_counting_refs_expr(e, visit_ids, turns_ids);
                }
            }
        }
        lir::Expr::CallBuiltin { args, .. } => {
            for a in args {
                collect_counting_refs_expr(a, visit_ids, turns_ids);
            }
        }
        lir::Expr::String(s) => {
            for p in &s.parts {
                if let lir::StringPart::Interpolation(e) = p {
                    collect_counting_refs_expr(e, visit_ids, turns_ids);
                }
            }
        }
        _ => {}
    }
}
