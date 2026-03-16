//! Opcode decode-dispatch loop.

use std::rc::Rc;

use brink_format::{
    ChoiceFlags, CountingFlags, DefinitionId, LineContent, LinePart, Opcode, PluralCategory,
    PluralResolver, SelectKey, Value,
};

use crate::error::RuntimeError;
use crate::list_ops;
use crate::program::Program;
use crate::state::StoryState;
use crate::story::{CallFrame, CallFrameType, ContainerPosition, Flow, PendingChoice, Stats};
use crate::value_ops::{self, BinaryOp};

/// Result of a single VM instruction step.
pub(crate) enum Stepped {
    /// Opcode executed (or bookkeeping done), keep going.
    Continue,
    /// A thread completed and was popped.
    ThreadCompleted,
    /// Hit `CallExternal` — External frame is on the stack with args.
    ExternalCall,
    /// Hit `Done` opcode — yield for pending choices or done.
    Done,
    /// Hit `End` opcode — story permanently ended.
    Ended,
}

/// Execute a single instruction (or bookkeeping operation).
///
/// The caller is responsible for looping and for enforcing safety limits.
#[expect(clippy::too_many_lines, clippy::similar_names)]
pub(crate) fn step(
    flow: &mut Flow,
    state: &mut impl StoryState,
    stats: &mut Stats,
) -> Result<Stepped, RuntimeError> {
    // ── Preamble: resolve current position ──────────────────────────────
    let thread = flow.current_thread_mut();
    let Some(frame) = thread.call_stack.last_mut() else {
        // Current thread's call stack is empty.
        if flow.can_pop_thread() {
            flow.pop_thread();
            stats.threads_completed += 1;
            return Ok(Stepped::ThreadCompleted);
        }
        return Ok(Stepped::Done);
    };

    // If the top frame is External, the orchestration layer failed to resolve it.
    if frame.frame_type == CallFrameType::External {
        if let Some(fn_id) = frame.external_fn_id {
            return Err(RuntimeError::UnresolvedExternalCall(fn_id));
        }
        return Err(RuntimeError::CallStackUnderflow);
    }

    let Some(pos) = frame.container_stack.last().copied() else {
        // Container stack empty — the frame has no more containers to execute.
        let frame_type = frame.frame_type;
        return handle_frame_exhaustion(flow, stats, frame_type);
    };

    let container = state.program().container(pos.container_idx);

    // Check if we've reached end of bytecode.
    if pos.offset >= container.bytecode.len() {
        let thread = flow.current_thread_mut();
        let frame = thread
            .call_stack
            .last_mut()
            .ok_or(RuntimeError::CallStackUnderflow)?;
        frame.container_stack.pop();
        if frame.container_stack.is_empty() {
            let frame_type = frame.frame_type;
            return handle_frame_exhaustion(flow, stats, frame_type);
        }
        return Ok(Stepped::Continue);
    }

    // ── Decode ──────────────────────────────────────────────────────────
    let mut offset = pos.offset;
    let op = Opcode::decode(&container.bytecode, &mut offset)?;
    stats.opcodes += 1;

    // Advance the offset in the position.
    {
        let thread = flow.current_thread_mut();
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

    // ── Dispatch ────────────────────────────────────────────────────────
    match op {
        // ── Output ──────────────────────────────────────────────────
        Opcode::EmitLine(idx, slot_count) => {
            let text = resolve_line(
                state.program(),
                flow,
                &pos,
                idx,
                slot_count,
                state.plural_resolver(),
            )?;
            flow.output.push_text(&text);
        }
        Opcode::EvalLine(idx, slot_count) => {
            let text = resolve_line(
                state.program(),
                flow,
                &pos,
                idx,
                slot_count,
                state.plural_resolver(),
            )?;
            flow.value_stack.push(Value::String(text.into()));
        }
        Opcode::EmitValue => {
            let val = flow.pop_value()?;
            let text = value_ops::stringify(&val, state.program());
            flow.output.push_text(&text);
        }
        Opcode::EmitNewline => {
            flow.output.push_newline();
        }
        Opcode::Glue => {
            flow.output.push_glue();
        }
        Opcode::EndChoice => {
            flow.skipping_choice = false;
        }
        Opcode::Nop | Opcode::SourceLocation(_, _) | Opcode::ThreadStart | Opcode::ThreadDone => {}

        // ── Lifecycle ────────────────────────────────────────────────
        Opcode::Done => {
            if flow.can_pop_thread() {
                flow.pop_thread();
                return Ok(Stepped::ThreadCompleted);
            }
            return Ok(Stepped::Done);
        }
        Opcode::End => {
            return Ok(Stepped::Ended);
        }

        // ── Container flow ──────────────────────────────────────────
        Opcode::EnterContainer(id) => {
            let idx = state
                .program()
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            // Increment visit count if flags set.
            let counting_flags = state.program().container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                state.increment_visit(id);
                state.set_turn_count(id, state.turn_index());
            }

            let thread = flow.current_thread_mut();
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
            let thread = flow.current_thread_mut();
            let frame = thread
                .call_stack
                .last_mut()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            frame.container_stack.pop();
        }

        // ── Control flow ────────────────────────────────────────────
        Opcode::Goto(id) => {
            if !flow.skipping_choice {
                goto_target(flow, state, id)?;
            }
        }
        Opcode::GotoIf(id) => {
            let val = flow.pop_value()?;
            if value_ops::is_truthy(&val) {
                goto_target(flow, state, id)?;
            }
        }
        Opcode::GotoVariable => {
            let val = flow.pop_value()?;
            if let Value::DivertTarget(id) = val {
                goto_target(flow, state, id)?;
            } else {
                return Err(RuntimeError::TypeError(
                    "goto_variable requires DivertTarget".into(),
                ));
            }
        }
        Opcode::Jump(rel) | Opcode::SequenceBranch(rel) => {
            apply_jump(flow, rel)?;
        }
        Opcode::JumpIfFalse(rel) => {
            let val = flow.pop_value()?;
            if !value_ops::is_truthy(&val) {
                apply_jump(flow, rel)?;
            }
        }

        // ── Stack & literals ─────────────────────────────────────────
        Opcode::PushInt(v) => flow.value_stack.push(Value::Int(v)),
        Opcode::PushFloat(v) => flow.value_stack.push(Value::Float(v)),
        Opcode::PushBool(v) => flow.value_stack.push(Value::Bool(v)),
        Opcode::PushString(idx) => {
            let s: Rc<str> = state.program().name(brink_format::NameId(idx)).into();
            flow.value_stack.push(Value::String(s));
        }
        Opcode::PushNull => {
            flow.value_stack.push(Value::Null);
        }
        Opcode::PushList(idx) => {
            let lv = state.program().list_literal(idx).clone();
            flow.value_stack.push(Value::List(Rc::new(lv)));
        }
        Opcode::PushDivertTarget(id) => {
            flow.value_stack.push(Value::DivertTarget(id));
        }
        Opcode::PushVarPointer(id) => {
            flow.value_stack.push(Value::VariablePointer(id));
        }
        Opcode::Pop => {
            flow.pop_value()?;
        }
        Opcode::Duplicate => {
            let val = flow.peek_value()?.clone();
            flow.value_stack.push(val);
        }

        // ── Arithmetic ──────────────────────────────────────────────
        Opcode::Add => binary(flow, state.program(), BinaryOp::Add)?,
        Opcode::Subtract => binary(flow, state.program(), BinaryOp::Subtract)?,
        Opcode::Multiply => binary(flow, state.program(), BinaryOp::Multiply)?,
        Opcode::Divide => binary(flow, state.program(), BinaryOp::Divide)?,
        Opcode::Modulo => binary(flow, state.program(), BinaryOp::Modulo)?,
        Opcode::Negate => {
            let val = flow.pop_value()?;
            let result = match val {
                Value::Int(n) => Value::Int(-n),
                Value::Float(n) => Value::Float(-n),
                _ => {
                    return Err(RuntimeError::TypeError("cannot negate non-numeric".into()));
                }
            };
            flow.value_stack.push(result);
        }

        // ── Comparison ──────────────────────────────────────────────
        Opcode::Equal => binary(flow, state.program(), BinaryOp::Equal)?,
        Opcode::NotEqual => binary(flow, state.program(), BinaryOp::NotEqual)?,
        Opcode::Greater => binary(flow, state.program(), BinaryOp::Greater)?,
        Opcode::GreaterOrEqual => binary(flow, state.program(), BinaryOp::GreaterOrEqual)?,
        Opcode::Less => binary(flow, state.program(), BinaryOp::Less)?,
        Opcode::LessOrEqual => binary(flow, state.program(), BinaryOp::LessOrEqual)?,

        // ── Logic ───────────────────────────────────────────────────
        Opcode::Not => {
            let val = flow.pop_value()?;
            flow.value_stack
                .push(Value::Bool(!value_ops::is_truthy(&val)));
        }
        Opcode::And => binary(flow, state.program(), BinaryOp::And)?,
        Opcode::Or => binary(flow, state.program(), BinaryOp::Or)?,

        // ── Global vars ─────────────────────────────────────────────
        Opcode::GetGlobal(id) => {
            let idx = state
                .program()
                .resolve_global(id)
                .ok_or(RuntimeError::UnresolvedGlobal(id))?;
            let val = state.global(idx).clone();
            flow.value_stack.push(val);
        }
        Opcode::SetGlobal(id) => {
            let idx = state
                .program()
                .resolve_global(id)
                .ok_or(RuntimeError::UnresolvedGlobal(id))?;
            let mut val = flow.pop_value()?;
            // Retain list origins: when assigning an empty list to a
            // global that holds a list, preserve the old origins so
            // LIST_ALL can still enumerate the original list definition.
            if let Value::List(new_lv) = &mut val
                && new_lv.items.is_empty()
                && new_lv.origins.is_empty()
                && let Value::List(old_lv) = state.global(idx)
            {
                Rc::make_mut(new_lv).origins.clone_from(&old_lv.origins);
            }
            state.set_global(idx, val);
        }

        // ── Temp vars ───────────────────────────────────────────────
        Opcode::DeclareTemp(slot) => {
            // New declaration stores as-is, including pointers.
            let val = flow.pop_value()?;
            let thread = flow.current_thread_mut();
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
        Opcode::SetTemp(slot) => {
            // Write-through: if the temp holds a pointer, write the new
            // value to the pointed-to location instead.
            let val = flow.pop_value()?;
            let thread = flow.current_thread_mut();
            let frame = thread
                .call_stack
                .last()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            let idx = slot as usize;
            let current = frame.temps.get(idx).cloned().unwrap_or(Value::Null);
            match current {
                Value::VariablePointer(target_id) => {
                    let global_idx = state
                        .program()
                        .resolve_global(target_id)
                        .ok_or(RuntimeError::UnresolvedGlobal(target_id))?;
                    state.set_global(global_idx, val);
                }
                Value::TempPointer {
                    slot: target_slot,
                    frame_depth,
                } => {
                    let thread = flow.current_thread_mut();
                    let target = thread
                        .call_stack
                        .get_mut(frame_depth as usize)
                        .ok_or(RuntimeError::CallStackUnderflow)?;
                    let ti = target_slot as usize;
                    while target.temps.len() <= ti {
                        target.temps.push(Value::Null);
                    }
                    target.temps[ti] = val;
                }
                _ => {
                    let thread = flow.current_thread_mut();
                    let frame = thread
                        .call_stack
                        .last_mut()
                        .ok_or(RuntimeError::CallStackUnderflow)?;
                    while frame.temps.len() <= idx {
                        frame.temps.push(Value::Null);
                    }
                    frame.temps[idx] = val;
                }
            }
        }
        Opcode::GetTemp(slot) => {
            // Auto-dereference: if temp holds a pointer, push the
            // pointed-to value instead.
            let thread = flow.current_thread();
            let frame = thread
                .call_stack
                .last()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            let val = frame
                .temps
                .get(slot as usize)
                .cloned()
                .unwrap_or(Value::Null);
            match val {
                Value::VariablePointer(target_id) => {
                    let global_idx = state
                        .program()
                        .resolve_global(target_id)
                        .ok_or(RuntimeError::UnresolvedGlobal(target_id))?;
                    let global_val = state.global(global_idx).clone();
                    flow.value_stack.push(global_val);
                }
                Value::TempPointer {
                    slot: target_slot,
                    frame_depth,
                } => {
                    let thread = flow.current_thread();
                    let target = thread
                        .call_stack
                        .get(frame_depth as usize)
                        .ok_or(RuntimeError::CallStackUnderflow)?;
                    let target_val = target
                        .temps
                        .get(target_slot as usize)
                        .cloned()
                        .unwrap_or(Value::Null);
                    flow.value_stack.push(target_val);
                }
                _ => {
                    flow.value_stack.push(val);
                }
            }
        }
        Opcode::GetTempRaw(slot) => {
            // Raw read: push the temp's value as-is (including pointers).
            let thread = flow.current_thread();
            let frame = thread
                .call_stack
                .last()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            let val = frame
                .temps
                .get(slot as usize)
                .cloned()
                .unwrap_or(Value::Null);
            flow.value_stack.push(val);
        }
        Opcode::PushTempPointer(slot) => {
            // Push a pointer to a temp variable. If the temp already holds
            // a pointer (VariablePointer or TempPointer), flatten through
            // to prevent double-indirection.
            let thread = flow.current_thread();
            let frame = thread
                .call_stack
                .last()
                .ok_or(RuntimeError::CallStackUnderflow)?;
            let current = frame
                .temps
                .get(slot as usize)
                .cloned()
                .unwrap_or(Value::Null);
            match current {
                Value::VariablePointer(_) | Value::TempPointer { .. } => {
                    // Flatten: pass the existing pointer through.
                    flow.value_stack.push(current);
                }
                _ => {
                    let thread = flow.current_thread();
                    #[expect(clippy::cast_possible_truncation)]
                    let depth = (thread.call_stack.len() - 1) as u16;
                    flow.value_stack.push(Value::TempPointer {
                        slot,
                        frame_depth: depth,
                    });
                }
            }
        }

        // ── Casts ───────────────────────────────────────────────────
        Opcode::CastToInt => {
            let val = flow.pop_value()?;
            flow.value_stack.push(value_ops::cast_to_int(&val));
        }
        Opcode::CastToFloat => {
            let val = flow.pop_value()?;
            flow.value_stack.push(value_ops::cast_to_float(&val));
        }

        // ── Math ────────────────────────────────────────────────────
        Opcode::Floor => {
            let val = flow.pop_value()?;
            let result = match val {
                Value::Float(f) => Value::Float(f.floor()),
                Value::Int(_) => val,
                _ => return Err(RuntimeError::TypeError("floor requires numeric".into())),
            };
            flow.value_stack.push(result);
        }
        Opcode::Ceiling => {
            let val = flow.pop_value()?;
            let result = match val {
                Value::Float(f) => Value::Float(f.ceil()),
                Value::Int(_) => val,
                _ => return Err(RuntimeError::TypeError("ceiling requires numeric".into())),
            };
            flow.value_stack.push(result);
        }
        Opcode::Pow => binary(flow, state.program(), BinaryOp::Pow)?,
        Opcode::Min => binary(flow, state.program(), BinaryOp::Min)?,
        Opcode::Max => binary(flow, state.program(), BinaryOp::Max)?,

        // ── Functions ───────────────────────────────────────────────
        Opcode::Call(id) => {
            let idx = state
                .program()
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            let counting_flags = state.program().container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                state.increment_visit(id);
                state.set_turn_count(id, state.turn_index());
            }

            // Capture output during function call — text output becomes
            // the return value when the frame is popped.
            flow.output.begin_capture();

            let current_pos = current_position(flow)?;
            let thread = flow.current_thread_mut();
            thread.call_stack.push(CallFrame {
                return_address: Some(current_pos),
                temps: Vec::new(),
                container_stack: vec![ContainerPosition {
                    container_idx: idx,
                    offset: 0,
                }],
                frame_type: CallFrameType::Function,
                external_fn_id: None,
            });
            stats.frames_pushed += 1;
        }
        Opcode::Return => {
            // The function already pushed its return value via `ev, <value>, /ev`.
            // It stays on the value stack; pop_call_frame just cleans up the checkpoint.
            pop_call_frame(flow, stats, true)?;
        }
        Opcode::TunnelCall(id) => {
            let idx = state
                .program()
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            let counting_flags = state.program().container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                state.increment_visit(id);
                state.set_turn_count(id, state.turn_index());
            }

            let current_pos = current_position(flow)?;
            let thread = flow.current_thread_mut();
            thread.call_stack.push(CallFrame {
                return_address: Some(current_pos),
                temps: Vec::new(),
                container_stack: vec![ContainerPosition {
                    container_idx: idx,
                    offset: 0,
                }],
                frame_type: CallFrameType::Tunnel,
                external_fn_id: None,
            });
            stats.frames_pushed += 1;
        }
        Opcode::ThreadCall(id) => {
            let idx = state
                .program()
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            // Fork the current thread — the fork inherits the full call
            // stack (including any enclosing Tunnel frames) so that
            // `fork_thread` at choice creation captures enough context
            // for `->->` to return through tunnels. The Thread frame
            // acts as a boundary: when it exhausts, the thread pops
            // without unwinding into inherited frames below.
            let (mut forked, cache_hit) = flow.fork_thread();
            forked.call_stack.push(CallFrame {
                return_address: None,
                temps: Vec::new(),
                container_stack: vec![ContainerPosition {
                    container_idx: idx,
                    offset: 0,
                }],
                frame_type: CallFrameType::Thread,
                external_fn_id: None,
            });
            flow.threads.push(forked);
            stats.threads_created += 1;
            stats.frames_pushed += 1;
            if cache_hit {
                stats.snapshot_cache_hits += 1;
            } else {
                stats.snapshot_cache_misses += 1;
            }
        }
        Opcode::TunnelCallVariable => {
            let val = flow.pop_value()?;
            let Value::DivertTarget(id) = val else {
                return Err(RuntimeError::TypeError(
                    "tunnel_call_variable requires DivertTarget".into(),
                ));
            };
            let idx = state
                .program()
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            let counting_flags = state.program().container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                state.increment_visit(id);
                state.set_turn_count(id, state.turn_index());
            }

            let current_pos = current_position(flow)?;
            let thread = flow.current_thread_mut();
            thread.call_stack.push(CallFrame {
                return_address: Some(current_pos),
                temps: Vec::new(),
                container_stack: vec![ContainerPosition {
                    container_idx: idx,
                    offset: 0,
                }],
                frame_type: CallFrameType::Tunnel,
                external_fn_id: None,
            });
            stats.frames_pushed += 1;
        }
        Opcode::CallVariable => {
            let val = flow.pop_value()?;
            let Value::DivertTarget(id) = val else {
                return Err(RuntimeError::TypeError(
                    "call_variable requires DivertTarget".into(),
                ));
            };
            let idx = state
                .program()
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            let counting_flags = state.program().container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                state.increment_visit(id);
                state.set_turn_count(id, state.turn_index());
            }

            flow.output.begin_capture();

            let current_pos = current_position(flow)?;
            let thread = flow.current_thread_mut();
            thread.call_stack.push(CallFrame {
                return_address: Some(current_pos),
                temps: Vec::new(),
                container_stack: vec![ContainerPosition {
                    container_idx: idx,
                    offset: 0,
                }],
                frame_type: CallFrameType::Function,
                external_fn_id: None,
            });
            stats.frames_pushed += 1;
        }
        Opcode::TunnelReturn => {
            // The eval block before ->-> pushes either void (normal
            // return) or a DivertTarget (tunnel onwards override).
            let val = flow.pop_value()?;

            // Strip Thread boundary frames — they are transparent to
            // ->->. This happens after choice selection when the fork
            // has [inherited..., Thread, choice-body] and ->-> needs
            // to reach the Tunnel frame below the Thread boundary.
            while flow
                .current_thread()
                .call_stack
                .last()
                .is_some_and(|f| f.frame_type == CallFrameType::Thread)
            {
                flow.current_thread_mut().call_stack.pop();
                stats.frames_popped += 1;
            }

            // If a DivertTarget, overwrite this frame's return address
            // so we divert there instead of the original caller.
            if let Value::DivertTarget(id) = val {
                let (idx, offset) = state
                    .program()
                    .resolve_target(id)
                    .ok_or(RuntimeError::UnresolvedDefinition(id))?;
                let thread = flow.current_thread_mut();
                let frame = thread
                    .call_stack
                    .last_mut()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                frame.return_address = Some(ContainerPosition {
                    container_idx: idx,
                    offset,
                });
            }
            pop_call_frame(flow, stats, true)?;
        }

        // ── Choices ─────────────────────────────────────────────────
        Opcode::BeginStringEval => {
            flow.output.begin_capture();
        }
        Opcode::EndStringEval => {
            let text = flow
                .output
                .end_capture()
                .ok_or(RuntimeError::CaptureUnderflow)?;
            flow.value_stack.push(Value::String(text.into()));
        }
        Opcode::BeginChoice(flags, target_id) => {
            handle_begin_choice(flow, state, stats, flags, target_id)?;
        }

        // ── Intrinsics ──────────────────────────────────────────────
        Opcode::VisitCount => {
            let val = flow.pop_value()?;
            if let Value::DivertTarget(id) = val {
                let count = state.visit_count(id);
                flow.value_stack.push(Value::Int(count.cast_signed()));
            } else {
                flow.value_stack.push(Value::Int(0));
            }
        }
        Opcode::CurrentVisitCount => {
            // The current container's visit count was already incremented
            // by EnterContainer, so subtract 1 to get the 0-based count
            // that ink sequences expect (0 on first visit).
            let pos = current_position(flow)?;
            let id = state.program().container(pos.container_idx).id;
            let count = state.visit_count(id);
            let zero_based = count.saturating_sub(1);
            flow.value_stack.push(Value::Int(zero_based.cast_signed()));
        }
        Opcode::TurnsSince => {
            let val = flow.pop_value()?;
            let result = if let Value::DivertTarget(id) = val {
                if let Some(last_turn) = state.turn_count(id) {
                    #[expect(clippy::cast_possible_wrap)]
                    let delta = (state.turn_index() - last_turn) as i32;
                    delta
                } else {
                    -1
                }
            } else {
                -1
            };
            flow.value_stack.push(Value::Int(result));
        }
        Opcode::TurnIndex => {
            flow.value_stack
                .push(Value::Int(state.turn_index().cast_signed()));
        }
        #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        Opcode::ChoiceCount => {
            flow.value_stack
                .push(Value::Int(flow.pending_choices.len() as i32));
        }
        Opcode::Random => {
            // Reference pops max first, then min.
            let max_val = flow.pop_value()?;
            let min_val = flow.pop_value()?;
            let max_i = match max_val {
                Value::Int(n) => n,
                Value::Float(f) => {
                    #[expect(clippy::cast_possible_truncation)]
                    {
                        f as i32
                    }
                }
                _ => 1,
            };
            let min_i = match min_val {
                Value::Int(n) => n,
                Value::Float(f) => {
                    #[expect(clippy::cast_possible_truncation)]
                    {
                        f as i32
                    }
                }
                _ => 0,
            };
            // +1 because RANDOM is inclusive of both min and max.
            let range = max_i.wrapping_sub(min_i).wrapping_add(1);
            let result = if range <= 0 {
                min_i
            } else {
                let result_seed = state.rng_seed().wrapping_add(state.previous_random());
                let next_random = state.next_random(result_seed);
                state.set_previous_random(next_random);
                (next_random % range) + min_i
            };
            flow.value_stack.push(Value::Int(result));
        }
        Opcode::SeedRandom => {
            let seed_val = flow.pop_value()?;
            let seed = match seed_val {
                Value::Int(n) => n,
                _ => 0,
            };
            state.set_rng_seed(seed);
            state.set_previous_random(0);
            flow.value_stack.push(Value::Null);
        }

        // ── Sequences ───────────────────────────────────────────────
        Opcode::Sequence(kind, count) => {
            handle_sequence(flow, state, kind, count)?;
        }

        // ── Tags ────────────────────────────────────────────────────
        Opcode::BeginTag => {
            flow.in_tag = true;
            flow.output.begin_capture();
        }
        Opcode::EndTag => {
            // end_capture returns None when there's no active checkpoint.
            // This happens in sequences: non-first branches start with `/#`
            // to close the *previous* branch's tag, but on a fresh visit
            // there's nothing to close. Silently skip in that case.
            if let Some(tag_text) = flow.output.end_capture() {
                let tag = tag_text.trim().to_string();
                flow.in_tag = false;
                if flow.output.has_checkpoint() {
                    // Inside a capture (choice text, function call) — store
                    // for the choice/function to consume.
                    flow.current_tags.push(tag);
                } else {
                    // Top-level output — associate with the current line.
                    flow.output.push_tag(tag);
                }
            }
        }

        // ── List operations ─────────────────────────────────────────
        Opcode::ListContains => list_ops::list_contains(flow)?,
        Opcode::ListNotContains => list_ops::list_not_contains(flow)?,
        Opcode::ListIntersect => list_ops::list_intersect(flow)?,
        Opcode::ListAll => list_ops::list_all(flow, state.program())?,
        Opcode::ListInvert => list_ops::list_invert(flow, state.program())?,
        Opcode::ListCount => list_ops::list_count(flow)?,
        Opcode::ListMin => list_ops::list_min(flow, state.program())?,
        Opcode::ListMax => list_ops::list_max(flow, state.program())?,
        Opcode::ListValue => list_ops::list_value(flow, state.program())?,
        Opcode::ListRange => list_ops::list_range(flow, state.program())?,
        Opcode::ListFromInt => list_ops::list_from_int(flow, state.program())?,
        Opcode::ListRandom => list_ops::list_random(flow, state)?,

        // ── External functions ──────────────────────────────────────
        Opcode::CallExternal(fn_id, arg_count) => {
            // Pop arguments from the value stack.
            let mut args = Vec::with_capacity(arg_count as usize);
            for _ in 0..arg_count {
                args.push(flow.pop_value()?);
            }
            args.reverse(); // Args were pushed left-to-right, popped right-to-left.

            let current_pos = current_position(flow)?;
            let thread = flow.current_thread_mut();
            thread.call_stack.push(CallFrame {
                return_address: Some(current_pos),
                temps: args,
                container_stack: Vec::new(),
                frame_type: CallFrameType::External,
                external_fn_id: Some(fn_id),
            });
            stats.frames_pushed += 1;
            return Ok(Stepped::ExternalCall);
        }
    }

    Ok(Stepped::Continue)
}

