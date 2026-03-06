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

    // ── Step 1: Plan containers ─────────────────────────────────────
    let plan = plan::plan_containers(files, index, &mut ids);

    // ── Step 2: Collect declarations ────────────────────────────────
    let globals = decls::collect_globals(files, index, &mut names, &resolutions);
    let (lists, list_items) = decls::collect_lists(files, index, &mut names);
    let externals = decls::collect_externals(files, index, &mut names);

    // ── Step 3: Lower containers ────────────────────────────────────
    let mut containers = Vec::with_capacity(plan.shells.len());

    for shell in &plan.shells {
        let container = lower_container_shell(files, shell, &resolutions, index, &mut names, &plan);
        containers.push(container);
    }

    // ── Step 4: Implicit structure ──────────────────────────────────
    apply_implicit_structure(&mut containers, &plan);

    // ── Step 5: Counting flags ──────────────────────────────────────
    apply_counting_flags(&mut containers);

    lir::Program {
        containers,
        globals,
        lists,
        list_items,
        externals,
        name_table: names.into_entries(),
    }
}

// ─── Container lowering ─────────────────────────────────────────────

fn lower_container_shell(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    plan_data: &plan::ContainerPlan,
) -> lir::Container {
    match shell.kind {
        lir::ContainerKind::Root => {
            lower_root_container(files, shell, resolutions, index, plan_data)
        }
        lir::ContainerKind::Knot => {
            lower_knot_container(files, shell, resolutions, index, names, plan_data)
        }
        lir::ContainerKind::Stitch => {
            lower_stitch_container(files, shell, resolutions, index, names, plan_data)
        }
        lir::ContainerKind::ChoiceTarget => {
            lower_choice_target_container(files, shell, resolutions, index, plan_data)
        }
        lir::ContainerKind::Gather => {
            lower_gather_container(files, shell, resolutions, index, plan_data)
        }
    }
}

fn make_ctx<'a>(
    file: FileId,
    resolutions: &'a ResolutionLookup,
    index: &'a SymbolIndex,
    temps: &'a TempMap,
    scope_path: String,
) -> LowerCtx<'a> {
    LowerCtx {
        file,
        resolutions,
        index,
        temps,
        scope_path,
    }
}

fn lower_root_container(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    plan_data: &plan::ContainerPlan,
) -> lir::Container {
    let mut body = Vec::new();
    let temp_map = TempMap::new();

    for &(file_id, hir_file) in files {
        let mut ctx = make_ctx(file_id, resolutions, index, &temp_map, String::new());
        let mut cc = 0;
        let mut gc = 0;
        let stmts = stmts::lower_block(
            &hir_file.root_content,
            &mut ctx,
            plan_data,
            &mut cc,
            &mut gc,
        );
        body.extend(stmts);
    }

    lir::Container {
        id: shell.id,
        path: shell.path.clone(),
        kind: lir::ContainerKind::Root,
        scope_root: None,
        params: Vec::new(),
        body,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
    }
}

fn lower_knot_container(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    plan_data: &plan::ContainerPlan,
) -> lir::Container {
    for &(file_id, hir_file) in files {
        for knot in &hir_file.knots {
            if knot.name.text != shell.path {
                continue;
            }
            let mut scope_blocks: Vec<&hir::Block> = vec![&knot.body];
            for stitch in &knot.stitches {
                scope_blocks.push(&stitch.body);
            }

            let temp_map = temps::alloc_temps(&knot.params, &scope_blocks);
            let temp_count = temp_map.total_slots();
            let params = lower_params(&knot.params, names, &temp_map);

            let mut ctx = make_ctx(file_id, resolutions, index, &temp_map, shell.path.clone());
            let mut cc = 0;
            let mut gc = 0;
            let body = stmts::lower_block(&knot.body, &mut ctx, plan_data, &mut cc, &mut gc);

            return lir::Container {
                id: shell.id,
                path: shell.path.clone(),
                kind: lir::ContainerKind::Knot,
                scope_root: None,
                params,
                body,
                counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
                temp_slot_count: temp_count,
            };
        }
    }
    empty_container(shell)
}

fn lower_stitch_container(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    plan_data: &plan::ContainerPlan,
) -> lir::Container {
    let parts: Vec<&str> = shell.path.splitn(2, '.').collect();
    let (knot_name, stitch_name) = match parts.as_slice() {
        [k, s] => (*k, *s),
        _ => return empty_container(shell),
    };

    for &(file_id, hir_file) in files {
        for knot in &hir_file.knots {
            if knot.name.text != knot_name {
                continue;
            }
            for stitch in &knot.stitches {
                if stitch.name.text != stitch_name {
                    continue;
                }
                let mut scope_blocks: Vec<&hir::Block> = vec![&knot.body];
                for s in &knot.stitches {
                    scope_blocks.push(&s.body);
                }
                let temp_map = temps::alloc_temps(&knot.params, &scope_blocks);
                let params = lower_params(&stitch.params, names, &temp_map);

                let mut ctx = make_ctx(file_id, resolutions, index, &temp_map, shell.path.clone());
                let mut cc = 0;
                let mut gc = 0;
                let body = stmts::lower_block(&stitch.body, &mut ctx, plan_data, &mut cc, &mut gc);

                return lir::Container {
                    id: shell.id,
                    path: shell.path.clone(),
                    kind: lir::ContainerKind::Stitch,
                    scope_root: shell.scope_root,
                    params,
                    body,
                    counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
                    temp_slot_count: 0,
                };
            }
        }
    }
    empty_container(shell)
}

