//! The **logic half** of the LIR (#397 waist): declarations, expressions,
//! assignments, conditionals, and call machinery. A small procedural IR
//! that is barely ink-specific — the subset a second (scripting) frontend
//! would target. Keep weave concepts out of this module.

use brink_format::{DefinitionId, NameId};

use crate::{InfixOp, PostfixOp, PrefixOp};

use super::Stmt;

// ─── Definitions ─────────────────────────────────────────────────────

// ─── Definitions ─────────────────────────────────────────────────────

/// A global variable or constant definition with its compile-time default.
#[derive(Clone, PartialEq)]
pub struct GlobalDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub mutable: bool,
    pub default: ConstValue,
    /// Flow-private (`#@local`) scope default. Always `false` for CONSTs
    /// and list globals.
    pub local: bool,
}

/// A list definition.
#[derive(Clone, PartialEq)]
pub struct ListDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// `(item_name, ordinal)` pairs in declaration order.
    pub items: Vec<(NameId, i32)>,
}

/// A single list item, independently addressable by its `DefinitionId`.
#[derive(Clone, PartialEq)]
pub struct ListItemDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// The parent list definition this item belongs to.
    pub origin: DefinitionId,
    pub ordinal: i32,
}

/// An external function declaration.
#[derive(Clone, PartialEq)]
pub struct ExternalDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub arg_count: u8,
    /// Ink-defined fallback body container, if any.
    pub fallback: Option<DefinitionId>,
}

/// A compile-time constant value for global variable defaults and
/// const initializers. These are always statically evaluable.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
    /// A list value — the set of active items, plus origin lists
    /// for typed empties.
    List {
        items: Vec<DefinitionId>,
        origins: Vec<DefinitionId>,
    },
    DivertTarget(DefinitionId),
    Null,
}

// ─── Assignment targets ──────────────────────────────────────────────

/// The resolved target of an assignment.
#[derive(Clone)]
pub enum AssignTarget {
    Global(DefinitionId),
    Temp(u16, NameId),
}

// ─── Call arguments ──────────────────────────────────────────────────

/// An argument at a call site, with ref-passing resolved.
#[derive(Clone)]
pub enum CallArg {
    /// A normal value argument.
    Value(Expr),
    /// `ref` argument targeting a global variable — emits `PushVarPointer`.
    RefGlobal(DefinitionId),
    /// `ref` argument targeting a temp variable — emits `PushTempPointer`.
    RefTemp(u16, NameId),
}

// ─── Conditionals ────────────────────────────────────────────────────

// ─── Conditionals and sequences ──────────────────────────────────────

/// Distinguishes the semantic forms of conditional blocks in LIR.
#[derive(Clone)]
pub enum CondKind {
    /// The first branch's condition is the initial condition of the
    /// conditional itself (emitted flat, not wrapped in the branch container).
    InitialCondition,
    /// Each branch has an independent boolean condition (wrapped inside
    /// its own container).
    IfElse,
    /// One expression evaluated once; each branch is a case value compared with `==`.
    Switch(Expr),
}

/// A block-level conditional with resolved branch conditions.
#[derive(Clone)]
pub struct Conditional {
    pub kind: CondKind,
    pub branches: Vec<CondBranch>,
}

/// A single branch in a conditional.
#[derive(Clone)]
pub struct CondBranch {
    /// `None` for the else branch.
    pub condition: Option<Expr>,
    pub body: Vec<Stmt>,
}

// ─── Expressions ─────────────────────────────────────────────────────

// ─── Expressions ─────────────────────────────────────────────────────

/// A resolved expression. All paths have been replaced with concrete
/// targets (global `DefinitionId`, temp slot, visit count, etc.).
#[derive(Clone)]
pub enum Expr {
    // ── Literals ─────────────────────────────────────────────────
    Int(i32),
    Float(f32),
    Bool(bool),
    String(StringExpr),
    Null,

    // ── Resolved references ─────────────────────────────────────
    /// Read a global variable (VAR, CONST, or list variable).
    GetGlobal(DefinitionId),
    /// Read a temp variable by slot index and name.
    GetTemp(u16, NameId),
    /// The visit count of a container (knot/stitch/label name used
    /// in expression context).
    VisitCount(DefinitionId),
    /// A divert target as a value (`-> knot` in expression context).
    DivertTarget(DefinitionId),
    /// A list literal — set of active item `DefinitionId`s, plus
    /// origin list `DefinitionId`s for typed empties.
    ListLiteral {
        items: Vec<DefinitionId>,
        origins: Vec<DefinitionId>,
    },

