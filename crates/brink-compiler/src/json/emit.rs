//! LIR statement/expression → `brink_json::Element` emission.

use std::collections::HashMap;

use brink_format::DefinitionId;
use brink_ir::lir;
use brink_json::{
    ChoicePoint, ChoicePointFlags, Container, ControlCommand, Divert, Element, InkList, InkValue,
    NativeFunction, ReadCountReference, VariableAssignment, VariableReference,
};

use super::Lookups;

// ─── Per-container emission context ─────────────────────────────────

pub struct ContainerCtx {
    /// Reverse map: temp slot → variable name.
    pub temp_names: HashMap<u16, String>,
    /// Fully qualified container path.
    pub path: String,
}

impl ContainerCtx {
    pub fn build_from_tree(container: &lir::Container, lookups: &Lookups, path: &str) -> Self {
        let mut temp_names = HashMap::new();
        for p in &container.params {
            temp_names.insert(p.slot, lookups.name(p.name).to_string());
        }
        for stmt in &container.body {
            collect_temp_names(stmt, lookups, &mut temp_names);
        }
        ContainerCtx {
            temp_names,
            path: path.to_string(),
        }
    }

    pub fn temp_name(&self, slot: u16) -> &str {
        self.temp_names
            .get(&slot)
            .map_or("_unknown", String::as_str)
    }
}

fn collect_temp_names(stmt: &lir::Stmt, lookups: &Lookups, out: &mut HashMap<u16, String>) {
    if let lir::Stmt::DeclareTemp { slot, name, .. } = stmt {
        out.insert(*slot, lookups.name(*name).to_string());
    }
    match stmt {
        lir::Stmt::Conditional(c) => {
            for branch in &c.branches {
                for s in &branch.body {
                    collect_temp_names(s, lookups, out);
                }
            }
        }
        lir::Stmt::Sequence(s) => {
            for branch in &s.branches {
                for st in branch {
                    collect_temp_names(st, lookups, out);
                }
            }
        }
        _ => {}
    }
}

// ─── Statement emission ─────────────────────────────────────────────

/// Emit a container's body, with access to the container's children for
/// building choice target containers inline.
pub fn emit_body(
    container: &lir::Container,
    lookups: &Lookups,
    cctx: &ContainerCtx,
) -> (Vec<Element>, HashMap<String, Element>) {
    let mut contents = Vec::new();
    let mut named = HashMap::new();

    for stmt in &container.body {
        emit_stmt(
            stmt,
            lookups,
            cctx,
            &mut contents,
            &mut named,
            &container.children,
        );
    }

    (contents, named)
}

/// Emit a list of statements (for branch bodies in conditionals/sequences
/// that don't have their own children).
fn emit_stmts(
    stmts: &[lir::Stmt],
    lookups: &Lookups,
    cctx: &ContainerCtx,
) -> (Vec<Element>, HashMap<String, Element>) {
    let mut contents = Vec::new();
    let mut named = HashMap::new();

    for stmt in stmts {
        emit_stmt(stmt, lookups, cctx, &mut contents, &mut named, &[]);
    }

    (contents, named)
}

fn emit_stmt(
    stmt: &lir::Stmt,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
    named: &mut HashMap<String, Element>,
    siblings: &[lir::Container],
) {
    match stmt {
        lir::Stmt::EmitContent(content) => emit_content(content, lookups, cctx, out),

        lir::Stmt::Divert(divert) => emit_divert(divert, lookups, cctx, out),

        lir::Stmt::TunnelCall(tunnel) => {
            for target in &tunnel.targets {
                if !target.args.is_empty() {
                    out.push(ev());
                    for arg in &target.args {
                        emit_call_arg(arg, lookups, cctx, out);
                    }
                    out.push(end_ev());
                }
                let path = divert_target_path(&target.target, lookups);
                let divert = match &target.target {
                    lir::DivertTarget::Variable(_) => Divert::TunnelVariable {
                        conditional: false,
                        path,
                    },
                    _ => Divert::Tunnel {
                        conditional: false,
                        path,
                    },
                };
                out.push(Element::Divert(divert));
            }
        }

        lir::Stmt::ThreadStart(thread) => {
            if !thread.args.is_empty() {
                out.push(ev());
                for arg in &thread.args {
                    emit_call_arg(arg, lookups, cctx, out);
                }
                out.push(end_ev());
            }
            out.push(Element::ControlCommand(ControlCommand::Thread));
            let path = divert_target_path(&thread.target, lookups);
            out.push(Element::Divert(Divert::Target {
                conditional: false,
                path,
            }));
        }

        lir::Stmt::DeclareTemp {
            slot: _,
            name,
            value,
        } => {
            out.push(ev());
            if let Some(e) = value {
                emit_expr(e, lookups, cctx, out);
            } else {
                out.push(Element::Value(InkValue::Integer(0)));
            }
            out.push(end_ev());
            let var_name = lookups.name(*name).to_string();
            out.push(Element::VariableAssignment(
                VariableAssignment::TemporaryAssignment {
                    variable: var_name,
                    reassign: false,
                },
            ));
        }

        lir::Stmt::Assign { target, op, value } => {
            emit_assign(target, *op, value, lookups, cctx, out);
        }

        lir::Stmt::Return(expr) => {
            out.push(ev());
            if let Some(e) = expr {
                emit_expr(e, lookups, cctx, out);
                out.push(end_ev());
                out.push(Element::ControlCommand(ControlCommand::FunctionReturn));
            } else {
                // Bare return (->->) is a tunnel return
                out.push(Element::Void);
                out.push(end_ev());
                out.push(Element::ControlCommand(ControlCommand::TunnelReturn));
            }
        }

        lir::Stmt::ExprStmt(expr) => {
            out.push(ev());
            emit_expr(expr, lookups, cctx, out);
            out.push(Element::ControlCommand(ControlCommand::Pop));
            out.push(end_ev());
        }

        lir::Stmt::ChoiceSet(cs) => emit_choice_set(cs, lookups, cctx, out, named, siblings),

        lir::Stmt::Conditional(cond) => emit_conditional(cond, lookups, cctx, out, named),

        lir::Stmt::Sequence(seq) => emit_sequence(seq, lookups, cctx, out, named),

        lir::Stmt::EndOfLine => {
            out.push(Element::Value(InkValue::String("\n".to_string())));
        }
    }
}