fn lower_choice_target_container(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    plan_data: &plan::ContainerPlan,
) -> lir::Container {
    if let Some(body) = find_choice_body(files, shell, resolutions, index, plan_data) {
        return lir::Container {
            id: shell.id,
            path: shell.path.clone(),
            kind: lir::ContainerKind::ChoiceTarget,
            scope_root: shell.scope_root,
            params: Vec::new(),
            body,
            counting_flags: CountingFlags::empty(),
            temp_slot_count: 0,
        };
    }
    empty_container(shell)
}

fn lower_gather_container(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    plan_data: &plan::ContainerPlan,
) -> lir::Container {
    if let Some(body) = find_gather_body(files, shell, resolutions, index, plan_data) {
        return lir::Container {
            id: shell.id,
            path: shell.path.clone(),
            kind: lir::ContainerKind::Gather,
            scope_root: shell.scope_root,
            params: Vec::new(),
            body,
            counting_flags: CountingFlags::empty(),
            temp_slot_count: 0,
        };
    }
    empty_container(shell)
}

// ─── Helpers ────────────────────────────────────────────────────────

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

fn empty_container(shell: &plan::ContainerShell) -> lir::Container {
    lir::Container {
        id: shell.id,
        path: shell.path.clone(),
        kind: shell.kind,
        scope_root: shell.scope_root,
        params: Vec::new(),
        body: Vec::new(),
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
    }
}

fn find_choice_body(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    plan_data: &plan::ContainerPlan,
) -> Option<Vec<lir::Stmt>> {
    for (key, &target_id) in &plan_data.choice_targets {
        if target_id != shell.id {
            continue;
        }
        let choice = find_hir_choice(files, key)?;
        let temp_map = build_scope_temp_map(files, shell);

        let mut ctx = make_ctx(key.file, resolutions, index, &temp_map, key.scope.clone());
        let mut cc = 0;
        let mut gc = 0;
        let mut body = stmts::lower_block(&choice.body, &mut ctx, plan_data, &mut cc, &mut gc);

        if let Some(ref divert) = choice.divert {
            let d = lower_hir_divert(divert, &mut ctx);
            body.push(lir::Stmt::Divert(d));
        }

        return Some(body);
    }
    None
}

fn find_gather_body(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    plan_data: &plan::ContainerPlan,
) -> Option<Vec<lir::Stmt>> {
    for (key, &target_id) in &plan_data.gather_targets {
        if target_id != shell.id {
            continue;
        }
        let (gather, parent_block, choice_set_pos) = find_hir_gather_with_context(files, key)?;
        let temp_map = build_scope_temp_map(files, shell);

        let mut ctx = make_ctx(key.file, resolutions, index, &temp_map, key.scope.clone());
        let mut body = Vec::new();

        // Emit gather's inline content
        if let Some(ref c) = gather.content {
            body.push(lir::Stmt::EmitContent(content::lower_content(c, &mut ctx)));
        }
        if let Some(ref d) = gather.divert {
            body.push(lir::Stmt::Divert(lower_hir_divert(d, &mut ctx)));
        }

        // Lower trailing statements from the parent block after the ChoiceSet.
        // These are the continuation — they belong in the gather container.
        let mut cc = 0;
        let mut gc = 0;
        let trailing = stmts::lower_block_from(
            parent_block,
            choice_set_pos + 1,
            &mut ctx,
            plan_data,
            &mut cc,
            &mut gc,
        );
        body.extend(trailing);

        return Some(body);
    }
    None
}

fn find_hir_choice<'a>(
    files: &'a [(FileId, &hir::HirFile)],
    key: &plan::ChoiceKey,
) -> Option<&'a hir::Choice> {
    let (_, hir_file) = files.iter().find(|&&(id, _)| id == key.file)?;
    let scope_parts: Vec<&str> = if key.scope.is_empty() {
        Vec::new()
    } else {
        key.scope.split('.').collect()
    };

    let block = find_scope_block(hir_file, &scope_parts)?;
    let mut counter = 0usize;
    find_choice_in_block(block, key.index, &mut counter)
}

fn find_choice_in_block<'a>(
    block: &'a hir::Block,
    target_index: usize,
    counter: &mut usize,
) -> Option<&'a hir::Choice> {
    for stmt in &block.stmts {
        if let hir::Stmt::ChoiceSet(cs) = stmt {
            for choice in &cs.choices {
                if *counter == target_index {
                    return Some(choice);
                }
                *counter += 1;
            }
        }
    }
    None
}