    // ── Operations ──────────────────────────────────────────────
    Prefix(PrefixOp, Box<Expr>),
    Infix(Box<Expr>, InfixOp, Box<Expr>),
    Postfix(Box<Expr>, PostfixOp),

    // ── Calls ───────────────────────────────────────────────────
    /// Call a knot/stitch as a function (ink `== function`).
    Call {
        target: DefinitionId,
        args: Vec<CallArg>,
    },
    /// Call an external function.
    CallExternal {
        target: DefinitionId,
        args: Vec<CallArg>,
        arg_count: u8,
    },
    /// Call a function through a global variable holding a divert target.
    CallVariable {
        target: DefinitionId,
        args: Vec<CallArg>,
    },
    /// Call a function through a temp/param variable holding a divert target.
    CallVariableTemp {
        slot: u16,
        name: NameId,
        args: Vec<CallArg>,
    },
    /// Call a built-in function (`TURNS_SINCE`, `LIST_COUNT`, etc.).
    CallBuiltin {
        builtin: BuiltinFn,
        args: Vec<Expr>,
    },
}

impl Expr {
    /// Returns true if this expression is a function call that may produce
    /// localized text output (`Call`, `CallVariable`, `CallVariableTemp`, `CallExternal`).
    /// Builtins (`TURNS_SINCE`, `LIST_COUNT`, etc.) are not included — they
    /// produce numeric/list values, not localized text.
    pub fn is_function_call(&self) -> bool {
        matches!(
            self,
            Self::Call { .. }
                | Self::CallVariable { .. }
                | Self::CallVariableTemp { .. }
                | Self::CallExternal { .. }
        )
    }
}

/// A string literal, possibly with interpolation.
#[derive(Clone)]
pub struct StringExpr {
    pub parts: Vec<StringPart>,
}

/// A part of a string literal.
#[derive(Clone)]
pub enum StringPart {
    /// Literal text.
    Literal(String),
    /// `{expr}` — interpolation within a string, resolved.
    Interpolation(Box<Expr>),
}

// ─── Built-in functions ──────────────────────────────────────────────

// ─── Built-in functions ──────────────────────────────────────────────

/// Ink built-in functions that compile to dedicated opcodes rather
/// than container calls.
///
/// These are recognized by name during HIR → LIR lowering. The analyzer
/// does not resolve them (they have no declaration) — LIR lowering
/// intercepts `Expr::Call` nodes whose paths match known built-in names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinFn {
    // ── Intrinsics ──────────────────────────────────────────────
    /// `TURNS_SINCE(-> container)` → turns since container was visited.
    TurnsSince,
    /// `READ_COUNT(-> container)` → visit count of container.
    ReadCount,
    /// `TURNS()` → current turn index (0-based).
    Turns,
    /// `CHOICE_COUNT()` → number of currently available choices.
    ChoiceCount,
    /// `RANDOM(min, max)` → random integer in range.
    Random,
    /// `SEED_RANDOM(seed)` → seed the RNG.
    SeedRandom,

    // ── Casts ───────────────────────────────────────────────────
    /// `INT(x)` → cast to integer.
    CastToInt,
    /// `FLOAT(x)` → cast to float.
    CastToFloat,

    // ── Math ────────────────────────────────────────────────────
    /// `FLOOR(x)` → floor.
    Floor,
    /// `CEILING(x)` → ceiling.
    Ceiling,
    /// `POW(a, b)` → exponentiation.
    Pow,
    /// `MIN(a, b)` → minimum.
    Min,
    /// `MAX(a, b)` → maximum.
    Max,

    // ── List operations ─────────────────────────────────────────
    /// `LIST_COUNT(list)` → number of set items.
    ListCount,
    /// `LIST_MIN(list)` → item with lowest ordinal.
    ListMin,
    /// `LIST_MAX(list)` → item with highest ordinal.
    ListMax,
    /// `LIST_ALL(list)` → all items from the list's origin.
    ListAll,
    /// `LIST_INVERT(list)` → complement (all items NOT in the set).
    ListInvert,
    /// `LIST_RANGE(list, min, max)` → subset by ordinal range.
    ListRange,
    /// `LIST_RANDOM(list)` → random item from the set.
    ListRandom,
    /// `LIST_VALUE(item)` → ordinal value as integer.
    ListValue,
    /// `LIST_FROM_INT(list_origin, value)` → item by ordinal.
    ListFromInt,
}
