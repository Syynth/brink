mod content;
mod context;
mod decls;
mod expr;
mod plan;
mod recognize;
mod stmts;
mod temps;

use std::collections::HashMap;

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
///
/// `file_paths` maps each `FileId` to its source file path for populating
/// `SourceLocation` on recognized lines.
#[expect(
    clippy::implicit_hasher,
    reason = "internal API, no need to generalize"
)]
pub fn lower_to_program(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    file_paths: &HashMap<FileId, String>,
) -> lir::Program {
    let resolutions = ResolutionLookup::build(resolutions);
    let mut names = NameTable::new();
    let mut ids = context::IdAllocator::new();

    // ── Step 1: Plan containers (pre-allocate IDs) ─────────────────
    let plan = plan::plan_containers(files, index, &mut ids);

    // ── Step 2: Collect declarations ────────────────────────────────
    let mut globals = decls::collect_globals(files, index, &mut names, &resolutions);
    let (lists, list_items, list_globals) = decls::collect_lists(files, index, &mut names);
    globals.extend(list_globals);
    let externals = decls::collect_externals(files, index, &mut names);

    // ── Step 3: Lower containers as a tree ──────────────────────────
    let root = lower_root(
        files,
        &resolutions,
        index,
        &mut names,
        &plan,
        &mut ids,
        file_paths,
    );

    // ── Step 4: Counting flags ──────────────────────────────────────
    let mut root = root;
    apply_counting_flags(&mut root, &globals);

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
    ids: &mut context::IdAllocator,
    file_paths: &HashMap<FileId, String>,
) -> lir::Container {
    let mut body = Vec::new();
    let mut children = Vec::new();

    // Allocate temp slots for root content (top-level ~ temp declarations).
    let root_blocks: Vec<&hir::Block> = files.iter().map(|(_, hir)| &hir.root_content).collect();
    let temp_map = temps::alloc_temps(&[], &[], &root_blocks);

    for &(file_id, hir_file) in files {
        let mut ctx = make_ctx(
            file_id,
            resolutions,
            index,
            &temp_map,
            names,
            ids,
            String::new(),
            &[],
            file_paths,
        );
        let mut cc = 0;
        let mut gc = 0;
        let mut sc = 0;
        let (stmts, mut block_children) = lower_block_with_children(
            &hir_file.root_content,
            &mut ctx,
            plan,
            &mut cc,
            &mut gc,
            &mut sc,
        );
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
                ids,
                plan,
                file_paths,
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
        labeled: false,
        inline: false,
        is_function: false,
    }
}

