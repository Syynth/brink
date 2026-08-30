//! Opcode decode-dispatch loop.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;

use brink_format::{
    ChoiceFlags, CountingFlags, DefinitionId, LineContent, LineEntry, LinePart, Opcode,
    PluralCategory, PluralResolver, SelectKey, Value,
};

use crate::collection_ops;
use crate::conversion_ops;
use crate::error::RuntimeError;
use crate::list_ops;
use crate::program::Program;
use crate::proj_ops;
use crate::rand_ops;
use crate::range_ops;
use crate::record_ops;
use crate::state::ContextAccess;
use crate::story::{
    CallFrame, CallFrameType, ContainerPosition, ExecMode, Flow, PendingChoice, PureCallbackState,
    Stats, classify_ran_out_of_content,
};
use crate::string_ops;
use crate::tower_ops;
use crate::value_ops::{self, BinaryOp};

/// Result of a single VM instruction step.
#[derive(Clone, Copy)]
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
///
/// Thin wrapper over [`step_impl`]: under the `effect-trace` feature it also
/// records a tracked turn-terminating fault (NS-A2, issue #1108 — the
/// `faults` row dimension's ground truth) against the definition scope that
/// was executing when the fault fired, before propagating the error
/// unchanged. A zero-cost passthrough in ordinary builds.
pub(crate) fn step<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
) -> Result<Stepped, RuntimeError> {
    let result = step_impl::<R>(flow, program, line_tables, context, stats, resolver);
    #[cfg(feature = "effect-trace")]
    if let Err(e) = &result
        && crate::effect_trace::is_tracked_fault(e)
        && let Some(def) = effect_trace_current_def(flow, program)
    {
        crate::effect_trace::record_fault(def);
    }
    result
}