// ─── Content emission ───────────────────────────────────────────────

fn emit_content(
    content: &lir::Content,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
) {
    for part in &content.parts {
        match part {
            lir::ContentPart::Text(s) => {
                out.push(Element::Value(InkValue::String(s.clone())));
            }
            lir::ContentPart::Glue => {
                out.push(Element::ControlCommand(ControlCommand::Glue));
            }
            lir::ContentPart::Interpolation(expr) => {
                out.push(ev());
                emit_expr(expr, lookups, cctx, out);
                out.push(Element::ControlCommand(ControlCommand::Output));
                out.push(end_ev());
            }
            lir::ContentPart::InlineConditional(cond) => {
                emit_conditional(cond, lookups, cctx, out, &mut HashMap::new());
            }
            lir::ContentPart::InlineSequence(seq) => {
                emit_sequence(seq, lookups, cctx, out, &mut HashMap::new());
            }
        }
    }

    for tag in &content.tags {
        out.push(Element::ControlCommand(ControlCommand::Tag));
        out.push(Element::Value(InkValue::String(tag.clone())));
        out.push(Element::ControlCommand(ControlCommand::EndTag));
    }
}

// ─── Divert emission ────────────────────────────────────────────────

fn emit_divert(
    divert: &lir::Divert,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
) {
    match &divert.target {
        lir::DivertTarget::Done => {
            out.push(Element::ControlCommand(ControlCommand::Done));
        }
        lir::DivertTarget::End => {
            out.push(Element::ControlCommand(ControlCommand::End));
        }
        lir::DivertTarget::Container(id) => {
            if divert.args.is_empty() {
                let path = lookups.container_path(*id);
                out.push(Element::Divert(Divert::Target {
                    conditional: false,
                    path,
                }));
            } else {
                out.push(ev());
                for arg in &divert.args {
                    emit_call_arg(arg, lookups, cctx, out);
                }
                out.push(end_ev());
                let path = lookups.container_path(*id);
                out.push(Element::Divert(Divert::Function {
                    conditional: false,
                    path,
                }));
            }
        }
        lir::DivertTarget::Variable(id) => {
            let name = lookups.global_name(*id);
            out.push(Element::Divert(Divert::Variable {
                conditional: false,
                path: name,
            }));
        }
    }
}

// ─── Assignment emission ────────────────────────────────────────────