#[expect(clippy::too_many_arguments)]
fn lower_knot(
    file_id: FileId,
    _hir_file: &hir::HirFile,
    knot: &hir::Knot,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    ids: &mut context::IdAllocator,
    plan: &plan::ContainerPlan,
    file_paths: &HashMap<FileId, String>,
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

    let temp_map = temps::alloc_temps(&knot.params, &knot.stitches, &scope_blocks);
    let temp_count = temp_map.total_slots();
    let params = lower_params(&knot.params, names, &temp_map);

    let knot_param_names: Vec<&str> = knot.params.iter().map(|p| p.name.text.as_str()).collect();
    let mut ctx = make_ctx(
        file_id,
        resolutions,
        index,
        &temp_map,
        names,
        ids,
        knot_name.clone(),
        &knot_param_names,
        file_paths,
    );
    let mut cc = 0;
    let mut gc = 0;
    let mut sc = 0;
    let (body, mut children) =
        lower_block_with_children(&knot.body, &mut ctx, plan, &mut cc, &mut gc, &mut sc);

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
            ids,
            plan,
            file_paths,
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
            target: lir::DivertTarget::Address(first_stitch.id),
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
        labeled: false,
        inline: false,
        is_function: knot.is_function,
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
    ids: &mut context::IdAllocator,
    plan: &plan::ContainerPlan,
    file_paths: &HashMap<FileId, String>,
) -> lir::Container {
    let stitch_name = &stitch.name.text;
    let stitch_path = format!("{}.{stitch_name}", knot.name.text);
    let stitch_id = plan
        .stitch_ids
        .get(stitch_path.as_str())
        .copied()
        .unwrap_or(plan.root_id);
    let params = lower_params(&stitch.params, names, temp_map);

    let stitch_param_names: Vec<&str> =
        stitch.params.iter().map(|p| p.name.text.as_str()).collect();
    let mut ctx = make_ctx(
        file_id,
        resolutions,
        index,
        temp_map,
        names,
        ids,
        stitch_path,
        &stitch_param_names,
        file_paths,
    );
    let mut cc = 0;
    let mut gc = 0;
    let mut sc = 0;
    let (body, children) =
        lower_block_with_children(&stitch.body, &mut ctx, plan, &mut cc, &mut gc, &mut sc);

    lir::Container {
        id: stitch_id,
        name: Some(stitch_name.clone()),
        kind: lir::ContainerKind::Stitch,
        params,
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
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
    seq_counter: &mut usize,
) -> (Vec<lir::Stmt>, Vec<lir::Container>) {
    let mut stmts = Vec::new();
    let mut children = Vec::new();
    let mut pos = 0;

    while pos < block.stmts.len() {
        let stmt = &block.stmts[pos];
        match stmt {
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

                // Build gather container from the continuation block.
                // The HIR nests all post-gather content into the continuation,
                // so no trailing-stmt consumption is needed.
                let gather_container = build_continuation_container(
                    &cs.continuation,
                    ctx,
                    plan,
                    gather_target,
                    *gather_counter - 1,
                    choice_counter,
                    gather_counter,
                );
                children.push(gather_container);
                pos += 1;
            }
            hir::Stmt::LabeledBlock(labeled) => {
                // Labeled block wrapping content (standalone gather or opening
                // gather pattern). Enter the wrapper container so execution
                // returns to the parent when the child finishes — this allows
                // sibling LabeledBlocks to chain (e.g. `- (opts) ... - (test)`).
                let wrapper_target = find_gather_target(ctx, plan, gather_counter);
                let wrapper_id = wrapper_target.unwrap_or(plan.root_id);

                stmts.push(lir::Stmt::EnterContainer(wrapper_id));

                let display_name = labeled
                    .label
                    .as_ref()
                    .map_or_else(|| format!("g-{}", *gather_counter - 1), |l| l.text.clone());

                let labeled_flag = labeled
                    .label
                    .as_ref()
                    .is_some_and(|label| ctx.lookup_address_id(&label.text).is_some());

                // Lower the labeled block's contents
                let mut inner_sc = 0;
                let (inner_stmts, inner_children) = lower_block_with_children(
                    labeled,
                    ctx,
                    plan,
                    choice_counter,
                    gather_counter,
                    &mut inner_sc,
                );

                children.push(lir::Container {
                    id: wrapper_id,
                    name: Some(display_name),
                    kind: lir::ContainerKind::Gather,
                    params: Vec::new(),
                    body: inner_stmts,
                    children: inner_children,
                    counting_flags: CountingFlags::empty(),
                    temp_slot_count: 0,
                    labeled: labeled_flag,
                    inline: true,
                    is_function: false,
                });
                pos += 1;
            }
            hir::Stmt::Conditional(cond) => {
                // Lower conditional branches with lower_block_with_children
                // so ChoiceSets inside branches produce child containers.
                // Each branch body is wrapped in its own child container.
                //
                // The `in_conditional_branch` flag in codegen suppresses `Done`
                // inside branch containers. This is correct because ink
                // conditionals can gate choice visibility — choices across all
                // branches form a single logical ChoiceSet, and the runtime
                // auto-presents pending choices on frame/container exhaustion
                // (vm.rs handle_frame_exhaustion), so no explicit `Done` is needed.
                let kind = match &cond.kind {
                    hir::CondKind::InitialCondition => lir::CondKind::InitialCondition,
                    hir::CondKind::IfElse => lir::CondKind::IfElse,
                    hir::CondKind::Switch(expr) => {
                        lir::CondKind::Switch(expr::lower_expr(expr, ctx))
                    }
                };

                let cond_idx = *seq_counter;
                *seq_counter += 1;

                // Push a scope prefix for this conditional so nested
                // conditionals inside branches get unique container paths.
                let cond_scope = format!("b-{cond_idx}");
                let old_scope = ctx.scope_path.clone();

                let branches = cond
                    .branches
                    .iter()
                    .enumerate()
                    .map(|(branch_idx, b)| {
                        let condition = b.condition.as_ref().map(|e| expr::lower_expr(e, ctx));

                        // Set scope_path for this branch so nested containers
                        // (choices, gathers, nested conditionals) get unique IDs.
                        let branch_scope = if old_scope.is_empty() {
                            format!("{cond_scope}.{branch_idx}")
                        } else {
                            format!("{old_scope}.{cond_scope}.{branch_idx}")
                        };
                        ctx.scope_path = branch_scope;

                        // Pass through parent choice/gather counters — a ChoiceSet
                        // inside a conditional shares the enclosing scope and must
                        // not collide with sibling gathers/choices.
                        let mut sc2 = 0;
                        let (body, branch_children) = lower_block_with_children(
                            &b.body,
                            ctx,
                            plan,
                            choice_counter,
                            gather_counter,
                            &mut sc2,
                        );

                        // Create a child container for this branch
                        let branch_path = if old_scope.is_empty() {
                            format!("{cond_scope}.{branch_idx}")
                        } else {
                            format!("{old_scope}.{cond_scope}.{branch_idx}")
                        };
                        let branch_id = ctx.ids.alloc_address(&branch_path);

                        let branch_container = lir::Container {
                            id: branch_id,
                            name: Some(format!("{branch_idx}")),
                            kind: lir::ContainerKind::ConditionalBranch,
                            params: Vec::new(),
                            body,
                            children: branch_children,
                            counting_flags: CountingFlags::empty(),
                            temp_slot_count: 0,
                            labeled: false,
                            inline: false,
                            is_function: false,
                        };
                        children.push(branch_container);

                        // The branch body in the Conditional struct is just EnterContainer
                        lir::CondBranch {
                            condition,
                            body: vec![lir::Stmt::EnterContainer(branch_id)],
                        }
                    })
                    .collect();

                // Restore scope_path after processing branches.
                ctx.scope_path = old_scope;

                stmts.push(lir::Stmt::Conditional(lir::Conditional { kind, branches }));
                pos += 1;
            }
            hir::Stmt::Sequence(seq) => {
                // Allocate a wrapper container for this sequence.
                let wrapper_id = ctx.alloc_sequence_id(*seq_counter);
                *seq_counter += 1;

                // Push the wrapper's name onto the scope path so that nested
                // sequences inside branches get unique IDs (e.g. `scope.s-0.s-0`
                // instead of colliding with the parent's `scope.s-0`).
                let display_name = format!("s-{}", *seq_counter - 1);
                let old_scope = ctx.scope_path.clone();
                ctx.scope_path = if old_scope.is_empty() {
                    display_name.clone()
                } else {
                    format!("{old_scope}.{display_name}")
                };

                // Lower each sequence branch into its own child container.
                // The wrapper's Sequence.branches hold [EnterContainer(branch_id)]
                // for each branch, and the actual branch content lives in child
                // containers.
                let mut wrapper_children = Vec::new();
                let branches: Vec<Vec<lir::Stmt>> = seq
                    .branches
                    .iter()
                    .enumerate()
                    .map(|(branch_idx, b)| {
                        let mut bc = 0;
                        let mut gc = 0;
                        let mut sc2 = 0;
                        let (body, branch_children) =
                            lower_block_with_children(b, ctx, plan, &mut bc, &mut gc, &mut sc2);

                        // Allocate a child container for this branch
                        let branch_path = if ctx.scope_path.is_empty() {
                            format!("{branch_idx}")
                        } else {
                            format!("{}.{branch_idx}", ctx.scope_path)
                        };
                        let branch_id = ctx.ids.alloc_address(&branch_path);

                        let branch_container = lir::Container {
                            id: branch_id,
                            name: Some(format!("{branch_idx}")),
                            kind: lir::ContainerKind::SequenceBranch,
                            params: Vec::new(),
                            body,
                            children: branch_children,
                            counting_flags: CountingFlags::empty(),
                            temp_slot_count: 0,
                            labeled: false,
                            inline: false,
                            is_function: false,
                        };
                        wrapper_children.push(branch_container);

                        // The branch body in the Sequence struct is just EnterContainer
                        vec![lir::Stmt::EnterContainer(branch_id)]
                    })
                    .collect();

                ctx.scope_path = old_scope;
                let wrapper = lir::Container {
                    id: wrapper_id,
                    name: Some(display_name),
                    kind: lir::ContainerKind::Sequence,
                    params: Vec::new(),
                    body: vec![lir::Stmt::Sequence(lir::Sequence {
                        kind: seq.kind,
                        branches,
                    })],
                    children: wrapper_children,
                    counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
                    temp_slot_count: 0,
                    labeled: false,
                    inline: false,
                    is_function: false,
                };
                children.push(wrapper);

                stmts.push(lir::Stmt::EnterContainer(wrapper_id));
                pos += 1;
            }
            _ => {
                if let Some(s) = stmts::lower_stmt(stmt, ctx) {
                    stmts.push(s);
                }
                // Drain any inline sequence containers created during content lowering.
                children.append(&mut ctx.pending_children);
                pos += 1;
            }
        }
    }

    (stmts, children)
}

