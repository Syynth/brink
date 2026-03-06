use brink_format::{CountingFlags, DefinitionId, NameId};

use crate::{AssignOp, InfixOp, PostfixOp, PrefixOp, SequenceType};

// ─── Program ─────────────────────────────────────────────────────────

/// The complete LIR program — a single merged, resolved representation
/// of all source files, ready for backend consumption.
pub struct Program {
    /// The root container — the top of the container tree.
    /// Child containers (knots, stitches, gathers, choice targets)
    /// are nested via `Container.children`.
    pub root: Container,

    /// Global variable definitions (VAR and CONST), with evaluated defaults.
    pub globals: Vec<GlobalDef>,

    /// List (enum) definitions with their items.
    pub lists: Vec<ListDef>,

    /// Individual list item definitions (each is independently addressable
    /// because bare item names are implicitly global in ink).
    pub list_items: Vec<ListItemDef>,

    /// External function declarations.
    pub externals: Vec<ExternalDef>,

    /// Interned name strings. Indexed by `NameId`. Contains definition
    /// names, variable names, list names, etc. — anything the runtime
    /// needs as a string for debugging, host binding, or inspection.
    pub name_table: Vec<String>,
}

// ─── Definitions ─────────────────────────────────────────────────────

/// A global variable or constant definition with its compile-time default.
pub struct GlobalDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub mutable: bool,
    pub default: ConstValue,
}

/// A list definition.
pub struct ListDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// `(item_name, ordinal)` pairs in declaration order.
    pub items: Vec<(NameId, i32)>,
}

/// A single list item, independently addressable by its `DefinitionId`.
pub struct ListItemDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// The parent list definition this item belongs to.
    pub origin: DefinitionId,
    pub ordinal: i32,
}

/// An external function declaration.
pub struct ExternalDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub arg_count: u8,
    /// Ink-defined fallback body container, if any.
    pub fallback: Option<DefinitionId>,
}

/// A compile-time constant value for global variable defaults and
/// const initializers. These are always statically evaluable.
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

// ─── Containers ──────────────────────────────────────────────────────

/// A single container — the fundamental compilation unit.
///
/// Every knot, stitch, gather, and choice target body is a container.
/// At this level there is no distinction between them — that's what
/// `kind` is for (diagnostics, debug output, and counting flag defaults).
///
/// Containers form a tree: the root contains knots, knots contain stitches
/// and choice/gather children, etc. The `children` vec holds nested containers.
pub struct Container {
    pub id: DefinitionId,
    /// Local name of this container (e.g. `"order"` for stitch `tavern.order`).
    /// `None` for the root container and anonymous gathers.
    pub name: Option<String>,
    pub kind: ContainerKind,
    /// Parameters (only meaningful for knots/stitches/functions).
    pub params: Vec<Param>,
    /// The body — a sequence of structured statements.
    pub body: Vec<Stmt>,
    /// Nested child containers (stitches, choice targets, gathers).
    pub children: Vec<Container>,
    pub counting_flags: CountingFlags,
    /// Total temp slot count for this scope. Only meaningful on scope
    /// roots (knots/functions). Child containers share the parent's
    /// call frame and use slots from this same pool.
    pub temp_slot_count: u16,
}

/// What source construct this container originated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    /// The implicit root container (top-level content before first knot).
    Root,
    /// A `== knot ==` or `== function knot ==`.
    Knot,
    /// A `= stitch` within a knot.
    Stitch,
    /// A gather (`-`) — convergence point after choices.
    Gather,
    /// The body of a selected choice.
    ChoiceTarget,
}

/// A parameter on a container (knot, stitch, or function).
pub struct Param {
    pub name: NameId,
    /// The temp slot index this parameter occupies in the call frame.
    pub slot: u16,
    /// `ref` parameter — caller passes a pointer.
    pub is_ref: bool,
    /// `->` parameter — caller passes a divert target.
    pub is_divert: bool,
}

// ─── Statements ──────────────────────────────────────────────────────

/// A statement within a container body. Structured — branches and
/// choice sets preserve their shape for both backends to consume.
pub enum Stmt {
    /// Emit a line of text content (with optional inline elements and tags).
    EmitContent(Content),

    /// `-> target` — divert to another container, DONE, or END.
    Divert(Divert),

    /// `->-> target` — tunnel call (push return, enter target).
    TunnelCall(TunnelCall),

    /// `<- target` — fork a thread.
    ThreadStart(ThreadStart),

    /// `~ temp x = expr` — declare a temp variable at a slot index.
    DeclareTemp {
        slot: u16,
        name: NameId,
        value: Option<Expr>,
    },

    /// `~ x = expr` / `~ x += expr` — assign to a variable.
    Assign {
        target: AssignTarget,
        op: AssignOp,
        value: Expr,
    },

    /// `~ return expr`
    Return(Option<Expr>),

    /// A set of choices presented to the player.
    ChoiceSet(ChoiceSet),