fn emit_assign(
    target: &lir::AssignTarget,
    op: brink_ir::AssignOp,
    value: &lir::Expr,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
) {
    match op {
        brink_ir::AssignOp::Set => {
            out.push(ev());
            emit_expr(value, lookups, cctx, out);
            out.push(end_ev());
            match target {
                lir::AssignTarget::Global(id) => {
                    let name = lookups.global_name(*id);
                    out.push(Element::VariableAssignment(
                        VariableAssignment::GlobalAssignment { variable: name },
                    ));
                }
                lir::AssignTarget::Temp(slot) => {
                    let name = cctx.temp_name(*slot).to_string();
                    out.push(Element::VariableAssignment(
                        VariableAssignment::TemporaryAssignment {
                            variable: name,
                            reassign: true,
                        },
                    ));
                }
            }
        }
        brink_ir::AssignOp::Add | brink_ir::AssignOp::Sub => {
            let op_fn = if op == brink_ir::AssignOp::Add {
                NativeFunction::Add
            } else {
                NativeFunction::Subtract
            };
            out.push(ev());
            match target {
                lir::AssignTarget::Global(id) => {
                    let name = lookups.global_name(*id);
                    out.push(Element::VariableReference(VariableReference {
                        variable: name.clone(),
                    }));
                    emit_expr(value, lookups, cctx, out);
                    out.push(Element::NativeFunction(op_fn));
                    out.push(end_ev());
                    out.push(Element::VariableAssignment(
                        VariableAssignment::GlobalAssignment { variable: name },
                    ));
                }
                lir::AssignTarget::Temp(slot) => {
                    let name = cctx.temp_name(*slot).to_string();
                    out.push(Element::VariableReference(VariableReference {
                        variable: name.clone(),
                    }));
                    emit_expr(value, lookups, cctx, out);
                    out.push(Element::NativeFunction(op_fn));
                    out.push(end_ev());
                    out.push(Element::VariableAssignment(
                        VariableAssignment::TemporaryAssignment {
                            variable: name,
                            reassign: true,
                        },
                    ));
                }
            }
        }
    }
}

// ─── Conditional emission ───────────────────────────────────────────

fn emit_conditional(
    cond: &lir::Conditional,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
    _named: &mut HashMap<String, Element>,
) {
    // We'll fill in nop_index after emitting all branches.
    // For now, use a placeholder and patch later.
    let mut branch_merge_indices: Vec<usize> = Vec::new();

    for branch in &cond.branches {
        if let Some(ref condition) = branch.condition {
            // Emit condition evaluation in the parent container
            out.push(ev());
            emit_expr(condition, lookups, cctx, out);
            out.push(end_ev());

            // Wrap conditional divert + branch body in anonymous container
            let wrapper_contents = vec![Element::Divert(Divert::Target {
                conditional: true,
                path: ".^.b".to_string(),
            })];

            let inner_cctx = ContainerCtx {
                temp_names: cctx.temp_names.clone(),
                path: String::new(), // Branch body uses relative paths
            };

            let (mut body_elems, sub_named) = emit_stmts(&branch.body, lookups, &inner_cctx);

            // Placeholder merge divert — will be patched after we know nop_index
            let merge_divert_idx = body_elems.len();
            body_elems.push(Element::Divert(Divert::Target {
                conditional: false,
                path: String::new(), // placeholder
            }));

            let mut branch_named = sub_named;
            branch_named.insert(
                "b".to_string(),
                Element::Container(Container {
                    flags: None,
                    name: None,
                    named_content: HashMap::new(),
                    contents: body_elems,
                }),
            );

            // Track where the merge divert is so we can patch it
            let wrapper_idx = out.len();
            out.push(Element::Container(Container {
                flags: None,
                name: None,
                named_content: branch_named,
                contents: wrapper_contents,
            }));
            branch_merge_indices.push(wrapper_idx);
            let _ = merge_divert_idx;
        } else {
            // Else branch — emit body inline (no wrapper)
            let inner_cctx = ContainerCtx {
                temp_names: cctx.temp_names.clone(),
                path: cctx.path.clone(),
            };
            let (body_elems, _sub_named) = emit_stmts(&branch.body, lookups, &inner_cctx);
            out.extend(body_elems);
        }
    }

    // Emit nop merge point — index is the element's position in the container
    let nop_index = out.len();
    out.push(Element::ControlCommand(ControlCommand::NoOperation));

    // Compute merge path
    let merge_path = if cctx.path.is_empty() {
        format!("{nop_index}")
    } else {
        format!("{}.{nop_index}", cctx.path)
    };

    // Patch merge diverts in each branch
    for &wrapper_idx in &branch_merge_indices {
        if let Element::Container(ref mut wrapper) = out[wrapper_idx]
            && let Some(Element::Container(branch_container)) = wrapper.named_content.get_mut("b")
        {
            // The last element before null should be the merge divert
            if let Some(el) = branch_container.contents.iter_mut().rev().find(
                |e| matches!(e, Element::Divert(Divert::Target { path, .. }) if path.is_empty()),
            ) {
                *el = Element::Divert(Divert::Target {
                    conditional: false,
                    path: merge_path.clone(),
                });
            }
        }
    }
}

// ─── Sequence emission ──────────────────────────────────────────────