#[expect(clippy::too_many_lines)]
fn step_impl<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
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
        return handle_frame_exhaustion(flow, program, line_tables, resolver, stats, frame_type);
    };

    let container = program.container(pos.container_idx);

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
            return handle_frame_exhaustion(
                flow,
                program,
                line_tables,
                resolver,
                stats,
                frame_type,
            );
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
            // Capture slot values from the stack, push a deferred LineRef.
            let mut slots = Vec::with_capacity(slot_count as usize);
            for _ in 0..slot_count {
                slots.push(flow.pop_value()?);
            }
            slots.reverse();
            // Look up precomputed flags for filtering.
            let scope_idx = program.scope_table_idx(pos.container_idx) as usize;
            let flags = line_tables
                .get(scope_idx)
                .and_then(|lines| lines.get(idx as usize))
                .map_or(brink_format::LineFlags::EMPTY, |entry| entry.flags);
            note_effect_emit(flow, program);
            flow.output
                .push_line_ref(pos.container_idx, idx, slots, flags);
        }
        Opcode::EvalLine(idx, slot_count) => {
            // EvalLine resolves eagerly — result goes on the value stack.
            let text = resolve_line(program, line_tables, flow, &pos, idx, slot_count, resolver)?;
            flow.value_stack.push(Value::String(text.into()));
        }
        Opcode::EmitValue => {
            let val = flow.pop_value()?;
            note_effect_emit(flow, program);
            flow.output.push_value_ref(val);
        }
        Opcode::EmitNewline => {
            flow.output.push_newline();
        }
        Opcode::Spring => {
            note_effect_emit(flow, program);
            flow.output.push_spring();
        }
        Opcode::Glue => {
            note_effect_emit(flow, program);
            flow.output.push_glue();
        }
        Opcode::AttachElement => {
            // Issue #2108: an `attach = StructName` convention handler's
            // claimed line. `call` (already evaluated by the preceding
            // codegen'd expression opcodes) leaves its result here — push
            // its fields into the output buffer's own append-only stream
            // (`OutputPart::ElementAttach`) rather than mutating a live
            // `Flow` field. See that variant's own doc for why: the buffer
            // defers a line's commitment until later content proves no
            // `Glue` reaches back over its `Newline`, so the VM may already
            // have stepped past a LATER run's own attach opcodes by the
            // time an EARLIER, still-buffered line is finally drained —
            // reading a live "current" field at that point would attribute
            // the wrong run's data to it. No `note_effect_emit` call: unlike
            // `EmitValue`, this never reaches the visible transcript
            // (ruling item 6, "AN EVENT EXISTS IFF A LINE EXISTS" — no
            // line, so no emit to attribute an effect to).
            let val = flow.pop_value()?;
            if let Value::Record { shape, fields } = &val
                && let Some(entry) = program.struct_shapes.get(shape.0 as usize)
                && entry.fields.len() == fields.len()
            {
                for (name, v) in entry.fields.iter().zip(fields.iter()) {
                    let key = program.name_checked(*name).unwrap_or("?").to_string();
                    let value = value_ops::stringify(v, program);
                    flow.output.push_element_attach(key, value);
                }
            }
            // A non-`Record` value (or a shape/field-count mismatch — the
            // struct-shapes table wire is malformed, or the compile-time
            // `attach = StructName` / return-type agreement check (E180)
            // somehow didn't fire) is not a compile error at this layer:
            // silently attaching nothing here mirrors `stringify`'s own
            // "total by construction" fallback for a stale `ShapeId`
            // rather than faulting the whole story over it.
        }
        Opcode::EndElementRun => {
            flow.output.push_element_attach_end();
        }
        Opcode::EndChoice => {
            flow.skipping_choice = false;
        }
        Opcode::Nop | Opcode::ThreadStart | Opcode::ThreadDone => {}

        // ── Lifecycle ────────────────────────────────────────────────
        Opcode::Done => {
            if flow.can_pop_thread() {
                flow.pop_thread();
                return Ok(Stepped::ThreadCompleted);
            }
            flow.did_safe_exit = true;
            return Ok(Stepped::Done);
        }
        Opcode::Yield => {
            // Pause for choice presentation. Like Done but does NOT
            // set did_safe_exit.
            if flow.can_pop_thread() {
                flow.pop_thread();
                return Ok(Stepped::ThreadCompleted);
            }
            // Only yield if there are actually choices to present.
            // If no choices were created, continue execution — the
            // choice set was empty but the story may have more content.
            if !flow.pending_choices.is_empty() {
                return Ok(Stepped::Done);
            }
            flow.did_unsafe_yield = true;
        }
        Opcode::End => {
            return Ok(Stepped::Ended);
        }

        // ── Container flow ──────────────────────────────────────────
        Opcode::EnterContainer(id) => {
            let idx = program
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            // Increment visit count if flags set.
            let counting_flags = program.container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                context.increment_visit(id);
                context.set_turn_count(id, context.turn_index());
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
                goto_target(flow, program, context, id)?;
            }
        }
        Opcode::GotoIf(id) => {
            let val = flow.pop_value()?;
            if value_ops::is_truthy(&val)? {
                goto_target(flow, program, context, id)?;
            }
        }
        Opcode::GotoVariable => {
            let val = flow.pop_value()?;
            if let Value::DivertTarget(id) = val {
                goto_target(flow, program, context, id)?;
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
            if !value_ops::is_truthy(&val)? {
                apply_jump(flow, rel)?;
            }
        }

        // ── Stack & literals ─────────────────────────────────────────
        Opcode::PushInt(v) => flow.value_stack.push(Value::Int(v)),
        Opcode::PushFloat(v) => flow.value_stack.push(Value::Float(v)),
        Opcode::PushBool(v) => flow.value_stack.push(Value::Bool(v)),
        Opcode::PushString(idx) => {
            let s: Arc<str> = program.name(brink_format::NameId(idx)).into();
            flow.value_stack.push(Value::String(s));
        }
        Opcode::PushNull => {
            flow.value_stack.push(Value::Null);
        }
        Opcode::PushList(idx) => {
            let lv = program.list_literal(idx).clone();
            flow.value_stack.push(Value::List(Arc::new(lv)));
        }
        Opcode::PushDivertTarget(id) => {
            flow.value_stack.push(Value::DivertTarget(id));
        }
        Opcode::PushVarPointer(id) => {
            // A `ref` argument targeting a global — emitted only at the
            // call site passing it (see `effect_trace`'s module docs): the
            // caller's own bytecode is what's executing here, so recording
            // a write now (conservatively, matching `record_ref_param_
            // writes`'s "a ref param might write" model) attributes it to
            // the same def the static analyzer charges, not to whichever
            // def eventually dereferences the pointer.
            note_effect_write(flow, program, id);
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
        Opcode::Add => binary(flow, program, BinaryOp::Add)?,
        Opcode::Subtract => binary(flow, program, BinaryOp::Subtract)?,
        Opcode::Multiply => binary(flow, program, BinaryOp::Multiply)?,
        Opcode::Divide => binary(flow, program, BinaryOp::Divide)?,
        Opcode::Modulo => binary(flow, program, BinaryOp::Modulo)?,
        Opcode::Negate => {
            let val = flow.pop_value()?;
            let result = match val {
                Value::Int(n) => Value::Int(-n),
                Value::Float(n) => Value::Float(-n),
                // Tower values negate componentwise (NS-A8, T3: glam's own
                // `Neg` impls, wholesale — a vector is numeric).
                Value::Vec2(v) => Value::Vec2(-v),
                Value::Vec3(v) => Value::Vec3(-v),
                Value::Vec4(v) => Value::Vec4(-v),
                Value::Quat(q) => Value::Quat(-q),
                Value::Mat2(m) => Value::Mat2(-m),
                Value::Mat3(m) => Value::Mat3(-m),
                Value::Mat4(m) => Value::Mat4(-m),
                _ => {
                    return Err(RuntimeError::TypeError("cannot negate non-numeric".into()));
                }
            };
            flow.value_stack.push(result);
        }

        // ── Comparison ──────────────────────────────────────────────
        Opcode::Equal => binary(flow, program, BinaryOp::Equal)?,
        Opcode::NotEqual => binary(flow, program, BinaryOp::NotEqual)?,
        Opcode::Greater => binary(flow, program, BinaryOp::Greater)?,
        Opcode::GreaterOrEqual => binary(flow, program, BinaryOp::GreaterOrEqual)?,
        Opcode::Less => binary(flow, program, BinaryOp::Less)?,
        Opcode::LessOrEqual => binary(flow, program, BinaryOp::LessOrEqual)?,

        // ── Logic ───────────────────────────────────────────────────
        Opcode::Not => {
            let val = flow.pop_value()?;
            flow.value_stack
                .push(Value::Bool(!value_ops::is_truthy(&val)?));
        }
        Opcode::And => binary(flow, program, BinaryOp::And)?,
        Opcode::Or => binary(flow, program, BinaryOp::Or)?,

        // ── Global vars ─────────────────────────────────────────────
        Opcode::GetGlobal(id) => {
            let idx = program
                .resolve_global(id)
                .ok_or(RuntimeError::UnresolvedGlobal(id))?;
            let val = context.global(idx).clone();
            note_value_share(&val);
            note_effect_read(flow, program, id);
            flow.value_stack.push(val);
        }
        Opcode::SetGlobal(id) => {
            guard_comparator_write(flow, "assigned a global variable")?;
            let idx = program
                .resolve_global(id)
                .ok_or(RuntimeError::UnresolvedGlobal(id))?;
            let mut val = flow.pop_value()?;
            // Retain list origins: when assigning an empty list to a
            // global that holds a list, preserve the old origins so
            // LIST_ALL can still enumerate the original list definition.
            if let Value::List(new_lv) = &mut val
                && new_lv.items.is_empty()
                && new_lv.origins.is_empty()
                && let Value::List(old_lv) = context.global(idx)
            {
                Arc::make_mut(new_lv).origins.clone_from(&old_lv.origins);
            }
            note_effect_write(flow, program, id);
            context.set_global(idx, val);
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
                    guard_comparator_write(flow, "assigned a global through a `ref` parameter")?;
                    let global_idx = program
                        .resolve_global(target_id)
                        .ok_or(RuntimeError::UnresolvedGlobal(target_id))?;
                    context.set_global(global_idx, val);
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
                // T1e (docs/t1e-spec.md §3): a projection-bound `ref`
                // parameter's write-through — root-cell RMW via the same
                // `proj_ops::write` an `Opcode::ProjWrite` dispatch would
                // call. Purely additive: this arm is unreachable for any
                // program that predates T1e (a `Value::Projection` is
                // constructed only by `Opcode::MakeProjection`, itself
                // emitted only for a real path-projection ref-argument).
                Value::Projection(p) => {
                    guard_comparator_write(flow, "wrote through a path projection")?;
                    proj_ops::write(program, context, p.cell, &p.segments, val)?;
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
                    let global_idx = program
                        .resolve_global(target_id)
                        .ok_or(RuntimeError::UnresolvedGlobal(target_id))?;
                    let global_val = context.global(global_idx).clone();
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
                // T1e: a projection-bound `ref` parameter's read — same
                // additive-only reasoning as `SetTemp`'s new arm above.
                Value::Projection(p) => {
                    let result = proj_ops::read(program, &*context, p.cell, &p.segments)?;
                    flow.value_stack.push(result);
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
        // ── Sharing discipline (T1b-4, docs/value-model-spec.md §5) ────
        Opcode::TakeGlobal(id) => {
            // No auto-dereference — mirrors `GetGlobal`/`SetGlobal`: a
            // ref-param pointer lives in a *temp*, never in a global slot
            // itself.
            let idx = program
                .resolve_global(id)
                .ok_or(RuntimeError::UnresolvedGlobal(id))?;
            let val = context.take_global(idx);
            note_effect_read(flow, program, id);
            flow.value_stack.push(val);
        }
        Opcode::TakeTemp(slot) => {
            // Auto-dereference, mirroring `GetTemp`: if the temp holds a
            // pointer, take from the *pointed-to* location and leave it
            // `Null` — the pointer itself stays in this slot untouched (a
            // `ref` param must keep pointing at its target for the rest of
            // the call, exactly like `GetTemp`/`SetTemp`'s write-through).
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
                Value::VariablePointer(target_id) => {
                    let global_idx = program
                        .resolve_global(target_id)
                        .ok_or(RuntimeError::UnresolvedGlobal(target_id))?;
                    let taken = context.take_global(global_idx);
                    flow.value_stack.push(taken);
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
                    #[expect(clippy::indexing_slicing, reason = "padded to ti + 1 above")]
                    let taken = mem::replace(&mut target.temps[ti], Value::Null);
                    flow.value_stack.push(taken);
                }
                // T1e: a projection-bound `ref` parameter's take — same
                // additive-only reasoning as `SetTemp`/`GetTemp`'s new arms.
                Value::Projection(p) => {
                    let taken = proj_ops::take(program, context, p.cell, &p.segments)?;
                    flow.value_stack.push(taken);
                }
                _ => {
                    let thread = flow.current_thread_mut();
                    let frame = thread
                        .call_stack
                        .last_mut()
                        .ok_or(RuntimeError::CallStackUnderflow)?;
                    let idx = slot as usize;
                    while frame.temps.len() <= idx {
                        frame.temps.push(Value::Null);
                    }
                    #[expect(clippy::indexing_slicing, reason = "padded to idx + 1 above")]
                    let taken = mem::replace(&mut frame.temps[idx], Value::Null);
                    flow.value_stack.push(taken);
                }
            }
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
                // T1e (docs/t1e-spec.md §2): a projection also flattens
                // through — forwarding a projection-bound `ref` parameter
                // (`heal(ref hp)` where `hp` is itself `ref`-bound) passes
                // the *same* `(root cell, segments)` on, never wraps it in
                // another layer of indirection. A compound projection is
                // never constructed this way (T1e-1's E080 durable-root
                // check rejects a param as a *new* `ref`'s root), so this
                // is always the bare-forward case.
                Value::VariablePointer(_) | Value::TempPointer { .. } | Value::Projection(_) => {
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
            flow.value_stack.push(value_ops::cast_to_int(&val)?);
        }
        Opcode::CastToFloat => {
            let val = flow.pop_value()?;
            flow.value_stack.push(value_ops::cast_to_float(&val)?);
        }

        // ── Math ────────────────────────────────────────────────────
        // `floor`/`ceil` need a `libm`-backed implementation that `core`
        // doesn't provide — std-only, like `powf` in `value_ops::float_op`.
        Opcode::Floor => {
            let val = flow.pop_value()?;
            let result = match val {
                #[cfg(feature = "std")]
                Value::Float(f) => Value::Float(f.floor()),
                #[cfg(not(feature = "std"))]
                Value::Float(_) => {
                    return Err(RuntimeError::Unimplemented(
                        "FLOOR() requires the `std` feature (no libm in no_std builds)".into(),
                    ));
                }
                Value::Int(_) => val,
                _ => return Err(RuntimeError::TypeError("floor requires numeric".into())),
            };
            flow.value_stack.push(result);
        }
        Opcode::Ceiling => {
            let val = flow.pop_value()?;
            let result = match val {
                #[cfg(feature = "std")]
                Value::Float(f) => Value::Float(f.ceil()),
                #[cfg(not(feature = "std"))]
                Value::Float(_) => {
                    return Err(RuntimeError::Unimplemented(
                        "CEILING() requires the `std` feature (no libm in no_std builds)".into(),
                    ));
                }
                Value::Int(_) => val,
                _ => return Err(RuntimeError::TypeError("ceiling requires numeric".into())),
            };
            flow.value_stack.push(result);
        }
        Opcode::Pow => binary(flow, program, BinaryOp::Pow)?,
        Opcode::Min => binary(flow, program, BinaryOp::Min)?,
        Opcode::Max => binary(flow, program, BinaryOp::Max)?,

        // ── Functions ───────────────────────────────────────────────
        Opcode::Call(id) => {
            let idx = program
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            let counting_flags = program.container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                context.increment_visit(id);
                context.set_turn_count(id, context.turn_index());
            }

            // Function output goes directly to the active output target.
            // Record the target length so trailing whitespace can be
            // trimmed on return (matching C#'s TrimWhitespaceFromFunctionEnd).
            let output_start = flow.output.target_len();
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
                function_output_start: Some(output_start),
            });
            stats.frames_pushed += 1;
        }
        Opcode::Return => {
            // The function already pushed its return value via `ev, <value>, /ev`.
            // It stays on the value stack; pop_call_frame just cleans up the frame.
            pop_call_frame(flow, program, line_tables, resolver, stats, true)?;
        }
        Opcode::TunnelCall(id) => {
            let idx = program
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            let counting_flags = program.container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                context.increment_visit(id);
                context.set_turn_count(id, context.turn_index());
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
                function_output_start: None,
            });
            stats.frames_pushed += 1;
        }
        Opcode::ThreadCall(id) => {
            let idx = program
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
                function_output_start: None,
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
            let idx = program
                .resolve_target(id)
                .map(|(idx, _)| idx)
                .ok_or(RuntimeError::UnresolvedDefinition(id))?;

            let counting_flags = program.container(idx).counting_flags;
            if counting_flags.contains(CountingFlags::VISITS) {
                context.increment_visit(id);
                context.set_turn_count(id, context.turn_index());
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
                function_output_start: None,
            });
            stats.frames_pushed += 1;
        }
        Opcode::CallVariable(argc) => {
            let val = flow.pop_value()?;
            match val {
                // Classic divert-target-variable call (oracle path, unchanged):
                // the target's own prologue self-consumes its declared params
                // off the stack, so `argc` is not needed here — untouched by
                // #721.
                Value::DivertTarget(id) => {
                    let idx = program
                        .resolve_target(id)
                        .map(|(idx, _)| idx)
                        .ok_or(RuntimeError::UnresolvedDefinition(id))?;

                    let counting_flags = program.container(idx).counting_flags;
                    if counting_flags.contains(CountingFlags::VISITS) {
                        context.increment_visit(id);
                        context.set_turn_count(id, context.turn_index());
                    }

                    let output_start = flow.output.target_len();
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
                        function_output_start: Some(output_start),
                    });
                    stats.frames_pushed += 1;
                }
                // T1c (docs/t1c-spec.md §3): the **direct** call form `f(args…)`
                // where `f` holds a function value dispatches through the same
                // `CallVariable` site (codegen pushes the supplied args, then
                // the callee, then `CallVariable(argc)`) — the divert-target
                // arm above stays the oracle path, this arm is inert for it.
                // `argc` is the exact count codegen pushed at *this* call site
                // (issue #721: never derive it from the resolved target's
                // arity — that made the pop count trivially match `enter_fn_
                // value`'s arity check, so a gradual-mode arity mismatch left
                // a stray value on the stack instead of faulting). Popping the
                // wire-carried `argc` here means a real mismatch surfaces as
                // `FunctionValueArity` from `enter_fn_value`, exactly like the
                // explicit `call(f, args…)` form (`CallValue(argc)`).
                Value::FnRef(_) | Value::Closure(_) => {
                    let supplied = pop_values(flow, argc as usize)?;
                    enter_fn_value(flow, program, context, stats, &val, supplied)?;
                }
                other => {
                    return Err(RuntimeError::NotCallable(value_type_name(&other)));
                }
            }
        }
        // ── Function values (T1c, docs/t1c-spec.md §3/§6, #700) ──────────
        Opcode::PushFnRef(id) => {
            flow.value_stack.push(Value::FnRef(id));
        }
        Opcode::MakeClosure {
            target,
            bound_count,
        } => {
            let (idx, _) = program
                .resolve_target(target)
                .ok_or(RuntimeError::UnresolvedDefinition(target))?;
            let params = program.container_params(idx);
            let n = bound_count as usize;
            // Pop the bound args (pushed in declared order; top is the last).
            let mut popped = pop_values(flow, n)?; // now in declared order
            let mut env = Vec::with_capacity(n);
            for (i, payload) in popped.drain(..).enumerate() {
                // Names/modes come from the target's signature (single source
                // of truth the rehydration check reads back against).
                let (name, is_ref) = params
                    .get(i)
                    .map_or((brink_format::NameId(0), false), |p| (p.name, p.is_ref));
                env.push(brink_format::ClosureEnvEntry {
                    name,
                    is_ref,
                    payload,
                });
            }
            flow.value_stack.push(Value::closure(target, env));
        }
        Opcode::CallValue(argc) => {
            let callee = flow.pop_value()?;
            match callee {
                Value::FnRef(_) | Value::Closure(_) => {
                    let supplied = pop_values(flow, argc as usize)?;
                    enter_fn_value(flow, program, context, stats, &callee, supplied)?;
                }
                // A divert-target callee (`call(f)` where `f` is a divert var)
                // dispatches like `CallVariable` — jump into the target,
                // ignoring `argc` (diverts don't take value args this way).
                Value::DivertTarget(id) => {
                    let idx = program
                        .resolve_target(id)
                        .map(|(idx, _)| idx)
                        .ok_or(RuntimeError::UnresolvedDefinition(id))?;
                    let counting_flags = program.container(idx).counting_flags;
                    if counting_flags.contains(CountingFlags::VISITS) {
                        context.increment_visit(id);
                        context.set_turn_count(id, context.turn_index());
                    }
                    let output_start = flow.output.target_len();
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
                        function_output_start: Some(output_start),
                    });
                    stats.frames_pushed += 1;
                }
                other => {
                    return Err(RuntimeError::NotCallable(value_type_name(&other)));
                }
            }
        }
        // T1c-3 (docs/t1c-spec.md §3): `bind(f, args…)` — val-only currying
        // over an existing function value. Pop the callee (top) then the
        // `argc` supplied args below it, append them to the callee's bound-arg
        // row (consuming the head of its remaining param row), and push the
        // new function value. A non-function callee or over-binding (more
        // args than the target has remaining params) is a turn-terminating
        // fault — never a silently truncated or garbage row.
        Opcode::BindValue(argc) => {
            let callee = flow.pop_value()?;
            let supplied = pop_values(flow, argc as usize)?;
            let bound = bind_fn_value(program, &callee, supplied)?;
            flow.value_stack.push(bound);
        }

        // ── Path projections (T1e, docs/t1e-spec.md §3) ──────────────
        Opcode::MakeProjection {
            root,
            segment_count,
        } => {
            // Codegen pushes segment values in source order; popping
            // (LIFO) collects them in reverse, so one final `reverse()`
            // restores source order — the same shape `MakeClosure`'s
            // bound-arg row uses via `pop_values`' `split_off` (which
            // preserves order because it slices, rather than popping one at
            // a time).
            let mut segments = Vec::with_capacity(segment_count as usize);
            for _ in 0..segment_count {
                segments.push(brink_format::ProjSegment::from_value(flow.pop_value()?));
            }
            segments.reverse();
            // Emitted only for a `ref` argument targeting a projected path
            // (`brink-codegen-inkb/src/expr.rs`) — same construction-time
            // attribution rationale as `PushVarPointer` above.
            note_effect_write(flow, program, root);
            flow.value_stack.push(Value::projection(root, segments));
        }
        Opcode::ProjRead => {
            let val = flow.pop_value()?;
            let Some(p) = val.as_projection() else {
                return Err(RuntimeError::TypeError(
                    "ProjRead requires a Projection value".into(),
                ));
            };
            let result = proj_ops::read(program, &*context, p.cell, &p.segments)?;
            flow.value_stack.push(result);
        }
        Opcode::ProjWrite => {
            guard_comparator_write(flow, "wrote through a path projection")?;
            let value = flow.pop_value()?;
            let proj = flow.pop_value()?;
            let Some(p) = proj.as_projection() else {
                return Err(RuntimeError::TypeError(
                    "ProjWrite requires a Projection value".into(),
                ));
            };
            proj_ops::write(program, context, p.cell, &p.segments, value)?;
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
                let (idx, offset) = program
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
            pop_call_frame(flow, program, line_tables, resolver, stats, true)?;
        }

        // ── Choices ─────────────────────────────────────────────────
        Opcode::BeginStringEval => {
            flow.output.begin_capture();
        }
        Opcode::EndStringEval => {
            let text = flow
                .output
                .end_capture(program, line_tables, resolver)
                .ok_or(RuntimeError::CaptureUnderflow)?;
            flow.value_stack.push(Value::String(text.into()));
        }
        Opcode::BeginFragment => {
            flow.output.begin_fragment();
        }
        Opcode::EndFragment => {
            let idx = flow
                .output
                .end_fragment()
                .ok_or(RuntimeError::CaptureUnderflow)?;
            flow.value_stack.push(Value::FragmentRef(idx));
        }
        Opcode::BeginChoice(flags, target_id) => {
            handle_begin_choice(flow, program, context, stats, flags, target_id)?;
        }

        // ── Intrinsics ──────────────────────────────────────────────
        Opcode::VisitCount => {
            let val = flow.pop_value()?;
            if let Value::DivertTarget(id) = val {
                let count = context.visit_count(id);
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
            let id = program.container(pos.container_idx).id;
            let count = context.visit_count(id);
            let zero_based = count.saturating_sub(1);
            flow.value_stack.push(Value::Int(zero_based.cast_signed()));
        }
        Opcode::TouchVisit => {
            // #3273: record a view of the named container without entering
            // it, and hand back the 0-based view index the branch math
            // needs. Pre-increment is deliberate: EnterContainer's
            // increment-then-subtract-1 dance (CurrentVisitCount above)
            // lands on the same 0-on-first-view number.
            let val = flow.pop_value()?;
            if let Value::DivertTarget(id) = val {
                let count = context.visit_count(id);
                context.increment_visit(id);
                flow.value_stack.push(Value::Int(count.cast_signed()));
            } else {
                // Mirror VisitCount's malformed-input tolerance — push 0,
                // record nothing.
                flow.value_stack.push(Value::Int(0));
            }
        }
        Opcode::ShuffleIndexOf => {
            let val = flow.pop_value()?;
            let path_hash = if let Value::DivertTarget(id) = val {
                program.resolve_target(id).map_or(0, |(container_idx, _)| {
                    program.container(container_idx).path_hash
                })
            } else {
                0
            };
            handle_shuffle_with_hash::<R>(flow, context, path_hash)?;
        }
        Opcode::TurnsSince => {
            let val = flow.pop_value()?;
            let result = if let Value::DivertTarget(id) = val {
                if let Some(last_turn) = context.turn_count(id) {
                    #[expect(clippy::cast_possible_wrap)]
                    let delta = (context.turn_index() - last_turn) as i32;
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
                .push(Value::Int(context.turn_index().cast_signed()));
        }
        #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        Opcode::ChoiceCount => {
            flow.value_stack
                .push(Value::Int(flow.pending_choices.len() as i32));
        }
        Opcode::Random => {
            // NS-A6: the frozen ink surface over the one RNG cell — same
            // write the brink draw verbs record.
            guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
            note_effect_write(flow, program, DefinitionId::RNG_CELL);
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
                let result_seed = context.rng_seed().wrapping_add(context.previous_random());
                let next_random = context.next_random::<R>(result_seed);
                context.set_previous_random(next_random);
                (next_random % range) + min_i
            };
            flow.value_stack.push(Value::Int(result));
        }
        Opcode::SeedRandom => {
            guard_comparator_write(flow, "reseeded the RNG (the RNG cell is world state)")?;
            note_effect_write(flow, program, DefinitionId::RNG_CELL);
            let seed_val = flow.pop_value()?;
            let seed = match seed_val {
                Value::Int(n) => n,
                _ => 0,
            };
            context.set_rng_seed(seed);
            context.set_previous_random(0);
            flow.value_stack.push(Value::Null);
        }

        // ── Sequences ───────────────────────────────────────────────
        Opcode::Sequence(kind, count) => {
            handle_sequence::<R>(flow, program, context, kind, count)?;
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
            if let Some(tag_text) = flow.output.end_capture(program, line_tables, resolver) {
                let tag = tag_text.trim().to_string();
                flow.in_tag = false;
                note_effect_tag(flow, program);
                if flow.output.has_checkpoint() {
                    // Inside a capture (choice text, function call) — store
                    // for the choice/function to consume.
                    flow.current_tags.push(tag);
                } else if flow.output.in_fragment_capture() {
                    // Inside a fragment — associate with the fragment so the
                    // consumer (e.g. BeginChoice) can pull them out.
                    flow.output.push_fragment_tag(tag);
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
        Opcode::ListAll => list_ops::list_all(flow, program)?,
        Opcode::ListInvert => list_ops::list_invert(flow, program)?,
        Opcode::ListCount => list_ops::list_count(flow)?,
        Opcode::ListMin => list_ops::list_min(flow, program)?,
        Opcode::ListMax => list_ops::list_max(flow, program)?,
        Opcode::ListValue => list_ops::list_value(flow, program)?,
        Opcode::ListRange => list_ops::list_range(flow, program)?,
        Opcode::ListFromInt => list_ops::list_from_int(flow, program)?,
        Opcode::ListRandom => {
            guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
            note_effect_write(flow, program, DefinitionId::RNG_CELL);
            list_ops::list_random::<R>(flow, context)?;
        }

        // ── Collections (T1b) ────────────────────────────────────────
        Opcode::ArrayNew(n) => collection_ops::array_new(flow, n)?,
        Opcode::MapNew(n) => collection_ops::map_new(flow, n)?,
        Opcode::IndexGet => collection_ops::index_get(flow)?,
        Opcode::IndexSet => collection_ops::index_set(flow)?,
        Opcode::CollectionLen => collection_ops::collection_len(flow)?,
        Opcode::MapGet => collection_ops::map_get(flow)?,
        Opcode::MapInsert => collection_ops::map_insert(flow)?,
        Opcode::MapRemove => collection_ops::map_remove(flow)?,
        Opcode::SeqRemoveAt => collection_ops::seq_remove_at(flow)?,
        Opcode::MapContains => collection_ops::map_contains(flow)?,
        Opcode::CollectionKeys => collection_ops::collection_keys(flow)?,
        Opcode::CollectionValues => collection_ops::collection_values(flow)?,
        Opcode::PushLiteral(idx) => collection_ops::push_literal(flow, program, idx)?,

        // ── Records (TM-4) ────────────────────────────────────────────
        Opcode::RecordNew(shape_id) => record_ops::record_new(flow, program, shape_id)?,
        Opcode::RecordGetDyn(name_id) => record_ops::record_get_dyn(flow, program, name_id)?,
        Opcode::RecordSetDyn(name_id) => record_ops::record_set_dyn(flow, program, name_id)?,
        Opcode::RecordGet(offset) => record_ops::record_get(flow, offset)?,
        Opcode::RecordSet(offset) => record_ops::record_set(flow, offset)?,

        // ── Conversion intrinsics (TM-3 completion, #659) ────────────
        // `int(x)` is ONE value-directed verb (NS-A5, `docs/stdlib-spec.md`
        // §7): over a range operand it is `rand::int` — one uniform draw
        // from the inhabited range, a write to the RNG cell — and over
        // everything else it keeps its TM-3 conversion semantics. The
        // dispatch happens here on the *runtime* operand because gradual
        // mode cannot classify the call site statically (an unannotated
        // temp holding a range must still draw); under `types = strict`
        // the checker has already proven which leg runs (and demanded the
        // NonEmptyRange evidence for the draw leg, E117).
        Opcode::ConvertInt => {
            if matches!(flow.value_stack.last(), Some(Value::Range { .. })) {
                guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
                note_effect_write(flow, program, DefinitionId::RNG_CELL);
                rand_ops::rand_int::<R>(flow, context)?;
            } else {
                conversion_ops::convert_to_int(flow)?;
            }
        }
        Opcode::ConvertFloat => conversion_ops::convert_to_float(flow)?,
        Opcode::ConvertString => conversion_ops::convert_to_string(flow, program)?,

        // ── Stdlib slice 1 completion (#857) ─────────────────────────
        Opcode::CharAt => string_ops::char_at(flow)?,

        // ── NS-A1: Option[T] + the ruled stdlib flips (#1107) ────────
        Opcode::PushNone => flow.value_stack.push(Value::none()),
        Opcode::MakeSome => {
            let inner = flow.pop_value()?;
            flow.value_stack.push(Value::some(inner));
        }
        Opcode::StrFind => string_ops::str_find(flow)?,
        Opcode::SeqIndexOf => collection_ops::seq_index_of(flow)?,
        Opcode::SeqMin => collection_ops::seq_min(flow)?,
        Opcode::SeqMax => collection_ops::seq_max(flow)?,
        Opcode::SeqFirst => collection_ops::seq_first(flow)?,
        Opcode::SeqLast => collection_ops::seq_last(flow)?,
        Opcode::SeqPop => collection_ops::seq_pop(flow)?,
        Opcode::MapGetOpt => collection_ops::map_get_opt(flow)?,
        Opcode::MapContainsValue => collection_ops::map_contains_value(flow)?,
        Opcode::MapClear => collection_ops::map_clear(flow)?,

        // ── B1: `or`-coalescing, short-circuited (issue #1471) ─────────
        // Pops `lhs`. `some(v)` pushes the unwrapped `v` and jumps `rel`
        // bytes forward, landing past the `rhs` bytecode codegen emitted
        // right after this instruction — the short-circuit itself, `rhs`
        // is simply never reached. `none` pushes nothing and falls
        // straight through into that `rhs` bytecode.
        Opcode::CoalesceSome(rel) => {
            let val = flow.pop_value()?;
            if let Some(inner) = value_ops::coalesce_unwrap_some(val)? {
                flow.value_stack.push(inner);
                apply_jump(flow, rel)?;
            }
        }

        // ── B1b: the `as` binding (issue #1475) ──────────────────────
        // Fused test-and-bind. The slot is always freshly allocated by
        // the binding, so — unlike `SetTemp` — this is a plain
        // frame-local store: no `VariablePointer`/`TempPointer`/
        // `Projection` write-through case can arise, because nothing has
        // ever written a pointer into a slot that only this op and the
        // binding's own reads address.
        Opcode::OptionBind(slot) => {
            let opt = flow.pop_value()?;
            let bound = match opt {
                Value::OptionVal(Some(payload)) => {
                    Some(Arc::try_unwrap(payload).unwrap_or_else(|shared| (*shared).clone()))
                }
                Value::OptionVal(None) => None,
                other => {
                    return Err(RuntimeError::AsBindingNotOption {
                        found: value_type_name(&other),
                    });
                }
            };
            let matched = bound.is_some();
            if let Some(value) = bound {
                let thread = flow.current_thread_mut();
                let frame = thread
                    .call_stack
                    .last_mut()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                let idx = slot as usize;
                while frame.temps.len() <= idx {
                    frame.temps.push(Value::Null);
                }
                frame.temps[idx] = value;
            }
            flow.value_stack.push(Value::Bool(matched));
        }

        // ── NS-A6: the `std::rand` draw verbs (#1112,
        // `docs/stdlib-spec.md` §7). Every draw is an ordinary write to
        // the one RNG cell (`DefinitionId::RNG_CELL`) — recorded for the
        // ground-truth harness exactly like a global-cell write. The
        // frozen ink ops (`Random`/`SeedRandom`/`ListRandom`) write the
        // same cell and carry the same instrumentation at their own
        // arms. ─────────────────────────────────────────────────────────
        Opcode::RandFloat => {
            guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
            note_effect_write(flow, program, DefinitionId::RNG_CELL);
            rand_ops::rand_float::<R>(flow, context);
        }
        Opcode::RandChance => {
            guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
            note_effect_write(flow, program, DefinitionId::RNG_CELL);
            rand_ops::rand_chance::<R>(flow, context)?;
        }
        Opcode::RandPick => {
            guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
            note_effect_write(flow, program, DefinitionId::RNG_CELL);
            rand_ops::rand_pick::<R>(flow, context)?;
        }
        Opcode::RandShuffle => {
            guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
            note_effect_write(flow, program, DefinitionId::RNG_CELL);
            rand_ops::rand_shuffle::<R>(flow, context)?;
        }

        // ── NS-A5: range values + the inhabited-range refinement
        // (#1111, `docs/stdlib-spec.md` §7, F7/F8). Construction and the
        // `non_empty` validator are pure — no draw, no RNG-cell write;
        // the draw leg of `int(range)` rides `ConvertInt` above. ────────
        Opcode::RangeMakeExcl => range_ops::range_make(flow, false)?,
        Opcode::RangeMakeIncl => range_ops::range_make(flow, true)?,
        Opcode::RangeNonEmpty => range_ops::range_non_empty(flow)?,

        // ── NS-A4: the ordering verbs (#1110, `docs/stdlib-spec.md`
        // §4b). `SeqSorted` is pure placement (dev NaN-fault / prod
        // pinned order — `collection_ops`); `SeqSortedBy` re-enters the
        // VM to run the user comparator (see `call_comparator`). ────────
        Opcode::SeqSorted => collection_ops::seq_sorted(flow)?,
        Opcode::SeqSortedBy => {
            seq_sorted_by::<R>(flow, program, line_tables, context, stats, resolver)?;
        }

        // ── The fn-value verb layer (issue #1679, `docs/stdlib-spec.md`
        // §4): the pure quartet re-enters the VM per element to run the
        // user callback (`call_pure_callback`), under the same pure·silent
        // contract, output isolation and dev-mode world-write guard the
        // NS-A4 comparator uses. The effectful pair (`each`/`map_each`,
        // slice 2) re-enters through `call_effectful_callback` instead —
        // the opposite contract: output reaches the transcript, world-writes
        // are legal. ───────────────────────────────────────────────────
        Opcode::SeqVerb(op) => match op {
            brink_format::SeqVerbOp::Map => {
                seq_map::<R>(flow, program, line_tables, context, stats, resolver, op)?;
            }
            brink_format::SeqVerbOp::Filter => {
                seq_filter::<R>(flow, program, line_tables, context, stats, resolver, op)?;
            }
            brink_format::SeqVerbOp::Fold => {
                seq_fold::<R>(flow, program, line_tables, context, stats, resolver, op)?;
            }
            brink_format::SeqVerbOp::FilterMap => {
                seq_filter_map::<R>(flow, program, line_tables, context, stats, resolver, op)?;
            }
            brink_format::SeqVerbOp::Each => {
                seq_each::<R>(flow, program, line_tables, context, stats, resolver, op)?;
            }
            brink_format::SeqVerbOp::MapEach => {
                seq_map_each::<R>(flow, program, line_tables, context, stats, resolver, op)?;
            }
        },

        // ── NS-A8: the numeric tower (#1114) — constructors + verbs.
        // Pure: no reads, no writes, no draws; wrong-operand-type is the
        // only fault path (`tower_ops`' module doc).
        Opcode::Tower(op) => tower_ops::tower_op(flow, op)?,

        // ── NS-A7: collections+ (#1113, `docs/stdlib-spec.md` §8) —
        // `Weighted[T]` construction, the `roll` draw (an RNG-cell write
        // like every draw), and the humble heap (ordering per the §4b
        // comparison core; `heap_push` carries the dev/prod NaN entry
        // check). ───────────────────────────────────────────────────────
        Opcode::Collect(op) => match op {
            brink_format::CollectOp::WeightedNew => collection_ops::weighted_new(flow)?,
            brink_format::CollectOp::RandRoll => {
                guard_comparator_write(flow, "advanced the RNG state (a draw is a write)")?;
                note_effect_write(flow, program, DefinitionId::RNG_CELL);
                rand_ops::rand_roll::<R>(flow, context)?;
            }
            brink_format::CollectOp::HeapPush => collection_ops::heap_push(flow)?,
            brink_format::CollectOp::HeapPop => collection_ops::heap_pop(flow)?,
            brink_format::CollectOp::HeapPeek => collection_ops::heap_peek(flow)?,
        },

        // ── External functions ──────────────────────────────────────
        Opcode::CallExternal(fn_id, arg_count) => {
            // Pop arguments from the value stack.
            let mut args = Vec::with_capacity(arg_count as usize);
            for _ in 0..arg_count {
                args.push(flow.pop_value()?);
            }
            args.reverse(); // Args were pushed left-to-right, popped right-to-left.

            // Attribute the call-kind atom to the *caller* — whichever def
            // is executing right before the external frame goes on the
            // stack — mirroring `record_call_edge`'s `external_calls`
            // (recorded while walking the calling def's own body).
            note_effect_call(flow, program, fn_id);

            let current_pos = current_position(flow)?;
            let thread = flow.current_thread_mut();
            thread.call_stack.push(CallFrame {
                return_address: Some(current_pos),
                temps: args,
                container_stack: Vec::new(),
                frame_type: CallFrameType::External,
                external_fn_id: Some(fn_id),
                function_output_start: None,
            });
            stats.frames_pushed += 1;
            return Ok(Stepped::ExternalCall);
        }
    }

    Ok(Stepped::Continue)
}

// ── Bench counters (issue #821 Workstream B seed) ────────────────────────────

/// Record an Arc-clone (cheap share) event if `val` is a collection-typed
/// `Value` (`Array`/`Map`/`Record`) — called right after the `.clone()` that
/// reads a global variable ([`Opcode::GetGlobal`]), the primary way a
/// collection becomes shared between two storage slots (value-model-spec
/// §5/§6's "sharing is O(1)" claim). No-op unless the `bench-counters`
/// feature is enabled.
#[cfg(feature = "bench-counters")]
#[inline]
pub(crate) fn note_value_share(val: &Value) {
    match val {
        Value::Array(_) | Value::Map(_) | Value::Record { .. } => {
            crate::bench_counters::record_arc_clone();
        }
        _ => {}
    }
}
#[cfg(not(feature = "bench-counters"))]
#[inline(always)]
pub(crate) fn note_value_share(_val: &Value) {}

// ── Ground-truth effect-atom recorder (issue #870, T2 effects epic) ──────────
//
// `note_effect_*` attribute an observed atom to the definition scope
// (`ContainerDef::scope_id` — the nearest enclosing knot/stitch/root,
// `Program::scope_ids`/`scope_table_idx`) executing *right now*: the
// current call frame's current container position, looked up the same way
// `world.rs`'s `interior_containers_by_scope`/`expand_knot_scope` already
// do. A silent `Err`/`None` (an exhausted call stack, an unresolved scope
// table) skips recording rather than propagating — this instrumentation
// must never turn a benign state into a hard error; see
// `crate::effect_trace`'s module docs for exactly which opcodes call these
// and why (attribution at pointer/projection *construction* time, not
// dereference time, to match the static analyzer's own call-site model).
// No-op — the scope lookup itself compiles out — unless the `effect-trace`
// feature is enabled.
#[cfg(feature = "effect-trace")]
fn effect_trace_current_def(flow: &Flow, program: &Program) -> Option<DefinitionId> {
    let pos = current_position(flow).ok()?;
    let scope_idx = program.scope_table_idx(pos.container_idx) as usize;
    program.scope_ids.get(scope_idx).copied()
}

#[cfg(feature = "effect-trace")]
fn note_effect_read(flow: &Flow, program: &Program, cell: DefinitionId) {
    if let Some(def) = effect_trace_current_def(flow, program) {
        crate::effect_trace::record_read(def, cell);
    }
}
#[cfg(not(feature = "effect-trace"))]
#[inline(always)]
fn note_effect_read(_flow: &Flow, _program: &Program, _cell: DefinitionId) {}

#[cfg(feature = "effect-trace")]
fn note_effect_write(flow: &Flow, program: &Program, cell: DefinitionId) {
    if let Some(def) = effect_trace_current_def(flow, program) {
        crate::effect_trace::record_write(def, cell);
    }
}
#[cfg(not(feature = "effect-trace"))]
#[inline(always)]
fn note_effect_write(_flow: &Flow, _program: &Program, _cell: DefinitionId) {}

/// NS-A2 (issue #1108): record a content emission — but only on the
/// *visible* output channel. Pushes routed into a string-eval capture
/// (`BeginStringEval`, tag collection, function-return capture) build
/// transient values, not player-visible content, and the static `emits`
/// dimension deliberately does not model them — the observation side
/// under-approximates there, which is the sound direction for the
/// observed ⊆ declared assertion. Fragment captures (choice text, line
/// slots) are visible content in principle, but `in_capture()` skips
/// them too — the observation side under-approximates there as well
/// (same sound direction); the static harvest still declares them.
#[cfg(feature = "effect-trace")]
fn note_effect_emit(flow: &Flow, program: &Program) {
    if flow.output.in_capture() {
        return;
    }
    if let Some(def) = effect_trace_current_def(flow, program) {
        crate::effect_trace::record_emit(def);
    }
}
#[cfg(not(feature = "effect-trace"))]
#[inline(always)]
fn note_effect_emit(_flow: &Flow, _program: &Program) {}

/// NS-A2 (issue #1108): record a tag-channel touch — every `EndTag`
/// destination (line tag, fragment tag, captured choice/function tag) is a
/// tag the host can observe.
#[cfg(feature = "effect-trace")]
fn note_effect_tag(flow: &Flow, program: &Program) {
    if let Some(def) = effect_trace_current_def(flow, program) {
        crate::effect_trace::record_tag(def);
    }
}
#[cfg(not(feature = "effect-trace"))]
#[inline(always)]
fn note_effect_tag(_flow: &Flow, _program: &Program) {}

#[cfg(feature = "effect-trace")]
fn note_effect_call(flow: &Flow, program: &Program, fn_id: DefinitionId) {
    if let Some(def) = effect_trace_current_def(flow, program)
        && let Some(entry) = program.external_fn(fn_id)
    {
        crate::effect_trace::record_call(def, program.name(entry.name).to_string());
    }
}
#[cfg(not(feature = "effect-trace"))]
#[inline(always)]
fn note_effect_call(_flow: &Flow, _program: &Program, _fn_id: DefinitionId) {}

// ── Function values (T1c, docs/t1c-spec.md §3/§6, #700) ──────────────────────

/// Human-readable type name for a runtime value — used by the function-value
/// dispatch faults (mirrors the per-module `type_name` helpers).
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Bool(_) => "bool",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::DivertTarget(_) => "divert_target",
        Value::VariablePointer(_) => "var_pointer",
        Value::TempPointer { .. } => "temp_pointer",
        Value::Null => "null",
        Value::FragmentRef(_) => "fragment_ref",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Record { .. } => "record",
        Value::FnRef(_) | Value::Closure(_) => "fn",
        Value::Handle { .. } => "handle",
        Value::Projection(_) => "projection",
        Value::OptionVal(_) => "option",
        Value::Range { .. } => "range",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Vec4(_) => "vec4",
        Value::Quat(_) => "quat",
        Value::Mat2(_) => "mat2",
        Value::Mat3(_) => "mat3",
        Value::Mat4(_) => "mat4",
        Value::Weighted(_) => "weighted",
    }
}