    /// Multiline `{ - cond: ... }` — block-level conditional.
    Conditional(Conditional),

    /// Multiline `{stopping: - ... - ...}` — block-level sequence.
    Sequence(Sequence),

    /// `~ expr` — expression evaluated for side effects.
    ExprStmt(Expr),
}

/// The resolved target of an assignment.
pub enum AssignTarget {
    Global(DefinitionId),
    Temp(u16),
}

// ─── Control flow ────────────────────────────────────────────────────

/// A divert — goto another container, DONE, or END.
pub struct Divert {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A tunnel call — push return point, enter target.
/// Chained tunnels (`->-> a ->-> b`) produce multiple targets.
pub struct TunnelCall {
    pub targets: Vec<TunnelTarget>,
}

/// A single target in a tunnel call chain.
pub struct TunnelTarget {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A thread fork — `<- target`.
pub struct ThreadStart {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A resolved divert destination.
pub enum DivertTarget {
    /// A named container.
    Container(DefinitionId),
    /// A variable holding a divert target value — `-> x` where `x` is a variable.
    Variable(DefinitionId),
    /// `-> DONE` — pause execution, can resume.
    Done,
    /// `-> END` — permanently end the story.
    End,
}

/// An argument at a call site, with ref-passing resolved.
pub enum CallArg {
    /// A normal value argument.
    Value(Expr),
    /// `ref` argument targeting a global variable — emits `PushVarPointer`.
    RefGlobal(DefinitionId),
    /// `ref` argument targeting a temp variable — emits `PushTempPointer`.
    RefTemp(u16),
}

// ─── Choice sets ─────────────────────────────────────────────────────

/// A set of choices presented to the player, with container boundaries
/// already decided.
pub struct ChoiceSet {
    pub choices: Vec<Choice>,
    /// The gather container that loose-end choices implicitly divert to.
    /// `None` if all choices have explicit diverts.
    pub gather_target: Option<DefinitionId>,
}

/// A single choice within a choice set.
///
/// Display and output content are pre-combined from the HIR's three-part
/// split (start/bracket/inner):
/// - **display** = start + bracket (shown in the choice list)
/// - **output** = start + inner (emitted after selection)
///
/// The choice body lives in a separate `Container` referenced by `target`.
pub struct Choice {
    /// `+` (sticky) vs `*` (once-only).
    pub is_sticky: bool,
    /// Invisible default choice (fallback).
    pub is_fallback: bool,
    /// Condition expression — choice is only available when true.
    pub condition: Option<Expr>,
    /// Text shown in the choice list (start + bracket content).
    pub display: Option<Content>,
    /// Text emitted after the player selects this choice (start + inner content).
    pub output: Option<Content>,
    /// The container holding the choice body (content after selection).
    pub target: DefinitionId,
    pub tags: Vec<String>,
}

// ─── Conditionals and sequences ──────────────────────────────────────

/// A block-level conditional with resolved branch conditions.
pub struct Conditional {
    pub branches: Vec<CondBranch>,
}

/// A single branch in a conditional.
pub struct CondBranch {
    /// `None` for the else branch.
    pub condition: Option<Expr>,
    pub body: Vec<Stmt>,
}

/// A block-level sequence (stopping, cycle, once, shuffle).
pub struct Sequence {
    pub kind: SequenceType,
    pub branches: Vec<Vec<Stmt>>,
}

// ─── Content and inline elements ─────────────────────────────────────

/// A line of text output with inline elements and tags.
///
/// Each `Content` maps to one line table entry in the bytecode output.
/// Backends decide the entry format: plain text for content with no
/// dynamic parts, or a template with slots for interpolated content.
pub struct Content {
    pub parts: Vec<ContentPart>,
    pub tags: Vec<String>,
}

/// A fragment within a content line.
pub enum ContentPart {
    /// Literal text.
    Text(String),
    /// `<>` — glue (suppresses line break).
    Glue,
    /// `{expr}` — interpolated expression, resolved.
    Interpolation(Expr),
    /// `{cond: a | b}` — inline conditional with resolved conditions.
    InlineConditional(Conditional),
    /// `{&a|b|c}` — inline sequence.
    InlineSequence(Sequence),
}

// ─── Expressions ─────────────────────────────────────────────────────

/// A resolved expression. All paths have been replaced with concrete
/// targets (global `DefinitionId`, temp slot, visit count, etc.).
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
    /// Read a temp variable by slot index.
    GetTemp(u16),
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
    /// Call a knot/stitch as a function.
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
    /// Call a built-in function (`TURNS_SINCE`, `LIST_COUNT`, etc.).
    CallBuiltin {
        builtin: BuiltinFn,
        args: Vec<Expr>,
    },
}

/// A string literal, possibly with interpolation.
pub struct StringExpr {
    pub parts: Vec<StringPart>,
}

/// A part of a string literal.
pub enum StringPart {
    /// Literal text.
    Literal(String),
    /// `{expr}` — interpolation within a string, resolved.
    Interpolation(Box<Expr>),
}

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
