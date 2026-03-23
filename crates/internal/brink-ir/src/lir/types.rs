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
    /// Whether this container originated from a source-level label
    /// (e.g. `- (loop)` gather or `* (firstOpt) [text]` choice).
    /// Used by counting flags: labeled containers with visit references
    /// get `COUNT_START_ONLY` so self-goto loops increment correctly.
    pub labeled: bool,
    /// When true, this container is emitted inline in the parent's body
    /// contents rather than as a named entry in `named_content`. Used by
    /// the first container in a gather-choice chain (`- * hello`).
    pub inline: bool,
    /// Whether this knot is a function (`== function foo ==`).
    /// Only meaningful when `kind == ContainerKind::Knot`.
    /// Used by codegen to decide whether inklecate's implicit stitch
    /// prefix (`.0`) should be inserted in container paths.
    pub is_function: bool,
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
    /// A wrapper container for a sequence (stopping, cycle, once, shuffle).
    /// Uses visit counting to select the active branch.
    Sequence,
    /// A single branch body within a sequence wrapper container.
    SequenceBranch,
    /// A single branch body within a block-level conditional.
    ConditionalBranch,
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
#[derive(Clone)]
pub enum Stmt {
    /// Emit a line of text content (with optional inline elements and tags).
    EmitContent(Content),

    /// Emit a recognized line (pattern recognizer matched).
    EmitLine(ContentEmission),

    /// Evaluate a recognized line and push the result onto the value stack.
    /// Used for choice display text that has been promoted to a line table entry.
    EvalLine(ContentEmission),

    /// Emit choice output content (start + inner) at the top of a choice
    /// target container. Emits content parts only — no newline or divert.
    /// The divert and newline are handled by the body stmts that follow.
    ///
    /// Skipped entirely by the JSON codegen — inklecate structures this
    /// content via child container references, not inline.
    ///
    /// When `emission` is `Some`, bytecode codegen uses `EmitLine` for the
    /// recognized line table entry instead of emitting inline content parts.
    ChoiceOutput {
        content: Content,
        emission: Option<ContentEmission>,
    },

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

    /// `~ return expr` (function) or `->->` (tunnel return).
    Return {
        value: Option<Expr>,
        /// When true, emit `TunnelReturn` instead of `Return`.
        is_tunnel: bool,
        /// Arguments for `->-> target(args)` tunnel onwards — pushed before
        /// the divert target value on the value stack.
        args: Vec<CallArg>,
    },

    /// A set of choices presented to the player.
    ChoiceSet(ChoiceSet),

    /// Multiline `{ - cond: ... }` — block-level conditional.
    Conditional(Conditional),

    /// Multiline `{stopping: - ... - ...}` — block-level sequence.
    Sequence(Sequence),

    /// Enter a child container (used for sequence wrappers).
    EnterContainer(brink_format::DefinitionId),

    /// `~ expr` — expression evaluated for side effects.
    ExprStmt(Expr),

    /// End-of-line marker — emitted after content (and any trailing inline
    /// divert on the same line). JSON backend emits `"\n"`, bytecode backend
    /// emits `EmitNewline` opcode.
    EndOfLine,
}

/// The resolved target of an assignment.
#[derive(Clone)]
pub enum AssignTarget {
    Global(DefinitionId),
    Temp(u16, NameId),
}

// ─── Control flow ────────────────────────────────────────────────────

