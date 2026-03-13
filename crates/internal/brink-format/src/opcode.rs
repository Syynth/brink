use core::fmt;

use crate::codec::{
    read_def_id, read_f32, read_i32, read_u8, read_u16, read_u32, write_def_id, write_f32,
    write_i32, write_u8, write_u16, write_u32,
};
use crate::id::DefinitionId;

// ── Discriminant bytes ──────────────────────────────────────────────────────

// Stack & literals
const PUSH_INT: u8 = 0x01;
const PUSH_FLOAT: u8 = 0x02;
const PUSH_BOOL: u8 = 0x03;
const PUSH_STRING: u8 = 0x04;
const PUSH_LIST: u8 = 0x05;
const PUSH_DIVERT_TARGET: u8 = 0x06;
const PUSH_NULL: u8 = 0x07;
const POP: u8 = 0x08;
const DUPLICATE: u8 = 0x09;

// Arithmetic
const ADD: u8 = 0x10;
const SUBTRACT: u8 = 0x11;
const MULTIPLY: u8 = 0x12;
const DIVIDE: u8 = 0x13;
const MODULO: u8 = 0x14;
const NEGATE: u8 = 0x15;

// Comparison
const EQUAL: u8 = 0x20;
const NOT_EQUAL: u8 = 0x21;
const GREATER: u8 = 0x22;
const GREATER_OR_EQUAL: u8 = 0x23;
const LESS: u8 = 0x24;
const LESS_OR_EQUAL: u8 = 0x25;

// Logic
const NOT: u8 = 0x28;
const AND: u8 = 0x29;
const OR: u8 = 0x2A;

// Global vars
const GET_GLOBAL: u8 = 0x30;
const SET_GLOBAL: u8 = 0x31;

// Temp vars
const DECLARE_TEMP: u8 = 0x34;
const GET_TEMP: u8 = 0x35;
const SET_TEMP: u8 = 0x36;
const GET_TEMP_RAW: u8 = 0x37;

// Variable pointers
const PUSH_VAR_POINTER: u8 = 0x38;
const PUSH_TEMP_POINTER: u8 = 0x39;

// Control flow
const JUMP: u8 = 0x40;
const JUMP_IF_FALSE: u8 = 0x41;
const GOTO: u8 = 0x42;
const GOTO_IF: u8 = 0x43;
const GOTO_VARIABLE: u8 = 0x44;

// Container flow
const ENTER_CONTAINER: u8 = 0x48;
const EXIT_CONTAINER: u8 = 0x49;

// Functions / tunnels
const CALL: u8 = 0x50;
const RETURN: u8 = 0x51;
const TUNNEL_CALL: u8 = 0x52;
const TUNNEL_RETURN: u8 = 0x53;
const TUNNEL_CALL_VARIABLE: u8 = 0x54;
const CALL_VARIABLE: u8 = 0x55;

// Threads
const THREAD_CALL: u8 = 0x57;
const THREAD_START: u8 = 0x58;
const THREAD_DONE: u8 = 0x59;

// Output
const EMIT_LINE: u8 = 0x60;
const EMIT_VALUE: u8 = 0x61;
const EMIT_NEWLINE: u8 = 0x62;
const GLUE: u8 = 0x63;
const BEGIN_TAG: u8 = 0x64;
const END_TAG: u8 = 0x65;
const EVAL_LINE: u8 = 0x66;

// Choices
const BEGIN_CHOICE: u8 = 0x72;
const END_CHOICE: u8 = 0x73;
// Sequences
const SEQUENCE: u8 = 0x78;
const SEQUENCE_BRANCH: u8 = 0x79;

// Intrinsics
const VISIT_COUNT: u8 = 0x80;
const TURNS_SINCE: u8 = 0x81;
const TURN_INDEX: u8 = 0x82;
const CHOICE_COUNT: u8 = 0x83;
const RANDOM: u8 = 0x84;
const SEED_RANDOM: u8 = 0x85;
const CURRENT_VISIT_COUNT: u8 = 0x86;

// Casts / math
const CAST_TO_INT: u8 = 0x90;
const CAST_TO_FLOAT: u8 = 0x91;
const FLOOR: u8 = 0x92;
const CEILING: u8 = 0x93;
const POW: u8 = 0x94;
const MIN: u8 = 0x95;
const MAX: u8 = 0x96;

// External fns
const CALL_EXTERNAL: u8 = 0xA0;

// List ops
const LIST_CONTAINS: u8 = 0xB0;
const LIST_NOT_CONTAINS: u8 = 0xB1;
const LIST_INTERSECT: u8 = 0xB2;
const LIST_ALL: u8 = 0xB5;
const LIST_INVERT: u8 = 0xB6;
const LIST_COUNT: u8 = 0xB7;
const LIST_MIN: u8 = 0xB8;
const LIST_MAX: u8 = 0xB9;
const LIST_VALUE: u8 = 0xBA;
const LIST_RANGE: u8 = 0xBB;
const LIST_FROM_INT: u8 = 0xBC;
const LIST_RANDOM: u8 = 0xBD;

