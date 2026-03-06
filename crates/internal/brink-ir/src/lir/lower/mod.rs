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
    let temp_map = TempMap::new();

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
        counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
        temp_slot_count: temp_count,
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
        counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
        temp_slot_count: 0,
    }
}

/// Lower a block, returning both statements and any child containers
/// (choice targets, gathers) produced by choice sets within the block.
///
/// When a `ChoiceSet` with a gather is encountered, remaining statements
/// go into the gather's body (not the current block).
fn lower_block_with_children(
    block: &hir::Block,
    ctx: &mut LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> (Vec<lir::Stmt>, Vec<lir::Container>) {
    let mut stmts = Vec::new();
    let mut children = Vec::new();

    for (pos, stmt) in block.stmts.iter().enumerate() {
        match stmt {
            hir::Stmt::ChoiceSet(cs) => {
                let gather_target = if cs.gather.is_some() {
                    find_gather_target(ctx, plan, gather_counter)
                } else {
                    None
                };

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

                // If there's a gather, build it with trailing statements
                if let Some(ref gather) = cs.gather {
                    let gather_container =
                        build_gather_container(gather, block, pos, ctx, plan, gather_target);
                    children.push(gather_container);
                    // Trailing statements went into the gather — stop here
                    break;
                }
            }
            _ => {
                if let Some(s) = stmts::lower_stmt(stmt, ctx, plan, choice_counter, gather_counter)
                {
                    stmts.push(s);
                }
            }
        }
    }

    (stmts, children)
}

fn lower_choice_with_child(
    choice: &hir::Choice,
    ctx: &mut LowerCtx<'_>,
    plan: &plan::ContainerPlan,
    choice_counter: &mut usize,
    _gather_target: Option<brink_format::DefinitionId>,
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

    // Combine display content: start + bracket
    let display = combine_content(
        choice.start_content.as_ref(),
        choice.bracket_content.as_ref(),
        ctx,
    );

    // Combine output content: start + inner
    let output = combine_content(
        choice.start_content.as_ref(),
        choice.inner_content.as_ref(),
        ctx,
    );

    let condition = choice.condition.as_ref().map(|e| expr::lower_expr(e, ctx));
    let tags = choice.tags.iter().map(|t| t.text.clone()).collect();

    // Lower choice body into a child container
    let mut cc = 0;
    let mut gc = 0;
    let (mut body, children) = lower_block_with_children(&choice.body, ctx, plan, &mut cc, &mut gc);

    if let Some(ref divert) = choice.divert {
        body.push(lir::Stmt::Divert(lower_hir_divert(divert, ctx)));
    }

    let child_name = format!("c{}", *choice_counter - 1);
    let child = lir::Container {
        id: target,
        name: Some(child_name),
        kind: lir::ContainerKind::ChoiceTarget,
        params: Vec::new(),
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
    };

    let lir_choice = lir::Choice {
        is_sticky: choice.is_sticky,
        is_fallback: choice.is_fallback,
        condition,
        display,
        output,
        target,
        tags,
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
) -> lir::Container {
    let id = gather_id.unwrap_or(plan.root_id);
    let name = gather.label.as_ref().map(|l| l.text.clone());
    let display_name = name.clone().unwrap_or_else(|| {
        // Anonymous gathers use gN naming
        "g-anon".to_string()
    });

    let mut body = Vec::new();

    // Emit gather's inline content
    if let Some(ref c) = gather.content {
        body.push(lir::Stmt::EmitContent(content::lower_content(c, ctx)));
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

    lir::Container {
        id,
        name: if gather.label.is_some() {
            Some(display_name)
        } else {
            // Use the planned name (gN) from the plan
            Some(display_name)
        },
        kind: lir::ContainerKind::Gather,
        params: Vec::new(),
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
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

fn combine_content(
    a: Option<&hir::Content>,
    b: Option<&hir::Content>,
    ctx: &mut LowerCtx<'_>,
) -> Option<lir::Content> {
    match (a, b) {
        (None, None) => None,
        (Some(content), None) | (None, Some(content)) => Some(content::lower_content(content, ctx)),
        (Some(a_content), Some(b_content)) => {
            let mut parts = Vec::new();
            for p in &a_content.parts {
                parts.push(content::lower_content_part_pub(p, ctx));
            }
            for p in &b_content.parts {
                parts.push(content::lower_content_part_pub(p, ctx));
            }
            let mut tags: Vec<String> = a_content.tags.iter().map(|t| t.text.clone()).collect();
            tags.extend(b_content.tags.iter().map(|t| t.text.clone()));
            Some(lir::Content { parts, tags })
        }
    }
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
            lir::Stmt::EmitContent(content) => {
                collect_counting_refs_content(content, visit_ids, turns_ids);
            }
            lir::Stmt::Assign { value: e, .. }
            | lir::Stmt::DeclareTemp { value: Some(e), .. }
            | lir::Stmt::Return(Some(e))
            | lir::Stmt::ExprStmt(e) => {
                collect_counting_refs_expr(e, visit_ids, turns_ids);
            }
            lir::Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    if let Some(ref cond) = choice.condition {
                        collect_counting_refs_expr(cond, visit_ids, turns_ids);
                    }
                    if let Some(ref d) = choice.display {
                        collect_counting_refs_content(d, visit_ids, turns_ids);
                    }
                    if let Some(ref o) = choice.output {
                        collect_counting_refs_content(o, visit_ids, turns_ids);
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