/// Pop `n` values off the value stack, returned in **push order** — index `0`
/// is the deepest of the popped run, index `n-1` was on top. Errors on
/// underflow rather than silently truncating.
fn pop_values(flow: &mut Flow, n: usize) -> Result<Vec<Value>, RuntimeError> {
    let len = flow.value_stack.len();
    if len < n {
        return Err(RuntimeError::StackUnderflow);
    }
    Ok(flow.value_stack.split_off(len - n))
}

/// Resolve a function value to its target container index and fn token. A
/// non-function value is a `NotCallable` fault (spec §3).
fn fn_value_target_idx(v: &Value, program: &Program) -> Result<(u32, DefinitionId), RuntimeError> {
    let target = v
        .fn_target()
        .ok_or_else(|| RuntimeError::NotCallable(value_type_name(v)))?;
    let (idx, _) = program
        .resolve_target(target)
        .ok_or(RuntimeError::UnresolvedDefinition(target))?;
    Ok((idx, target))
}

fn mode_str(is_ref: bool) -> &'static str {
    if is_ref { "ref" } else { "val" }
}

/// `bind(f, supplied…)` (T1c-3, docs/t1c-spec.md §3): produce a new function
/// value with `supplied` appended to `callee`'s bound-arg row. The appended
/// entries are always `val` (the remaining params after the bound prefix are
/// val-only by construction — `ref` params are bound away at creation), taking
/// their param name from the target's signature at the appended position for
/// rehydration-check parity with `#fn`/`MakeClosure`. Faults: a non-function
/// callee (`NotCallable`); binding more args than the target has remaining
/// params (`FunctionValueArity` — over-binding, never a truncated row).
fn bind_fn_value(
    program: &Program,
    callee: &Value,
    supplied: Vec<Value>,
) -> Result<Value, RuntimeError> {
    let (idx, target) = fn_value_target_idx(callee, program)?;
    let arity = program.container(idx).param_count as usize;
    let params = program.container_params(idx);

    // Existing bound prefix (zero for a bare `FnRef`).
    let existing: &[brink_format::ClosureEnvEntry] = match callee {
        Value::Closure(c) => c.env.as_slice(),
        _ => &[],
    };
    let bound = existing.len();

    // Over-binding is a fault (§3): bound + supplied must not exceed arity.
    if bound + supplied.len() > arity {
        return Err(RuntimeError::FunctionValueArity {
            expected: arity,
            got: bound + supplied.len(),
            bound,
            supplied: supplied.len(),
        });
    }

    let mut env = Vec::with_capacity(bound + supplied.len());
    env.extend_from_slice(existing);
    for (i, payload) in supplied.into_iter().enumerate() {
        // Name/mode from the target's signature at the appended position.
        // The remaining params are val-only, so `is_ref` is false here; we
        // still read the recorded mode so a (malformed) `ref` param faults
        // cleanly at invoke via the shared rehydration check rather than
        // silently misbinding.
        let (name, is_ref) = params
            .get(bound + i)
            .map_or((brink_format::NameId(0), false), |p| (p.name, p.is_ref));
        env.push(brink_format::ClosureEnvEntry {
            name,
            is_ref,
            payload,
        });
    }

    Ok(Value::closure(target, env))
}