/// Build a gather container from a `ChoiceSet`'s continuation block.
///
/// The continuation's label becomes the container name, its stmts become
/// the body (lowered via `lower_block_with_children` to handle nested
/// `ChoiceSet`s in gather-choice chains).
fn build_continuation_container(
    continuation: &hir::Block,
    ctx: &mut LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    gather_id: Option<brink_format::DefinitionId>,
    gather_index: usize,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> lir::Container {
    let id = gather_id.unwrap_or(plan.root_id);
    let display_name = continuation
        .label
        .as_ref()
        .map_or_else(|| format!("g-{gather_index}"), |l| l.text.clone());

    // Check if the gather has a source-level label that resolves.
    let labeled = continuation
        .label
        .as_ref()
        .is_some_and(|label| ctx.lookup_address_id(&label.text).is_some());

    if continuation.stmts.is_empty() && continuation.label.is_none() {
        // Empty continuation with no label — implicit gather with Done
        return lir::Container {
            id,
            name: Some(display_name),
            kind: lir::ContainerKind::Gather,
            params: Vec::new(),
            body: vec![lir::Stmt::Divert(lir::Divert {
                target: lir::DivertTarget::Done,
                args: Vec::new(),
            })],
            children: Vec::new(),
            counting_flags: CountingFlags::empty(),
            temp_slot_count: 0,
            labeled: false,
            inline: false,
            is_function: false,
        };
    }

    // Lower continuation stmts — may contain nested ChoiceSets (gather-choice chains)
    let mut sc = 0;
    let (body, children) = lower_block_with_children(
        continuation,
        ctx,
        plan,
        choice_counter,
        gather_counter,
        &mut sc,
    );

    lir::Container {
        id,
        name: Some(display_name),
        kind: lir::ContainerKind::Gather,
        params: Vec::new(),
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        labeled,
        inline: false,
        is_function: false,
    }
}

#[expect(clippy::too_many_lines, reason = "choice lowering has many parts")]
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

    // ── Compose and recognize display/output content at HIR level ──
    // Display = start + bracket, Output = start + inner.
    let display_hir = recognize::compose_hir_content_opt(
        choice.start_content.as_ref(),
        choice.bracket_content.as_ref(),
    );
    let output_hir = recognize::compose_hir_content_opt(
        choice.start_content.as_ref(),
        choice.inner_content.as_ref(),
    );

    // Skip recognition when composed content starts with whitespace-only
    // text — the inline emission path's `push_text` suppresses leading whitespace
    // that `EvalLine`/`EmitLine` would preserve, changing observable behavior.
    let display_ws = display_hir
        .as_ref()
        .is_some_and(recognize::starts_with_whitespace_only_text);
    let output_ws = output_hir
        .as_ref()
        .is_some_and(recognize::starts_with_whitespace_only_text);

    let display_emission = if display_ws {
        None
    } else {
        display_hir
            .as_ref()
            .and_then(|c| recognize::try_recognize(c, ctx))
    };
    let output_emission = if output_ws {
        None
    } else {
        output_hir
            .as_ref()
            .and_then(|c| recognize::try_recognize(c, ctx))
    };

    let condition = choice.condition.as_ref().map(|e| expr::lower_expr(e, ctx));
    let tags: Vec<Vec<lir::ContentPart>> = choice
        .tags
        .iter()
        .map(|t| content::lower_content_parts_pub(&t.parts, ctx))
        .collect();

    // Lower choice body into a child container.
    // Update scope_path to match the planner's convention so nested
    // choice/gather keys resolve to the correct container IDs.
    let old_scope = ctx.scope_path.clone();
    ctx.scope_path = format!("{}.c{}", old_scope, *choice_counter - 1);
    let mut cc = 0;
    let mut gc = 0;
    let mut sc = 0;
    let (body_stmts, mut children) =
        lower_block_with_children(&choice.body, ctx, plan, &mut cc, &mut gc, &mut sc);
    ctx.scope_path = old_scope;

    // Build the choice target container body. The output after selecting
    // a choice is: ChoiceOutput(content) + body stmts.
    // The HIR body already contains the inline divert and EndOfLine as
    // its first statements, so they flow naturally into the LIR body.
    let mut body: Vec<lir::Stmt> = Vec::new();

    // 1. Choice output preamble: start+inner content with their tags.
    // Tags on start/inner content appear in the output after choosing;
    // bracket-only tags are suppressed (they only affect choice display).
    {
        let mut output_parts = Vec::new();
        let mut output_tags = Vec::new();
        if let Some(ref sc) = start_content {
            output_parts.extend(sc.parts.clone());
            output_tags.extend(sc.tags.clone());
        }
        if let Some(ref ic) = inner_content {
            output_parts.extend(ic.parts.clone());
            output_tags.extend(ic.tags.clone());
        }
        if !output_parts.is_empty() || !output_tags.is_empty() {
            body.push(lir::Stmt::ChoiceOutput {
                content: lir::Content {
                    parts: output_parts,
                    tags: output_tags,
                },
                emission: output_emission.clone(),
            });
        }
    }

    // 2. Body statements from the choice's block (includes inline divert + EndOfLine)
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
            target: lir::DivertTarget::Address(gather_id),
            args: Vec::new(),
        };

        if body_ends_with_choice_set {
            // The body ends with a ChoiceSet → `done` stops execution,
            // so a divert appended to the body would be dead code.
            // Instead, patch the innermost gather container so that
            // after the inner gather's content, execution flows to the
            // outer gather. This recurses through nested choice-set-
            // in-gather chains (multi-level weaves).
            patch_innermost_gather(&mut children, divert);
        } else {
            body.push(lir::Stmt::Divert(divert));
        }
    }

    // Check if the choice has a source-level label that resolves.
    let labeled = choice
        .label
        .as_ref()
        .is_some_and(|label| ctx.lookup_address_id(&label.text).is_some());

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
        labeled,
        inline: false,
        is_function: false,
    };

    let lir_choice = lir::Choice {
        is_sticky: choice.is_sticky,
        is_fallback: choice.is_fallback,
        condition,
        start_content,
        choice_only_content,
        inner_content,
        display_emission,
        output_emission,
        target,
        tags,
    };

    (lir_choice, Some(child))
}

