//! Opcode decode-dispatch loop.

use brink_format::{ChoiceFlags, CountingFlags, DefinitionId, LineContent, Opcode, Value};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::story::{CallFrame, CallFrameType, ContainerPosition, PendingChoice, Story};
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
        let thread = story.flow.current_thread_mut();
        let Some(frame) = thread.call_stack.last_mut() else {
            // Current thread's call stack is empty.
            if story.flow.can_pop_thread() {
                story.flow.pop_thread();
            } else {
                return Ok(VmYield::Done);
            }
            continue;
        };

        let Some(pos) = frame.container_stack.last().copied() else {
            // Container stack empty — the frame has no more containers to execute.
            let frame_type = frame.frame_type;
            if frame_type != CallFrameType::Function && !story.flow.pending_choices.is_empty() {
                // Non-function frame with pending choices: the frame is
                // waiting for a choice selection. The thread fork captured
                // at choice creation preserves the state for resumption.
                if story.flow.can_pop_thread() {
                    story.flow.pop_thread();
                } else {
                    return Ok(VmYield::Done);
                }
            } else {
                pop_call_frame(story, false)?;
                if story.flow.current_thread().call_stack.is_empty() {
                    if story.flow.can_pop_thread() {
                        story.flow.pop_thread();
                    } else {
                        return Ok(VmYield::Done);
                    }
                }
            }
            continue;
        };

        let container = program.container(pos.container_idx);

        // 2. Check if we've reached end of bytecode.
        if pos.offset >= container.bytecode.len() {
            // Pop from container_stack.
            let thread = story.flow.current_thread_mut();
            let frame = thread
                .call_stack
                .last_mut()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            frame.container_stack.pop();
            if frame.container_stack.is_empty() {
                let frame_type = frame.frame_type;
                if frame_type != CallFrameType::Function && !story.flow.pending_choices.is_empty() {
                    // Non-function frame with pending choices: the frame
                    // is waiting for a choice. The thread fork captured at
                    // choice creation preserves the state for resumption.
                    if story.flow.can_pop_thread() {
                        story.flow.pop_thread();
                    } else {
                        return Ok(VmYield::Done);
                    }
                } else {
                    pop_call_frame(story, false)?;
                    if story.flow.current_thread().call_stack.is_empty() {
                        if story.flow.can_pop_thread() {
                            story.flow.pop_thread();
                        } else {
                            return Ok(VmYield::Done);
                        }
                    }
                }
            }
            continue;
        }

        // 3. Decode opcode.
        let mut offset = pos.offset;
        let op = Opcode::decode(&container.bytecode, &mut offset)?;

        // Advance the offset in the position.
        {
            let thread = story.flow.current_thread_mut();
            let frame = thread
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
                story.flow.output.push_text(&text);
            }
            Opcode::EvalLine(idx) => {
                let text = resolve_line(program, &pos, idx);
                story.flow.value_stack.push(Value::String(text));
            }
            Opcode::EmitValue => {
                let val = story.flow.pop_value()?;
                let text = value_ops::stringify(&val);
                story.flow.output.push_text(&text);
            }
            Opcode::EmitNewline => {
                story.flow.output.push_newline();
            }
            Opcode::Glue => {
                story.flow.output.push_glue();
            }
            Opcode::EndChoice => {
                story.flow.skipping_choice = false;
            }
            Opcode::Nop
            | Opcode::SourceLocation(_, _)
            | Opcode::EndChoiceSet
            | Opcode::ChoiceOutput(_)
            | Opcode::ThreadStart
            | Opcode::ThreadDone => {}

            // ── Lifecycle ────────────────────────────────────────────────
            Opcode::Done => {
                if story.flow.can_pop_thread() {
                    story.flow.pop_thread();
                } else {
                    return Ok(VmYield::Done);
                }
            }
            Opcode::End => {
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

                let thread = story.flow.current_thread_mut();
                let frame = thread
                    .call_stack
                    .last_mut()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                frame.container_stack.push(ContainerPosition {
                    container_idx: idx,
                    offset: 0,
                });
            }
            Opcode::ExitContainer => {
                let thread = story.flow.current_thread_mut();
                let frame = thread
                    .call_stack
                    .last_mut()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                frame.container_stack.pop();
            }

            // ── Control flow ────────────────────────────────────────────
            Opcode::Goto(id) => {
                if !story.flow.skipping_choice {
                    goto_target(story, program, id)?;
                }
            }
            Opcode::GotoIf(id) => {
                let val = story.flow.pop_value()?;
                if value_ops::is_truthy(&val) {
                    goto_target(story, program, id)?;
                }
            }
            Opcode::GotoVariable => {
                let val = story.flow.pop_value()?;
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
                let val = story.flow.pop_value()?;
                if !value_ops::is_truthy(&val) {
                    apply_jump(story, rel)?;
                }
            }

            // ── Stack & literals ─────────────────────────────────────────
            Opcode::PushInt(v) => story.flow.value_stack.push(Value::Int(v)),
            Opcode::PushFloat(v) => story.flow.value_stack.push(Value::Float(v)),
            Opcode::PushBool(v) => story.flow.value_stack.push(Value::Bool(v)),
            Opcode::PushString(idx) => {
                let s = program.name(brink_format::NameId(idx)).to_owned();
                story.flow.value_stack.push(Value::String(s));
            }
            Opcode::PushNull | Opcode::PushList(_) => {
                story.flow.value_stack.push(Value::Null);
            }
            Opcode::PushDivertTarget(id) => {
                story.flow.value_stack.push(Value::DivertTarget(id));
            }
            Opcode::Pop => {
                story.flow.pop_value()?;
            }
            Opcode::Duplicate => {
                let val = story.flow.peek_value()?.clone();
                story.flow.value_stack.push(val);
            }

            // ── Arithmetic ──────────────────────────────────────────────
            Opcode::Add => binary(story, BinaryOp::Add)?,
            Opcode::Subtract => binary(story, BinaryOp::Subtract)?,
            Opcode::Multiply => binary(story, BinaryOp::Multiply)?,
            Opcode::Divide => binary(story, BinaryOp::Divide)?,
            Opcode::Modulo => binary(story, BinaryOp::Modulo)?,
            Opcode::Negate => {
                let val = story.flow.pop_value()?;
                let result = match val {
                    Value::Int(n) => Value::Int(-n),
                    Value::Float(n) => Value::Float(-n),
                    _ => {
                        return Err(RuntimeError::TypeError("cannot negate non-numeric".into()));
                    }
                };
                story.flow.value_stack.push(result);
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
                let val = story.flow.pop_value()?;
                story
                    .flow
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
                story.flow.value_stack.push(val);
            }
            Opcode::SetGlobal(id) => {
                let idx = program
                    .resolve_global(id)
                    .ok_or(RuntimeError::UnresolvedGlobal(id))?;
                let val = story.flow.pop_value()?;
                story.globals[idx as usize] = val;
            }

            // ── Temp vars ───────────────────────────────────────────────
            Opcode::DeclareTemp(slot) | Opcode::SetTemp(slot) => {
                let val = story.flow.pop_value()?;
                let thread = story.flow.current_thread_mut();
                let frame = thread
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
                let thread = story.flow.current_thread();
                let frame = thread
                    .call_stack
                    .last()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                let val = frame
                    .temps
                    .get(slot as usize)
                    .cloned()
                    .unwrap_or(Value::Null);
                story.flow.value_stack.push(val);
            }

            // ── Casts ───────────────────────────────────────────────────
            Opcode::CastToInt => {
                let val = story.flow.pop_value()?;
                story.flow.value_stack.push(value_ops::cast_to_int(&val));
            }
            Opcode::CastToFloat => {
                let val = story.flow.pop_value()?;
                story.flow.value_stack.push(value_ops::cast_to_float(&val));
            }

            // ── Math ────────────────────────────────────────────────────
            Opcode::Floor => {
                let val = story.flow.pop_value()?;
                let result = match val {
                    #[expect(clippy::cast_possible_truncation)]
                    Value::Float(f) => Value::Int(f.floor() as i32),
                    Value::Int(_) => val,
                    _ => return Err(RuntimeError::TypeError("floor requires numeric".into())),
                };
                story.flow.value_stack.push(result);
            }
            Opcode::Ceiling => {
                let val = story.flow.pop_value()?;
                let result = match val {
                    #[expect(clippy::cast_possible_truncation)]
                    Value::Float(f) => Value::Int(f.ceil() as i32),
                    Value::Int(_) => val,
                    _ => return Err(RuntimeError::TypeError("ceiling requires numeric".into())),
                };
                story.flow.value_stack.push(result);
            }
            Opcode::Pow => binary(story, BinaryOp::Pow)?,
            Opcode::Min => binary(story, BinaryOp::Min)?,
            Opcode::Max => binary(story, BinaryOp::Max)?,

            // ── Functions ───────────────────────────────────────────────
            Opcode::Call(id) => {
                let idx = program
                    .resolve_container(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                let container = program.container(idx);
                if container.counting_flags.contains(CountingFlags::VISITS) {
                    *story.visit_counts.entry(id).or_insert(0) += 1;
                }

                // Capture output during function call — text output becomes
                // the return value when the frame is popped.
                story.flow.output.begin_capture();

                let current_pos = current_position(story)?;
                let thread = story.flow.current_thread_mut();
                thread.call_stack.push(CallFrame {
                    return_address: Some(current_pos),
                    temps: Vec::new(),
                    container_stack: vec![ContainerPosition {
                        container_idx: idx,
                        offset: 0,
                    }],
                    frame_type: CallFrameType::Function,
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

                let container = program.container(idx);
                if container.counting_flags.contains(CountingFlags::VISITS) {
                    *story.visit_counts.entry(id).or_insert(0) += 1;
                }

                let current_pos = current_position(story)?;
                let thread = story.flow.current_thread_mut();
                thread.call_stack.push(CallFrame {
                    return_address: Some(current_pos),
                    temps: Vec::new(),
                    container_stack: vec![ContainerPosition {
                        container_idx: idx,
                        offset: 0,
                    }],
                    frame_type: CallFrameType::Tunnel,
                });
            }
            Opcode::ThreadCall(id) => {
                let idx = program
                    .resolve_container(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                story.flow.push_thread(CallFrame {
                    return_address: None,
                    temps: Vec::new(),
                    container_stack: vec![ContainerPosition {
                        container_idx: idx,
                        offset: 0,
                    }],
                    frame_type: CallFrameType::Root,
                });
            }
            Opcode::TunnelCallVariable => {
                let val = story.flow.pop_value()?;
                let Value::DivertTarget(id) = val else {
                    return Err(RuntimeError::TypeError(
                        "tunnel_call_variable requires DivertTarget".into(),
                    ));
                };
                let idx = program
                    .resolve_container(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                let container = program.container(idx);
                if container.counting_flags.contains(CountingFlags::VISITS) {
                    *story.visit_counts.entry(id).or_insert(0) += 1;
                }

                let current_pos = current_position(story)?;
                let thread = story.flow.current_thread_mut();
                thread.call_stack.push(CallFrame {
                    return_address: Some(current_pos),
                    temps: Vec::new(),
                    container_stack: vec![ContainerPosition {
                        container_idx: idx,
                        offset: 0,
                    }],
                    frame_type: CallFrameType::Tunnel,
                });
            }
            Opcode::TunnelReturn => {
                // The eval block before ->-> pushes either void (normal
                // return) or a DivertTarget (tunnel onwards override).
                // Pop it: if it's a DivertTarget, overwrite this frame's
                // return address so we divert there instead of returning.
                let val = story.flow.pop_value()?;
                if let Value::DivertTarget(id) = val {
                    let (idx, offset) = program
                        .resolve_target(id)
                        .ok_or(RuntimeError::UnresolvedDefinition(id))?;
                    let thread = story.flow.current_thread_mut();
                    let frame = thread
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
                story.flow.output.begin_capture();
            }
            Opcode::EndStringEval => {
                let text = story
                    .flow
                    .output
                    .end_capture()
                    .ok_or(RuntimeError::CaptureUnderflow)?;
                story.flow.value_stack.push(Value::String(text));
            }
            Opcode::BeginChoiceSet => {
                story.flow.pending_choices.clear();
            }
            Opcode::BeginChoice(flags, target_id) => {
                handle_begin_choice(story, program, flags, target_id)?;
            }

            // ── Intrinsics ──────────────────────────────────────────────
            Opcode::VisitCount => {
                let val = story.flow.pop_value()?;
                if let Value::DivertTarget(id) = val {
                    let count = story.visit_counts.get(&id).copied().unwrap_or(0);
                    story.flow.value_stack.push(Value::Int(count.cast_signed()));
                } else {
                    story.flow.value_stack.push(Value::Int(0));
                }
            }
            Opcode::CurrentVisitCount => {
                // The current container's visit count was already incremented
                // by EnterContainer, so subtract 1 to get the 0-based count
                // that ink sequences expect (0 on first visit).
                let pos = current_position(story)?;
                let id = program.container(pos.container_idx).id;
                let count = story.visit_counts.get(&id).copied().unwrap_or(0);
                let zero_based = count.saturating_sub(1);
                story
                    .flow
                    .value_stack
                    .push(Value::Int(zero_based.cast_signed()));
            }
            Opcode::TurnsSince => {
                // Stub: return -1 (never visited) for now.
                let _val = story.flow.pop_value()?;
                story.flow.value_stack.push(Value::Int(-1));
            }
            Opcode::TurnIndex => {
                story
                    .flow
                    .value_stack
                    .push(Value::Int(story.flow.turn_index.cast_signed()));
            }
            #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            Opcode::ChoiceCount => {
                story
                    .flow
                    .value_stack
                    .push(Value::Int(story.flow.pending_choices.len() as i32));
            }
            Opcode::Random => {
                // Stub: push 0.
                let _max = story.flow.pop_value()?;
                let _min = story.flow.pop_value()?;
                story.flow.value_stack.push(Value::Int(0));
            }
            Opcode::SeedRandom => {
                let _seed = story.flow.pop_value()?;
            }

            // ── Sequences ───────────────────────────────────────────────
            Opcode::Sequence(kind, count) => {
                handle_sequence(story, kind, count)?;
            }

            // ── Tags ────────────────────────────────────────────────────
            Opcode::BeginTag => {
                story.flow.in_tag = true;
                story.flow.output.begin_capture();
            }
            Opcode::EndTag => {
                let tag_text = story
                    .flow
                    .output
                    .end_capture()
                    .ok_or(RuntimeError::CaptureUnderflow)?;
                story.flow.current_tags.push(tag_text);
                story.flow.in_tag = false;
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
    let thread = story.flow.current_thread_mut();
    let popped = thread
        .call_stack
        .pop()
        .ok_or(RuntimeError::CallStackUnderflow)?;

    if popped.frame_type == CallFrameType::Function {
        if is_explicit_return {
            // Explicit `~ret`: return value is already on the value stack.
            // Discard the capture checkpoint; text stays in the output.
            story.flow.output.discard_capture();
        } else {
            // Implicit return: capture text output as the return value.
            // Trim trailing newlines — function bodies end with `\n` but
            // inline callers (`{f()}`) expect clean text without trailing breaks.
            let text = story
                .flow
                .output
                .end_capture()
                .ok_or(RuntimeError::CaptureUnderflow)?;
            let text = text.trim_end_matches('\n').to_owned();
            story.flow.value_stack.push(Value::String(text));
        }
    }

    if let Some(ret) = popped.return_address {
        resume_at(story, ret);
    }

    Ok(())
}

fn binary(story: &mut Story, op: BinaryOp) -> Result<(), RuntimeError> {
    let right = story.flow.pop_value()?;
    let left = story.flow.pop_value()?;
    let result = value_ops::binary_op(op, &left, &right)?;
    story.flow.value_stack.push(result);
    Ok(())
}

/// Resume execution at a return address.
fn resume_at(story: &mut Story, pos: ContainerPosition) {
    let thread = story.flow.current_thread_mut();
    if let Some(frame) = thread.call_stack.last_mut()
        && let Some(top) = frame.container_stack.last_mut()
    {
        *top = pos;
    }
}

fn goto_target(story: &mut Story, program: &Program, id: DefinitionId) -> Result<(), RuntimeError> {
    let (container_idx, byte_offset) = program
        .resolve_target(id)
        .ok_or(RuntimeError::UnresolvedDefinition(id))?;

    let thread = story.flow.current_thread_mut();
    let frame = thread
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
    let already_on_stack = frame
        .container_stack
        .iter()
        .any(|p| p.container_idx == container_idx);

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

    // Increment visit count conditionally:
    // - New container (not already on stack): always count.
    // - Already on stack + COUNT_START_ONLY at offset 0: count (gather loops).
    // - Already on stack without COUNT_START_ONLY: don't count (self-loops
    //   in VISITS-only knots shouldn't inflate the visit counter).
    let container = program.container(container_idx);
    if container.counting_flags.contains(CountingFlags::VISITS) {
        let should_count = if already_on_stack {
            container
                .counting_flags
                .contains(CountingFlags::COUNT_START_ONLY)
                && byte_offset == 0
        } else {
            true
        };
        if should_count {
            *story.visit_counts.entry(id).or_insert(0) += 1;
        }
    }

    Ok(())
}

fn apply_jump(story: &mut Story, relative: i32) -> Result<(), RuntimeError> {
    let thread = story.flow.current_thread_mut();
    let frame = thread
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
    let thread = story.flow.current_thread();
    let frame = thread
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
        let condition = story.flow.pop_value()?;
        if !value_ops::is_truthy(&condition) {
            // Skip this choice — pop remaining text values and mark skipping.
            if flags.has_choice_only_content {
                let _ = story.flow.value_stack.pop();
            }
            if flags.has_start_content {
                let _ = story.flow.value_stack.pop();
            }
            story.flow.skipping_choice = true;
            return Ok(());
        }
    }

    // 1b. Once-only check: skip if the target container was already visited.
    if flags.once_only {
        let visit_count = story.visit_counts.get(&target_id).copied().unwrap_or(0);
        if visit_count > 0 {
            if flags.has_choice_only_content {
                let _ = story.flow.value_stack.pop();
            }
            if flags.has_start_content {
                let _ = story.flow.value_stack.pop();
            }
            story.flow.skipping_choice = true;
            return Ok(());
        }
    }

    // 2. Pop choice text strings (choice-only is on top, start below).
    let choice_only_text = if flags.has_choice_only_content {
        match story.flow.value_stack.pop() {
            Some(Value::String(s)) => s,
            Some(other) => value_ops::stringify(&other),
            None => String::new(),
        }
    } else {
        String::new()
    };

    let start_text = if flags.has_start_content {
        match story.flow.value_stack.pop() {
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

    let idx = story.flow.pending_choices.len();
    let thread_fork = story.flow.fork_thread();
    story.flow.pending_choices.push(PendingChoice {
        display_text,
        target_id,
        target_idx,
        target_offset,
        flags,
        original_index: idx,
        output_line_idx: None,
        thread_fork,
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
    let val = story.flow.pop_value()?;
    let visit_count = if let Value::DivertTarget(id) = val {
        story.visit_counts.get(&id).copied().unwrap_or(0)
    } else {
        0
    };

    let count = u32::from(count);
    if count == 0 {
        story.flow.value_stack.push(Value::Int(0));
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

    story.flow.value_stack.push(Value::Int(idx.cast_signed()));
    Ok(())
}