/// Validate a function-value call and assemble its full argument row (T1c,
/// docs/t1c-spec.md §3/§6). Runs the §6 rehydration check (each bound entry's
/// name + mode must still match the current signature at the same position),
/// the §3 arity check (`bound + supplied == declared arity`), and the §3
/// cross-flow ref-`#@local` guard, then returns the target container index,
/// its fn token, and the full argument row (bound prefix in declared order,
/// then the supplied val-only args).
///
/// Shared by in-story dispatch ([`enter_fn_value`]) and host-directed
/// evaluation ([`FlowInstance::begin_function_value_eval`](crate::story::FlowInstance::begin_function_value_eval))
/// so both paths enforce the identical fault set — never a silent misbinding
/// on one path only.
pub(crate) fn prepare_fn_value_call(
    program: &Program,
    callee: &Value,
    supplied: Vec<Value>,
) -> Result<(u32, DefinitionId, Vec<Value>), RuntimeError> {
    let (idx, target) = fn_value_target_idx(callee, program)?;
    let arity = program.container(idx).param_count as usize;
    let params = program.container_params(idx);

    let empty_env: &[brink_format::ClosureEnvEntry] = &[];
    let env = match callee {
        Value::Closure(c) => c.env.as_slice(),
        _ => empty_env,
    };

    // Rehydration validation (§6): each bound entry's name + mode must still
    // match the current signature at the same position. A closure saved against
    // an earlier compile that renamed / reordered / re-moded a param faults
    // here — a defined fault, never a silent misbinding.
    for (i, entry) in env.iter().enumerate() {
        let Some(p) = params.get(i) else {
            return Err(RuntimeError::FunctionValueRehydrationMismatch(format!(
                "bound param #{i} no longer exists on the target signature"
            )));
        };
        if p.name != entry.name || p.is_ref != entry.is_ref {
            let want = program.name_checked(p.name).unwrap_or("?");
            let got = program.name_checked(entry.name).unwrap_or("?");
            return Err(RuntimeError::FunctionValueRehydrationMismatch(format!(
                "bound param #{i} was `{got}` ({}) but the target now declares `{want}` ({})",
                mode_str(entry.is_ref),
                mode_str(p.is_ref),
            )));
        }
    }

    // Arity (§3): bound + supplied must exactly equal the declared arity.
    let bound = env.len();
    let got = bound + supplied.len();
    if got != arity {
        return Err(RuntimeError::FunctionValueArity {
            expected: arity,
            got,
            bound,
            supplied: supplied.len(),
        });
    }

    // Cross-flow ref-`#@local` fault (§3, #597): a `ref`-bound flow-private
    // cell can only be dereferenced safely from its creating flow. T1c ships
    // the fault instead of creating-flow identity, so invoking a closure that
    // `ref`-binds a `#@local` global faults — never a silent cross-flow
    // misbinding. A `ref`-bound World `VAR` is shared and invokes freely.
    for entry in env {
        if entry.is_ref
            && let Value::VariablePointer(id) = &entry.payload
            && program
                .resolve_global(*id)
                .is_some_and(|slot| program.global_is_local(slot))
        {
            return Err(RuntimeError::FunctionValueCrossFlowLocal(
                program.global_var_name(*id).unwrap_or("?").to_owned(),
            ));
        }
    }

    // Assemble the full arg row: bound prefix (declared order) then the
    // supplied val args. The target's prologue pops them (DeclareTemp, in
    // reverse) into its param slots.
    let mut full = Vec::with_capacity(bound + supplied.len());
    for entry in env {
        full.push(entry.payload.clone());
    }
    full.extend(supplied);
    Ok((idx, target, full))
}