// Lifecycle
const DONE: u8 = 0xF0;
const END: u8 = 0xF1;
const NOP: u8 = 0xF2;

// String eval
const BEGIN_STRING_EVAL: u8 = 0xE0;
const END_STRING_EVAL: u8 = 0xE1;

// Debug
const SOURCE_LOCATION: u8 = 0xFE;

// ── Types ───────────────────────────────────────────────────────────────────

/// The kind of sequence/shuffle container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequenceKind {
    Cycle,
    Stopping,
    OnceOnly,
    Shuffle,
}

impl SequenceKind {
    fn to_byte(self) -> u8 {
        match self {
            Self::Cycle => 0,
            Self::Stopping => 1,
            Self::OnceOnly => 2,
            Self::Shuffle => 3,
        }
    }

    fn from_byte(b: u8) -> Result<Self, DecodeError> {
        match b {
            0 => Ok(Self::Cycle),
            1 => Ok(Self::Stopping),
            2 => Ok(Self::OnceOnly),
            3 => Ok(Self::Shuffle),
            _ => Err(DecodeError::InvalidSequenceKind(b)),
        }
    }
}

/// Flags packed into a `BeginChoice` instruction.
///
/// Under the single-pop protocol, `BeginChoice` pops at most **one** display
/// string from the stack when `has_start_content || has_choice_only_content`.
/// The two content flags are metadata indicating which parts of the original
/// ink choice contributed to that string — the runtime does not pop them
/// separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(clippy::struct_excessive_bools)]
pub struct ChoiceFlags {
    pub has_condition: bool,
    /// Original choice had `start` content (text before `[`).
    pub has_start_content: bool,
    /// Original choice had `choice_only` content (text inside `[]`).
    /// Under the single-pop protocol this is metadata only — no extra stack pop.
    pub has_choice_only_content: bool,
    pub once_only: bool,
    pub is_invisible_default: bool,
}

impl ChoiceFlags {
    fn to_byte(self) -> u8 {
        let mut b = 0u8;
        if self.has_condition {
            b |= 0x01;
        }
        if self.has_start_content {
            b |= 0x02;
        }
        if self.has_choice_only_content {
            b |= 0x04;
        }
        if self.once_only {
            b |= 0x08;
        }
        if self.is_invisible_default {
            b |= 0x10;
        }
        b
    }

    fn from_byte(b: u8) -> Self {
        Self {
            has_condition: b & 0x01 != 0,
            has_start_content: b & 0x02 != 0,
            has_choice_only_content: b & 0x04 != 0,
            once_only: b & 0x08 != 0,
            is_invisible_default: b & 0x10 != 0,
        }
    }
}