fn resolve_line(
    program: &Program,
    flow: &mut Flow,
    pos: &ContainerPosition,
    idx: u16,
    slot_count: u8,
    resolver: Option<&dyn PluralResolver>,
) -> Result<String, RuntimeError> {
    // Pop slot values from the stack (LIFO order — reverse to match slot indices).
    let mut slots = Vec::with_capacity(slot_count as usize);
    for _ in 0..slot_count {
        slots.push(flow.pop_value()?);
    }
    slots.reverse();

    let lines = program.line_table(pos.container_idx);
    let Some(entry) = lines.get(idx as usize) else {
        return Ok(String::new());
    };

    match &entry.content {
        LineContent::Plain(s) => Ok(s.clone()),
        LineContent::Template(parts) => {
            let mut result = String::new();
            for part in parts {
                match part {
                    LinePart::Literal(s) => result.push_str(s),
                    LinePart::Slot(n) => {
                        if let Some(val) = slots.get(*n as usize) {
                            result.push_str(&value_ops::stringify(val, program));
                        }
                    }
                    LinePart::Select {
                        slot,
                        variants,
                        default,
                    } => {
                        let text = resolve_select(*slot, variants, default, &slots, resolver);
                        result.push_str(text);
                    }
                }
            }
            Ok(result)
        }
    }
}