/// Dispatch through a function value (T1c, docs/t1c-spec.md §3/§6): validate the
/// bound env against the *current* signature (rehydration), check arity, guard
/// the cross-flow ref-`#@local` fault, then push the full arg row (bound prefix
/// in declared order, then the supplied val-only args) and enter the target
/// exactly like a plain [`Call`](Opcode::Call).
fn enter_fn_value(
    flow: &mut Flow,
    program: &Program,
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    callee: &Value,
    supplied: Vec<Value>,
) -> Result<(), RuntimeError> {
    let (idx, target, full_args) = prepare_fn_value_call(program, callee, supplied)?;

    // Push the full arg row (bound prefix then supplied val args).
    for v in full_args {
        flow.value_stack.push(v);
    }

    // Enter the target (identical frame setup to `Opcode::Call`).
    let counting_flags = program.container(idx).counting_flags;
    if counting_flags.contains(CountingFlags::VISITS) {
        context.increment_visit(target);
        context.set_turn_count(target, context.turn_index());
    }
    let output_start = flow.output.target_len();
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
        function_output_start: Some(output_start),
    });
    stats.frames_pushed += 1;
    Ok(())
}

/// F34 (ruled 2026-07-19): the dev-mode world-write guard for pure-callback
/// frames. Called at each VM write seam — global assignment (direct, or
/// write-through via a `ref`-parameter pointer / path projection) and every
/// RNG-cell advance — *before* the write lands. Inside a **pure** callback
/// (`flow.pure_callback.depth > 0 && !flow.pure_callback.effectful` — a
/// `sort_by`/`sorted_by` comparator, or the pure quartet's callback since
/// issue #1679) under [`ExecMode::Dev`] the write is the turn-terminating
/// [`RuntimeError::ComparatorWroteState`] fault; under [`ExecMode::Prod`]
/// the check is skipped entirely and the write executes (defined +
/// deterministic — the stable merge-sort's comparison sequence is fixed,
/// and the fn-value verbs walk their array in iteration order). Inside an
/// **effectful** callback (`each`/`map_each`, issue #1679 slice 2) the
/// guard never fires, in either mode — world-writes are exactly what that
/// pair exists to permit (`docs/stdlib-spec.md` §4). This is NOT merely the
/// innermost scope: `flow.pure_callback.effectful` is sticky to the whole
/// ancestry ([`enter_callback_scope`]) — an `each`/`map_each` nested
/// *inside* a pure-required scope (a `map` callback, a `sort_by`
/// comparator) does not disarm the guard for that enclosing pure scope, so
/// a pure callback can't launder a world-write through a nested effectful
/// one. Outside any callback this is a single predictable depth-is-zero
/// branch on data already in `Flow` — no instrumentation threads through
/// the production write path.
///
/// Deliberately NOT guarded:
/// - visit/turn-count increments — the callback's own in-story dispatch
///   counts visits by rule (NS-A4), so a callback calling knot functions
///   stays legal in both modes;
/// - reads (`GetGlobal`) — E119's static bound owns the read posture (for
///   the verbs it gates at all); and the read half of an RMW
///   (`TakeGlobal`/`TakeTemp`-via-pointer, which transiently nulls the
///   cell): codegen pairs every take with a write-back, so the guard fires
///   at the write-back before the cell is overwritten, and the fault is
///   turn-terminating anyway;
/// - shuffle sequences — they derive a fresh RNG from `path_hash` + visit
///   count + story seed and never advance the RNG cell.
#[inline]
fn guard_comparator_write(flow: &Flow, what: &'static str) -> Result<(), RuntimeError> {
    if flow.pure_callback.depth > 0
        && !flow.pure_callback.effectful
        && flow.exec_mode == ExecMode::Dev
    {
        let verb = flow.pure_callback.verb;
        return Err(RuntimeError::ComparatorWroteState {
            verb,
            role: callback_role(verb),
            what,
        });
    }
    Ok(())
}

/// The author-facing noun for `verb`'s callee, shared by every
/// [`RuntimeError::ComparatorEscaped`]/[`RuntimeError::ComparatorWroteState`]
/// site: `"comparator"` for the NS-A4 pair (`sort_by`/`sorted_by`),
/// `"callback"` for the fn-value verb trio (`map`/`filter`/`fold`, issue
/// #1679). `call_pure_callback` and `guard_comparator_write` are shared
/// across both families — this is what keeps a `map`/`filter`/`fold`
/// author from being told they wrote a bad *comparator*.
#[inline]
fn callback_role(verb: &str) -> &'static str {
    match verb {
        "sort_by" | "sorted_by" => "comparator",
        _ => "callback",
    }
}

/// Per-comparator-call step budget for `sort_by`/`sorted_by` (NS-A4). The
/// whole sort runs inside ONE outer VM step, so the driver-level step
/// limit can't interrupt a divergent comparator — this local cap does
/// (the "VM tests must not hang" discipline). Nested comparator steps
/// still bump `stats.steps`, so they also count against the outer
/// driver's budget once the op returns.
const COMPARATOR_STEP_LIMIT: u64 = 1_000_000;

/// Maximum in-flight nested comparator evaluations (a comparator that
/// itself sorts with a comparator recurses through `step` on the Rust
/// stack — this bounds that recursion).
const COMPARATOR_DEPTH_LIMIT: u16 = 8;

/// `SeqSortedBy` (NS-A4, `docs/stdlib-spec.md` §4b, F0 ruled 2026-07-19):
/// `[a, cmp]` → `[a']` — sort by a user comparator function value
/// `fn(T, T): int` (negative = less, zero = tie, positive = greater).
/// Stable; the §4b guarantee floor ("some permutation of the input, never
/// worse") holds by construction. One op serves `sort_by` (statement-only,
/// RMW write-back) and `sorted_by` (functional), so faults name `sort_by`.
///
/// No NaN pre-scan here (F14: `sort_by` does not inherit `F:float` — the
/// comparator owns the element semantics); the comparator's own faults
/// propagate as turn-terminating faults, and comparator misbehavior the VM
/// can observe (choices, `-> DONE`/`-> END`, external calls, divergence)
/// is [`RuntimeError::ComparatorEscaped`].
fn seq_sorted_by<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
) -> Result<(), RuntimeError> {
    let cmp = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "sort_by",
            expected: "an array",
            found: value_type_name(&container),
        });
    };
    if !matches!(cmp, Value::FnRef(_) | Value::Closure(_)) {
        return Err(RuntimeError::ComparatorNotAFunction {
            verb: "sort_by",
            found: value_type_name(&cmp),
        });
    }
    let outer = enter_pure_callback(flow, "sort_by")?;
    let mut sorted: Vec<Value> = items.as_ref().clone();
    let result = collection_ops::fallible_stable_sort(&mut sorted, &mut |a, b| {
        call_comparator::<R>(
            flow,
            program,
            line_tables,
            context,
            stats,
            resolver,
            &cmp,
            a.clone(),
            b.clone(),
        )
    });
    flow.pure_callback = outer;
    result?;
    flow.value_stack.push(Value::array(sorted));
    Ok(())
}

/// Enter a pure-callback scope for `verb`: check the nesting-depth bound,
/// bump the depth, and return the caller's [`PureCallbackState`] so the
/// scope can be closed by restoring it (`flow.pure_callback = outer`) on
/// every exit path — including the error paths, which is why this returns
/// the saved state rather than relying on a matching decrement.
fn enter_pure_callback(
    flow: &mut Flow,
    verb: &'static str,
) -> Result<PureCallbackState, RuntimeError> {
    enter_callback_scope(flow, verb, false)
}