// `lower_gather_choice_chain` and `build_gather_container` removed in Phase 2.
// Gather-choice chains are now handled via nested continuation blocks in the
// HIR, lowered naturally by `lower_block_with_children` + `build_continuation_container`.

// ─── Helpers ────────────────────────────────────────────────────────

#[expect(clippy::too_many_arguments)]
fn make_ctx<'a>(
    file: FileId,
    resolutions: &'a ResolutionLookup,
    index: &'a SymbolIndex,
    temps: &'a TempMap,
    names: &'a mut NameTable,
    ids: &'a mut context::IdAllocator,
    scope_path: String,
    param_names: &[&str],
    file_paths: &'a HashMap<FileId, String>,
) -> LowerCtx<'a> {
    LowerCtx {
        file,
        resolutions,
        index,
        temps,
        names,
        ids,
        scope_path,
        pending_children: Vec::new(),
        visible_temps: param_names.iter().map(|s| (*s).to_string()).collect(),
        file_paths,
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

fn apply_counting_flags(root: &mut lir::Container, globals: &[lir::GlobalDef]) {
    let mut visit_ids = Vec::new();
    let mut turns_ids = Vec::new();

    // Collect phase: walk entire tree for explicit visit/turn refs
    collect_counting_refs_tree(root, &mut visit_ids, &mut turns_ids);

    // Also scan global variable defaults for DivertTarget values
    // (e.g. `VAR x = -> knot` — the target could be reached via variable divert)
    for g in globals {
        if let lir::ConstValue::DivertTarget(id) = &g.default {
            visit_ids.push(*id);
            turns_ids.push(*id);
        }
    }

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
        // Labeled containers (gathers with labels like `- (loop)`) need
        // COUNT_START_ONLY so that self-goto loops correctly increment
        // the visit count in the runtime's goto_target handler.
        if container.labeled {
            container.counting_flags |= CountingFlags::COUNT_START_ONLY;
        }
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
            lir::Stmt::EmitLine(emission) | lir::Stmt::EvalLine(emission) => {
                // Template slot expressions may contain counting refs.
                if let lir::RecognizedLine::Template { slot_exprs, .. } = &emission.line {
                    for e in slot_exprs {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                }
                // Tags may contain dynamic expressions — traverse them.
                for tag in &emission.tags {
                    for part in tag {
                        if let lir::ContentPart::Interpolation(e) = part {
                            collect_counting_refs_expr(e, visit_ids, turns_ids);
                        }
                    }
                }
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
                    // Traverse recognized emissions for counting refs in slot exprs.
                    for emission in choice
                        .display_emission
                        .iter()
                        .chain(choice.output_emission.iter())
                    {
                        if let lir::RecognizedLine::Template { slot_exprs, .. } = &emission.line {
                            for e in slot_exprs {
                                collect_counting_refs_expr(e, visit_ids, turns_ids);
                            }
                        }
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
            lir::Stmt::Divert(d) => {
                for arg in &d.args {
                    if let lir::CallArg::Value(e) = arg {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                }
            }
            lir::Stmt::TunnelCall(tc) => {
                for t in &tc.targets {
                    for arg in &t.args {
                        if let lir::CallArg::Value(e) = arg {
                            collect_counting_refs_expr(e, visit_ids, turns_ids);
                        }
                    }
                }
            }
            lir::Stmt::ThreadStart(ts) => {
                for arg in &ts.args {
                    if let lir::CallArg::Value(e) = arg {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                }
            }
            // EnterContainer, DeclareTemp(None), Return(None), etc.
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
            // Text, Glue, EnterSequence
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
        lir::Expr::DivertTarget(id) => {
            // Any container whose address is taken could be reached via
            // variable divert/tunnel — conservatively mark for visit tracking.
            visit_ids.push(*id);
            turns_ids.push(*id);
        }
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

/// Recursively find the innermost gather container in a chain of
/// gather-contains-`ChoiceSet` nesting and patch it with the given divert.
///
/// When a choice body ends with a `ChoiceSet`, its gather container may
/// itself end with another `ChoiceSet` (multi-level weaves). The divert
/// to the outer gather must be placed in the innermost gather that
/// doesn't end with yet another `ChoiceSet`, otherwise it becomes dead
/// code after the `done` emitted by codegen for the `ChoiceSet`.
fn patch_innermost_gather(children: &mut [lir::Container], divert: lir::Divert) {
    let Some(gather) = children
        .last_mut()
        .filter(|c| c.kind == lir::ContainerKind::Gather)
    else {
        return;
    };

    let gather_body_ends_with_choice_set = gather
        .body
        .last()
        .is_some_and(|s| matches!(s, lir::Stmt::ChoiceSet(_)));

    if gather_body_ends_with_choice_set {
        // Recurse into the gather's children to find the deeper gather
        patch_innermost_gather(&mut gather.children, divert);
        return;
    }

    let gather_body_ends_terminal = gather.body.last().is_some_and(|s| {
        matches!(
            s,
            lir::Stmt::Divert(d)
                if matches!(
                    d.target,
                    lir::DivertTarget::End
                        | lir::DivertTarget::Done
                        | lir::DivertTarget::Address(_)
                )
        )
    });

    if gather_body_ends_terminal {
        // Replace the terminal (e.g., Done) with the outer gather divert
        let last_idx = gather.body.len() - 1;
        gather.body[last_idx] = lir::Stmt::Divert(divert);
    } else {
        gather.body.push(lir::Stmt::Divert(divert));
    }
}