fn emit_sequence(
    seq: &lir::Sequence,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
    named: &mut HashMap<String, Element>,
) {
    let n = seq.branches.len();

    // Preamble: compute sequence index
    #[expect(clippy::cast_possible_wrap)]
    let n_i64 = n as i64;
    out.push(ev());
    if seq.kind.contains(brink_ir::SequenceType::SHUFFLE) {
        out.push(Element::Value(InkValue::Integer(n_i64)));
        out.push(Element::ControlCommand(ControlCommand::Sequence));
    } else if seq.kind == brink_ir::SequenceType::CYCLE {
        out.push(Element::ControlCommand(ControlCommand::Visit));
        out.push(Element::Value(InkValue::Integer(n_i64)));
        out.push(Element::NativeFunction(NativeFunction::Modulo));
    } else {
        // Stopping, Once, and any other default: clamp at last index
        out.push(Element::ControlCommand(ControlCommand::Visit));
        out.push(Element::Value(InkValue::Integer(n_i64 - 1)));
        out.push(Element::NativeFunction(NativeFunction::Min));
    }
    out.push(end_ev());

    // Branch dispatch: for each branch, compare and conditional divert
    for i in 0..n {
        let sname = format!("s{i}");
        out.push(ev());
        out.push(Element::ControlCommand(ControlCommand::Duplicate));
        #[expect(clippy::cast_possible_wrap)]
        out.push(Element::Value(InkValue::Integer(i as i64)));
        out.push(Element::NativeFunction(NativeFunction::Equal));
        out.push(end_ev());
        out.push(Element::Divert(Divert::Target {
            conditional: true,
            path: format!(".^.{sname}"),
        }));
    }

    // Nop as merge/fallthrough point
    out.push(Element::ControlCommand(ControlCommand::NoOperation));

    // Emit branch containers
    for (i, branch) in seq.branches.iter().enumerate() {
        let sname = format!("s{i}");
        let inner_cctx = ContainerCtx {
            temp_names: cctx.temp_names.clone(),
            path: if cctx.path.is_empty() {
                sname.clone()
            } else {
                format!("{}.{sname}", cctx.path)
            },
        };

        let mut branch_contents = Vec::new();
        // Pop the sequence index off the stack
        branch_contents.push(Element::ControlCommand(ControlCommand::Pop));

        let (body_contents, branch_named) = emit_stmts(branch, lookups, &inner_cctx);
        branch_contents.extend(body_contents);

        // Divert to merge point
        branch_contents.push(Element::Divert(Divert::Target {
            conditional: false,
            path: ".^.^.nop".to_string(),
        }));

        let container = Container {
            flags: None,
            name: None,
            named_content: branch_named,
            contents: branch_contents,
        };
        named.insert(sname, Element::Container(container));
    }
}

// ─── Choice set emission ────────────────────────────────────────────

fn emit_choice_set(
    cs: &lir::ChoiceSet,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
    named: &mut HashMap<String, Element>,
    siblings: &[lir::Container],
) {
    // Record the contents index before each outer container so we can
    // compute the path for the $r2 → s divert in the choice targets.
    let mut choice_outer_indices: Vec<usize> = Vec::new();

    for (i, choice) in cs.choices.iter().enumerate() {
        let c_name = format!("c-{i}");
        choice_outer_indices.push(out.len());
        emit_choice_outer(choice, lookups, cctx, out, &c_name);
    }

    // Build the c-N choice target containers and add to named_content.
    for (i, choice) in cs.choices.iter().enumerate() {
        let c_name = format!("c-{i}");
        let outer_index = choice_outer_indices[i];

        // Find the matching ChoiceTarget child container by DefinitionId.
        let child = siblings
            .iter()
            .find(|c| c.id == choice.target && c.kind == lir::ContainerKind::ChoiceTarget);

        if let Some(child_container) = child {
            let child_path = if cctx.path.is_empty() {
                c_name.clone()
            } else {
                format!("{}.{c_name}", cctx.path)
            };
            let target_container = build_choice_target(
                child_container,
                choice,
                &child_path,
                outer_index,
                cs.gather_target,
                lookups,
                cctx,
            );
            named.insert(c_name, Element::Container(target_container));
        }
    }

    // Build gather container and add to named_content.
    // The LIR always provides a gather (implicit or explicit) for every choice set.
    if let Some(gather_id) = cs.gather_target
        && let Some(gather) = siblings
            .iter()
            .find(|c| c.id == gather_id && c.kind == lir::ContainerKind::Gather)
    {
        let gather_name = gather.name.as_deref().unwrap_or("g-0");
        let gather_path = if cctx.path.is_empty() {
            gather_name.to_string()
        } else {
            format!("{}.{gather_name}", cctx.path)
        };
        let gather_cctx = ContainerCtx::build_from_tree(gather, lookups, &gather_path);
        let (gather_contents, gather_named) = emit_body(gather, lookups, &gather_cctx);
        named.insert(
            gather_name.to_string(),
            Element::Container(Container {
                flags: None,
                name: None,
                named_content: gather_named,
                contents: gather_contents,
            }),
        );
    }
}