/// Shared body of [`enter_pure_callback`] (`sort_by`'s comparator) and
/// every `SeqVerb` op (`map`/`filter`/`fold`/`filter_map`/`each`/
/// `map_each`, via [`seq_map`]/[`seq_filter`]/[`seq_fold`]/
/// [`seq_filter_map`]/[`seq_each`]/[`seq_map_each`]) — the nesting-depth
/// check and the state swap are identical for every caller; only the
/// `effectful` bit differs. The `SeqVerb` family calls this directly with
/// [`SeqVerbOp::is_effectful`](brink_format::SeqVerbOp::is_effectful),
/// which single-sources the pure/effectful classification: there is
/// exactly one place a new `SeqVerbOp` variant's contract can be gotten
/// wrong.
///
/// Purity is **sticky**: an `each`/`map_each` scope entered *inside* a
/// pure-required scope (`sort_by`'s comparator, or the pure quartet's
/// callback) does not disarm [`guard_comparator_write`] for the enclosing
/// scope. Without this, `map(a, f)` with an opaque `f` (routed through a
/// variable — exactly the case E119 cannot prove, which is why the dev-mode
/// guard exists at all) whose body calls `each(b, g)` with a writing `g`
/// would perform a real world-write inside a pure-required `map` callback
/// under Dev with no fault, because [`enter_callback_scope`] would simply
/// overwrite `flow.pure_callback` with the inner (effectful) scope. So the
/// *effective* effectful bit is `effectful && outer.effectful` whenever
/// there is an enclosing scope (`outer.depth > 0`) — an inner scope can
/// only be effectful if every enclosing scope is too. A top-level entry
/// (`outer.depth == 0`, no enclosing scope) is unaffected and keeps
/// whichever bit it was called with.
fn enter_callback_scope(
    flow: &mut Flow,
    verb: &'static str,
    effectful: bool,
) -> Result<PureCallbackState, RuntimeError> {
    if flow.pure_callback.depth >= COMPARATOR_DEPTH_LIMIT {
        return Err(RuntimeError::ComparatorEscaped {
            verb,
            role: callback_role(verb),
            what: "recursed past the nesting depth limit",
        });
    }
    let outer = flow.pure_callback;
    let effective_effectful = effectful && (outer.depth == 0 || outer.effectful);
    flow.pure_callback = PureCallbackState {
        depth: outer.depth + 1,
        verb,
        effectful: effective_effectful,
    };
    Ok(outer)
}

/// Pop and validate the `(array, callback)` operand pair every fn-value
/// verb shares (`docs/stdlib-spec.md` §4, issue #1679). The callback is on
/// top — codegen pushes the array first, exactly like `SeqSortedBy`.
fn pop_seq_and_callback(
    flow: &mut Flow,
    verb: &'static str,
    expected: &'static str,
) -> Result<(Vec<Value>, Value), RuntimeError> {
    let f = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb,
            expected: "an array",
            found: value_type_name(&container),
        });
    };
    if !matches!(f, Value::FnRef(_) | Value::Closure(_)) {
        return Err(RuntimeError::CallbackNotAFunction {
            verb,
            expected,
            found: value_type_name(&f),
        });
    }
    Ok((items.as_ref().clone(), f))
}

/// `SeqVerb(Map)` (`docs/stdlib-spec.md` §4, issue #1679): `[a, f]` →
/// `[a']` — the array of `f(x)` for each element, in iteration order.
///
/// The callback is pure-required by the 2026-07-18 ruling, which is what
/// makes the iteration order unobservable and licenses fusion; the runtime
/// nevertheless walks the array front-to-back, so the one fusion-visible
/// artifact §4 leaves unspecified (which element's fault fires first) is
/// simply "the earliest one" here.
fn seq_map<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    op: brink_format::SeqVerbOp,
) -> Result<(), RuntimeError> {
    const VERB: &str = "map";
    let (items, f) = pop_seq_and_callback(flow, VERB, "`fn(T): U`")?;
    let outer = enter_callback_scope(flow, VERB, op.is_effectful())?;
    let mut out = Vec::with_capacity(items.len());
    let result = (|| -> Result<(), RuntimeError> {
        for item in items {
            out.push(call_pure_callback::<R>(
                flow,
                program,
                line_tables,
                context,
                stats,
                resolver,
                VERB,
                &f,
                vec![item],
            )?);
        }
        Ok(())
    })();
    flow.pure_callback = outer;
    result?;
    flow.value_stack.push(Value::array(out));
    Ok(())
}

/// `SeqVerb(Filter)` (`docs/stdlib-spec.md` §4, issue #1679): `[a, pred]` →
/// `[a']` — the elements for which `pred(x)` is `true`, in iteration order.
/// A non-bool predicate return is a turn-terminating fault: a silent
/// truthiness coercion here would quietly change which elements survive.
fn seq_filter<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    op: brink_format::SeqVerbOp,
) -> Result<(), RuntimeError> {
    const VERB: &str = "filter";
    let (items, pred) = pop_seq_and_callback(flow, VERB, "`fn(T): bool`")?;
    let outer = enter_callback_scope(flow, VERB, op.is_effectful())?;
    let mut out = Vec::new();
    let result = (|| -> Result<(), RuntimeError> {
        for item in items {
            let keep = call_pure_callback::<R>(
                flow,
                program,
                line_tables,
                context,
                stats,
                resolver,
                VERB,
                &pred,
                vec![item.clone()],
            )?;
            match keep {
                Value::Bool(true) => out.push(item),
                Value::Bool(false) => {}
                other => {
                    return Err(RuntimeError::CallbackReturnType {
                        verb: VERB,
                        expected: "a bool",
                        found: value_type_name(&other),
                    });
                }
            }
        }
        Ok(())
    })();
    flow.pure_callback = outer;
    result?;
    flow.value_stack.push(Value::array(out));
    Ok(())
}

/// `SeqVerb(Fold)` (`docs/stdlib-spec.md` §4, issue #1679): `[a, init, f]`
/// → `[acc]` — the left fold. `acc` starts at `init` and becomes
/// `f(acc, x)` for each element in iteration order; an empty array yields
/// `init` unchanged (no absence case, so no `Option` — contrast `min`/`max`
/// under the §4 absence-returns doctrine).
fn seq_fold<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    op: brink_format::SeqVerbOp,
) -> Result<(), RuntimeError> {
    const VERB: &str = "fold";
    // Operand order at the op is `seq`, `init`, `f` — the callback is on
    // top, so the shared pair-popper cannot be reused directly: pop the
    // callback, then the init, then validate the array underneath.
    let f = flow.pop_value()?;
    let init = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb: VERB,
            expected: "an array",
            found: value_type_name(&container),
        });
    };
    if !matches!(f, Value::FnRef(_) | Value::Closure(_)) {
        return Err(RuntimeError::CallbackNotAFunction {
            verb: VERB,
            expected: "`fn(U, T): U`",
            found: value_type_name(&f),
        });
    }
    let items = items.as_ref().clone();
    let outer = enter_callback_scope(flow, VERB, op.is_effectful())?;
    let mut acc = init;
    let result = (|| -> Result<(), RuntimeError> {
        for item in items {
            acc = call_pure_callback::<R>(
                flow,
                program,
                line_tables,
                context,
                stats,
                resolver,
                VERB,
                &f,
                vec![acc.clone(), item],
            )?;
        }
        Ok(())
    })();
    flow.pure_callback = outer;
    result?;
    flow.value_stack.push(acc);
    Ok(())
}

/// `SeqVerb(FilterMap)` (`docs/stdlib-spec.md` §4, issue #1679 slice 2):
/// `[a, f]` → `[a']` — the Option-mapper: `f(x)` for each element, kept
/// unwrapped when `some(v)`, dropped when `none`, in iteration order. Pure
/// callback, same contract as `map`/`filter`/`fold`; a non-Option return is
/// a turn-terminating fault, exactly like `filter`'s non-bool predicate
/// return — coercing here would silently change which elements survive.
fn seq_filter_map<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    op: brink_format::SeqVerbOp,
) -> Result<(), RuntimeError> {
    const VERB: &str = "filter_map";
    let (items, f) = pop_seq_and_callback(flow, VERB, "`fn(T): Option[U]`")?;
    let outer = enter_callback_scope(flow, VERB, op.is_effectful())?;
    let mut out = Vec::new();
    let result = (|| -> Result<(), RuntimeError> {
        for item in items {
            let mapped = call_pure_callback::<R>(
                flow,
                program,
                line_tables,
                context,
                stats,
                resolver,
                VERB,
                &f,
                vec![item],
            )?;
            match mapped {
                Value::OptionVal(Some(inner)) => out.push((*inner).clone()),
                Value::OptionVal(None) => {}
                other => {
                    return Err(RuntimeError::CallbackReturnType {
                        verb: VERB,
                        expected: "an Option",
                        found: value_type_name(&other),
                    });
                }
            }
        }
        Ok(())
    })();
    flow.pure_callback = outer;
    result?;
    flow.value_stack.push(Value::array(out));
    Ok(())
}

/// `SeqVerb(Each)` (`docs/stdlib-spec.md` §4, issue #1679 slice 2): `[a, f]`
/// → `[null]` — the effectful "do something per element, no result"
/// spelling: `f(x)` runs once per element, in iteration order, for its side
/// effects; the return value is discarded. Sequential and never fused, by
/// rule (not by construction the way the pure quartet's fusion license
/// works — `each`'s whole point is that side-effect order IS observable).
///
/// **Effectful**, not pure: entering the scope with
/// [`SeqVerbOp::is_effectful`](brink_format::SeqVerbOp::is_effectful)
/// `true` (via [`enter_callback_scope`]) disarms [`guard_comparator_write`]
/// for this callback's world-writes, and [`call_effectful_callback`] lets
/// its output reach the transcript instead of capturing and discarding it.
/// What the pure quartet's callback may never do is exactly what `each`'s
/// callback exists to do.
fn seq_each<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    op: brink_format::SeqVerbOp,
) -> Result<(), RuntimeError> {
    const VERB: &str = "each";
    let (items, f) = pop_seq_and_callback(flow, VERB, "`fn(T)`")?;
    let outer = enter_callback_scope(flow, VERB, op.is_effectful())?;
    let result = (|| -> Result<(), RuntimeError> {
        for item in items {
            call_effectful_callback::<R>(
                flow,
                program,
                line_tables,
                context,
                stats,
                resolver,
                VERB,
                &f,
                vec![item],
            )?;
        }
        Ok(())
    })();
    flow.pure_callback = outer;
    result?;
    flow.value_stack.push(Value::Null);
    Ok(())
}

/// `SeqVerb(MapEach)` (`docs/stdlib-spec.md` §4, issue #1679 slice 2):
/// `[a, f]` → `[a']` — `map`'s effectful twin: the array of `f(x)` for each
/// element, in iteration order, sequential and never fused; unlike `map`,
/// `f` may write globals and emit output. See [`seq_each`]'s doc for the
/// effectful-vs-pure contract split.
fn seq_map_each<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    op: brink_format::SeqVerbOp,
) -> Result<(), RuntimeError> {
    const VERB: &str = "map_each";
    let (items, f) = pop_seq_and_callback(flow, VERB, "`fn(T): U`")?;
    let outer = enter_callback_scope(flow, VERB, op.is_effectful())?;
    let mut out = Vec::with_capacity(items.len());
    let result = (|| -> Result<(), RuntimeError> {
        for item in items {
            out.push(call_effectful_callback::<R>(
                flow,
                program,
                line_tables,
                context,
                stats,
                resolver,
                VERB,
                &f,
                vec![item],
            )?);
        }
        Ok(())
    })();
    flow.pure_callback = outer;
    result?;
    flow.value_stack.push(Value::array(out));
    Ok(())
}

/// Evaluate a `sort_by`/`sorted_by` comparator against one pair of
/// comparands and interpret its return as an [`Ordering`](core::cmp::Ordering)
/// (F0's ruled shape: negative = less, zero = tie, positive = greater). A
/// non-int return is [`RuntimeError::ComparatorReturnType`]; everything else
/// is [`call_pure_callback`]'s contract.
#[expect(
    clippy::too_many_arguments,
    reason = "the VM environment (the step signature) plus the callee and comparands"
)]
fn call_comparator<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    cmp: &Value,
    a: Value,
    b: Value,
) -> Result<core::cmp::Ordering, RuntimeError> {
    const VERB: &str = "sort_by";
    let ret = call_pure_callback::<R>(
        flow,
        program,
        line_tables,
        context,
        stats,
        resolver,
        VERB,
        cmp,
        vec![a, b],
    )?;
    match ret {
        Value::Int(i) => Ok(i.cmp(&0)),
        other => Err(RuntimeError::ComparatorReturnType {
            verb: VERB,
            found: value_type_name(&other),
        }),
    }
}

/// Evaluate a **pure** callback function value against one argument row —
/// the NS-A4 comparator verbs and the pure quartet
/// (`map`/`filter`/`fold`/`filter_map`, issue #1679): output is captured
/// and discarded (silent by contract; the checker enforces it where the
/// callback's origin is provable — E119 — and this isolation is the
/// gradual-mode residual, mirroring `begin_function_eval`). Thin wrapper
/// over [`call_callback`] with `capture_output: true`; see that function's
/// doc for the shared re-entrancy mechanics.
///
/// The caller is responsible for having entered a pure-callback scope
/// ([`enter_pure_callback`]) — that is what bounds nesting depth and arms
/// the dev-mode world-write guard.
#[expect(
    clippy::too_many_arguments,
    reason = "the VM environment (the step signature) plus the verb, callee and argument row"
)]
fn call_pure_callback<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    verb: &'static str,
    callee: &Value,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    call_callback::<R>(
        flow,
        program,
        line_tables,
        context,
        stats,
        resolver,
        verb,
        callee,
        args,
        true,
    )
}

