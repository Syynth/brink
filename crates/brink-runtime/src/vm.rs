//! Opcode decode-dispatch loop.

use brink_format::{ChoiceFlags, CountingFlags, DefinitionId, LineContent, Opcode, Value};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::story::{CallFrame, ContainerPosition, PendingChoice, Story, StoryStatus};
use crate::value_ops::{self, BinaryOp};

/// Result of a single step through the VM.
pub(crate) enum VmYield {
    /// Done opcode — yield text (may have choices pending).
    Done,
    /// End opcode — story permanently ended.
    End,
}

/// Execute opcodes until a yield condition is reached.
/// Maximum opcodes per step to prevent infinite loops.
const MAX_OPS_PER_STEP: u32 = 100_000;

#[expect(clippy::too_many_lines)]
pub(crate) fn run(story: &mut Story, program: &Program) -> Result<VmYield, RuntimeError> {
    let mut op_count: u32 = 0;
    loop {
        op_count += 1;
        if op_count > MAX_OPS_PER_STEP {
            return Ok(VmYield::Done);
        }
        // 1. Get current position.
        let Some(frame) = story.call_stack.last_mut() else {
            return Ok(VmYield::Done);
        };

        let Some(pos) = frame.container_stack.last().copied() else {
            // Container stack empty — pop call frame (implicit return, no explicit value).
            pop_call_frame(story, false)?;
            if story.call_stack.is_empty() {
                return Ok(VmYield::Done);
            }
            continue;
        };

        let container = program.container(pos.container_idx);

        // 2. Check if we've reached end of bytecode.
        if pos.offset >= container.bytecode.len() {
            // Pop from container_stack.
            let frame = story
                .call_stack
                .last_mut()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            frame.container_stack.pop();
            if frame.container_stack.is_empty() {
                if !story.pending_choices.is_empty() {
                    // Choices are pending — keep the call frame alive so
                    // choose() can set the next execution position on it.
                    return Ok(VmYield::Done);
                }
                // Pop call frame (implicit return, no explicit value).
                pop_call_frame(story, false)?;
                if story.call_stack.is_empty() {
                    return Ok(VmYield::Done);
                }
            }
            continue;
        }

        // 3. Decode opcode.
        let mut offset = pos.offset;
        let op = Opcode::decode(&container.bytecode, &mut offset)?;

        // Advance the offset in the position.
        {
            let frame = story
                .call_stack
                .last_mut()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            let top = frame
                .container_stack
                .last_mut()
                .ok_or(RuntimeError::ContainerStackUnderflow)?;
            top.offset = offset;
        }

        // 4. Dispatch.
        match op {
            // ── Output ──────────────────────────────────────────────────
            Opcode::EmitLine(idx) => {
                let text = resolve_line(program, &pos, idx);
                story.output.push_text(&text);
            }
            Opcode::EvalLine(idx) => {
                let text = resolve_line(program, &pos, idx);
                story.value_stack.push(Value::String(text));
            }
            Opcode::EmitValue => {
                let val = story.pop_value()?;
                let text = value_ops::stringify(&val);
                story.output.push_text(&text);
            }
            Opcode::EmitNewline => {
                story.output.push_newline();
            }
            Opcode::Glue => {
                story.output.push_glue();
            }
            Opcode::EndChoice => {
                story.skipping_choice = false;
            }
            Opcode::Nop
            | Opcode::SourceLocation(_, _)
            | Opcode::EndChoiceSet
            | Opcode::ChoiceOutput(_)
            | Opcode::ThreadStart
            | Opcode::ThreadDone => {}

            // ── Lifecycle ────────────────────────────────────────────────
            Opcode::Done => {
                if !story.pending_choices.is_empty() {
                    story.status = StoryStatus::WaitingForChoice;
                }
                return Ok(VmYield::Done);
            }
            Opcode::End => {
                story.status = StoryStatus::Ended;
                return Ok(VmYield::End);
            }

            // ── Container flow ──────────────────────────────────────────
            Opcode::EnterContainer(id) => {
                let idx = program
                    .resolve_container(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                // Increment visit count if flags set.
                let container = program.container(idx);
                if container.counting_flags.contains(CountingFlags::VISITS) {
                    *story.visit_counts.entry(id).or_insert(0) += 1;
                }

                let frame = story
                    .call_stack
                    .last_mut()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                frame.container_stack.push(ContainerPosition {
                    container_idx: idx,
                    offset: 0,
                });
            }
            Opcode::ExitContainer => {
                let frame = story
                    .call_stack
                    .last_mut()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                frame.container_stack.pop();
            }

            // ── Control flow ────────────────────────────────────────────
            Opcode::Goto(id) => {
                if !story.skipping_choice {
                    goto_target(story, program, id)?;
                }
            }
            Opcode::GotoIf(id) => {
                let val = story.pop_value()?;
                if value_ops::is_truthy(&val) {
                    goto_target(story, program, id)?;
                }
            }
            Opcode::GotoVariable => {
                let val = story.pop_value()?;
                if let Value::DivertTarget(id) = val {
                    goto_target(story, program, id)?;
                } else {
                    return Err(RuntimeError::TypeError(
                        "goto_variable requires DivertTarget".into(),
                    ));
                }
            }
            Opcode::Jump(rel) | Opcode::SequenceBranch(rel) => {
                apply_jump(story, rel)?;
            }
            Opcode::JumpIfFalse(rel) => {
                let val = story.pop_value()?;
                if !value_ops::is_truthy(&val) {
                    apply_jump(story, rel)?;
                }
            }

            // ── Stack & literals ─────────────────────────────────────────
            Opcode::PushInt(v) => story.value_stack.push(Value::Int(v)),
            Opcode::PushFloat(v) => story.value_stack.push(Value::Float(v)),
            Opcode::PushBool(v) => story.value_stack.push(Value::Bool(v)),
            Opcode::PushString(idx) => {
                let s = program.name(brink_format::NameId(idx)).to_owned();
                story.value_stack.push(Value::String(s));
            }
            Opcode::PushNull | Opcode::PushList(_) => story.value_stack.push(Value::Null),
            Opcode::PushDivertTarget(id) => story.value_stack.push(Value::DivertTarget(id)),
            Opcode::Pop => {
                story.pop_value()?;
            }
            Opcode::Duplicate => {
                let val = story.peek_value()?.clone();
                story.value_stack.push(val);
            }

            // ── Arithmetic ──────────────────────────────────────────────
            Opcode::Add => binary(story, BinaryOp::Add)?,
            Opcode::Subtract => binary(story, BinaryOp::Subtract)?,
            Opcode::Multiply => binary(story, BinaryOp::Multiply)?,
            Opcode::Divide => binary(story, BinaryOp::Divide)?,
            Opcode::Modulo => binary(story, BinaryOp::Modulo)?,
            Opcode::Negate => {
                let val = story.pop_value()?;
                let result = match val {
                    Value::Int(n) => Value::Int(-n),
                    Value::Float(n) => Value::Float(-n),
                    _ => {
                        return Err(RuntimeError::TypeError("cannot negate non-numeric".into()));
                    }
                };
                story.value_stack.push(result);
            }

            // ── Comparison ──────────────────────────────────────────────
            Opcode::Equal => binary(story, BinaryOp::Equal)?,
            Opcode::NotEqual => binary(story, BinaryOp::NotEqual)?,
            Opcode::Greater => binary(story, BinaryOp::Greater)?,
            Opcode::GreaterOrEqual => binary(story, BinaryOp::GreaterOrEqual)?,
            Opcode::Less => binary(story, BinaryOp::Less)?,
            Opcode::LessOrEqual => binary(story, BinaryOp::LessOrEqual)?,

            // ── Logic ───────────────────────────────────────────────────
            Opcode::Not => {
                let val = story.pop_value()?;
                story
                    .value_stack
                    .push(Value::Bool(!value_ops::is_truthy(&val)));
            }
            Opcode::And => binary(story, BinaryOp::And)?,
            Opcode::Or => binary(story, BinaryOp::Or)?,

            // ── Global vars ─────────────────────────────────────────────
            Opcode::GetGlobal(id) => {
                let idx = program
                    .resolve_global(id)
                    .ok_or(RuntimeError::UnresolvedGlobal(id))?;
                let val = story.globals[idx as usize].clone();
                story.value_stack.push(val);
            }
            Opcode::SetGlobal(id) => {
                let idx = program
                    .resolve_global(id)
                    .ok_or(RuntimeError::UnresolvedGlobal(id))?;
                let val = story.pop_value()?;
                story.globals[idx as usize] = val;
            }

            // ── Temp vars ───────────────────────────────────────────────
            Opcode::DeclareTemp(slot) | Opcode::SetTemp(slot) => {
                let val = story.pop_value()?;
                let frame = story
                    .call_stack
                    .last_mut()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                let idx = slot as usize;
                while frame.temps.len() <= idx {
                    frame.temps.push(Value::Null);
                }
                frame.temps[idx] = val;
            }
            Opcode::GetTemp(slot) => {
                let frame = story
                    .call_stack
                    .last()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                let val = frame
                    .temps
                    .get(slot as usize)
                    .cloned()
                    .unwrap_or(Value::Null);
                story.value_stack.push(val);
            }

            // ── Casts ───────────────────────────────────────────────────
            Opcode::CastToInt => {
                let val = story.pop_value()?;
                story.value_stack.push(value_ops::cast_to_int(&val));
            }
            Opcode::CastToFloat => {
                let val = story.pop_value()?;
                story.value_stack.push(value_ops::cast_to_float(&val));
            }

            // ── Math ────────────────────────────────────────────────────
            Opcode::Floor => {
                let val = story.pop_value()?;
                let result = match val {
                    #[expect(clippy::cast_possible_truncation)]
                    Value::Float(f) => Value::Int(f.floor() as i32),
                    Value::Int(_) => val,
                    _ => return Err(RuntimeError::TypeError("floor requires numeric".into())),
                };
                story.value_stack.push(result);
            }
            Opcode::Ceiling => {
                let val = story.pop_value()?;
                let result = match val {
                    #[expect(clippy::cast_possible_truncation)]
                    Value::Float(f) => Value::Int(f.ceil() as i32),
                    Value::Int(_) => val,
                    _ => return Err(RuntimeError::TypeError("ceiling requires numeric".into())),
                };
                story.value_stack.push(result);
            }
            Opcode::Pow => binary(story, BinaryOp::Pow)?,
            Opcode::Min => binary(story, BinaryOp::Min)?,
            Opcode::Max => binary(story, BinaryOp::Max)?,

            // ── Functions ───────────────────────────────────────────────
            Opcode::Call(id) => {
                let idx = program
                    .resolve_container(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                // Capture output during function call — text output becomes
                // the return value when the frame is popped.
                story.output.begin_capture();

                let current_pos = current_position(story)?;
                story.call_stack.push(CallFrame {
                    return_address: Some(current_pos),
                    temps: Vec::new(),
                    container_stack: vec![ContainerPosition {
                        container_idx: idx,
                        offset: 0,
                    }],
                    is_function_call: true,
                });
            }
            Opcode::Return => {
                // The function already pushed its return value via `ev, <value>, /ev`.
                // It stays on the value stack; pop_call_frame just cleans up the checkpoint.
                pop_call_frame(story, true)?;
            }
            Opcode::TunnelCall(id) => {
                let idx = program
                    .resolve_container(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                let current_pos = current_position(story)?;
                story.call_stack.push(CallFrame {
                    return_address: Some(current_pos),
                    temps: Vec::new(),
                    container_stack: vec![ContainerPosition {
                        container_idx: idx,
                        offset: 0,
                    }],
                    is_function_call: false,
                });
            }
            Opcode::TunnelCallVariable => {
                let val = story.pop_value()?;
                let Value::DivertTarget(id) = val else {
                    return Err(RuntimeError::TypeError(
                        "tunnel_call_variable requires DivertTarget".into(),
                    ));
                };
                let idx = program
                    .resolve_container(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                let current_pos = current_position(story)?;
                story.call_stack.push(CallFrame {
                    return_address: Some(current_pos),
                    temps: Vec::new(),
                    container_stack: vec![ContainerPosition {
                        container_idx: idx,
                        offset: 0,
                    }],
                    is_function_call: false,
                });
            }
            Opcode::TunnelReturn => {
                // The eval block before ->-> pushes either void (normal
                // return) or a DivertTarget (tunnel onwards override).
                // Pop it: if it's a DivertTarget, overwrite this frame's
                // return address so we divert there instead of returning.
                let val = story.pop_value()?;
                if let Value::DivertTarget(id) = val {
                    let (idx, offset) = program
                        .resolve_target(id)
                        .ok_or(RuntimeError::UnresolvedDefinition(id))?;
                    let frame = story
                        .call_stack
                        .last_mut()
                        .ok_or(RuntimeError::CallStackUnderflow)?;
                    frame.return_address = Some(ContainerPosition {
                        container_idx: idx,
                        offset,
                    });
                }
                pop_call_frame(story, true)?;
            }

            // ── Choices ─────────────────────────────────────────────────
            Opcode::BeginStringEval => {
                story.output.begin_capture();
            }
            Opcode::EndStringEval => {
                let text = story
                    .output
                    .end_capture()
                    .ok_or(RuntimeError::CaptureUnderflow)?;
                story.value_stack.push(Value::String(text));
            }
            Opcode::BeginChoiceSet => {
                story.pending_choices.clear();
            }
            Opcode::BeginChoice(flags, target_id) => {
                handle_begin_choice(story, program, flags, target_id)?;
            }

            // ── Intrinsics ──────────────────────────────────────────────
            Opcode::VisitCount => {
                let val = story.pop_value()?;
                if let Value::DivertTarget(id) = val {
                    let count = story.visit_counts.get(&id).copied().unwrap_or(0);
                    story.value_stack.push(Value::Int(count.cast_signed()));
                } else {
                    story.value_stack.push(Value::Int(0));
                }
            }
            Opcode::TurnsSince => {
                // Stub: return -1 (never visited) for now.
                let _val = story.pop_value()?;
                story.value_stack.push(Value::Int(-1));
            }
            Opcode::TurnIndex => {
                story
                    .value_stack
                    .push(Value::Int(story.turn_index.cast_signed()));
            }
            #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            Opcode::ChoiceCount => {
                story
                    .value_stack
                    .push(Value::Int(story.pending_choices.len() as i32));
            }
            Opcode::Random => {
                // Stub: push 0.
                let _max = story.pop_value()?;
                let _min = story.pop_value()?;
                story.value_stack.push(Value::Int(0));
            }
            Opcode::SeedRandom => {
                let _seed = story.pop_value()?;
            }

            // ── Sequences ───────────────────────────────────────────────
            Opcode::Sequence(kind, count) => {
                handle_sequence(story, kind, count)?;
            }

            // ── Tags ────────────────────────────────────────────────────
            Opcode::BeginTag => {
                story.in_tag = true;
                story.output.begin_capture();
            }
            Opcode::EndTag => {
                let tag_text = story
                    .output
                    .end_capture()
                    .ok_or(RuntimeError::CaptureUnderflow)?;
                story.current_tags.push(tag_text);
                story.in_tag = false;
            }

            // ── Deferred ────────────────────────────────────────────────
            Opcode::CallExternal(_, _)
            | Opcode::ListContains
            | Opcode::ListNotContains
            | Opcode::ListIntersect
            | Opcode::ListUnion
            | Opcode::ListExcept
            | Opcode::ListAll
            | Opcode::ListInvert
            | Opcode::ListCount
            | Opcode::ListMin
            | Opcode::ListMax
            | Opcode::ListValue
            | Opcode::ListRange
            | Opcode::ListFromInt
            | Opcode::ListRandom => {
                return Err(RuntimeError::Unimplemented(format!("{op:?}")));
            }
        }
    }
}

fn resolve_line(program: &Program, pos: &ContainerPosition, idx: u16) -> String {
    let lines = program.line_table(pos.container_idx);
    if let Some(entry) = lines.get(idx as usize) {
        match &entry.content {
            LineContent::Plain(s) => s.clone(),
            LineContent::Template(_parts) => {
                // Template lines need slot resolution - stub for now
                "[template]".to_owned()
            }
        }
    } else {
        String::new()
    }
}

/// Pop a call frame and handle function-call output capture.
///
/// For function calls (`is_function_call`):
/// - `is_explicit_return = true` (from `~ret`): the function already pushed
///   its return value via `ev, <value>, /ev`. We just discard the capture
///   checkpoint, leaving any text in the output and the return value on the
///   value stack.
/// - `is_explicit_return = false` (implicit return via bytecode exhaustion):
///   the function didn't push a return value. Capture text output and push
///   it as a `Value::String`.
fn pop_call_frame(story: &mut Story, is_explicit_return: bool) -> Result<(), RuntimeError> {
    let popped = story
        .call_stack
        .pop()
        .ok_or(RuntimeError::CallStackUnderflow)?;

    if popped.is_function_call {
        if is_explicit_return {
            // Explicit `~ret`: return value is already on the value stack.
            // Discard the capture checkpoint; text stays in the output.
            story.output.discard_capture();
        } else {
            // Implicit return: capture text output as the return value.
            // Trim trailing newlines — function bodies end with `\n` but
            // inline callers (`{f()}`) expect clean text without trailing breaks.
            let text = story
                .output
                .end_capture()
                .ok_or(RuntimeError::CaptureUnderflow)?;
            let text = text.trim_end_matches('\n').to_owned();
            story.value_stack.push(Value::String(text));
        }
    }

    if let Some(ret) = popped.return_address {
        resume_at(story, ret);
    }

    Ok(())
}

fn binary(story: &mut Story, op: BinaryOp) -> Result<(), RuntimeError> {
    let right = story.pop_value()?;
    let left = story.pop_value()?;
    let result = value_ops::binary_op(op, &left, &right)?;
    story.value_stack.push(result);
    Ok(())
}

/// Resume execution at a return address.
fn resume_at(story: &mut Story, pos: ContainerPosition) {
    if let Some(frame) = story.call_stack.last_mut()
        && let Some(top) = frame.container_stack.last_mut()
    {
        *top = pos;
    }
}

fn goto_target(story: &mut Story, program: &Program, id: DefinitionId) -> Result<(), RuntimeError> {
    let (container_idx, byte_offset) = program
        .resolve_target(id)
        .ok_or(RuntimeError::UnresolvedDefinition(id))?;

    let frame = story
        .call_stack
        .last_mut()
        .ok_or(RuntimeError::CallStackUnderflow)?;

    // Goto semantics: transfer control within the current call frame.
    //
    // If the target container is already on the container stack, truncate
    // above it (unwind) and set the offset — this handles break diverts
    // like `.^.^.^.15`.
    //
    // If the target is NOT on the stack, clear the stack and push it —
    // this handles cross-knot gotos like `-> another_knot`.
    if let Some(pos) = frame
        .container_stack
        .iter()
        .rposition(|p| p.container_idx == container_idx)
    {
        frame.container_stack.truncate(pos + 1);
        frame.container_stack[pos].offset = byte_offset;
    } else {
        frame.container_stack.clear();
        frame.container_stack.push(ContainerPosition {
            container_idx,
            offset: byte_offset,
        });
    }

    // Increment visit count if tracking.
    let container = program.container(container_idx);
    if container.counting_flags.contains(CountingFlags::VISITS) {
        *story.visit_counts.entry(id).or_insert(0) += 1;
    }

    Ok(())
}

fn apply_jump(story: &mut Story, relative: i32) -> Result<(), RuntimeError> {
    let frame = story
        .call_stack
        .last_mut()
        .ok_or(RuntimeError::CallStackUnderflow)?;
    let top = frame
        .container_stack
        .last_mut()
        .ok_or(RuntimeError::ContainerStackUnderflow)?;

    // The offset was already advanced past the jump instruction.
    // The relative offset is from the current position.
    #[expect(clippy::cast_sign_loss)]
    if relative >= 0 {
        top.offset = top.offset.wrapping_add(relative as usize);
    } else {
        let abs = relative.unsigned_abs() as usize;
        top.offset = top.offset.wrapping_sub(abs);
    }
    Ok(())
}

fn current_position(story: &Story) -> Result<ContainerPosition, RuntimeError> {
    let frame = story
        .call_stack
        .last()
        .ok_or(RuntimeError::CallStackUnderflow)?;
    let pos = frame
        .container_stack
        .last()
        .copied()
        .ok_or(RuntimeError::ContainerStackUnderflow)?;
    Ok(pos)
}

fn handle_begin_choice(
    story: &mut Story,
    program: &Program,
    flags: ChoiceFlags,
    target_id: DefinitionId,
) -> Result<(), RuntimeError> {
    // Pop values in reverse order of how they were pushed onto the stack.
    // The ink eval block pushes: [start_text], [choice_only_text], [condition]
    // So the condition (if present) is on top, then choice text strings.

    // 1. Pop condition first (it was evaluated last, so it's on top).
    if flags.has_condition {
        let condition = story.pop_value()?;
        if !value_ops::is_truthy(&condition) {
            // Skip this choice — pop remaining text values and mark skipping.
            if flags.has_choice_only_content {
                let _ = story.value_stack.pop();
            }
            if flags.has_start_content {
                let _ = story.value_stack.pop();
            }
            story.skipping_choice = true;
            return Ok(());
        }
    }

    // 1b. Once-only check: skip if the target container was already visited.
    if flags.once_only {
        let visit_count = story.visit_counts.get(&target_id).copied().unwrap_or(0);
        if visit_count > 0 {
            if flags.has_choice_only_content {
                let _ = story.value_stack.pop();
            }
            if flags.has_start_content {
                let _ = story.value_stack.pop();
            }
            story.skipping_choice = true;
            return Ok(());
        }
    }

    // 2. Pop choice text strings (choice-only is on top, start below).
    let choice_only_text = if flags.has_choice_only_content {
        match story.value_stack.pop() {
            Some(Value::String(s)) => s,
            Some(other) => value_ops::stringify(&other),
            None => String::new(),
        }
    } else {
        String::new()
    };

    let start_text = if flags.has_start_content {
        match story.value_stack.pop() {
            Some(Value::String(s)) => s,
            Some(other) => value_ops::stringify(&other),
            None => String::new(),
        }
    } else {
        String::new()
    };

    let display_text = format!("{start_text}{choice_only_text}");

    let (target_idx, target_offset) = program
        .resolve_target(target_id)
        .ok_or(RuntimeError::UnresolvedDefinition(target_id))?;

    let idx = story.pending_choices.len();
    story.pending_choices.push(PendingChoice {
        display_text,
        target_id,
        target_idx,
        target_offset,
        flags,
        original_index: idx,
        output_line_idx: None,
    });

    Ok(())
}

fn handle_sequence(
    story: &mut Story,
    kind: brink_format::SequenceKind,
    count: u8,
) -> Result<(), RuntimeError> {
    // The visit count of the current container determines the sequence index.
    // Pop the divert target from the stack to identify the container.
    let val = story.pop_value()?;
    let visit_count = if let Value::DivertTarget(id) = val {
        story.visit_counts.get(&id).copied().unwrap_or(0)
    } else {
        0
    };

    let count = u32::from(count);
    if count == 0 {
        story.value_stack.push(Value::Int(0));
        return Ok(());
    }

    let idx = match kind {
        brink_format::SequenceKind::Cycle | brink_format::SequenceKind::Shuffle => {
            visit_count % count
        }
        brink_format::SequenceKind::Stopping => visit_count.min(count - 1),
        brink_format::SequenceKind::OnceOnly => {
            if visit_count < count {
                visit_count
            } else {
                count // past the end -> skip all branches
            }
        }
    };

    story.value_stack.push(Value::Int(idx.cast_signed()));
    Ok(())
}
