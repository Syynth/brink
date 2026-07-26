//! Instruction/opcode decode grammar-rule cluster: the `.inkt` `code` field
//! — one `parse_instruction` match arm per mnemonic, plus the operand
//! decoders it shares with every arm.
//!
//! Pure `mod` extraction (issue #685) from the former monolithic `read.rs` —
//! no logic changes, only the module boundary is new.

use super::primitives::parse_def_id;
use super::{InktParseError, P, Rule};
use crate::id::DefinitionId;
use crate::opcode::{ChoiceFlags, Opcode, SequenceKind};

// ── Code field ──────────────────────────────────────────────────────────────

pub(super) fn parse_code_field(pair: P<'_>) -> Result<Vec<u8>, InktParseError> {
    let mut bytecode = Vec::new();
    for child in pair.into_inner() {
        if child.as_rule() == Rule::instruction {
            let op = parse_instruction(child)?;
            op.encode(&mut bytecode);
        }
    }
    Ok(bytecode)
}

#[expect(clippy::too_many_lines)]
fn parse_instruction(pair: P<'_>) -> Result<Opcode, InktParseError> {
    let mut inner = pair.into_inner();
    let mnemonic_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected opcode mnemonic".into(),
        line: 0,
        col: 0,
    })?;
    let mnemonic = mnemonic_pair.as_str();

    let operands: Vec<P<'_>> = inner.collect();

    match mnemonic {
        // Stack & literals
        "push_int" => Ok(Opcode::PushInt(parse_operand_i32(&operands, 0, mnemonic)?)),
        "push_float" => Ok(Opcode::PushFloat(parse_operand_f32(
            &operands, 0, mnemonic,
        )?)),
        "push_bool" => {
            let s = operand_str(&operands, 0, mnemonic)?;
            Ok(Opcode::PushBool(s == "true"))
        }
        "push_string" => Ok(Opcode::PushString(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "push_list" => Ok(Opcode::PushList(parse_operand_u16(&operands, 0, mnemonic)?)),
        "push_divert_target" => Ok(Opcode::PushDivertTarget(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "push_null" => Ok(Opcode::PushNull),
        "pop" => Ok(Opcode::Pop),
        "duplicate" => Ok(Opcode::Duplicate),

        // Arithmetic
        "add" => Ok(Opcode::Add),
        "subtract" => Ok(Opcode::Subtract),
        "multiply" => Ok(Opcode::Multiply),
        "divide" => Ok(Opcode::Divide),
        "modulo" => Ok(Opcode::Modulo),
        "negate" => Ok(Opcode::Negate),

        // Comparison
        "equal" => Ok(Opcode::Equal),
        "not_equal" => Ok(Opcode::NotEqual),
        "greater" => Ok(Opcode::Greater),
        "greater_or_equal" => Ok(Opcode::GreaterOrEqual),
        "less" => Ok(Opcode::Less),
        "less_or_equal" => Ok(Opcode::LessOrEqual),

        // Logic
        "not" => Ok(Opcode::Not),
        "and" => Ok(Opcode::And),
        "or" => Ok(Opcode::Or),

        // Global vars
        "get_global" => Ok(Opcode::GetGlobal(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "set_global" => Ok(Opcode::SetGlobal(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),

        // Temp vars
        "declare_temp" => Ok(Opcode::DeclareTemp(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "get_temp" => Ok(Opcode::GetTemp(parse_operand_u16(&operands, 0, mnemonic)?)),
        "set_temp" => Ok(Opcode::SetTemp(parse_operand_u16(&operands, 0, mnemonic)?)),
        "get_temp_raw" => Ok(Opcode::GetTempRaw(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),

        // Variable pointers
        "push_var_pointer" => Ok(Opcode::PushVarPointer(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "push_temp_pointer" => Ok(Opcode::PushTempPointer(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),

        // Control flow
        "jump" => Ok(Opcode::Jump(parse_operand_i32(&operands, 0, mnemonic)?)),
        "jump_if_false" => Ok(Opcode::JumpIfFalse(parse_operand_i32(
            &operands, 0, mnemonic,
        )?)),
        "goto" => Ok(Opcode::Goto(parse_operand_def_id(&operands, 0, mnemonic)?)),
        "goto_if" => Ok(Opcode::GotoIf(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "goto_variable" => Ok(Opcode::GotoVariable),

        // Container flow
        "enter_container" => Ok(Opcode::EnterContainer(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "exit_container" => Ok(Opcode::ExitContainer),

        // Functions / tunnels
        "call" => Ok(Opcode::Call(parse_operand_def_id(&operands, 0, mnemonic)?)),
        "return" => Ok(Opcode::Return),
        "tunnel_call" => Ok(Opcode::TunnelCall(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "tunnel_return" => Ok(Opcode::TunnelReturn),
        "tunnel_call_variable" => Ok(Opcode::TunnelCallVariable),
        "call_variable" => {
            // "argc=N" is parsed as a kv_operand. Extract the value after "=".
            let kv_str = operand_str(&operands, 0, mnemonic)?;
            let argc_str = kv_str.strip_prefix("argc=").unwrap_or(kv_str);
            let argc: u8 = argc_str.parse().map_err(|_| InktParseError {
                message: format!("invalid argc in call_variable: {kv_str}"),
                line: 0,
                col: 0,
            })?;
            Ok(Opcode::CallVariable(argc))
        }

        // Threads
        "thread_call" => Ok(Opcode::ThreadCall(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "thread_start" => Ok(Opcode::ThreadStart),
        "thread_done" => Ok(Opcode::ThreadDone),

        // Output
        "emit_line" => {
            let idx = parse_operand_u16(&operands, 0, mnemonic)?;
            let slots = parse_operand_u8(&operands, 1, mnemonic)?;
            Ok(Opcode::EmitLine(idx, slots))
        }
        "emit_value" => Ok(Opcode::EmitValue),
        "emit_newline" => Ok(Opcode::EmitNewline),
        "spring" => Ok(Opcode::Spring),
        "glue" => Ok(Opcode::Glue),
        "begin_tag" => Ok(Opcode::BeginTag),
        "end_tag" => Ok(Opcode::EndTag),
        "eval_line" => {
            let idx = parse_operand_u16(&operands, 0, mnemonic)?;
            let slots = parse_operand_u8(&operands, 1, mnemonic)?;
            Ok(Opcode::EvalLine(idx, slots))
        }

        // Choices
        "begin_choice" => {
            let flags = parse_choice_flags_operand(&operands, 0, mnemonic)?;
            let target = parse_operand_def_id(&operands, 1, mnemonic)?;
            Ok(Opcode::BeginChoice(flags, target))
        }
        "end_choice" => Ok(Opcode::EndChoice),

        // Sequences
        "sequence" => {
            let kind_str = operand_str(&operands, 0, mnemonic)?;
            let kind = match kind_str {
                "cycle" => SequenceKind::Cycle,
                "stopping" => SequenceKind::Stopping,
                "once_only" => SequenceKind::OnceOnly,
                "shuffle" => SequenceKind::Shuffle,
                _ => {
                    return Err(InktParseError {
                        message: format!("unknown sequence kind: {kind_str}"),
                        line: 0,
                        col: 0,
                    });
                }
            };
            let count: u8 =
                operand_str(&operands, 1, mnemonic)?
                    .parse()
                    .map_err(|_| InktParseError {
                        message: "invalid sequence count".into(),
                        line: 0,
                        col: 0,
                    })?;
            Ok(Opcode::Sequence(kind, count))
        }
        "sequence_branch" => Ok(Opcode::SequenceBranch(parse_operand_i32(
            &operands, 0, mnemonic,
        )?)),

        // Intrinsics
        "visit_count" => Ok(Opcode::VisitCount),
        "current_visit_count" => Ok(Opcode::CurrentVisitCount),
        "turns_since" => Ok(Opcode::TurnsSince),
        "turn_index" => Ok(Opcode::TurnIndex),
        "choice_count" => Ok(Opcode::ChoiceCount),
        "random" => Ok(Opcode::Random),
        "seed_random" => Ok(Opcode::SeedRandom),

        // Casts / math
        "cast_to_int" => Ok(Opcode::CastToInt),
        "cast_to_float" => Ok(Opcode::CastToFloat),
        "floor" => Ok(Opcode::Floor),
        "ceiling" => Ok(Opcode::Ceiling),
        "pow" => Ok(Opcode::Pow),
        "min" => Ok(Opcode::Min),
        "max" => Ok(Opcode::Max),

        // External fns
        "call_external" => {
            let id = parse_operand_def_id(&operands, 0, mnemonic)?;
            // "argc=N" is parsed as a kv_operand. Extract the value after "=".
            let kv_str = operand_str(&operands, 1, mnemonic)?;
            let argc_str = kv_str.strip_prefix("argc=").unwrap_or(kv_str);
            let argc: u8 = argc_str.parse().map_err(|_| InktParseError {
                message: format!("invalid argc in call_external: {kv_str}"),
                line: 0,
                col: 0,
            })?;
            Ok(Opcode::CallExternal(id, argc))
        }

        // List ops
        "list_contains" => Ok(Opcode::ListContains),
        "list_not_contains" => Ok(Opcode::ListNotContains),
        "list_intersect" => Ok(Opcode::ListIntersect),
        "list_all" => Ok(Opcode::ListAll),
        "list_invert" => Ok(Opcode::ListInvert),
        "list_count" => Ok(Opcode::ListCount),
        "list_min" => Ok(Opcode::ListMin),
        "list_max" => Ok(Opcode::ListMax),
        "list_value" => Ok(Opcode::ListValue),
        "list_range" => Ok(Opcode::ListRange),
        "list_from_int" => Ok(Opcode::ListFromInt),
        "list_random" => Ok(Opcode::ListRandom),

        // Collections (T1b)
        "array_new" => Ok(Opcode::ArrayNew(parse_operand_u32(&operands, 0, mnemonic)?)),
        "map_new" => Ok(Opcode::MapNew(parse_operand_u32(&operands, 0, mnemonic)?)),
        "index_get" => Ok(Opcode::IndexGet),
        "index_set" => Ok(Opcode::IndexSet),
        "collection_len" => Ok(Opcode::CollectionLen),
        "map_get" => Ok(Opcode::MapGet),
        "map_insert" => Ok(Opcode::MapInsert),
        "map_remove" => Ok(Opcode::MapRemove),
        "map_contains" => Ok(Opcode::MapContains),
        "collection_keys" => Ok(Opcode::CollectionKeys),
        "collection_values" => Ok(Opcode::CollectionValues),
        "push_literal" => Ok(Opcode::PushLiteral(parse_operand_u32(
            &operands, 0, mnemonic,
        )?)),

        // Sharing discipline (T1b-4)
        "take_global" => Ok(Opcode::TakeGlobal(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "take_temp" => Ok(Opcode::TakeTemp(parse_operand_u16(&operands, 0, mnemonic)?)),

        // Lifecycle
        "done" => Ok(Opcode::Done),
        "yield" => Ok(Opcode::Yield),
        "end" => Ok(Opcode::End),
        "nop" => Ok(Opcode::Nop),

        // String eval
        "begin_string_eval" => Ok(Opcode::BeginStringEval),
        "end_string_eval" => Ok(Opcode::EndStringEval),

        // Fragment capture
        "begin_fragment" => Ok(Opcode::BeginFragment),
        "end_fragment" => Ok(Opcode::EndFragment),

        // Records (TM-4, `docs/typed-mode-spec.md` §6) — read-side leg paired
        // with `write_opcode`'s `record_new`/`record_get_dyn`/`record_set_dyn`/
        // `record_get`/`record_set` mnemonics (issue #871, the #742 write/read
        // asymmetry class).
        "record_new" => Ok(Opcode::RecordNew(parse_operand_u32(
            &operands, 0, mnemonic,
        )?)),
        "record_get_dyn" => Ok(Opcode::RecordGetDyn(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "record_set_dyn" => Ok(Opcode::RecordSetDyn(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "record_get" => Ok(Opcode::RecordGet(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),
        "record_set" => Ok(Opcode::RecordSet(parse_operand_u16(
            &operands, 0, mnemonic,
        )?)),

        // Conversion intrinsics (TM-3 completion, issue #659/#871)
        "convert_int" => Ok(Opcode::ConvertInt),
        "convert_float" => Ok(Opcode::ConvertFloat),
        "convert_string" => Ok(Opcode::ConvertString),

        // Function values (T1c, `docs/t1c-spec.md` §3/§6, issue #871) —
        // read-side leg paired with `write_opcode`'s `push_fn_ref`/
        // `make_closure`/`call_value`/`bind_value` mnemonics.
        "push_fn_ref" => Ok(Opcode::PushFnRef(parse_operand_def_id(
            &operands, 0, mnemonic,
        )?)),
        "make_closure" => {
            let target = parse_operand_def_id(&operands, 0, mnemonic)?;
            let bound_count = parse_kv_operand_u8(&operands, 1, "bound=", mnemonic)?;
            Ok(Opcode::MakeClosure {
                target,
                bound_count,
            })
        }
        "call_value" => Ok(Opcode::CallValue(parse_kv_operand_u8(
            &operands, 0, "argc=", mnemonic,
        )?)),
        "bind_value" => Ok(Opcode::BindValue(parse_kv_operand_u8(
            &operands, 0, "argc=", mnemonic,
        )?)),

        // Path projections (T1e, `docs/t1e-spec.md` §3, issue #871) —
        // read-side leg paired with `write_opcode`'s `make_projection`/
        // `proj_read`/`proj_write` mnemonics.
        "make_projection" => {
            let root = parse_operand_def_id(&operands, 0, mnemonic)?;
            let segment_count = parse_kv_operand_u8(&operands, 1, "segments=", mnemonic)?;
            Ok(Opcode::MakeProjection {
                root,
                segment_count,
            })
        }
        "proj_read" => Ok(Opcode::ProjRead),
        "proj_write" => Ok(Opcode::ProjWrite),

        // Stdlib slice 1 completion (#857)
        "char_at" => Ok(Opcode::CharAt),

        // NS-A1 Option + stdlib flips
        "push_none" => Ok(Opcode::PushNone),
        "make_some" => Ok(Opcode::MakeSome),
        "str_find" => Ok(Opcode::StrFind),
        "seq_index_of" => Ok(Opcode::SeqIndexOf),
        "seq_min" => Ok(Opcode::SeqMin),
        "seq_max" => Ok(Opcode::SeqMax),
        "seq_first" => Ok(Opcode::SeqFirst),
        "seq_last" => Ok(Opcode::SeqLast),
        "seq_pop" => Ok(Opcode::SeqPop),
        "map_get_opt" => Ok(Opcode::MapGetOpt),
        "map_contains_value" => Ok(Opcode::MapContainsValue),
        "map_clear" => Ok(Opcode::MapClear),

        // B1 `or`-coalescing (issue #1460)
        "coalesce" => Ok(Opcode::Coalesce),

        // Seq `remove_at` (issue #1484)
        "seq_remove_at" => Ok(Opcode::SeqRemoveAt),

        // NS-A6 rand verbs
        "rand_float" => Ok(Opcode::RandFloat),
        "rand_chance" => Ok(Opcode::RandChance),
        "rand_pick" => Ok(Opcode::RandPick),
        "rand_shuffle" => Ok(Opcode::RandShuffle),
        "range_make_excl" => Ok(Opcode::RangeMakeExcl),
        "range_make_incl" => Ok(Opcode::RangeMakeIncl),
        "range_non_empty" => Ok(Opcode::RangeNonEmpty),

        // NS-A4 ordering verbs (#1110)
        "seq_sorted" => Ok(Opcode::SeqSorted),
        "seq_sorted_by" => Ok(Opcode::SeqSortedBy),

        // Debug
        "source_location" => {
            // Written as "source_location LINE:COL" — parsed as source_loc operand
            let s = operand_str(&operands, 0, mnemonic)?;
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() != 2 {
                return Err(InktParseError {
                    message: format!("invalid source_location: {s}"),
                    line: 0,
                    col: 0,
                });
            }
            let line: u32 = parts[0].parse().map_err(|_| InktParseError {
                message: "invalid line".into(),
                line: 0,
                col: 0,
            })?;
            let col: u32 = parts[1].parse().map_err(|_| InktParseError {
                message: "invalid col".into(),
                line: 0,
                col: 0,
            })?;
            Ok(Opcode::SourceLocation(line, col))
        }

        // NS-A8 numeric tower: the TowerOp mnemonic IS the instruction word
        // (`make_vec2` … `tower_lerp`) — one wire opcode, thirteen
        // spellings, `TowerOp::mnemonic`/`from_mnemonic` the single pairing.
        _ => tower_mnemonic_opcode(mnemonic)
            .or_else(|| collect_mnemonic_opcode(mnemonic))
            .ok_or_else(|| InktParseError {
                message: format!("unknown opcode: {mnemonic}"),
                line: mnemonic_pair.line_col().0,
                col: mnemonic_pair.line_col().1,
            }),
    }
}

/// The `.inkt` reader leg for the NS-A8 `Tower` opcode family: resolve a
/// mnemonic to `Opcode::Tower(kind)` via [`crate::TowerOp::from_mnemonic`].
fn tower_mnemonic_opcode(mnemonic: &str) -> Option<Opcode> {
    crate::TowerOp::from_mnemonic(mnemonic).map(Opcode::Tower)
}

/// The `.inkt` reader leg for the NS-A7 `Collect` opcode family: resolve a
/// mnemonic to `Opcode::Collect(kind)` via [`crate::CollectOp::from_mnemonic`].
fn collect_mnemonic_opcode(mnemonic: &str) -> Option<Opcode> {
    crate::CollectOp::from_mnemonic(mnemonic).map(Opcode::Collect)
}

fn parse_choice_flags_operand(
    operands: &[P<'_>],
    idx: usize,
    context: &str,
) -> Result<ChoiceFlags, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    let mut flags = ChoiceFlags {
        has_condition: false,
        has_start_content: false,
        has_choice_only_content: false,
        once_only: false,
        is_invisible_default: false,
    };
    if s == "none" {
        return Ok(flags);
    }
    for part in s.split('+') {
        match part {
            "cond" => flags.has_condition = true,
            "start" => flags.has_start_content = true,
            "choice_only" => flags.has_choice_only_content = true,
            "once" => flags.once_only = true,
            "invis_default" => flags.is_invisible_default = true,
            _ => {
                return Err(InktParseError {
                    message: format!("unknown choice flag: {part}"),
                    line: 0,
                    col: 0,
                });
            }
        }
    }
    Ok(flags)
}

// ── Operand helpers ─────────────────────────────────────────────────────────

fn operand_str<'a>(
    operands: &'a [P<'_>],
    idx: usize,
    context: &str,
) -> Result<&'a str, InktParseError> {
    let op = operands.get(idx).ok_or_else(|| InktParseError {
        message: format!("missing operand {idx} for {context}"),
        line: 0,
        col: 0,
    })?;
    // The operand rule wraps the actual value. Get the inner pair.
    let inner = op.clone().into_inner().next();
    match inner {
        Some(p) => Ok(p.as_str()),
        None => Ok(op.as_str()),
    }
}

fn parse_operand_i32(operands: &[P<'_>], idx: usize, context: &str) -> Result<i32, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid i32 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_f32(operands: &[P<'_>], idx: usize, context: &str) -> Result<f32, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid f32 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_u8(operands: &[P<'_>], idx: usize, context: &str) -> Result<u8, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid u8 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_u16(operands: &[P<'_>], idx: usize, context: &str) -> Result<u16, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid u16 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_u32(operands: &[P<'_>], idx: usize, context: &str) -> Result<u32, InktParseError> {
    let s = operand_str(operands, idx, context)?;
    s.parse().map_err(|_| InktParseError {
        message: format!("invalid u32 operand for {context}: {s}"),
        line: 0,
        col: 0,
    })
}

/// Parse a `kv_operand` of the form `"<prefix><value>"` (e.g. `"bound=3"`,
/// `"segments=2"`) into its numeric value. Mirrors the inline `argc=`
/// stripping already used by `call_variable`/`call_external`, generalized so
/// `make_closure`'s `bound=` and `make_projection`'s `segments=` operands
/// (issue #871) don't each duplicate it.
fn parse_kv_operand_u8(
    operands: &[P<'_>],
    idx: usize,
    prefix: &str,
    context: &str,
) -> Result<u8, InktParseError> {
    let kv_str = operand_str(operands, idx, context)?;
    let value_str = kv_str.strip_prefix(prefix).unwrap_or(kv_str);
    value_str.parse().map_err(|_| InktParseError {
        message: format!("invalid {prefix}value in {context}: {kv_str}"),
        line: 0,
        col: 0,
    })
}

fn parse_operand_def_id(
    operands: &[P<'_>],
    idx: usize,
    context: &str,
) -> Result<DefinitionId, InktParseError> {
    let op = operands.get(idx).ok_or_else(|| InktParseError {
        message: format!("missing operand {idx} for {context}"),
        line: 0,
        col: 0,
    })?;
    // Drill into the operand to get the def_id inner pair
    let inner = op.clone().into_inner().next().unwrap_or_else(|| op.clone());
    parse_def_id(inner)
}