/// Resolve a Select part against its slot value.
///
/// Cascade: Exact → Keyword → Cardinal/Ordinal → default.
fn resolve_select<'a>(
    slot: u8,
    variants: &'a [(SelectKey, String)],
    default: &'a str,
    slots: &[Value],
    resolver: Option<&dyn PluralResolver>,
) -> &'a str {
    let Some(val) = slots.get(slot as usize) else {
        return default;
    };

    // Coerce slot value to integer for numeric matching.
    #[expect(clippy::cast_possible_truncation)]
    let n: Option<i64> = match val {
        Value::Int(i) => Some(i64::from(*i)),
        Value::Float(f) => Some(*f as i64),
        _ => None,
    };

    // 1. Exact match (integer equality).
    if let Some(n) = n {
        #[expect(clippy::cast_possible_truncation)]
        let n32 = n as i32;
        for (key, text) in variants {
            if let SelectKey::Exact(e) = key
                && *e == n32
            {
                return text;
            }
        }
    }

    // 2. Keyword match (string equality against stringified value).
    let stringified = match val {
        Value::String(s) => Some(s.as_ref()),
        _ => None,
    };
    if let Some(s) = stringified {
        for (key, text) in variants {
            if let SelectKey::Keyword(k) = key
                && k == s
            {
                return text;
            }
        }
    }

    // 3. Plural resolution (Cardinal/Ordinal) via resolver.
    if let (Some(n), Some(r)) = (n, resolver) {
        // Try cardinal keys.
        let cardinal: PluralCategory = r.cardinal(n, None);
        for (key, text) in variants {
            if let SelectKey::Cardinal(cat) = key
                && *cat == cardinal
            {
                return text;
            }
        }

        // Try ordinal keys.
        let ordinal: PluralCategory = r.ordinal(n);
        for (key, text) in variants {
            if let SelectKey::Ordinal(cat) = key
                && *cat == ordinal
            {
                return text;
            }
        }
    }

    // 4. Fallback.
    default
}