/// Emit a choice's outer container (inline in parent contents).
///
/// Contains the $r return variable pattern, start content in "s" sub-container,
/// choice-only content, condition, and `ChoicePoint`.
#[expect(clippy::too_many_lines)]
fn emit_choice_outer(
    choice: &lir::Choice,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
    c_name: &str,
) {
    let has_start = choice
        .start_content
        .as_ref()
        .is_some_and(|c| !c.parts.is_empty());
    let has_choice_only = choice
        .choice_only_content
        .as_ref()
        .is_some_and(|c| !c.parts.is_empty());

    let mut outer_contents = Vec::new();
    let mut outer_named: HashMap<String, Element> = HashMap::new();

    // Compute the path for this outer container based on its index in the
    // parent's contents array.
    let outer_index = out.len();
    let outer_path = if cctx.path.is_empty() {
        format!("{outer_index}")
    } else {
        format!("{}.{outer_index}", cctx.path)
    };

    if has_start || has_choice_only || choice.condition.is_some() {
        outer_contents.push(ev());
    }

    if has_start {
        // $r = $r1 (store return address pointing to $r1 label)
        let r1_path = format!("{outer_path}.$r1");
        outer_contents.push(Element::Value(InkValue::DivertTarget(r1_path)));
        outer_contents.push(Element::VariableAssignment(
            VariableAssignment::TemporaryAssignment {
                variable: "$r".to_string(),
                reassign: false,
            },
        ));

        // BeginString, divert to .^.s, return label $r1, EndString
        outer_contents.push(Element::ControlCommand(ControlCommand::BeginStringEval));
        outer_contents.push(Element::Divert(Divert::Target {
            conditional: false,
            path: ".^.s".to_string(),
        }));
        outer_contents.push(Element::Container(Container {
            flags: None,
            name: Some("$r1".to_string()),
            named_content: HashMap::new(),
            contents: Vec::new(),
        }));
        outer_contents.push(Element::ControlCommand(ControlCommand::EndStringEval));

        // Build the "s" container with start content + -> $r variable divert
        let mut s_contents = Vec::new();
        if let Some(ref start) = choice.start_content {
            emit_content_parts_inline(&start.parts, lookups, cctx, &mut s_contents);
        }
        s_contents.push(Element::Divert(Divert::Variable {
            conditional: false,
            path: "$r".to_string(),
        }));
        outer_named.insert(
            "s".to_string(),
            Element::Container(Container {
                flags: None,
                name: None,
                named_content: HashMap::new(),
                contents: s_contents,
            }),
        );
    }

    // Choice-only content
    if has_choice_only {
        outer_contents.push(Element::ControlCommand(ControlCommand::BeginStringEval));
        if let Some(ref choice_only) = choice.choice_only_content {
            emit_content_parts_inline(&choice_only.parts, lookups, cctx, &mut outer_contents);
        }
        outer_contents.push(Element::ControlCommand(ControlCommand::EndStringEval));
    }

    // Condition
    if let Some(ref cond) = choice.condition {
        emit_expr(cond, lookups, cctx, &mut outer_contents);
    }

    if has_start || has_choice_only || choice.condition.is_some() {
        outer_contents.push(end_ev());
    }

    // Build flags
    let mut flags = ChoicePointFlags::empty();
    if !choice.is_sticky {
        flags |= ChoicePointFlags::ONCE_ONLY;
    }
    if choice.is_fallback {
        flags |= ChoicePointFlags::IS_INVISIBLE_DEFAULT;
    }
    if choice.condition.is_some() {
        flags |= ChoicePointFlags::HAS_CONDITION;
    }
    if has_start {
        flags |= ChoicePointFlags::HAS_START_CONTENT;
    }
    if has_choice_only {
        flags |= ChoicePointFlags::HAS_CHOICE_ONLY_CONTENT;
    }

    let c_path = if cctx.path.is_empty() {
        c_name.to_string()
    } else {
        format!("{}.{c_name}", cctx.path)
    };
    outer_contents.push(Element::ChoicePoint(ChoicePoint {
        target: c_path,
        flags,
    }));

    // Tags
    for tag in &choice.tags {
        outer_contents.push(Element::ControlCommand(ControlCommand::Tag));
        outer_contents.push(Element::Value(InkValue::String(tag.clone())));
        outer_contents.push(Element::ControlCommand(ControlCommand::EndTag));
    }

    // Choices with start content use an outer container (for the $r/$r1/s pattern).
    // Bracket-only choices (no start content) emit their elements inline.
    if has_start {
        out.push(Element::Container(Container {
            flags: None,
            name: None,
            named_content: outer_named,
            contents: outer_contents,
        }));
    } else {
        out.extend(outer_contents);
    }
}