/// Evaluate an **effectful** callback function value against one argument
/// row — `each`/`map_each` (issue #1679 slice 2): output reaches the
/// transcript, exactly like an ordinary in-story function call, instead of
/// being captured and discarded. Thin wrapper over [`call_callback`] with
/// `capture_output: false`.
///
/// The caller is responsible for having entered an effectful-callback scope
/// ([`enter_callback_scope`] with `effectful: true`) — that is what bounds
/// nesting depth and disarms the dev-mode world-write guard for this scope.
#[expect(
    clippy::too_many_arguments,
    reason = "the VM environment (the step signature) plus the verb, callee and argument row"
)]
fn call_effectful_callback<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    verb: &'static str,
    callee: &Value,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    call_callback::<R>(
        flow,
        program,
        line_tables,
        context,
        stats,
        resolver,
        verb,
        callee,
        args,
        false,
    )
}

/// The re-entrancy seam shared by every fn-value verb family (NS-A4's
/// comparator pair, the pure quartet, and the effectful pair) and by
/// [`call_comparator`]: push a boundary frame (`FunctionEvalFromGame`,
/// `return_address: None` — the `begin_function_eval` shape), drive [`step`]
/// until the frame pops, and read the return value off the value stack.
/// `capture_output` selects which of the two runtime contracts this call
/// runs under — `true` isolates output (the pure quartet's callback is
/// silent by contract), `false` lets it reach the transcript (`each`/
/// `map_each`, issue #1679 slice 2, whose whole point is that effects are
/// visible). Nothing else about the mechanics differs: one seam, one set of
/// bounds, so the families cannot drift apart by accident.
///
/// In-story dispatch semantics apply (visit counting, exactly like
/// `enter_fn_value`) regardless of `capture_output`; callback behavior the
/// VM cannot honor mid-op — choices, `-> DONE`/`-> END`, external calls
/// (there is no handler down here), divergence past
/// [`COMPARATOR_STEP_LIMIT`] — is a turn-terminating
/// [`RuntimeError::ComparatorEscaped`] fault, as is returning nothing at
/// all, for both contracts: that limitation is architectural (no handler
/// exists down here), not a purity rule, so being effectful doesn't lift it.
///
/// The caller is responsible for having entered the matching callback scope
/// ([`enter_pure_callback`], or [`enter_callback_scope`] directly with
/// `effectful: true`) — that is what bounds nesting depth and sets the
/// dev-mode world-write guard's posture.
#[expect(
    clippy::too_many_arguments,
    reason = "the VM environment (the step signature) plus the verb, callee, argument row and \
              the output-capture switch"
)]
fn call_callback<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    stats: &mut Stats,
    resolver: Option<&dyn PluralResolver>,
    verb: &'static str,
    callee: &Value,
    args: Vec<Value>,
    capture_output: bool,
) -> Result<Value, RuntimeError> {
    let (container_idx, target, full_args) = prepare_fn_value_call(program, callee, args)?;

    let value_floor = flow.value_stack.len();
    let choice_floor = flow.pending_choices.len();
    let thread_floor = flow.threads.len();

    // Pure contract: isolate output — anything the callback emits routes to
    // the capture scratch space and never reaches the transcript. Effectful
    // contract: skip the capture entirely, so `OutputBuffer::target`
    // routes straight to the transcript, same as an ordinary function call.
    if capture_output {
        flow.output.begin_capture();
    }
    let output_start = flow.output.target_len();

    // In-story dispatch counts visits, exactly like `enter_fn_value`.
    let counting_flags = program.container(container_idx).counting_flags;
    if counting_flags.contains(CountingFlags::VISITS) {
        context.increment_visit(target);
        context.set_turn_count(target, context.turn_index());
    }

    let depth_floor = flow.current_thread().call_stack.len();
    flow.current_thread_mut().call_stack.push(CallFrame {
        return_address: None,
        temps: Vec::new(),
        container_stack: vec![ContainerPosition {
            container_idx,
            offset: 0,
        }],
        frame_type: CallFrameType::FunctionEvalFromGame,
        external_fn_id: None,
        function_output_start: Some(output_start),
    });
    stats.frames_pushed += 1;
    for v in full_args {
        flow.value_stack.push(v);
    }

    let role = callback_role(verb);
    let mut steps = 0u64;
    let outcome: Result<(), RuntimeError> = loop {
        steps += 1;
        stats.steps += 1;
        if steps > COMPARATOR_STEP_LIMIT {
            break Err(RuntimeError::ComparatorEscaped {
                verb,
                role,
                what: "exceeded the nested evaluation step budget",
            });
        }
        let stepped = match step::<R>(flow, program, line_tables, context, stats, resolver) {
            Ok(s) => s,
            Err(e) => break Err(e),
        };
        match stepped {
            Stepped::Done | Stepped::Ended => {
                break Err(RuntimeError::ComparatorEscaped {
                    verb,
                    role,
                    what: "reached `-> DONE`/`-> END`",
                });
            }
            Stepped::ExternalCall => {
                break Err(RuntimeError::ComparatorEscaped {
                    verb,
                    role,
                    what: "called an external function",
                });
            }
            Stepped::Continue | Stepped::ThreadCompleted => {}
        }
        if flow.pending_choices.len() > choice_floor {
            break Err(RuntimeError::ComparatorEscaped {
                verb,
                role,
                what: "presented a choice",
            });
        }
        // Boundary frame popped (and any forked threads unwound) — the
        // comparator has returned.
        if flow.threads.len() <= thread_floor
            && flow.current_thread().call_stack.len() <= depth_floor
        {
            break Ok(());
        }
    };
    // Pure contract: end the capture on every path — discard whatever the
    // callback emitted (silent by contract; see the fn docs). Effectful
    // contract: nothing to end — output already landed in the transcript.
    if capture_output {
        let _captured = flow.output.end_capture(program, line_tables, resolver);
    }
    outcome?;

    let mut ret: Option<Value> = None;
    while flow.value_stack.len() > value_floor {
        let v = flow.value_stack.pop();
        if ret.is_none() {
            ret = v;
        }
    }
    ret.ok_or_else(|| {
        // Pre-#1679, a comparator that fell off without returning was
        // `ComparatorReturnType { found: "no return value" }` — folding it
        // into the shared `ComparatorEscaped` "returned no value" case
        // (below) would be an unannounced wording change on an
        // already-shipped fault. Keep `sort_by`/`sorted_by` on the old
        // variant; the trio's callbacks (first release, no established
        // wording to preserve) take the shared `ComparatorEscaped` path.
        if role == "comparator" {
            RuntimeError::ComparatorReturnType {
                verb,
                found: "no return value",
            }
        } else {
            RuntimeError::ComparatorEscaped {
                verb,
                role,
                what: "returned no value",
            }
        }
    })
}

fn resolve_line(
    program: &Program,
    line_tables: &[Vec<LineEntry>],
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

    let scope_idx = program.scope_table_idx(pos.container_idx) as usize;
    let lines = &line_tables[scope_idx];
    let Some(entry) = lines.get(idx as usize) else {
        return Ok(String::new());
    };

    match &entry.content {
        LineContent::Plain(s) => Ok(s.clone()),
        LineContent::Template(parts) => Ok(resolve_line_parts(parts, program, &slots, resolver)),
    }
}