/// Handle a frame whose container stack has been exhausted.
///
/// Returns the appropriate [`Stepped`] variant:
/// - `ThreadCompleted` when a thread boundary is done and popped.
/// - `Done` when the last thread/frame is exhausted.
/// - `Continue` when a frame was popped and execution can proceed.
///
/// - **Thread**: the thread boundary is done — pop the entire thread.
///   Inherited frames below the Thread frame are never unwound into.
/// - **Non-function with pending choices**: the frame is waiting for a
///   choice selection. Pop the thread so other threads can run.
/// - **Otherwise**: pop the call frame normally (implicit return).
fn handle_frame_exhaustion(
    flow: &mut Flow,
    stats: &mut Stats,
    frame_type: CallFrameType,
) -> Result<Stepped, RuntimeError> {
    if frame_type == CallFrameType::Thread {
        // Thread boundary exhausted — thread is done. Pop it without
        // touching inherited frames below. ThreadCall always creates a
        // child thread, so can_pop_thread is expected to be true.
        if flow.can_pop_thread() {
            flow.pop_thread();
            stats.threads_completed += 1;
            return Ok(Stepped::ThreadCompleted);
        }
        return Ok(Stepped::Done);
    }

    if frame_type != CallFrameType::Function && !flow.pending_choices.is_empty() {
        // Non-function frame with pending choices: the fork captured at
        // choice creation preserves the state for resumption.
        if flow.can_pop_thread() {
            flow.pop_thread();
            stats.threads_completed += 1;
            return Ok(Stepped::ThreadCompleted);
        }
        return Ok(Stepped::Done);
    }

    pop_call_frame(flow, stats, false)?;
    if flow.current_thread().call_stack.is_empty() {
        if flow.can_pop_thread() {
            flow.pop_thread();
            stats.threads_completed += 1;
            return Ok(Stepped::ThreadCompleted);
        }
        return Ok(Stepped::Done);
    }
    Ok(Stepped::Continue)
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
fn pop_call_frame(
    flow: &mut Flow,
    stats: &mut Stats,
    is_explicit_return: bool,
) -> Result<(), RuntimeError> {
    let thread = flow.current_thread_mut();
    let popped = thread
        .call_stack
        .pop()
        .ok_or(RuntimeError::CallStackUnderflow)?;
    stats.frames_popped += 1;

    if popped.frame_type == CallFrameType::Function {
        if is_explicit_return {
            // Explicit `~ret`: return value is already on the value stack.
            // Discard the capture checkpoint; text stays in the output.
            flow.output.discard_capture();
        } else {
            // Implicit return: capture text output as the return value.
            // Trim trailing newlines — function bodies end with `\n` but
            // inline callers (`{f()}`) expect clean text without trailing breaks.
            let text = flow
                .output
                .end_capture()
                .ok_or(RuntimeError::CaptureUnderflow)?;
            let text: Rc<str> = text.trim_end_matches('\n').into();
            flow.value_stack.push(Value::String(text));
        }
    }

    if let Some(ret) = popped.return_address {
        resume_at(flow, ret);
    }

    Ok(())
}

fn binary(flow: &mut Flow, program: &Program, op: BinaryOp) -> Result<(), RuntimeError> {
    let right = flow.pop_value()?;
    let left = flow.pop_value()?;
    let result = value_ops::binary_op(op, &left, &right, program)?;
    flow.value_stack.push(result);
    Ok(())
}

/// Resume execution at a return address.
fn resume_at(flow: &mut Flow, pos: ContainerPosition) {
    let thread = flow.current_thread_mut();
    if let Some(frame) = thread.call_stack.last_mut()
        && let Some(top) = frame.container_stack.last_mut()
    {
        *top = pos;
    }
}

fn goto_target(
    flow: &mut Flow,
    state: &mut impl StoryState,
    id: DefinitionId,
) -> Result<(), RuntimeError> {
    let (container_idx, byte_offset) = state
        .program()
        .resolve_target(id)
        .ok_or(RuntimeError::UnresolvedDefinition(id))?;

    let thread = flow.current_thread_mut();
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
    let counting_flags = state.program().container(container_idx).counting_flags;
    if counting_flags.contains(CountingFlags::VISITS) {
        let should_count = if already_on_stack {
            counting_flags.contains(CountingFlags::COUNT_START_ONLY) && byte_offset == 0
        } else {
            true
        };
        if should_count {
            state.increment_visit(id);
            state.set_turn_count(id, state.turn_index());
        }
    }

    Ok(())
}

fn apply_jump(flow: &mut Flow, relative: i32) -> Result<(), RuntimeError> {
    let thread = flow.current_thread_mut();
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

fn current_position(flow: &Flow) -> Result<ContainerPosition, RuntimeError> {
    let thread = flow.current_thread();
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

#[expect(clippy::similar_names)]
fn handle_begin_choice(
    flow: &mut Flow,
    state: &mut impl StoryState,
    stats: &mut Stats,
    flags: ChoiceFlags,
    target_id: DefinitionId,
) -> Result<(), RuntimeError> {
    // Single-pop protocol: stack contains [display_string?], [condition?]
    // with condition on top (evaluated last). Either content flag means
    // there is one display string on the stack.
    let has_display = flags.has_start_content || flags.has_choice_only_content;

    // 1. Pop condition first (it was evaluated last, so it's on top).
    if flags.has_condition {
        let condition = flow.pop_value()?;
        if !value_ops::is_truthy(&condition) {
            if has_display {
                let _ = flow.value_stack.pop();
            }
            flow.skipping_choice = true;
            return Ok(());
        }
    }

    // 1b. Once-only check: skip if the target container was already visited.
    if flags.once_only {
        let visit_count = state.visit_count(target_id);
        if visit_count > 0 {
            if has_display {
                let _ = flow.value_stack.pop();
            }
            flow.skipping_choice = true;
            return Ok(());
        }
    }

    // 2. Pop the single display string.
    let display_text = if has_display {
        match flow.value_stack.pop() {
            Some(Value::String(s)) => (*s).to_owned(),
            Some(other) => value_ops::stringify(&other, state.program()),
            None => String::new(),
        }
    } else {
        String::new()
    };

    let (target_idx, target_offset) = state
        .program()
        .resolve_target(target_id)
        .ok_or(RuntimeError::UnresolvedDefinition(target_id))?;

    let idx = flow.pending_choices.len();
    let (thread_fork, cache_hit) = flow.fork_thread();
    stats.threads_created += 1;
    if cache_hit {
        stats.snapshot_cache_hits += 1;
    } else {
        stats.snapshot_cache_misses += 1;
    }
    let tags = std::mem::take(&mut flow.current_tags);
    flow.pending_choices.push(PendingChoice {
        display_text,
        target_id,
        target_idx,
        target_offset,
        flags,
        original_index: idx,
        tags,
        thread_fork,
    });

    Ok(())
}

fn handle_sequence(
    flow: &mut Flow,
    state: &mut impl StoryState,
    kind: brink_format::SequenceKind,
    count: u8,
) -> Result<(), RuntimeError> {
    if kind == brink_format::SequenceKind::Shuffle {
        return handle_shuffle_sequence(flow, state);
    }

    // Non-shuffle sequences: pop divert target, use visit count.
    let val = flow.pop_value()?;
    let visit_count = if let Value::DivertTarget(id) = val {
        state.visit_count(id)
    } else {
        0
    };

    let count = u32::from(count);
    if count == 0 {
        flow.value_stack.push(Value::Int(0));
        return Ok(());
    }

    let idx = match kind {
        brink_format::SequenceKind::Cycle => visit_count % count,
        brink_format::SequenceKind::Stopping => visit_count.min(count - 1),
        brink_format::SequenceKind::OnceOnly => {
            if visit_count < count {
                visit_count
            } else {
                count // past the end -> skip all branches
            }
        }
        brink_format::SequenceKind::Shuffle => unreachable!(),
    };

    flow.value_stack.push(Value::Int(idx.cast_signed()));
    Ok(())
}

/// `NextSequenceShuffleIndex` — reference ink implementation.
///
/// Pops `numElements` (Int) and `seqCount` (Int) from the value stack.
/// Uses a partial Fisher-Yates shuffle seeded with `path_hash + loopIndex + story_seed`.
#[expect(clippy::cast_sign_loss)]
fn handle_shuffle_sequence(
    flow: &mut Flow,
    state: &mut impl StoryState,
) -> Result<(), RuntimeError> {
    let num_elements = match flow.pop_value()? {
        Value::Int(n) => n,
        other => {
            return Err(RuntimeError::TypeError(format!(
                "Shuffle: expected Int for numElements, got {other:?}"
            )));
        }
    };
    let seq_count = match flow.pop_value()? {
        Value::Int(n) => n,
        other => {
            return Err(RuntimeError::TypeError(format!(
                "Shuffle: expected Int for seqCount, got {other:?}"
            )));
        }
    };

    if num_elements == 0 {
        flow.value_stack.push(Value::Int(0));
        return Ok(());
    }

    let loop_index = seq_count / num_elements;
    let iteration_index = seq_count % num_elements;

    // Get path_hash from the current container.
    let pos = current_position(flow)?;
    let path_hash = state.program().container(pos.container_idx).path_hash;

    // Seed RNG with path_hash + loopIndex + story_seed (matching reference).
    let seed = path_hash
        .wrapping_add(loop_index)
        .wrapping_add(state.rng_seed());

    // Pre-generate all needed random values from a single seeded RNG instance.
    let random_values = state.random_sequence(seed, (iteration_index + 1) as usize);

    // Partial Fisher-Yates: maintain unpicked list, pick iterationIndex+1 elements.
    let mut unpicked: Vec<i32> = (0..num_elements).collect();

    for i in 0..=iteration_index {
        let chosen = random_values[i as usize] as usize % unpicked.len();
        let chosen_index = unpicked[chosen];
        unpicked.swap_remove(chosen);

        if i == iteration_index {
            flow.value_stack.push(Value::Int(chosen_index));
            return Ok(());
        }
    }

    // Should not reach here.
    flow.value_stack.push(Value::Int(0));
    Ok(())
}