/// Build a choice target container (c-N) with the $r2 preamble for replaying
/// start content, followed by the choice body, divert to gather, and
/// counting flags.
fn build_choice_target(
    child: &lir::Container,
    choice: &lir::Choice,
    child_path: &str,
    outer_index: usize,
    gather_target: Option<DefinitionId>,
    lookups: &Lookups,
    cctx: &ContainerCtx,
) -> Container {
    let child_cctx = ContainerCtx::build_from_tree(child, lookups, child_path);
    let mut contents = Vec::new();

    // $r2 preamble: replay start content from the outer container's "s"
    if choice.start_content.is_some() {
        let r2_path = format!("{child_path}.$r2");
        let s_path = if cctx.path.is_empty() {
            format!("{outer_index}.s")
        } else {
            format!("{}.{outer_index}.s", cctx.path)
        };

        contents.push(ev());
        contents.push(Element::Value(InkValue::DivertTarget(r2_path)));
        contents.push(end_ev());
        contents.push(Element::VariableAssignment(
            VariableAssignment::TemporaryAssignment {
                variable: "$r".to_string(),
                reassign: false,
            },
        ));
        contents.push(Element::Divert(Divert::Target {
            conditional: false,
            path: s_path,
        }));
        contents.push(Element::Container(Container {
            flags: None,
            name: Some("$r2".to_string()),
            named_content: HashMap::new(),
            contents: Vec::new(),
        }));
    }

    // Emit the choice body, excluding the trailing gather divert (emitted separately below).
    let (mut body_contents, body_named) = emit_body(child, lookups, &child_cctx);

    // The LIR body may end with a divert to the gather container. In inklecate's
    // format this divert comes AFTER the \n separator, so we pop it and re-add it
    // after the newline. Any other trailing divert (e.g. Done from `-> DONE`)
    // stays in place — the gather divert will be appended after it.
    let gather_divert_element = if let Some(gather_id) = gather_target {
        let gather_path = lookups.container_path(gather_id);
        // Check if the body ends with a divert to the gather — if so, pop it.
        let last_is_gather = body_contents.last().is_some_and(
            |el| matches!(el, Element::Divert(Divert::Target { path, .. }) if *path == gather_path),
        );
        if last_is_gather {
            body_contents.pop();
        }
        // Always emit the gather divert after \n.
        Some(Element::Divert(Divert::Target {
            conditional: false,
            path: gather_path,
        }))
    } else {
        None
    };

    // Emit the choice's inner_content (text after `]`) before the body.
    if let Some(ref inner) = choice.inner_content {
        emit_content(inner, lookups, cctx, &mut contents);
        contents.push(Element::Value(InkValue::String("\n".to_string())));
        contents.extend(body_contents);
    } else if choice.start_content.is_some() {
        // Has preamble — inline diverts (from `-> target` on the choice line)
        // come before the \n separator, body content comes after.
        let split = body_contents
            .iter()
            .position(|el| !is_inline_divert_element(el))
            .unwrap_or(body_contents.len());
        let after_newline = body_contents.split_off(split);
        contents.extend(body_contents);
        contents.push(Element::Value(InkValue::String("\n".to_string())));
        contents.extend(after_newline);
    } else {
        // Bracket-only (no preamble) — \n comes first, then body
        contents.push(Element::Value(InkValue::String("\n".to_string())));
        contents.extend(body_contents);
    }

    // Always emit the gather divert after the \n separator.
    if let Some(gd) = gather_divert_element {
        contents.push(gd);
    }

    let flags = super::convert_counting_flags(child.counting_flags);

    Container {
        flags: Some(flags),
        name: None,
        named_content: body_named,
        contents,
    }
}

/// Emit content parts inline (without trailing newline), for use in
/// choice start content and choice-only content.
fn emit_content_parts_inline(
    parts: &[lir::ContentPart],
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
) {
    for part in parts {
        match part {
            lir::ContentPart::Text(s) => {
                out.push(Element::Value(InkValue::String(s.clone())));
            }
            lir::ContentPart::Glue => {
                out.push(Element::ControlCommand(ControlCommand::Glue));
            }
            lir::ContentPart::Interpolation(expr) => {
                out.push(ev());
                emit_expr(expr, lookups, cctx, out);
                out.push(Element::ControlCommand(ControlCommand::Output));
                out.push(end_ev());
            }
            lir::ContentPart::InlineConditional(cond) => {
                emit_conditional(cond, lookups, cctx, out, &mut HashMap::new());
            }
            lir::ContentPart::InlineSequence(seq) => {
                emit_sequence(seq, lookups, cctx, out, &mut HashMap::new());
            }
        }
    }
}

// ─── Expression emission ────────────────────────────────────────────