/// Resolve a sequence of `LinePart`s to flat text — shared by
/// `resolve_line` (a `Template`'s own parts) and recursively by a
/// [`LinePart::Span`]'s `children`.
///
/// A span is presentational (§4.3) and this runtime's current public API
/// has no structured span surface yet (`docs/prose-dialect-spec.md`
/// §7/§9.1 ⏳) — same posture `output/mod.rs`'s twin
/// `resolve_line_parts` documents — so it resolves to its children's
/// concatenated text, tag name/attrs stripped.
fn resolve_line_parts(
    parts: &[LinePart],
    program: &Program,
    slots: &[Value],
    resolver: Option<&dyn PluralResolver>,
) -> String {
    let mut result = String::new();
    for part in parts {
        match part {
            LinePart::Literal(s) => result.push_str(s),
            LinePart::Slot(n) => {
                if let Some(val) = slots.get(*n as usize) {
                    // B4 (`docs/stdlib-spec.md` §1.6b): this is a
                    // template-slot display boundary — same
                    // forgiveness as `output/mod.rs`'s
                    // `resolve_line_ref`. Currently unreachable in
                    // production (`lir::Stmt::EvalLine`, this
                    // function's only caller, is never constructed
                    // by any lowering path — choice display goes
                    // through `EmitLine` + `Fragment` instead), but
                    // routed through the same seam so it can't
                    // silently diverge if that ever changes.
                    result.push_str(&value_ops::stringify_display(val, program));
                }
            }
            LinePart::Select {
                slot,
                variants,
                default,
            } => {
                let text = resolve_select(*slot, variants, default, slots, resolver);
                result.push_str(text);
            }
            LinePart::Span { children, .. } => {
                result.push_str(&resolve_line_parts(children, program, slots, resolver));
            }
        }
    }
    result
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
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    stats: &mut Stats,
    frame_type: CallFrameType,
) -> Result<Stepped, RuntimeError> {
    // Classify *why*, from the exhausted frame's shape right now — before
    // anything below pops it (issue #1993). This is only meaningful for a
    // `Done` returned from *this* call: that is the sole case the deferred
    // `RanOutOfContent` fault (one `continue_single` later) can be
    // reporting on. It must NOT be written to `flow` on a branch that
    // resumes execution (a completed thread with more to run, a popped
    // frame with content still below it) — doing so unconditionally would
    // let a transient exhaustion elsewhere on the same flow (e.g. a
    // `Story::call_function` boundary evaluating a function that calls a
    // void helper) clobber a cause an *earlier*, still-pending exhaustion
    // already recorded, which is then read stale by a later, unrelated
    // `Done`. So: compute it now, but stash it on `flow` only at each
    // `return Ok(Stepped::Done)` below.
    let can_pop = flow.current_thread().call_stack.len() > 1;
    let cause = classify_ran_out_of_content(frame_type, can_pop);

    if frame_type == CallFrameType::Thread {
        // Thread boundary exhausted — thread is done. Pop it without
        // touching inherited frames below. ThreadCall always creates a
        // child thread, so can_pop_thread is expected to be true.
        if flow.can_pop_thread() {
            flow.pop_thread();
            stats.threads_completed += 1;
            return Ok(Stepped::ThreadCompleted);
        }
        flow.ran_out_of_content_cause = cause;
        return Ok(Stepped::Done);
    }

    if !matches!(
        frame_type,
        CallFrameType::Function | CallFrameType::FunctionEvalFromGame
    ) && !flow.pending_choices.is_empty()
    {
        // Non-function frame with pending choices: the fork captured at
        // choice creation preserves the state for resumption.
        if flow.can_pop_thread() {
            flow.pop_thread();
            stats.threads_completed += 1;
            return Ok(Stepped::ThreadCompleted);
        }
        flow.ran_out_of_content_cause = cause;
        return Ok(Stepped::Done);
    }

    pop_call_frame(flow, program, line_tables, resolver, stats, false)?;
    if flow.current_thread().call_stack.is_empty() {
        if flow.can_pop_thread() {
            flow.pop_thread();
            stats.threads_completed += 1;
            return Ok(Stepped::ThreadCompleted);
        }
        flow.ran_out_of_content_cause = cause;
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
    _program: &Program,
    _line_tables: &[Vec<LineEntry>],
    _resolver: Option<&dyn PluralResolver>,
    stats: &mut Stats,
    is_explicit_return: bool,
) -> Result<(), RuntimeError> {
    let thread = flow.current_thread_mut();
    let popped = thread
        .call_stack
        .pop()
        .ok_or(RuntimeError::CallStackUnderflow)?;
    stats.frames_popped += 1;

    if matches!(
        popped.frame_type,
        CallFrameType::Function | CallFrameType::FunctionEvalFromGame
    ) {
        // Trim trailing whitespace from the function's output region,
        // matching the C# runtime's TrimWhitespaceFromFunctionEnd.
        if let Some(start) = popped.function_output_start {
            flow.output.trim_function_end(start);
        }
        if !is_explicit_return {
            // Implicit return: function returns void.
            flow.value_stack.push(Value::Null);
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

/// Transfer control to a divert target within the current call frame,
/// incrementing visit/turn counts per the target container's counting flags.
/// Used by the `Goto`/`GotoIf`/`GotoVariable` opcodes, and by
/// `FlowInstance::choose_path_string` so a host-directed jump behaves
/// exactly like an in-story `-> target` divert.
pub(crate) fn goto_target(
    flow: &mut Flow,
    program: &Program,
    context: &mut (impl ContextAccess + ?Sized),
    id: DefinitionId,
) -> Result<(), RuntimeError> {
    let (container_idx, byte_offset) = program
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
    let counting_flags = program.container(container_idx).counting_flags;
    if counting_flags.contains(CountingFlags::VISITS) {
        let should_count = if already_on_stack {
            counting_flags.contains(CountingFlags::COUNT_START_ONLY) && byte_offset == 0
        } else {
            true
        };
        if should_count {
            context.increment_visit(id);
            context.set_turn_count(id, context.turn_index());
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

fn handle_begin_choice(
    flow: &mut Flow,
    program: &Program,
    context: &mut (impl ContextAccess + ?Sized),
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
        if !value_ops::is_truthy(&condition)? {
            if has_display {
                let _ = flow.value_stack.pop();
            }
            flow.skipping_choice = true;
            return Ok(());
        }
    }

    // 1b. Once-only check: skip if the target container was already visited.
    if flags.once_only {
        let visit_count = context.visit_count(target_id);
        if visit_count > 0 {
            if has_display {
                let _ = flow.value_stack.pop();
            }
            flow.skipping_choice = true;
            return Ok(());
        }
    }

    // 2. Pop the display value.
    let display = if has_display {
        match flow.value_stack.pop() {
            Some(Value::FragmentRef(idx)) => {
                // Pull any tags stored on the fragment into current_tags
                // so they end up on the PendingChoice.
                if let Some(frag_tags) = flow.output.fragment_tags(idx) {
                    flow.current_tags.extend(frag_tags.iter().cloned());
                }
                crate::story::ChoiceDisplay::Fragment(idx)
            }
            Some(Value::String(s)) => crate::story::ChoiceDisplay::Text((*s).to_owned()),
            // B4 (`docs/stdlib-spec.md` §1.6b): a choice's display value is
            // itself a display boundary. Currently unreachable for an
            // `Option` in practice — current codegen always produces a
            // `Value::FragmentRef` or `Value::String` here, never a bare
            // `Option` — but routed through `stringify_display` so this
            // fallback can't silently diverge from the interpolation
            // boundary if that ever changes.
            Some(other) => {
                crate::story::ChoiceDisplay::Text(value_ops::stringify_display(&other, program))
            }
            None => crate::story::ChoiceDisplay::Text(String::new()),
        }
    } else {
        crate::story::ChoiceDisplay::Text(String::new())
    };

    let (target_idx, target_offset) = program
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
    let tags = mem::take(&mut flow.current_tags);
    flow.pending_choices.push(PendingChoice {
        display,
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

fn handle_sequence<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    context: &mut (impl ContextAccess + ?Sized),
    kind: brink_format::SequenceKind,
    count: u8,
) -> Result<(), RuntimeError> {
    if kind == brink_format::SequenceKind::Shuffle {
        return handle_shuffle_sequence::<R>(flow, program, context);
    }

    // Non-shuffle sequences: pop divert target, use visit count.
    let val = flow.pop_value()?;
    let visit_count = if let Value::DivertTarget(id) = val {
        context.visit_count(id)
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
fn handle_shuffle_sequence<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    program: &Program,
    context: &mut (impl ContextAccess + ?Sized),
) -> Result<(), RuntimeError> {
    // Get path_hash from the current container.
    let pos = current_position(flow)?;
    let path_hash = program.container(pos.container_idx).path_hash;
    handle_shuffle_with_hash::<R>(flow, context, path_hash)
}

/// The shuffle-selection core, parameterized by the seeding `path_hash` —
/// the current container's for [`Opcode::Sequence`]`(Shuffle)`, the named
/// container's for [`Opcode::ShuffleIndexOf`] (#3273). One implementation,
/// so the two spellings cannot drift.
#[expect(clippy::cast_sign_loss)]
fn handle_shuffle_with_hash<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
    path_hash: i32,
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

    // Seed RNG with path_hash + loopIndex + story_seed (matching reference).
    let seed = path_hash
        .wrapping_add(loop_index)
        .wrapping_add(context.rng_seed());

    // Pre-generate all needed random values from a single seeded RNG instance.
    let random_values = context.random_sequence::<R>(seed, (iteration_index + 1) as usize);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;
    use crate::story::PendingTerminal;

    /// A bare `Flow` for exercising [`guard_comparator_write`] — only
    /// `pure_callback` and `exec_mode` matter to the guard.
    fn test_flow() -> Flow {
        Flow {
            threads: Vec::new(),
            value_stack: Vec::new(),
            output: OutputBuffer::new(),
            pending_choices: Vec::new(),
            current_tags: Vec::new(),
            in_tag: false,
            skipping_choice: false,
            did_safe_exit: false,
            did_unsafe_yield: false,
            ran_out_of_content_cause: crate::RanOutOfContentCause::default(),
            exec_mode: ExecMode::default(),
            pure_callback: crate::story::PureCallbackState::default(),
            next_block_id: 0,
            pending_terminal: PendingTerminal::default(),
        }
    }

    // ── F34: the comparator write-guard seam ─────────────────────────────

    #[test]
    fn guard_is_inert_outside_a_comparator_in_both_modes() {
        let mut flow = test_flow();
        assert_eq!(flow.exec_mode, ExecMode::Dev, "dev is the default");
        assert!(guard_comparator_write(&flow, "assigned a global variable").is_ok());
        flow.exec_mode = ExecMode::Prod;
        assert!(guard_comparator_write(&flow, "assigned a global variable").is_ok());
    }

    #[test]
    fn guard_faults_inside_a_comparator_under_dev() {
        let mut flow = test_flow();
        flow.pure_callback = PureCallbackState {
            depth: 1,
            verb: "sort_by",
            effectful: false,
        };
        let err = guard_comparator_write(&flow, "assigned a global variable").unwrap_err();
        assert!(
            matches!(
                err,
                RuntimeError::ComparatorWroteState {
                    verb: "sort_by",
                    role: "comparator",
                    what: "assigned a global variable",
                }
            ),
            "{err:?}"
        );
    }

    /// The guard reports the verb whose callback is actually running — the
    /// fn-value verbs (#1679) share the seam with the NS-A4 comparator, so
    /// a `map` callback's write must not be blamed on `sort_by`.
    #[test]
    fn guard_names_the_fn_value_verb_whose_callback_is_running() {
        let mut flow = test_flow();
        flow.pure_callback = PureCallbackState {
            depth: 1,
            verb: "map",
            effectful: false,
        };
        let err =
            guard_comparator_write(&flow, "advanced the random number generator").unwrap_err();
        assert!(
            matches!(
                err,
                RuntimeError::ComparatorWroteState {
                    verb: "map",
                    role: "callback",
                    what: "advanced the random number generator",
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn guard_is_skipped_inside_a_comparator_under_prod() {
        let mut flow = test_flow();
        flow.pure_callback = PureCallbackState {
            depth: 1,
            verb: "sort_by",
            effectful: false,
        };
        flow.exec_mode = ExecMode::Prod;
        assert!(guard_comparator_write(&flow, "assigned a global variable").is_ok());
    }

    /// [`enter_pure_callback`] returns the caller's state so a nested scope
    /// restores rather than blindly decrements — and refuses to nest past
    /// the depth bound (the "VM tests must not hang" recursion guard).
    #[test]
    fn enter_pure_callback_nests_and_bounds() {
        let mut flow = test_flow();
        let outer = enter_pure_callback(&mut flow, "map").unwrap();
        assert_eq!(outer.depth, 0);
        assert_eq!(flow.pure_callback.depth, 1);
        assert_eq!(flow.pure_callback.verb, "map");

        let inner = enter_pure_callback(&mut flow, "fold").unwrap();
        assert_eq!(inner.depth, 1);
        assert_eq!(flow.pure_callback.verb, "fold");
        flow.pure_callback = inner;
        assert_eq!(flow.pure_callback.verb, "map", "the outer verb is restored");
        flow.pure_callback = outer;
        assert_eq!(flow.pure_callback.depth, 0);

        flow.pure_callback = PureCallbackState {
            depth: COMPARATOR_DEPTH_LIMIT,
            verb: "filter",
            effectful: false,
        };
        let err = enter_pure_callback(&mut flow, "filter").unwrap_err();
        assert!(
            matches!(
                err,
                RuntimeError::ComparatorEscaped {
                    verb: "filter",
                    role: "callback",
                    what: "recursed past the nesting depth limit",
                }
            ),
            "{err:?}"
        );
    }

    /// The effectful pair's whole point (issue #1679 slice 2): a world-write
    /// inside `each`/`map_each`'s callback must NOT fault, in either mode —
    /// contrast [`guard_faults_inside_a_comparator_under_dev`], which proves
    /// the exact same write DOES fault for a pure callback at the same
    /// depth. Only `effectful` differs between the two tests.
    #[test]
    fn guard_is_disarmed_inside_an_effectful_callback_in_both_modes() {
        let mut flow = test_flow();
        flow.pure_callback = PureCallbackState {
            depth: 1,
            verb: "each",
            effectful: true,
        };
        assert!(
            guard_comparator_write(&flow, "assigned a global variable").is_ok(),
            "each's world-writes must be legal under dev mode"
        );
        flow.exec_mode = ExecMode::Prod;
        assert!(guard_comparator_write(&flow, "assigned a global variable").is_ok());
    }

    /// [`enter_callback_scope`] with `effectful: true` shares the depth
    /// bound with [`enter_pure_callback`] (both recurse through [`step`] on
    /// the Rust stack) but marks the scope `effectful: true` — the bit
    /// [`guard_comparator_write`] reads. This is exactly what
    /// [`seq_each`]/[`seq_map_each`] do, driven by
    /// [`SeqVerbOp::is_effectful`](brink_format::SeqVerbOp::is_effectful).
    #[test]
    fn enter_callback_scope_sets_the_effectful_bit_and_shares_the_depth_bound() {
        let mut flow = test_flow();
        let outer = enter_callback_scope(&mut flow, "map_each", true).unwrap();
        assert_eq!(outer.depth, 0);
        assert_eq!(flow.pure_callback.depth, 1);
        assert_eq!(flow.pure_callback.verb, "map_each");
        assert!(flow.pure_callback.effectful);
        flow.pure_callback = outer;

        flow.pure_callback = PureCallbackState {
            depth: COMPARATOR_DEPTH_LIMIT,
            verb: "each",
            effectful: true,
        };
        let err = enter_callback_scope(&mut flow, "each", true).unwrap_err();
        assert!(
            matches!(
                err,
                RuntimeError::ComparatorEscaped {
                    verb: "each",
                    role: "callback",
                    what: "recursed past the nesting depth limit",
                }
            ),
            "{err:?}"
        );
    }

    /// Purity must be sticky (regression for the review finding on
    /// [`enter_callback_scope`]): `map(a, f)` with an opaque `f` — exactly
    /// the case E119 cannot prove, which is why the dev-mode guard exists —
    /// whose body calls `each(b, g)` must NOT disarm the guard for the
    /// enclosing `map` scope just because `each`'s own scope is effectful.
    /// An outer pure frame (`map`) followed by a nested effectful scope
    /// (`each`, entered via [`enter_callback_scope`] with `effectful: true`
    /// — what [`seq_each`] actually calls) must still fault on a
    /// world-write, and it must still be blamed on the outer `map`, not
    /// `each` — `flow.pure_callback` at the write seam is whatever the
    /// *innermost* active scope is, and that scope's effective `effectful`
    /// bit must have inherited the outer scope's purity.
    #[test]
    fn purity_is_sticky_through_a_nested_effectful_callback() {
        let mut flow = test_flow();
        let outer_map = enter_pure_callback(&mut flow, "map").unwrap();
        assert_eq!(flow.pure_callback.depth, 1);
        assert!(!flow.pure_callback.effectful);

        let outer_each = enter_callback_scope(&mut flow, "each", true).unwrap();
        assert_eq!(flow.pure_callback.depth, 2);
        assert_eq!(flow.pure_callback.verb, "each");
        assert!(
            !flow.pure_callback.effectful,
            "each nested inside map's pure scope must not itself read as effectful"
        );

        let err = guard_comparator_write(&flow, "assigned a global variable").unwrap_err();
        assert!(
            matches!(
                err,
                RuntimeError::ComparatorWroteState {
                    verb: "each",
                    role: "callback",
                    what: "assigned a global variable",
                }
            ),
            "a world-write inside the nested each callback must still fault while a pure \
             map scope encloses it: {err:?}"
        );

        flow.pure_callback = outer_each;
        flow.pure_callback = outer_map;
        assert_eq!(flow.pure_callback.depth, 0);
    }

    /// A top-level `each`/`map_each` (no enclosing pure scope) is unaffected
    /// by the stickiness fix — `outer.depth == 0` short-circuits the
    /// inheritance check, so the requested `effectful` bit is honored as
    /// before.
    #[test]
    fn effectful_at_the_top_level_is_unaffected_by_stickiness() {
        let mut flow = test_flow();
        let outer = enter_callback_scope(&mut flow, "each", true).unwrap();
        assert_eq!(outer.depth, 0);
        assert!(flow.pure_callback.effectful);
        assert!(guard_comparator_write(&flow, "assigned a global variable").is_ok());
    }
}