/// A divert — goto another container, DONE, or END.
#[derive(Clone)]
pub struct Divert {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A tunnel call — push return point, enter target.
/// Chained tunnels (`->-> a ->-> b`) produce multiple targets.
#[derive(Clone)]
pub struct TunnelCall {
    pub targets: Vec<TunnelTarget>,
}

/// A single target in a tunnel call chain.
#[derive(Clone)]
pub struct TunnelTarget {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A thread fork — `<- target`.
#[derive(Clone)]
pub struct ThreadStart {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A resolved divert destination.
#[derive(Clone)]
pub enum DivertTarget {
    /// A named address.
    Address(DefinitionId),
    /// A global variable holding a divert target value — `-> x` where `x` is a global variable.
    Variable(DefinitionId),
    /// A temp/parameter variable holding a divert target value — `-> x` where `x` is a parameter.
    VariableTemp(u16, NameId),
    /// `-> DONE` — pause execution, can resume.
    Done,
    /// `-> END` — permanently end the story.
    End,
}

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

// ─── Choice sets ─────────────────────────────────────────────────────

/// A set of choices presented to the player, with container boundaries
/// already decided.
#[derive(Clone)]
pub struct ChoiceSet {
    pub choices: Vec<Choice>,
    /// The gather container that loose-end choices implicitly divert to.
    /// `None` if all choices have explicit diverts.
    pub gather_target: Option<DefinitionId>,
}

/// A single choice within a choice set.
///
/// Content is stored as the original three-part split from the HIR:
/// - `start_content` = text before `[` — shared between display and output
/// - `choice_only_content` = text inside `[...]` — display only
/// - `inner_content` = text after `]` — output only
///
/// The choice body lives in a separate `Container` referenced by `target`.
#[derive(Clone)]
pub struct Choice {
    /// `+` (sticky) vs `*` (once-only).
    pub is_sticky: bool,
    /// Invisible default choice (fallback).
    pub is_fallback: bool,
    /// Condition expression — choice is only available when true.
    pub condition: Option<Expr>,
    /// Text before `[` — appears in both choice list and output.
    pub start_content: Option<Content>,
    /// Text inside `[...]` — appears only in the choice list.
    pub choice_only_content: Option<Content>,
    /// Text after `]` — appears only after selection.
    pub inner_content: Option<Content>,
    /// Recognized display text (start+bracket) for the line table.
    /// `Some` when pattern recognition succeeds on the composed display content.
    pub display_emission: Option<ContentEmission>,
    /// Recognized output text (start+inner) for the line table.
    /// `Some` when pattern recognition succeeds on the composed output content.
    pub output_emission: Option<ContentEmission>,
    /// The container holding the choice body (content after selection).
    pub target: DefinitionId,
    pub tags: Vec<Vec<ContentPart>>,
}

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

/// A block-level sequence (stopping, cycle, once, shuffle).
#[derive(Clone)]
pub struct Sequence {
    pub kind: SequenceType,
    pub branches: Vec<Vec<Stmt>>,
}

// ─── Recognized content (pattern recognizer output) ──────────────────

/// Metadata computed during recognition while HIR provenance is available.
#[derive(Clone)]
pub struct LineMetadata {
    pub source_hash: u64,
    pub slot_info: Vec<brink_format::SlotInfo>,
    pub source_location: Option<brink_format::SourceLocation>,
}

/// A recognized line pattern from content analysis.
#[derive(Clone)]
pub enum RecognizedLine {
    Plain(String),
    Template {
        parts: Vec<brink_format::LinePart>,
        slot_exprs: Vec<Expr>,
    },
}

/// Result of pattern recognition on a content line.
#[derive(Clone)]
pub struct ContentEmission {
    pub line: RecognizedLine,
    pub metadata: LineMetadata,
    pub tags: Vec<Vec<ContentPart>>,
}

// ─── Content and inline elements ─────────────────────────────────────

/// A line of text output with inline elements and tags.
///
/// Each `Content` maps to one line table entry in the bytecode output.
/// Backends decide the entry format: plain text for content with no
/// dynamic parts, or a template with slots for interpolated content.
#[derive(Clone)]
pub struct Content {
    pub parts: Vec<ContentPart>,
    pub tags: Vec<Vec<ContentPart>>,
}

/// A fragment within a content line.
#[derive(Clone)]
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
    /// Enter a child sequence container (inline sequence wrapper).
    EnterSequence(brink_format::DefinitionId),
}

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