#[expect(clippy::cast_lossless, clippy::too_many_lines)]
pub fn emit_expr(expr: &lir::Expr, lookups: &Lookups, cctx: &ContainerCtx, out: &mut Vec<Element>) {
    match expr {
        lir::Expr::Int(n) => out.push(Element::Value(InkValue::Integer(*n as i64))),
        lir::Expr::Float(f) => out.push(Element::Value(InkValue::Float(*f as f64))),
        lir::Expr::Bool(b) => out.push(Element::Value(InkValue::Bool(*b))),
        lir::Expr::Null => out.push(Element::Void),

        lir::Expr::String(s) => {
            out.push(Element::ControlCommand(ControlCommand::BeginStringEval));
            for part in &s.parts {
                match part {
                    lir::StringPart::Literal(t) => {
                        out.push(Element::Value(InkValue::String(t.clone())));
                    }
                    lir::StringPart::Interpolation(e) => {
                        emit_expr(e, lookups, cctx, out);
                        out.push(Element::ControlCommand(ControlCommand::Output));
                    }
                }
            }
            out.push(Element::ControlCommand(ControlCommand::EndStringEval));
        }

        lir::Expr::GetGlobal(id) => {
            let name = lookups.global_name(*id);
            out.push(Element::VariableReference(VariableReference {
                variable: name,
            }));
        }

        lir::Expr::GetTemp(slot) => {
            let name = cctx.temp_name(*slot).to_string();
            out.push(Element::VariableReference(VariableReference {
                variable: name,
            }));
        }

        lir::Expr::VisitCount(id) => {
            let path = lookups.container_path(*id);
            out.push(Element::ReadCount(ReadCountReference { variable: path }));
        }

        lir::Expr::DivertTarget(id) => {
            let path = lookups.container_path(*id);
            out.push(Element::Value(InkValue::DivertTarget(path)));
        }

        lir::Expr::ListLiteral { items, origins } => {
            let mut map = std::collections::HashMap::new();
            for &item_id in items {
                if let Some((qualified_name, ordinal)) = lookups.list_item_info(item_id) {
                    map.insert(qualified_name, ordinal as i64);
                }
            }
            let origin_names: Vec<String> = origins
                .iter()
                .filter_map(|&id| lookups.list_name(id))
                .collect();
            out.push(Element::Value(InkValue::List(InkList {
                items: map,
                origins: origin_names,
            })));
        }

        lir::Expr::Prefix(op, inner) => {
            emit_expr(inner, lookups, cctx, out);
            match op {
                brink_ir::PrefixOp::Negate => {
                    out.push(Element::NativeFunction(NativeFunction::Negate));
                }
                brink_ir::PrefixOp::Not => out.push(Element::NativeFunction(NativeFunction::Not)),
            }
        }

        lir::Expr::Infix(lhs, op, rhs) => {
            emit_expr(lhs, lookups, cctx, out);
            emit_expr(rhs, lookups, cctx, out);
            out.push(Element::NativeFunction(infix_to_native(*op)));
        }

        lir::Expr::Postfix(inner, op) => {
            // Postfix increment/decrement is compiled as a compound assignment pattern.
            // In expression context this is tricky — the reference compiler handles it
            // at the statement level. For now, emit the inner expression.
            emit_expr(inner, lookups, cctx, out);
            match op {
                brink_ir::PostfixOp::Increment => {
                    out.push(Element::Value(InkValue::Integer(1)));
                    out.push(Element::NativeFunction(NativeFunction::Add));
                }
                brink_ir::PostfixOp::Decrement => {
                    out.push(Element::Value(InkValue::Integer(1)));
                    out.push(Element::NativeFunction(NativeFunction::Subtract));
                }
            }
        }

        lir::Expr::Call { target, args } => {
            for arg in args {
                emit_call_arg(arg, lookups, cctx, out);
            }
            let path = lookups.container_path(*target);
            out.push(Element::Divert(Divert::Function {
                conditional: false,
                path,
            }));
        }

        lir::Expr::CallExternal {
            target,
            args,
            arg_count,
        } => {
            for arg in args {
                emit_call_arg(arg, lookups, cctx, out);
            }
            let name = lookups.global_name(*target);
            out.push(Element::Divert(Divert::ExternalFunction {
                conditional: false,
                name,
                arg_count: u32::from(*arg_count),
            }));
        }

        lir::Expr::CallBuiltin { builtin, args } => {
            emit_builtin(*builtin, args, lookups, cctx, out);
        }
    }
}