/// Errors that can occur when decoding from bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Not enough bytes remaining for the expected operand.
    UnexpectedEof,
    /// Unknown opcode discriminant byte.
    UnknownOpcode(u8),
    /// Invalid definition id (bad tag byte).
    InvalidDefinitionId(u64),
    /// Invalid sequence kind byte.
    InvalidSequenceKind(u8),
    /// .inkb magic bytes are not `INKB`.
    BadMagic([u8; 4]),
    /// .inkb version is not supported.
    UnsupportedVersion(u16),
    /// A string field contained invalid UTF-8.
    InvalidUtf8,
    /// Unknown value type discriminant.
    InvalidValueType(u8),
    /// Unknown select key discriminant.
    InvalidSelectKey(u8),
    /// Unknown line part discriminant.
    InvalidLinePart(u8),
    /// Unknown line content discriminant.
    InvalidLineContent(u8),
    /// Unknown plural category discriminant.
    InvalidPluralCategory(u8),
    /// Unknown section kind tag in .inkb offset table.
    InvalidSectionKind(u8),
    /// Required section kind missing from .inkb offset table.
    MissingSectionKind(u8),
    /// File size field doesn't match actual buffer length.
    FileSizeMismatch { expected: u32, actual: usize },
    /// CRC-32 checksum of section data doesn't match header.
    ChecksumMismatch { expected: u32, actual: u32 },
    /// Section offset table is structurally invalid (out of bounds or not monotonic).
    InvalidSectionOffset { kind: u8, offset: u32 },
    /// `.inkl` magic bytes are not `INKL`.
    BadInklMagic([u8; 4]),
    /// `.inkl` version is not supported.
    UnsupportedInklVersion(u8),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of bytecode"),
            Self::UnknownOpcode(b) => write!(f, "unknown opcode: {b:#04x}"),
            Self::InvalidDefinitionId(raw) => {
                write!(f, "invalid definition id: {raw:#018x}")
            }
            Self::InvalidSequenceKind(b) => write!(f, "invalid sequence kind: {b}"),
            Self::BadMagic(m) => write!(f, "bad magic: {m:02x?}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported .inkb version: {v}"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 in string field"),
            Self::InvalidValueType(b) => write!(f, "invalid value type: {b:#04x}"),
            Self::InvalidSelectKey(b) => write!(f, "invalid select key: {b:#04x}"),
            Self::InvalidLinePart(b) => write!(f, "invalid line part: {b:#04x}"),
            Self::InvalidLineContent(b) => write!(f, "invalid line content: {b:#04x}"),
            Self::InvalidPluralCategory(b) => write!(f, "invalid plural category: {b:#04x}"),
            Self::InvalidSectionKind(b) => write!(f, "invalid section kind: {b:#04x}"),
            Self::MissingSectionKind(b) => write!(f, "missing required section kind: {b:#04x}"),
            Self::FileSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "file size mismatch: header says {expected}, actual {actual}"
                )
            }
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "checksum mismatch: header {expected:#010x}, computed {actual:#010x}"
                )
            }
            Self::InvalidSectionOffset { kind, offset } => {
                write!(
                    f,
                    "invalid section offset: kind {kind:#04x} at offset {offset}"
                )
            }
            Self::BadInklMagic(m) => write!(f, "bad .inkl magic: {m:02x?}"),
            Self::UnsupportedInklVersion(v) => write!(f, "unsupported .inkl version: {v}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// A single VM instruction with its operands.
#[derive(Debug, Clone, PartialEq)]
pub enum Opcode {
    // ── Stack & literals ────────────────────────────────────────────────
    PushInt(i32),
    PushFloat(f32),
    PushBool(bool),
    PushString(u16),
    PushList(u16),
    PushDivertTarget(DefinitionId),
    PushNull,
    Pop,
    Duplicate,

    // ── Arithmetic ──────────────────────────────────────────────────────
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Negate,

    // ── Comparison ──────────────────────────────────────────────────────
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,

    // ── Logic ───────────────────────────────────────────────────────────
    Not,
    And,
    Or,

    // ── Global vars ─────────────────────────────────────────────────────
    GetGlobal(DefinitionId),
    SetGlobal(DefinitionId),

    // ── Temp vars ───────────────────────────────────────────────────────
    DeclareTemp(u16),
    GetTemp(u16),
    SetTemp(u16),
    /// Get a temp's raw value without auto-dereference (for passing a ref onward).
    GetTempRaw(u16),

    // ── Variable pointers ──────────────────────────────────────────────
    /// Push a pointer to a global variable onto the eval stack.
    PushVarPointer(DefinitionId),
    /// Push a pointer to a temp variable onto the eval stack.
    PushTempPointer(u16),

    // ── Control flow ────────────────────────────────────────────────────
    Jump(i32),
    JumpIfFalse(i32),
    Goto(DefinitionId),
    GotoIf(DefinitionId),
    GotoVariable,

    // ── Container flow ──────────────────────────────────────────────────
    EnterContainer(DefinitionId),
    ExitContainer,

    // ── Functions / tunnels ─────────────────────────────────────────────
    Call(DefinitionId),
    Return,
    TunnelCall(DefinitionId),
    TunnelReturn,
    TunnelCallVariable,
    CallVariable,

    // ── Threads ─────────────────────────────────────────────────────────
    ThreadCall(DefinitionId),
    ThreadStart,
    ThreadDone,

    // ── Output ──────────────────────────────────────────────────────────
    EmitLine(u16),
    EmitValue,
    EmitNewline,
    Glue,
    BeginTag,
    EndTag,
    EvalLine(u16),

    // ── Choices ─────────────────────────────────────────────────────────
    BeginChoice(ChoiceFlags, DefinitionId),
    EndChoice,

    // ── Sequences ───────────────────────────────────────────────────────
    Sequence(SequenceKind, u8),
    SequenceBranch(i32),

    // ── Intrinsics ──────────────────────────────────────────────────────
    /// Pop a `DivertTarget` from the stack, push its visit count.
    VisitCount,
    /// Push the visit count of the *current* container (no stack input).
    CurrentVisitCount,
    TurnsSince,
    TurnIndex,
    ChoiceCount,
    Random,
    SeedRandom,

    // ── Casts / math ────────────────────────────────────────────────────
    CastToInt,
    CastToFloat,
    Floor,
    Ceiling,
    Pow,
    Min,
    Max,

    // ── External fns ────────────────────────────────────────────────────
    CallExternal(DefinitionId, u8),

    // ── List ops ────────────────────────────────────────────────────────
    ListContains,
    ListNotContains,
    ListIntersect,
    ListAll,
    ListInvert,
    ListCount,
    ListMin,
    ListMax,
    ListValue,
    ListRange,
    ListFromInt,
    ListRandom,

    // ── Lifecycle ───────────────────────────────────────────────────────
    Done,
    End,
    Nop,

    // ── String eval ─────────────────────────────────────────────────────
    BeginStringEval,
    EndStringEval,

    // ── Debug ───────────────────────────────────────────────────────────
    SourceLocation(u32, u32),
}

// ── Opcode encode / decode ──────────────────────────────────────────────────

impl Opcode {
    /// Encode this instruction into the byte buffer.
    #[expect(clippy::too_many_lines)]
    pub fn encode(&self, buf: &mut Vec<u8>) {
        match *self {
            // Stack & literals
            Self::PushInt(v) => {
                write_u8(buf, PUSH_INT);
                write_i32(buf, v);
            }
            Self::PushFloat(v) => {
                write_u8(buf, PUSH_FLOAT);
                write_f32(buf, v);
            }
            Self::PushBool(v) => {
                write_u8(buf, PUSH_BOOL);
                write_u8(buf, u8::from(v));
            }
            Self::PushString(idx) => {
                write_u8(buf, PUSH_STRING);
                write_u16(buf, idx);
            }
            Self::PushList(idx) => {
                write_u8(buf, PUSH_LIST);
                write_u16(buf, idx);
            }
            Self::PushDivertTarget(id) => {
                write_u8(buf, PUSH_DIVERT_TARGET);
                write_def_id(buf, id);
            }
            Self::PushNull => write_u8(buf, PUSH_NULL),
            Self::Pop => write_u8(buf, POP),
            Self::Duplicate => write_u8(buf, DUPLICATE),

            // Arithmetic
            Self::Add => write_u8(buf, ADD),
            Self::Subtract => write_u8(buf, SUBTRACT),
            Self::Multiply => write_u8(buf, MULTIPLY),
            Self::Divide => write_u8(buf, DIVIDE),
            Self::Modulo => write_u8(buf, MODULO),
            Self::Negate => write_u8(buf, NEGATE),

            // Comparison
            Self::Equal => write_u8(buf, EQUAL),
            Self::NotEqual => write_u8(buf, NOT_EQUAL),
            Self::Greater => write_u8(buf, GREATER),
            Self::GreaterOrEqual => write_u8(buf, GREATER_OR_EQUAL),
            Self::Less => write_u8(buf, LESS),
            Self::LessOrEqual => write_u8(buf, LESS_OR_EQUAL),

            // Logic
            Self::Not => write_u8(buf, NOT),
            Self::And => write_u8(buf, AND),
            Self::Or => write_u8(buf, OR),

            // Global vars
            Self::GetGlobal(id) => {
                write_u8(buf, GET_GLOBAL);
                write_def_id(buf, id);
            }
            Self::SetGlobal(id) => {
                write_u8(buf, SET_GLOBAL);
                write_def_id(buf, id);
            }

            // Temp vars
            Self::DeclareTemp(idx) => {
                write_u8(buf, DECLARE_TEMP);
                write_u16(buf, idx);
            }
            Self::GetTemp(idx) => {
                write_u8(buf, GET_TEMP);
                write_u16(buf, idx);
            }
            Self::SetTemp(idx) => {
                write_u8(buf, SET_TEMP);
                write_u16(buf, idx);
            }
            Self::GetTempRaw(idx) => {
                write_u8(buf, GET_TEMP_RAW);
                write_u16(buf, idx);
            }

            // Variable pointers
            Self::PushVarPointer(id) => {
                write_u8(buf, PUSH_VAR_POINTER);
                write_def_id(buf, id);
            }
            Self::PushTempPointer(slot) => {
                write_u8(buf, PUSH_TEMP_POINTER);
                write_u16(buf, slot);
            }

            // Control flow
            Self::Jump(offset) => {
                write_u8(buf, JUMP);
                write_i32(buf, offset);
            }
            Self::JumpIfFalse(offset) => {
                write_u8(buf, JUMP_IF_FALSE);
                write_i32(buf, offset);
            }
            Self::Goto(id) => {
                write_u8(buf, GOTO);
                write_def_id(buf, id);
            }
            Self::GotoIf(id) => {
                write_u8(buf, GOTO_IF);
                write_def_id(buf, id);
            }
            Self::GotoVariable => write_u8(buf, GOTO_VARIABLE),

            // Container flow
            Self::EnterContainer(id) => {
                write_u8(buf, ENTER_CONTAINER);
                write_def_id(buf, id);
            }
            Self::ExitContainer => write_u8(buf, EXIT_CONTAINER),

            // Functions / tunnels
            Self::Call(id) => {
                write_u8(buf, CALL);
                write_def_id(buf, id);
            }
            Self::Return => write_u8(buf, RETURN),
            Self::TunnelCall(id) => {
                write_u8(buf, TUNNEL_CALL);
                write_def_id(buf, id);
            }
            Self::TunnelReturn => write_u8(buf, TUNNEL_RETURN),
            Self::TunnelCallVariable => write_u8(buf, TUNNEL_CALL_VARIABLE),
            Self::CallVariable => write_u8(buf, CALL_VARIABLE),

            // Threads
            Self::ThreadCall(id) => {
                write_u8(buf, THREAD_CALL);
                write_def_id(buf, id);
            }
            Self::ThreadStart => write_u8(buf, THREAD_START),
            Self::ThreadDone => write_u8(buf, THREAD_DONE),

            // Output
            Self::EmitLine(idx) => {
                write_u8(buf, EMIT_LINE);
                write_u16(buf, idx);
            }
            Self::EmitValue => write_u8(buf, EMIT_VALUE),
            Self::EmitNewline => write_u8(buf, EMIT_NEWLINE),
            Self::Glue => write_u8(buf, GLUE),
            Self::BeginTag => write_u8(buf, BEGIN_TAG),
            Self::EndTag => write_u8(buf, END_TAG),
            Self::EvalLine(idx) => {
                write_u8(buf, EVAL_LINE);
                write_u16(buf, idx);
            }

            // Choices
            Self::BeginChoice(flags, target) => {
                write_u8(buf, BEGIN_CHOICE);
                write_u8(buf, flags.to_byte());
                write_def_id(buf, target);
            }
            Self::EndChoice => write_u8(buf, END_CHOICE),

            // Sequences
            Self::Sequence(kind, count) => {
                write_u8(buf, SEQUENCE);
                write_u8(buf, kind.to_byte());
                write_u8(buf, count);
            }
            Self::SequenceBranch(offset) => {
                write_u8(buf, SEQUENCE_BRANCH);
                write_i32(buf, offset);
            }

            // Intrinsics
            Self::VisitCount => write_u8(buf, VISIT_COUNT),
            Self::CurrentVisitCount => write_u8(buf, CURRENT_VISIT_COUNT),
            Self::TurnsSince => write_u8(buf, TURNS_SINCE),
            Self::TurnIndex => write_u8(buf, TURN_INDEX),
            Self::ChoiceCount => write_u8(buf, CHOICE_COUNT),
            Self::Random => write_u8(buf, RANDOM),
            Self::SeedRandom => write_u8(buf, SEED_RANDOM),

            // Casts / math
            Self::CastToInt => write_u8(buf, CAST_TO_INT),
            Self::CastToFloat => write_u8(buf, CAST_TO_FLOAT),
            Self::Floor => write_u8(buf, FLOOR),
            Self::Ceiling => write_u8(buf, CEILING),
            Self::Pow => write_u8(buf, POW),
            Self::Min => write_u8(buf, MIN),
            Self::Max => write_u8(buf, MAX),

            // External fns
            Self::CallExternal(id, argc) => {
                write_u8(buf, CALL_EXTERNAL);
                write_def_id(buf, id);
                write_u8(buf, argc);
            }

            // List ops
            Self::ListContains => write_u8(buf, LIST_CONTAINS),
            Self::ListNotContains => write_u8(buf, LIST_NOT_CONTAINS),
            Self::ListIntersect => write_u8(buf, LIST_INTERSECT),
            Self::ListAll => write_u8(buf, LIST_ALL),
            Self::ListInvert => write_u8(buf, LIST_INVERT),
            Self::ListCount => write_u8(buf, LIST_COUNT),
            Self::ListMin => write_u8(buf, LIST_MIN),
            Self::ListMax => write_u8(buf, LIST_MAX),
            Self::ListValue => write_u8(buf, LIST_VALUE),
            Self::ListRange => write_u8(buf, LIST_RANGE),
            Self::ListFromInt => write_u8(buf, LIST_FROM_INT),
            Self::ListRandom => write_u8(buf, LIST_RANDOM),

            // Lifecycle
            Self::Done => write_u8(buf, DONE),
            Self::End => write_u8(buf, END),
            Self::Nop => write_u8(buf, NOP),

            // String eval
            Self::BeginStringEval => write_u8(buf, BEGIN_STRING_EVAL),
            Self::EndStringEval => write_u8(buf, END_STRING_EVAL),

            // Debug
            Self::SourceLocation(line, col) => {
                write_u8(buf, SOURCE_LOCATION);
                write_u32(buf, line);
                write_u32(buf, col);
            }
        }
    }

    /// Decode a single instruction from `buf` starting at `*offset`.
    ///
    /// On success, `*offset` is advanced past the consumed bytes.
    #[expect(clippy::too_many_lines)]
    pub fn decode(buf: &[u8], offset: &mut usize) -> Result<Self, DecodeError> {
        let disc = read_u8(buf, offset)?;

        let op = match disc {
            // Stack & literals
            PUSH_INT => Self::PushInt(read_i32(buf, offset)?),
            PUSH_FLOAT => Self::PushFloat(read_f32(buf, offset)?),
            PUSH_BOOL => Self::PushBool(read_u8(buf, offset)? != 0),
            PUSH_STRING => Self::PushString(read_u16(buf, offset)?),
            PUSH_LIST => Self::PushList(read_u16(buf, offset)?),
            PUSH_DIVERT_TARGET => Self::PushDivertTarget(read_def_id(buf, offset)?),
            PUSH_NULL => Self::PushNull,
            POP => Self::Pop,
            DUPLICATE => Self::Duplicate,

            // Arithmetic
            ADD => Self::Add,
            SUBTRACT => Self::Subtract,
            MULTIPLY => Self::Multiply,
            DIVIDE => Self::Divide,
            MODULO => Self::Modulo,
            NEGATE => Self::Negate,

            // Comparison
            EQUAL => Self::Equal,
            NOT_EQUAL => Self::NotEqual,
            GREATER => Self::Greater,
            GREATER_OR_EQUAL => Self::GreaterOrEqual,
            LESS => Self::Less,
            LESS_OR_EQUAL => Self::LessOrEqual,

            // Logic
            NOT => Self::Not,
            AND => Self::And,
            OR => Self::Or,

            // Global vars
            GET_GLOBAL => Self::GetGlobal(read_def_id(buf, offset)?),
            SET_GLOBAL => Self::SetGlobal(read_def_id(buf, offset)?),

            // Temp vars
            DECLARE_TEMP => Self::DeclareTemp(read_u16(buf, offset)?),
            GET_TEMP => Self::GetTemp(read_u16(buf, offset)?),
            SET_TEMP => Self::SetTemp(read_u16(buf, offset)?),
            GET_TEMP_RAW => Self::GetTempRaw(read_u16(buf, offset)?),

            // Variable pointers
            PUSH_VAR_POINTER => Self::PushVarPointer(read_def_id(buf, offset)?),
            PUSH_TEMP_POINTER => Self::PushTempPointer(read_u16(buf, offset)?),

            // Control flow
            JUMP => Self::Jump(read_i32(buf, offset)?),
            JUMP_IF_FALSE => Self::JumpIfFalse(read_i32(buf, offset)?),
            GOTO => Self::Goto(read_def_id(buf, offset)?),
            GOTO_IF => Self::GotoIf(read_def_id(buf, offset)?),
            GOTO_VARIABLE => Self::GotoVariable,

            // Container flow
            ENTER_CONTAINER => Self::EnterContainer(read_def_id(buf, offset)?),
            EXIT_CONTAINER => Self::ExitContainer,

            // Functions / tunnels
            CALL => Self::Call(read_def_id(buf, offset)?),
            RETURN => Self::Return,
            TUNNEL_CALL => Self::TunnelCall(read_def_id(buf, offset)?),
            TUNNEL_RETURN => Self::TunnelReturn,
            TUNNEL_CALL_VARIABLE => Self::TunnelCallVariable,
            CALL_VARIABLE => Self::CallVariable,

            // Threads
            THREAD_CALL => Self::ThreadCall(read_def_id(buf, offset)?),
            THREAD_START => Self::ThreadStart,
            THREAD_DONE => Self::ThreadDone,

            // Output
            EMIT_LINE => Self::EmitLine(read_u16(buf, offset)?),
            EMIT_VALUE => Self::EmitValue,
            EMIT_NEWLINE => Self::EmitNewline,
            GLUE => Self::Glue,
            BEGIN_TAG => Self::BeginTag,
            END_TAG => Self::EndTag,
            EVAL_LINE => Self::EvalLine(read_u16(buf, offset)?),

            // Choices
            BEGIN_CHOICE => {
                let flags = ChoiceFlags::from_byte(read_u8(buf, offset)?);
                let target = read_def_id(buf, offset)?;
                Self::BeginChoice(flags, target)
            }
            END_CHOICE => Self::EndChoice,

            // Sequences
            SEQUENCE => {
                let kind = SequenceKind::from_byte(read_u8(buf, offset)?)?;
                let count = read_u8(buf, offset)?;
                Self::Sequence(kind, count)
            }
            SEQUENCE_BRANCH => Self::SequenceBranch(read_i32(buf, offset)?),

            // Intrinsics
            VISIT_COUNT => Self::VisitCount,
            CURRENT_VISIT_COUNT => Self::CurrentVisitCount,
            TURNS_SINCE => Self::TurnsSince,
            TURN_INDEX => Self::TurnIndex,
            CHOICE_COUNT => Self::ChoiceCount,
            RANDOM => Self::Random,
            SEED_RANDOM => Self::SeedRandom,

            // Casts / math
            CAST_TO_INT => Self::CastToInt,
            CAST_TO_FLOAT => Self::CastToFloat,
            FLOOR => Self::Floor,
            CEILING => Self::Ceiling,
            POW => Self::Pow,
            MIN => Self::Min,
            MAX => Self::Max,

            // External fns
            CALL_EXTERNAL => {
                let id = read_def_id(buf, offset)?;
                let argc = read_u8(buf, offset)?;
                Self::CallExternal(id, argc)
            }

            // List ops
            LIST_CONTAINS => Self::ListContains,
            LIST_NOT_CONTAINS => Self::ListNotContains,
            LIST_INTERSECT => Self::ListIntersect,
            LIST_ALL => Self::ListAll,
            LIST_INVERT => Self::ListInvert,
            LIST_COUNT => Self::ListCount,
            LIST_MIN => Self::ListMin,
            LIST_MAX => Self::ListMax,
            LIST_VALUE => Self::ListValue,
            LIST_RANGE => Self::ListRange,
            LIST_FROM_INT => Self::ListFromInt,
            LIST_RANDOM => Self::ListRandom,

            // Lifecycle
            DONE => Self::Done,
            END => Self::End,
            NOP => Self::Nop,

            // String eval
            BEGIN_STRING_EVAL => Self::BeginStringEval,
            END_STRING_EVAL => Self::EndStringEval,

            // Debug
            SOURCE_LOCATION => {
                let line = read_u32(buf, offset)?;
                let col = read_u32(buf, offset)?;
                Self::SourceLocation(line, col)
            }

            _ => return Err(DecodeError::UnknownOpcode(disc)),
        };

        Ok(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DefinitionTag;

    fn roundtrip(op: &Opcode) {
        let mut buf = Vec::new();
        op.encode(&mut buf);
        let mut offset = 0;
        let decoded = Opcode::decode(&buf, &mut offset).unwrap();
        assert_eq!(*op, decoded, "roundtrip failed for {op:?}");
        assert_eq!(offset, buf.len(), "not all bytes consumed for {op:?}");
    }

    fn test_id() -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, 0xBEEF)
    }

    fn global_id() -> DefinitionId {
        DefinitionId::new(DefinitionTag::GlobalVar, 42)
    }

    fn ext_id() -> DefinitionId {
        DefinitionId::new(DefinitionTag::ExternalFn, 0xCAFE)
    }

    #[test]
    fn roundtrip_stack_literals() {
        roundtrip(&Opcode::PushInt(0));
        roundtrip(&Opcode::PushInt(-1));
        roundtrip(&Opcode::PushInt(i32::MAX));
        roundtrip(&Opcode::PushInt(i32::MIN));
        roundtrip(&Opcode::PushFloat(0.0));
        roundtrip(&Opcode::PushFloat(3.125));
        roundtrip(&Opcode::PushFloat(f32::NEG_INFINITY));
        roundtrip(&Opcode::PushBool(true));
        roundtrip(&Opcode::PushBool(false));
        roundtrip(&Opcode::PushString(0));
        roundtrip(&Opcode::PushString(u16::MAX));
        roundtrip(&Opcode::PushList(7));
        roundtrip(&Opcode::PushDivertTarget(test_id()));
        roundtrip(&Opcode::PushNull);
        roundtrip(&Opcode::Pop);
        roundtrip(&Opcode::Duplicate);
    }

    #[test]
    fn roundtrip_arithmetic() {
        for op in [
            Opcode::Add,
            Opcode::Subtract,
            Opcode::Multiply,
            Opcode::Divide,
            Opcode::Modulo,
            Opcode::Negate,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_comparison() {
        for op in [
            Opcode::Equal,
            Opcode::NotEqual,
            Opcode::Greater,
            Opcode::GreaterOrEqual,
            Opcode::Less,
            Opcode::LessOrEqual,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_logic() {
        for op in [Opcode::Not, Opcode::And, Opcode::Or] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_globals() {
        roundtrip(&Opcode::GetGlobal(global_id()));
        roundtrip(&Opcode::SetGlobal(global_id()));
    }

    #[test]
    fn roundtrip_temps() {
        roundtrip(&Opcode::DeclareTemp(0));
        roundtrip(&Opcode::GetTemp(5));
        roundtrip(&Opcode::SetTemp(u16::MAX));
        roundtrip(&Opcode::GetTempRaw(3));
    }

    #[test]
    fn roundtrip_var_pointer() {
        roundtrip(&Opcode::PushVarPointer(global_id()));
        roundtrip(&Opcode::PushTempPointer(0));
        roundtrip(&Opcode::PushTempPointer(u16::MAX));
    }

    #[test]
    fn roundtrip_control_flow() {
        roundtrip(&Opcode::Jump(0));
        roundtrip(&Opcode::Jump(-42));
        roundtrip(&Opcode::JumpIfFalse(100));
        roundtrip(&Opcode::Goto(test_id()));
        roundtrip(&Opcode::GotoIf(test_id()));
        roundtrip(&Opcode::GotoVariable);
    }

    #[test]
    fn roundtrip_container_flow() {
        roundtrip(&Opcode::EnterContainer(test_id()));
        roundtrip(&Opcode::ExitContainer);
    }

    #[test]
    fn roundtrip_functions_tunnels() {
        roundtrip(&Opcode::Call(test_id()));
        roundtrip(&Opcode::Return);
        roundtrip(&Opcode::TunnelCall(test_id()));
        roundtrip(&Opcode::TunnelReturn);
        roundtrip(&Opcode::TunnelCallVariable);
        roundtrip(&Opcode::CallVariable);
    }

    #[test]
    fn roundtrip_threads() {
        roundtrip(&Opcode::ThreadCall(test_id()));
        roundtrip(&Opcode::ThreadStart);
        roundtrip(&Opcode::ThreadDone);
    }

    #[test]
    fn roundtrip_output() {
        roundtrip(&Opcode::EmitLine(0));
        roundtrip(&Opcode::EmitLine(999));
        roundtrip(&Opcode::EmitValue);
        roundtrip(&Opcode::EmitNewline);
        roundtrip(&Opcode::Glue);
        roundtrip(&Opcode::BeginTag);
        roundtrip(&Opcode::EndTag);
        roundtrip(&Opcode::EvalLine(0));
        roundtrip(&Opcode::EvalLine(42));
    }

    #[test]
    fn roundtrip_choices() {
        roundtrip(&Opcode::BeginChoice(
            ChoiceFlags {
                has_condition: true,
                has_start_content: false,
                has_choice_only_content: true,
                once_only: false,
                is_invisible_default: true,
            },
            test_id(),
        ));
        roundtrip(&Opcode::BeginChoice(
            ChoiceFlags {
                has_condition: false,
                has_start_content: true,
                has_choice_only_content: false,
                once_only: true,
                is_invisible_default: false,
            },
            test_id(),
        ));
        roundtrip(&Opcode::EndChoice);
    }

    #[test]
    fn roundtrip_sequences() {
        for kind in [
            SequenceKind::Cycle,
            SequenceKind::Stopping,
            SequenceKind::OnceOnly,
            SequenceKind::Shuffle,
        ] {
            roundtrip(&Opcode::Sequence(kind, 5));
        }
        roundtrip(&Opcode::SequenceBranch(-10));
        roundtrip(&Opcode::SequenceBranch(0));
    }

    #[test]
    fn roundtrip_intrinsics() {
        for op in [
            Opcode::VisitCount,
            Opcode::CurrentVisitCount,
            Opcode::TurnsSince,
            Opcode::TurnIndex,
            Opcode::ChoiceCount,
            Opcode::Random,
            Opcode::SeedRandom,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_casts_math() {
        for op in [
            Opcode::CastToInt,
            Opcode::CastToFloat,
            Opcode::Floor,
            Opcode::Ceiling,
            Opcode::Pow,
            Opcode::Min,
            Opcode::Max,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_call_external() {
        roundtrip(&Opcode::CallExternal(ext_id(), 3));
        roundtrip(&Opcode::CallExternal(ext_id(), 0));
    }

    #[test]
    fn roundtrip_list_ops() {
        for op in [
            Opcode::ListContains,
            Opcode::ListNotContains,
            Opcode::ListIntersect,
            Opcode::ListAll,
            Opcode::ListInvert,
            Opcode::ListCount,
            Opcode::ListMin,
            Opcode::ListMax,
            Opcode::ListValue,
            Opcode::ListRange,
            Opcode::ListFromInt,
            Opcode::ListRandom,
        ] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_lifecycle() {
        for op in [Opcode::Done, Opcode::End, Opcode::Nop] {
            roundtrip(&op);
        }
    }

    #[test]
    fn roundtrip_string_eval() {
        roundtrip(&Opcode::BeginStringEval);
        roundtrip(&Opcode::EndStringEval);
    }

    #[test]
    fn roundtrip_debug() {
        roundtrip(&Opcode::SourceLocation(1, 0));
        roundtrip(&Opcode::SourceLocation(u32::MAX, u32::MAX));
    }

    #[test]
    fn decode_unknown_opcode() {
        let buf = [0xFF];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::UnknownOpcode(0xFF));
    }

    #[test]
    fn decode_unexpected_eof() {
        // PushInt needs 4 more bytes after the discriminant.
        let buf = [PUSH_INT, 0x00];
        let mut offset = 0;
        let err = Opcode::decode(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::UnexpectedEof);
    }

    #[test]
    fn decode_multiple_instructions() {
        let ops = vec![
            Opcode::PushInt(42),
            Opcode::PushBool(true),
            Opcode::Add,
            Opcode::Done,
        ];
        let mut buf = Vec::new();
        for op in &ops {
            op.encode(&mut buf);
        }
        let mut offset = 0;
        for expected in &ops {
            let decoded = Opcode::decode(&buf, &mut offset).unwrap();
            assert_eq!(*expected, decoded);
        }
        assert_eq!(offset, buf.len());
    }

    #[test]
    fn choice_flags_roundtrip() {
        for bits in 0..32u8 {
            let flags = ChoiceFlags::from_byte(bits);
            assert_eq!(flags.to_byte(), bits);
        }
    }
}
