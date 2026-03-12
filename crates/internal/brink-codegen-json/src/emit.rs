//! LIR statement/expression → `brink_json::Element` emission.

use std::collections::HashMap;

use brink_format::DefinitionId;
use brink_ir::lir;
use brink_json::{
    ChoicePoint, ChoicePointFlags, Container, ControlCommand, Divert, Element, InkList, InkValue,
    NativeFunction, ReadCountReference, VariableAssignment, VariableReference,
};

use crate::Lookups;

// ─── Per-container emission context ─────────────────────────────────

pub struct ContainerCtx {
    /// Fully qualified container path.
    pub path: String,
    /// Number of param `temp=` elements prepended before the body.
    /// Internal index references must be offset by this amount.
    pub param_offset: usize,
    /// Extra nesting levels not reflected in `path` (e.g., inside a
    /// conditional branch body `{b: [...]}`). Added to `source_depth`
    /// in `compact_path` calls.
    pub depth_offset: usize,
}

impl ContainerCtx {
    pub fn build_from_tree(container: &lir::Container, _lookups: &Lookups, path: &str) -> Self {
        ContainerCtx {
            path: path.to_string(),
            param_offset: container.params.len(),
            depth_offset: 0,
        }
    }

    pub fn compact_path(&self, base_depth: usize, target: &str) -> String {
        compact_path(&self.path, base_depth + self.depth_offset, target)
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

#[expect(clippy::too_many_lines)]
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

        // Choice output content (start+inner) is not emitted in JSON format.
        // Inklecate structures this as child container references in the
        // choice target path, not as inline content. The divert and newline
        // are now separate body stmts and will be emitted by those arms.
        lir::Stmt::ChoiceOutput(_) => {}

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
                let abs = divert_target_path(&target.target, lookups);
                let path = cctx.compact_path(1, &abs);
                let divert = match &target.target {
                    lir::DivertTarget::Variable(_) | lir::DivertTarget::VariableTemp(..) => {
                        Divert::TunnelVariable {
                            conditional: false,
                            path,
                        }
                    }
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
            let abs = divert_target_path(&thread.target, lookups);
            let path = cctx.compact_path(1, &abs);
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

        lir::Stmt::Return {
            value,
            is_tunnel,
            args,
        } => {
            out.push(ev());
            for arg in args {
                emit_call_arg(arg, lookups, cctx, out);
            }
            if let Some(e) = value {
                emit_expr(e, lookups, cctx, out);
                out.push(end_ev());
                if *is_tunnel {
                    out.push(Element::ControlCommand(ControlCommand::TunnelReturn));
                } else {
                    out.push(Element::ControlCommand(ControlCommand::FunctionReturn));
                }
            } else {
                out.push(Element::Void);
                out.push(end_ev());
                if *is_tunnel {
                    out.push(Element::ControlCommand(ControlCommand::TunnelReturn));
                } else {
                    out.push(Element::ControlCommand(ControlCommand::FunctionReturn));
                }
            }
        }

        lir::Stmt::ExprStmt(expr) => {
            out.push(ev());
            emit_expr(expr, lookups, cctx, out);
            out.push(Element::ControlCommand(ControlCommand::Pop));
            out.push(end_ev());
        }

        lir::Stmt::ChoiceSet(cs) => emit_choice_set(cs, lookups, cctx, out, named, siblings),

        lir::Stmt::Conditional(cond) => emit_conditional(cond, lookups, cctx, out, named, false),

        lir::Stmt::Sequence(seq) => emit_sequence(seq, lookups, cctx, out, named),

        lir::Stmt::EnterContainer(id) => {
            // For JSON codegen, inline the child container's body (used for sequence wrappers).
            if let Some(child) = siblings.iter().find(|c| c.id == *id) {
                for child_stmt in &child.body {
                    emit_stmt(child_stmt, lookups, cctx, out, named, &child.children);
                }
            }
        }

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
                emit_conditional(cond, lookups, cctx, out, &mut HashMap::new(), true);
            }
            lir::ContentPart::InlineSequence(seq) => {
                emit_sequence(seq, lookups, cctx, out, &mut HashMap::new());
            }
            lir::ContentPart::EnterSequence(_) => {
                // EnterSequence for inline sequences — the JSON codegen should not
                // encounter this in practice because inline sequences are emitted
                // inline. If we reach here, it's a no-op.
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
        lir::DivertTarget::Address(id) => {
            if divert.args.is_empty() {
                let abs = lookups.container_path(*id);
                let path = cctx.compact_path(1, &abs);
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
                let abs = lookups.container_path(*id);
                let path = cctx.compact_path(1, &abs);
                // Regular divert with args — NOT a function call (no return address).
                out.push(Element::Divert(Divert::Target {
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
        lir::DivertTarget::VariableTemp(_, name_id) => {
            let name = lookups.name(*name_id).to_owned();
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
                        VariableAssignment::GlobalAssignment {
                            variable: name,
                            reassign: true,
                        },
                    ));
                }
                lir::AssignTarget::Temp(_slot, name_id) => {
                    let name = lookups.name(*name_id).to_string();
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
                        VariableAssignment::GlobalAssignment {
                            variable: name,
                            reassign: true,
                        },
                    ));
                }
                lir::AssignTarget::Temp(_slot, name_id) => {
                    let name = lookups.name(*name_id).to_string();
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
    is_inline: bool,
) {
    match &cond.kind {
        lir::CondKind::Switch(switch_expr) => {
            emit_switch_conditional(switch_expr, &cond.branches, lookups, cctx, out, is_inline);
        }
        lir::CondKind::InitialCondition | lir::CondKind::IfElse => {
            emit_if_conditional(cond, lookups, cctx, out, is_inline);
        }
    }
}

/// Emit an if/else-if/else conditional (no switch expression).
fn emit_if_conditional(
    cond: &lir::Conditional,
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
    is_inline: bool,
) {
    let mut branch_merge_indices: Vec<usize> = Vec::new();
    let condition_is_flat = matches!(cond.kind, lir::CondKind::InitialCondition);

    for branch in &cond.branches {
        // Non-inline branches get a "\n" prepended after emission. Account
        // for this shift in param_offset so nested merge paths are correct.
        let newline_offset = usize::from(!is_inline);
        let inner_cctx = ContainerCtx {
            path: cctx.path.clone(),
            param_offset: newline_offset,
            depth_offset: cctx.depth_offset + 2,
        };

        let (mut body_elems, sub_named) = emit_stmts(&branch.body, lookups, &inner_cctx);
        if !is_inline {
            body_elems.insert(0, Element::Value(InkValue::String("\n".to_string())));
        }

        // Placeholder merge divert — patched after we know nop_index
        body_elems.push(Element::Divert(Divert::Target {
            conditional: false,
            path: String::new(),
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

        let mut wrapper_contents = Vec::new();
        if let Some(ref condition) = branch.condition {
            if condition_is_flat {
                out.push(ev());
                emit_expr(condition, lookups, cctx, out);
                out.push(end_ev());
            } else {
                wrapper_contents.push(ev());
                emit_expr(condition, lookups, cctx, &mut wrapper_contents);
                wrapper_contents.push(end_ev());
            }
            wrapper_contents.push(Element::Divert(Divert::Target {
                conditional: true,
                path: ".^.b".to_string(),
            }));
        } else {
            wrapper_contents.push(Element::Divert(Divert::Target {
                conditional: false,
                path: ".^.b".to_string(),
            }));
        }

        let wrapper_idx = out.len();
        out.push(Element::Container(Container {
            flags: None,
            name: None,
            named_content: branch_named,
            contents: wrapper_contents,
        }));
        branch_merge_indices.push(wrapper_idx);
    }

    let nop_index = out.len() + cctx.param_offset;
    out.push(Element::ControlCommand(ControlCommand::NoOperation));

    if !is_inline {
        out.push(Element::Value(InkValue::String("\n".to_string())));
    }

    let merge_abs = if cctx.path.is_empty() {
        format!("{nop_index}")
    } else {
        format!("{}.{nop_index}", cctx.path)
    };
    // The merge target (nop) is in the same output vector as the branch
    // wrappers. The branch body is always 3 levels deep (wrapper → named "b"
    // → body), so use a fixed depth of 3 without depth_offset.
    let merge_path = compact_path(&cctx.path, 3, &merge_abs);

    for &wrapper_idx in &branch_merge_indices {
        if let Element::Container(ref mut wrapper) = out[wrapper_idx]
            && let Some(Element::Container(branch_container)) = wrapper.named_content.get_mut("b")
            && let Some(el) = branch_container.contents.iter_mut().rev().find(
                |e| matches!(e, Element::Divert(Divert::Target { path, .. }) if path.is_empty()),
            )
        {
            *el = Element::Divert(Divert::Target {
                conditional: false,
                path: merge_path.clone(),
            });
        }
    }
}

/// Emit a switch conditional using the "du" (duplicate) pattern.
///
/// Structure: `ev, <switch_expr>, /ev, [du, ev, <case>, ==, /ev, cond_divert, {b: [pop, \n, ...body, merge_divert]}], ...`
fn emit_switch_conditional(
    switch_expr: &lir::Expr,
    branches: &[lir::CondBranch],
    lookups: &Lookups,
    cctx: &ContainerCtx,
    out: &mut Vec<Element>,
    is_inline: bool,
) {
    // Evaluate the switch expression once
    out.push(ev());
    emit_expr(switch_expr, lookups, cctx, out);
    out.push(end_ev());

    let mut branch_merge_indices: Vec<usize> = Vec::new();

    for branch in branches {
        // Switch branches get "pop" (always) and "\n" (non-inline) prepended
        // after emission. Account for this shift in param_offset so nested
        // merge paths are correct.
        let prepend_count = 1 + usize::from(!is_inline);
        let inner_cctx = ContainerCtx {
            path: cctx.path.clone(),
            param_offset: prepend_count,
            depth_offset: cctx.depth_offset + 2,
        };

        let (mut body_elems, sub_named) = emit_stmts(&branch.body, lookups, &inner_cctx);

        // Switch branch bodies start with "pop" (remove duplicated value)
        // followed by "\n" for multiline
        body_elems.insert(0, Element::ControlCommand(ControlCommand::Pop));
        if !is_inline {
            body_elems.insert(1, Element::Value(InkValue::String("\n".to_string())));
        }

        // Placeholder merge divert
        body_elems.push(Element::Divert(Divert::Target {
            conditional: false,
            path: String::new(),
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

        let mut wrapper_contents = Vec::new();
        if let Some(ref case_value) = branch.condition {
            // "du" — duplicate the switch value on the stack
            wrapper_contents.push(Element::ControlCommand(ControlCommand::Duplicate));
            // ev, <case_value>, ==, /ev
            wrapper_contents.push(ev());
            emit_expr(case_value, lookups, cctx, &mut wrapper_contents);
            wrapper_contents.push(Element::NativeFunction(NativeFunction::Equal));
            wrapper_contents.push(end_ev());
            wrapper_contents.push(Element::Divert(Divert::Target {
                conditional: true,
                path: ".^.b".to_string(),
            }));
        } else {
            // else branch — unconditional divert
            wrapper_contents.push(Element::Divert(Divert::Target {
                conditional: false,
                path: ".^.b".to_string(),
            }));
        }

        let wrapper_idx = out.len();
        out.push(Element::Container(Container {
            flags: None,
            name: None,
            named_content: branch_named,
            contents: wrapper_contents,
        }));
        branch_merge_indices.push(wrapper_idx);
    }

    // Pop the switch value remaining on the stack after all branches
    out.push(Element::ControlCommand(ControlCommand::Pop));

    let nop_index = out.len() + cctx.param_offset;
    out.push(Element::ControlCommand(ControlCommand::NoOperation));

    if !is_inline {
        out.push(Element::Value(InkValue::String("\n".to_string())));
    }

    let merge_abs = if cctx.path.is_empty() {
        format!("{nop_index}")
    } else {
        format!("{}.{nop_index}", cctx.path)
    };
    // Same as if-conditional: merge target is in the same output vector,
    // branch body is 3 levels deep, no depth_offset needed.
    let merge_path = compact_path(&cctx.path, 3, &merge_abs);

    for &wrapper_idx in &branch_merge_indices {
        if let Element::Container(ref mut wrapper) = out[wrapper_idx]
            && let Some(Element::Container(branch_container)) = wrapper.named_content.get_mut("b")
            && let Some(el) = branch_container.contents.iter_mut().rev().find(
                |e| matches!(e, Element::Divert(Divert::Target { path, .. }) if path.is_empty()),
            )
        {
            *el = Element::Divert(Divert::Target {
                conditional: false,
                path: merge_path.clone(),
            });
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
            path: if cctx.path.is_empty() {
                sname.clone()
            } else {
                format!("{}.{sname}", cctx.path)
            },
            param_offset: 0,
            depth_offset: 0,
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
        // Use the LIR child container's name (globally indexed) when available,
        // falling back to local index for backwards compatibility.
        let c_name = siblings
            .iter()
            .find(|c| c.id == choice.target && c.kind == lir::ContainerKind::ChoiceTarget)
            .and_then(|c| c.name.clone())
            .unwrap_or_else(|| format!("c-{i}"));
        choice_outer_indices.push(out.len());
        emit_choice_outer(choice, lookups, cctx, out, &c_name);
    }

    // Build the c-N choice target containers and add to named_content.
    let mut any_uses_gather = false;
    for (i, choice) in cs.choices.iter().enumerate() {
        let outer_index = choice_outer_indices[i];

        // Find the matching ChoiceTarget child container by DefinitionId.
        let child = siblings
            .iter()
            .find(|c| c.id == choice.target && c.kind == lir::ContainerKind::ChoiceTarget);

        if let Some(child_container) = child {
            let c_name = child_container
                .name
                .clone()
                .unwrap_or_else(|| format!("c-{i}"));
            let child_path = if cctx.path.is_empty() {
                c_name.clone()
            } else {
                format!("{}.{c_name}", cctx.path)
            };
            let (target_container, uses_gather) = build_choice_target(
                child_container,
                choice,
                &child_path,
                outer_index,
                cs.gather_target,
                lookups,
                cctx,
            );
            any_uses_gather |= uses_gather;
            named.insert(c_name, Element::Container(target_container));
        }
    }

    // Build gather container and add to named_content.
    // Skip if no choice target references the gather (all end terminally).
    if any_uses_gather
        && let Some(gather_id) = cs.gather_target
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
        let (mut gather_contents, gather_named) = emit_body(gather, lookups, &gather_cctx);

        // Inklecate appends a "done" sub-container named g-{N+1} after
        // explicit gather bodies that have text content (not just a bare divert),
        // unless the gather ends with a terminal exit (-> END / -> DONE).
        let has_content = gather
            .body
            .iter()
            .any(|s| matches!(s, lir::Stmt::EmitContent(_) | lir::Stmt::ChoiceOutput(_)));
        let ends_terminal = gather.body.last().is_some_and(|s| {
            matches!(
                s,
                lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Done | lir::DivertTarget::End)
            )
        });
        if has_content && !ends_terminal {
            let next_gather_index = gather_name
                .strip_prefix("g-")
                .and_then(|s| s.parse::<usize>().ok())
                .map_or_else(|| format!("{gather_name}-done"), |i| format!("g-{}", i + 1));
            let done_container = Element::Container(Container {
                flags: None,
                name: Some(next_gather_index),
                named_content: HashMap::new(),
                contents: vec![Element::ControlCommand(ControlCommand::Done)],
            });
            gather_contents.push(done_container);
        }

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

    // Choice point target: c-N is in named_content of the parent container.
    // When the choice has start content, the choice point lives inside an
    // anonymous wrapper container, so we need depth=2 to reach the parent's
    // named_content. Otherwise the elements are inline and depth=1 suffices.
    let c_abs = if cctx.path.is_empty() {
        c_name.to_string()
    } else {
        format!("{}.{c_name}", cctx.path)
    };
    let depth = if has_start { 2 } else { 1 };
    let c_path = cctx.compact_path(depth, &c_abs);
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
#[expect(clippy::too_many_lines)]
fn build_choice_target(
    child: &lir::Container,
    choice: &lir::Choice,
    child_path: &str,
    outer_index: usize,
    gather_target: Option<DefinitionId>,
    lookups: &Lookups,
    cctx: &ContainerCtx,
) -> (Container, bool) {
    // Check if the choice target has nested choice/gather children.
    let has_nested_choices = child.children.iter().any(|c| {
        matches!(
            c.kind,
            lir::ContainerKind::ChoiceTarget | lir::ContainerKind::Gather
        )
    });

    let child_cctx = ContainerCtx::build_from_tree(child, lookups, child_path);
    let mut contents = Vec::new();

    // $r2 preamble: replay start content from the outer container's "s"
    if choice.start_content.is_some() {
        let r2_path = format!("{child_path}.$r2");
        let s_abs = if cctx.path.is_empty() {
            format!("{outer_index}.s")
        } else {
            format!("{}.{outer_index}.s", cctx.path)
        };
        let s_path = compact_path(child_path, 1, &s_abs);

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

    // Emit the choice body.
    let (mut body_contents, body_named) = emit_body(child, lookups, &child_cctx);

    // The LIR body may end with a divert to the gather container. In inklecate's
    // format this divert comes AFTER the \n separator, so we pop it and re-add it
    // after the newline. Skip this for nested choices — their gather diverts
    // are inside the nested choice targets, not at this level.
    //
    // When the LIR body has content (EmitContent/EndOfLine) followed by a
    // terminal divert (Done/End), the body handles its own exit and the
    // gather divert is unnecessary. Inklecate omits it in that case.
    let body_has_content_then_terminal = {
        let has_content = child.body.iter().any(|s| {
            matches!(
                s,
                lir::Stmt::EmitContent(_) | lir::Stmt::ChoiceOutput(_) | lir::Stmt::EndOfLine
            )
        });
        let ends_terminal = child.body.last().is_some_and(|s| {
            matches!(
                s,
                lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Done | lir::DivertTarget::End)
            )
        });
        has_content && ends_terminal
    };
    let gather_divert_element = if has_nested_choices || body_has_content_then_terminal {
        None
    } else if let Some(gather_id) = gather_target {
        let gather_abs = lookups.container_path(gather_id);
        let gather_path = compact_path(child_path, 1, &gather_abs);
        // Check if the body's last element is a divert to the gather. The
        // emitted body already contains compact paths, so compare against
        // both the compact and absolute forms.
        let last_is_gather = body_contents.last().is_some_and(|el| {
            matches!(el, Element::Divert(Divert::Target { path, .. })
                if *path == gather_abs || *path == gather_path)
        });
        if last_is_gather {
            body_contents.pop();
        }
        Some(Element::Divert(Divert::Target {
            conditional: false,
            path: gather_path,
        }))
    } else {
        None
    };

    // Emit the choice's inner_content (text after `]`) before the body.
    // The body_contents already contains the divert (if any) and \n in the
    // correct order from the HIR body stmts, so we just extend directly.
    if let Some(ref inner) = choice.inner_content {
        emit_content(inner, lookups, cctx, &mut contents);
    }
    contents.extend(body_contents);

    // Emit the gather divert after the \n separator (if not suppressed).
    let uses_gather = gather_divert_element.is_some();
    if let Some(gd) = gather_divert_element {
        contents.push(gd);
    }

    let flags = crate::convert_counting_flags(child.counting_flags);
    let flags_opt = if flags.is_empty() { None } else { Some(flags) };

    if has_nested_choices {
        // When the choice target has nested choices, the body text stays
        // flat and the choice set content wraps in an inner anonymous container.
        // Split at the start of the first choice's eval block — find the first
        // ChoicePoint and scan back to its matching BeginLogicalEval.
        let first_cp = contents
            .iter()
            .position(|el| matches!(el, Element::ChoicePoint(_)))
            .unwrap_or(contents.len());
        let split_idx = contents[..first_cp]
            .iter()
            .rposition(|el| {
                matches!(
                    el,
                    Element::ControlCommand(ControlCommand::BeginLogicalEval)
                )
            })
            .unwrap_or(first_cp);

        let mut choice_content = contents.split_off(split_idx);

        // Remove trailing gather diverts from the inner container — nested
        // choice targets have their own gather diverts.
        while choice_content
            .last()
            .is_some_and(|el| matches!(el, Element::Divert(Divert::Target { .. })))
        {
            choice_content.pop();
        }

        // The gather container (g-0) belongs at the outer level, not inside
        // the nested choice inner container. Only choice targets go inside.
        let mut inner_named = body_named;
        inner_named.retain(|k, _| !k.starts_with("g-"));

        let inner = Container {
            flags: None,
            name: None,
            named_content: inner_named,
            contents: choice_content,
        };
        contents.push(Element::Container(inner));

        (
            Container {
                flags: flags_opt,
                name: None,
                named_content: HashMap::new(),
                contents,
            },
            uses_gather,
        )
    } else {
        (
            Container {
                flags: flags_opt,
                name: None,
                named_content: body_named,
                contents,
            },
            uses_gather,
        )
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
                emit_conditional(cond, lookups, cctx, out, &mut HashMap::new(), true);
            }
            lir::ContentPart::InlineSequence(seq) => {
                emit_sequence(seq, lookups, cctx, out, &mut HashMap::new());
            }
            lir::ContentPart::EnterSequence(_) => {}
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

        lir::Expr::GetTemp(_slot, name_id) => {
            let name = lookups.name(*name_id).to_string();
            out.push(Element::VariableReference(VariableReference {
                variable: name,
            }));
        }

        lir::Expr::VisitCount(id) => {
            let abs = lookups.container_path(*id);
            let path = cctx.compact_path(1, &abs);
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
            let abs = lookups.container_path(*target);
            let path = cctx.compact_path(1, &abs);
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

        lir::Expr::CallVariable { target, args } => {
            for arg in args {
                emit_call_arg(arg, lookups, cctx, out);
            }
            let name = lookups.global_name(*target);
            out.push(Element::Divert(Divert::FunctionVariable {
                conditional: false,
                path: name,
            }));
        }

        lir::Expr::CallVariableTemp { name, args, .. } => {
            for arg in args {
                emit_call_arg(arg, lookups, cctx, out);
            }
            let var_name = lookups.name(*name).to_string();
            out.push(Element::Divert(Divert::FunctionVariable {
                conditional: false,
                path: var_name,
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
        lir::CallArg::RefTemp(_slot, name_id) => {
            let name = lookups.name(*name_id).to_string();
            out.push(Element::Value(InkValue::VariablePointer(name)));
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

fn ev() -> Element {
    Element::ControlCommand(ControlCommand::BeginLogicalEval)
}

fn end_ev() -> Element {
    Element::ControlCommand(ControlCommand::EndLogicalEval)
}

fn divert_target_path(target: &lir::DivertTarget, lookups: &Lookups) -> String {
    match target {
        lir::DivertTarget::Address(id) => lookups.container_path(*id),
        lir::DivertTarget::Variable(id) => lookups.global_name(*id),
        lir::DivertTarget::VariableTemp(_, name_id) => lookups.name(*name_id).to_owned(),
        lir::DivertTarget::Done => "done".to_string(),
        lir::DivertTarget::End => "end".to_string(),
    }
}

/// Compute the compact (shortest) path string, like inklecate's `CompactPathString`.
///
/// `source_container` is the dot-separated path of the container holding the
/// source object. `source_depth` is how many levels deeper the source is
/// (1 for a direct content element, 2 for an element inside a named child, etc.).
/// `target` is the absolute path of the target.
///
/// Returns the shorter of the relative (`.^...`) and absolute representations.
fn compact_path(source_container: &str, source_depth: usize, target: &str) -> String {
    let src_comps: Vec<&str> = if source_container.is_empty() {
        vec![]
    } else {
        source_container.split('.').collect()
    };
    let tgt_comps: Vec<&str> = if target.is_empty() {
        return target.to_string();
    } else {
        target.split('.').collect()
    };

    // Find length of shared prefix between source container and target.
    let min_len = src_comps.len().min(tgt_comps.len());
    let mut shared = 0;
    for i in 0..min_len {
        if src_comps[i] == tgt_comps[i] {
            shared = i + 1;
        } else {
            break;
        }
    }

    if shared == 0 {
        return target.to_string();
    }

    // Upward moves: from source object up to the shared ancestor.
    let ups = (src_comps.len() - shared) + source_depth;
    let downs = &tgt_comps[shared..];

    let mut parts = vec!["^"; ups];
    parts.extend_from_slice(downs);

    let relative = format!(".{}", parts.join("."));

    if relative.len() < target.len() {
        relative
    } else {
        target.to_string()
    }
}

fn infix_to_native(op: brink_ir::InfixOp) -> NativeFunction {
    match op {
        brink_ir::InfixOp::Add => NativeFunction::Add,
        brink_ir::InfixOp::Sub => NativeFunction::Subtract,
        brink_ir::InfixOp::Mul => NativeFunction::Multiply,
        brink_ir::InfixOp::Div => NativeFunction::Divide,
        brink_ir::InfixOp::Mod => NativeFunction::Modulo,
        brink_ir::InfixOp::Intersect => NativeFunction::Intersect,
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
        lir::BuiltinFn::ReadCount => NativeFunction::ReadCount,
    }
}