fn emit_call_arg(
    arg: &lir::CallArg,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
) {
    match arg {
        lir::CallArg::Value(e) => emit_expr(e, lookups, cctx, out),
        lir::CallArg::RefGlobal(id) => {
            let name = lookups.global_name(*id);
            out.push(Element::Value(InkValue::VariablePointer(name)));
        }
        lir::CallArg::RefTemp(slot) => {
            let name = cctx.temp_name(*slot).to_string();
            out.push(Element::Value(InkValue::VariablePointer(name)));
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Returns true if the element is a divert-like instruction that should appear
/// before the `\n` separator in a choice target (inline diverts from the choice
/// line itself, e.g. `* choice -> DONE`).
fn is_inline_divert_element(el: &Element) -> bool {
    matches!(
        el,
        Element::Divert(_) | Element::ControlCommand(ControlCommand::Done | ControlCommand::End)
    )
}

fn ev() -> Element {
    Element::ControlCommand(ControlCommand::BeginLogicalEval)
}

fn end_ev() -> Element {
    Element::ControlCommand(ControlCommand::EndLogicalEval)
}

fn divert_target_path(target: &lir::DivertTarget, lookups: &Lookups) -> String {
    match target {
        lir::DivertTarget::Container(id) => lookups.container_path(*id),
        lir::DivertTarget::Variable(id) => lookups.global_name(*id),
        lir::DivertTarget::Done => "done".to_string(),
        lir::DivertTarget::End => "end".to_string(),
    }
}

fn infix_to_native(op: brink_ir::InfixOp) -> NativeFunction {
    match op {
        brink_ir::InfixOp::Add => NativeFunction::Add,
        brink_ir::InfixOp::Sub => NativeFunction::Subtract,
        brink_ir::InfixOp::Mul => NativeFunction::Multiply,
        brink_ir::InfixOp::Div => NativeFunction::Divide,
        brink_ir::InfixOp::Mod => NativeFunction::Modulo,
        brink_ir::InfixOp::Pow => NativeFunction::Pow,
        brink_ir::InfixOp::Eq => NativeFunction::Equal,
        brink_ir::InfixOp::NotEq => NativeFunction::NotEqual,
        brink_ir::InfixOp::Lt => NativeFunction::LessThan,
        brink_ir::InfixOp::Gt => NativeFunction::GreaterThan,
        brink_ir::InfixOp::LtEq => NativeFunction::LessThanEqual,
        brink_ir::InfixOp::GtEq => NativeFunction::GreaterThanEqual,
        brink_ir::InfixOp::And => NativeFunction::And,
        brink_ir::InfixOp::Or => NativeFunction::Or,
        brink_ir::InfixOp::Has => NativeFunction::Has,
        brink_ir::InfixOp::HasNot => NativeFunction::HasNot,
    }
}

fn emit_builtin(
    builtin: lir::BuiltinFn,
    args: &[lir::Expr],
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
) {
    match builtin {
        // TURNS_SINCE(-> target) → emit target divert value, then "turns" control command
        lir::BuiltinFn::TurnsSince => {
            for arg in args {
                emit_expr(arg, lookups, cctx, out);
            }
            out.push(Element::ControlCommand(ControlCommand::Turns));
        }
        // CHOICE_COUNT() → "choiceCnt" control command (no args)
        lir::BuiltinFn::ChoiceCount => {
            out.push(Element::ControlCommand(ControlCommand::ChoiceCount));
        }
        _ => {
            for arg in args {
                emit_expr(arg, lookups, cctx, out);
            }
            out.push(Element::NativeFunction(builtin_to_native(builtin)));
        }
    }
}

fn builtin_to_native(b: lir::BuiltinFn) -> NativeFunction {
    match b {
        // These are handled specially by emit_builtin, but provide a fallback
        lir::BuiltinFn::TurnsSince | lir::BuiltinFn::ChoiceCount | lir::BuiltinFn::Random => {
            NativeFunction::Random
        }
        lir::BuiltinFn::SeedRandom => NativeFunction::SeedRandom,
        lir::BuiltinFn::CastToInt => NativeFunction::IntCast,
        lir::BuiltinFn::CastToFloat => NativeFunction::FloatCast,
        lir::BuiltinFn::Floor => NativeFunction::Floor,
        lir::BuiltinFn::Ceiling => NativeFunction::Ceiling,
        lir::BuiltinFn::Pow => NativeFunction::Pow,
        lir::BuiltinFn::Min => NativeFunction::Min,
        lir::BuiltinFn::Max => NativeFunction::Max,
        lir::BuiltinFn::ListCount => NativeFunction::ListCount,
        lir::BuiltinFn::ListMin => NativeFunction::ListMin,
        lir::BuiltinFn::ListMax => NativeFunction::ListMax,
        lir::BuiltinFn::ListAll => NativeFunction::ListAll,
        lir::BuiltinFn::ListInvert => NativeFunction::ListInvert,
        lir::BuiltinFn::ListRange => NativeFunction::ListRange,
        lir::BuiltinFn::ListRandom => NativeFunction::ListRandom,
        lir::BuiltinFn::ListValue => NativeFunction::ListValue,
        lir::BuiltinFn::ListFromInt => NativeFunction::ListInt,
    }
}