/// Find a gather in the HIR, returning it along with its parent block and the
/// position of the `ChoiceSet` within that block (needed for trailing stmts).
fn find_hir_gather_with_context<'a>(
    files: &'a [(FileId, &hir::HirFile)],
    key: &plan::GatherKey,
) -> Option<(&'a hir::Gather, &'a hir::Block, usize)> {
    let (_, hir_file) = files.iter().find(|&&(id, _)| id == key.file)?;
    let scope_parts: Vec<&str> = if key.scope.is_empty() {
        Vec::new()
    } else {
        key.scope.split('.').collect()
    };

    let block = find_scope_block(hir_file, &scope_parts)?;
    let mut counter = 0usize;
    for (pos, stmt) in block.stmts.iter().enumerate() {
        if let hir::Stmt::ChoiceSet(cs) = stmt
            && let Some(ref gather) = cs.gather
        {
            if counter == key.index {
                return Some((gather, block, pos));
            }
            counter += 1;
        }
    }
    None
}

fn find_scope_block<'a>(
    hir_file: &'a hir::HirFile,
    scope_parts: &[&str],
) -> Option<&'a hir::Block> {
    if scope_parts.is_empty() {
        return Some(&hir_file.root_content);
    }

    // Resolve base block: knot, or knot.stitch
    let knot = hir_file
        .knots
        .iter()
        .find(|k| k.name.text == scope_parts[0])?;
    let mut rest = &scope_parts[1..];

    let mut block = if let Some(&next) = rest.first() {
        if !next.starts_with('c') && !next.starts_with('g') {
            // It's a stitch name
            let stitch = knot.stitches.iter().find(|s| s.name.text == next)?;
            rest = &rest[1..];
            &stitch.body
        } else {
            &knot.body
        }
    } else {
        return Some(&knot.body);
    };

    // Traverse remaining cN/gN segments into nested choice/gather bodies
    for &segment in rest {
        if let Some(idx_str) = segment.strip_prefix('c') {
            let idx: usize = idx_str.parse().ok()?;
            block = find_nth_choice_body(block, idx)?;
        } else {
            // Unknown segment type — bail
            return None;
        }
    }

    Some(block)
}

/// Find the body of the Nth choice (across all choice sets) in a block.
fn find_nth_choice_body(block: &hir::Block, target_index: usize) -> Option<&hir::Block> {
    let mut counter = 0usize;
    for stmt in &block.stmts {
        if let hir::Stmt::ChoiceSet(cs) = stmt {
            for choice in &cs.choices {
                if counter == target_index {
                    return Some(&choice.body);
                }
                counter += 1;
            }
        }
    }
    None
}

fn build_scope_temp_map(
    files: &[(FileId, &hir::HirFile)],
    shell: &plan::ContainerShell,
) -> TempMap {
    let scope_knot = if shell.path.contains('.') {
        shell.path.split('.').next().unwrap_or("")
    } else {
        &shell.path
    };

    for &(_, hir_file) in files {
        for knot in &hir_file.knots {
            if knot.name.text == scope_knot {
                let mut scope_blocks: Vec<&hir::Block> = vec![&knot.body];
                for stitch in &knot.stitches {
                    scope_blocks.push(&stitch.body);
                }
                return temps::alloc_temps(&knot.params, &scope_blocks);
            }
        }
    }

    TempMap::new()
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

// ─── Implicit structure ─────────────────────────────────────────────

fn apply_implicit_structure(containers: &mut [lir::Container], plan: &plan::ContainerPlan) {
    // 1. First-stitch auto-enter
    let stitch_ids: Vec<(String, brink_format::DefinitionId)> = containers
        .iter()
        .filter(|c| c.kind == lir::ContainerKind::Stitch)
        .map(|c| (c.path.clone(), c.id))
        .collect();

    for container in containers.iter_mut() {
        if container.kind == lir::ContainerKind::Knot && container.body.is_empty() {
            let prefix = format!("{}.", container.path);
            if let Some((_, stitch_id)) = stitch_ids.iter().find(|(p, _)| p.starts_with(&prefix)) {
                container.body.push(lir::Stmt::Divert(lir::Divert {
                    target: lir::DivertTarget::Container(*stitch_id),
                    args: Vec::new(),
                }));
            }
        }
    }

    // 2. Root container implicit DONE
    if let Some(root) = containers.iter_mut().find(|c| c.id == plan.root_id) {
        let ends_with_divert = root
            .body
            .last()
            .is_some_and(|s| matches!(s, lir::Stmt::Divert(_)));
        if !ends_with_divert {
            root.body.push(lir::Stmt::Divert(lir::Divert {
                target: lir::DivertTarget::Done,
                args: Vec::new(),
            }));
        }
    }
}

// ─── Counting flags ─────────────────────────────────────────────────

fn apply_counting_flags(containers: &mut [lir::Container]) {
    let mut visit_ids = Vec::new();
    let mut turns_ids = Vec::new();

    for container in containers.iter() {
        collect_counting_refs(&container.body, &mut visit_ids, &mut turns_ids);
    }

    for container in containers.iter_mut() {
        if visit_ids.contains(&container.id) {
            container.counting_flags |= CountingFlags::VISITS;
        }
        if turns_ids.contains(&container.id) {
            container.counting_flags |= CountingFlags::TURNS;
        }
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
