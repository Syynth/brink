use brink_format::{AliasEntry, CountingFlags, DefinitionId, NameId};

use crate::lir::lower::CoalesceShape;
use crate::{AssignOp, InfixOp, PostfixOp, PrefixOp, SequenceType};

// ─── Program ─────────────────────────────────────────────────────────

/// The complete LIR program — a single merged, resolved representation
/// of all source files, ready for backend consumption.
#[derive(Clone)]
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

    /// TM-4c (`docs/typed-mode-spec.md` §6): every declared `STRUCT` shape,
    /// in the order [`lower_to_program`](super::lower_to_program) assigned
    /// their ids (topological file order, then source declaration order
    /// within a file — deterministic, never a `HashMap` iteration order).
    /// `StructShapeDef::id` indexes this `Vec` directly (`id as usize ==`
    /// its own position), so codegen can hand it straight to
    /// `brink_format::StoryData::struct_shapes`.
    pub struct_shapes: Vec<StructShapeDef>,

    /// M-2b (`docs/modules-spec.md` §4): the `DefinitionId`s of every
    /// `#@private` definition, sorted ascending by raw id. Collected from the
    /// resolved [`SymbolIndex`](crate::symbols::SymbolIndex) (whose
    /// per-symbol `visibility` already carries declaration-flips-default),
    /// never from a `HashMap` iteration order. Codegen hands this straight to
    /// `brink_format::StoryData::private_defs`; the runtime uses it to refuse
    /// host semantic access. Empty for the all-public pre-modules world.
    pub private_defs: Vec<DefinitionId>,

    /// M-3 (`docs/modules-spec.md` §5): old→new `DefinitionId` rename
    /// records from every `#@was(old_name)` directive in the project,
    /// sorted by `old` (deterministic regardless of file/symbol iteration
    /// order — codegen hands this straight to
    /// `brink_format::StoryData::alias_table`). Empty unless the source
    /// uses `#@was`.
    pub aliases: Vec<AliasEntry>,
}

/// One declared `STRUCT` shape (TM-4c). Mirrors
/// `brink_format::StructShapeDef` field-for-field; codegen maps this 1:1
/// into that format type. Kept as its own `brink-ir`-local type rather than
/// reusing `brink_format::StructShapeDef` directly, matching this module's
/// existing convention ([`GlobalDef`] vs. `brink_format::GlobalVarDef`,
/// etc.) of never committing the LIR to a specific backend's wire shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructShapeDef {
    pub id: u32,
    pub name: NameId,
    /// Declared fields, in shape declaration order — the order
    /// `RecordNew`/`RecordGet`/`RecordSet` offsets index into.
    pub fields: Vec<NameId>,
}

// ─── Definitions ─────────────────────────────────────────────────────

/// A global variable or constant definition with its compile-time default.
#[derive(Clone)]
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
#[derive(Clone)]
pub struct ListDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// `(item_name, ordinal)` pairs in declaration order.
    pub items: Vec<(NameId, i32)>,
}

/// A single list item, independently addressable by its `DefinitionId`.
#[derive(Clone)]
pub struct ListItemDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// The parent list definition this item belongs to.
    pub origin: DefinitionId,
    pub ordinal: i32,
}

/// An external function declaration.
#[derive(Clone)]
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
    /// A compile-time-constant array (all elements constant-foldable),
    /// destined for the T1b literal pool (`docs/format-v4-rfc.md` §2).
    Array(Vec<ConstValue>),
    /// A compile-time-constant map (all keys/values constant-foldable).
    /// Keys are restricted to the ratified scalar domain by construction —
    /// [`ConstMapKey`] has no variant for anything else.
    Map(Vec<(ConstMapKey, ConstValue)>),
    /// A compile-time-constant record — a `Name#{…}` / `Name { … }`
    /// construction literal used as a `VAR`/`CONST` declaration default
    /// (issue #1530). `shape_id` is the dense `ShapeId` the project's
    /// `ShapeTable` assigned to the named `STRUCT`, and `fields` is in that
    /// shape's **declaration** order — the same flat, shape-ordered layout
    /// `Value::Record` and the `RecordNew` opcode use, so no reordering is
    /// left for codegen to do.
    ///
    /// Only ever well-formed: a literal whose shape doesn't resolve, that
    /// misses a declared field, or that supplies an undeclared one never
    /// reaches this variant (see `decls::eval_const_struct_literal`).
    Record {
        shape_id: u32,
        fields: Vec<ConstValue>,
    },
    /// A zero-bound function value baked into a declaration default
    /// (`VAR f = #fn(name)`), T1c — `docs/t1c-spec.md` §2/§6.
    FnRef(DefinitionId),
    /// A bound function value baked into a declaration default
    /// (`VAR f = #fn(name, args…)`), T1c. `env` is the bound prefix in
    /// declared order.
    Closure {
        target: DefinitionId,
        env: Vec<ConstClosureEntry>,
    },
}

/// One bound-arg entry of a compile-time-constant [`ConstValue::Closure`].
/// The param `name` is kept as a string (not a `NameId`) because it is
/// interned into the story name table at codegen — `const_to_value` dedups it
/// against the target's param names that are already in the table.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstClosureEntry {
    /// A `val` param bound to a compile-time snapshot.
    Val { name: String, value: ConstValue },
    /// A `ref` param bound to a durable cell (a global `VAR`) — codegens to a
    /// `VariablePointer`.
    Ref { name: String, cell: DefinitionId },
}

/// A compile-time-constant map key — the ratified scalar domain
/// (value-model-spec §4: int/string/bool). Kept distinct from [`ConstValue`]
/// so an invalid key type is a *type error at construction*, not a runtime
/// possibility to guard against when emitting the literal pool.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstMapKey {
    Int(i32),
    Str(String),
    Bool(bool),
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent structural flags, not a state machine"
)]
#[derive(Clone)]
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
    /// Flow-private (`#@local`) scope default. Only ever `true` on
    /// knot/stitch containers; interior containers stay `false` (subtree
    /// coverage is resolved by the runtime).
    pub local: bool,
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
#[derive(Clone)]
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

    // ── T1b logic blocks (docs/t1b-surface-spec.md §2) ──────────────
    //
    // `~ { … }` blocks are pure logic — no weave concepts (content, choices,
    // diverts, gathers, threads) ever appear in these bodies; `if`/`else`
    // reuse `Stmt::Conditional` above (identical shape: a list of
    // `(Option<condition>, body)` branches, no sub-containers needed since
    // block bodies never contain choices).
    /// `while cond { … }`. Compiles to a flat backward-jump loop in the
    /// enclosing container's own bytecode (no child container) — loops run
    /// under the existing VM step limit like all bytecode.
    LogicWhile(LogicWhile),
    /// `break` — jump past the innermost enclosing `LogicWhile`.
    LogicBreak,
    /// `continue` — jump to the innermost enclosing `LogicWhile`'s condition
    /// re-check.
    LogicContinue,
}

/// A `while cond { … }` loop body (T1b).
#[derive(Clone)]
pub struct LogicWhile {
    pub condition: Expr,
    pub body: Vec<Stmt>,
    /// Statements that run after `body` completes each iteration, before the
    /// condition re-check — also where `continue` jumps to. Empty for an
    /// author-written `while`; the `for`-loop desugar (§2: `for x in arr`)
    /// puts the index increment here so `continue` still advances the loop
    /// instead of infinite-looping.
    pub post: Vec<Stmt>,
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
    /// A real path-projection `ref` argument (`ref npc.hp`, `ref
    /// party[leader].hp`, T1e-2, `docs/t1e-spec.md` §2/§3) — a durable
    /// global root plus one or more segment expressions, evaluated once
    /// (snapshot-at-creation, spec §1(1)) and emitted as `MakeProjection`.
    /// A bare zero-segment `ref x` never reaches this variant — it lowers
    /// exactly like today's unmarked ref-argument binding
    /// ([`RefGlobal`]/[`RefTemp`]).
    RefProjection {
        root: DefinitionId,
        segments: Vec<Expr>,
    },
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
    /// Word-break spring — conditional space resolved by the runtime.
    Spring,
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
    /// Move a global's current value out, leaving `Value::Null` behind —
    /// the take-half of the take → `make_mut` → write-back RMW discipline
    /// (`docs/value-model-spec.md` §5). Never produced by ordinary
    /// expression lowering; only by indexed-assignment/mutator RMW
    /// desugaring's bare-variable fast path (`lir::lower::blocks`).
    TakeGlobal(DefinitionId),
    /// Move a temp's current value out, leaving `Value::Null` behind —
    /// `TakeGlobal`'s temp-slot counterpart. Same production sites.
    TakeTemp(u16, NameId),
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
    /// Every `InfixOp` except [`InfixOp::Coalesce`] — that one variant is
    /// special-cased at lowering time and never reaches this generic form;
    /// see [`Coalesce`](Self::Coalesce)'s own doc for why.
    Infix(Box<Expr>, InfixOp, Box<Expr>),
    Postfix(Box<Expr>, PostfixOp),
    /// One step of `x or default` (B1, `docs/stdlib-spec.md` §1.6a, issue
    /// #1460), short-circuited per issue #1471's ruling —
    /// `hir::Expr::Infix(_, InfixOp::Coalesce, _)`'s dedicated lowering,
    /// never folded into the generic [`Infix`](Self::Infix) form the way
    /// every other `InfixOp` is. Codegens to a real branch
    /// (`Opcode::CoalesceSome`) rather than a binary opcode, so `rhs` is
    /// only evaluated when `lhs` turns out to be `none` — a binary opcode
    /// cannot do that, since both operands would already be on the stack
    /// before it ran.
    ///
    /// `shape` is the collapse-vs-two-Option decision the ruled typing
    /// makes (`(Option[T],T)->T` vs `(Option[T],Option[T])->Option[U]`).
    /// It has to be decided *before* the branch runs: the retired binary
    /// opcode read the answer off `rhs`'s actual runtime value, but
    /// short-circuiting means `rhs` may never run by the time the join
    /// point needs to know. So lowering **consumes the analyzer's recorded
    /// verdict** for the step (`ctx.tables.coalesce`, keyed at the chain root —
    /// RULED 2026-07-26, `docs/decision-log.md` "Lowering consumes analyzer
    /// types") rather than sniffing `rhs`'s syntax, which could not see
    /// through a call's return type or an `Option`-typed local anyway.
    Coalesce {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        shape: CoalesceShape,
    },

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

    // ── Function values (T1c, docs/t1c-spec.md §2/§3) ───────────────
    /// `#fn(target, bound…)` — create a function value. With no bound args
    /// this codegens to `PushFnRef` (a zero-bound `FnRef`); with bound args
    /// to `MakeClosure` (a `Closure`). `bound` is the bound prefix in declared
    /// order — a `ref` param is a [`CallArg::RefGlobal`] (a captured durable
    /// cell), a `val` param a [`CallArg::Value`] snapshot.
    MakeFnValue {
        target: DefinitionId,
        bound: Vec<CallArg>,
    },
    /// Call *through* a function value: the direct form `f(args…)` where `f`
    /// holds a fn value, and the explicit `call(f, args…)` form. `callee`
    /// evaluates to the function value; `args` are the supplied (val-only)
    /// remaining params. Codegens to `CallValue(argc)`.
    CallValue {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `bind(f, args…)` — the val-only currying stdlib intrinsic (T1c-3,
    /// docs/t1c-spec.md §3). `callee` evaluates to the function value being
    /// curried; `args` are the val-only args appended to its bound-arg row
    /// (consuming the head of its remaining param row). Codegens to
    /// `BindValue(argc)`; over-binding is a runtime fault at the op.
    BindValue {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },

    // ── Collections (T1b, docs/t1b-surface-spec.md §3-4) ────────────
    /// A compile-time-constant collection literal — emitted via the V4
    /// literal pool (`PushLiteral(idx)`), deduplicated at codegen.
    ConstLiteral(ConstValue),
    /// `#[e0, e1, …]` where at least one element is not constant-foldable —
    /// evaluates each element then `ArrayNew(n)`.
    ArrayNew(Vec<Expr>),
    /// `#{k0: v0, …}` where at least one entry is not constant-foldable —
    /// evaluates each key/value pair then `MapNew(n)` (n = pair count).
    MapNew(Vec<(Expr, Expr)>),
    /// `base[index]` (read). Turn-terminating fault on out-of-bounds array
    /// index or missing map key (value-model-spec §6).
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `IndexSet` as an expression: evaluates `base`, `index`, `value` and
    /// pushes the *updated* container — the take → `make_mut` → write-back
    /// primitive that indexed-assignment lowering composes (`docs/t1b-
    /// surface-spec.md` §4). Never produced by ordinary expression lowering;
    /// only by indexed-assignment desugaring.
    IndexSet {
        base: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },
    /// `[container]` → `Int` length. Internal — used by `for`-loop lowering
    /// over arrays; not surfaced to authors until the `len()` stdlib
    /// function lands (T1b-3).
    CollectionLen(Box<Expr>),
    /// `[map]` → `Array` of keys in insertion order. Internal — used by
    /// `for`-loop lowering over maps; not surfaced to authors until the
    /// `keys()` stdlib function lands (T1b-3).
    CollectionKeys(Box<Expr>),
    /// `[map]` → `Array` of values in insertion order. The `values()`
    /// stdlib pure function (T1b-3, `docs/t1b-surface-spec.md` §5).
    CollectionValues(Box<Expr>),
    /// `[container, needle]` → `Bool`. The `contains(x, v)` stdlib pure
    /// function (T1b-3, §5) — arrays: element containment; maps: key
    /// containment.
    CollectionContains {
        container: Box<Expr>,
        needle: Box<Expr>,
    },
    /// `IndexSet`'s sibling for the `push`/`insert` stdlib mutators (T1b-3,
    /// §5): evaluates `base`, `key`, `value` and pushes the *updated*
    /// container — arrays: `Vec::insert(key, value)` (key is an index,
    /// `<= len` inclusive so `push(a, v)` can lower to `insert(a, len(a),
    /// v)`); maps: insert-or-overwrite by key. Never produced by ordinary
    /// expression lowering; only by the mutator-statement desugaring
    /// (`lir::lower::blocks`), which follows the same take → `make_mut` →
    /// write-back RMW discipline `IndexSet` uses.
    CollectionInsert {
        base: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
    },
    /// `IndexSet`'s sibling for the `remove` stdlib mutator (T1b-3, §5):
    /// evaluates `base`, `key` and pushes the *updated* container — remove
    /// by key, no-op if absent. **Map-only as of issue #1484**: `remove`
    /// uniformly names identity-based, idempotent-total removal; a
    /// non-map `base` is a runtime fault (`NotIndexable`). Never produced
    /// by ordinary expression lowering; only by the mutator-statement
    /// desugaring. The array-index leg this used to cover is
    /// [`SeqRemoveAt`](Self::SeqRemoveAt).
    CollectionRemove {
        base: Box<Expr>,
        key: Box<Expr>,
    },
    /// `IndexSet`'s sibling for the `remove_at` stdlib mutator (issue
    /// #1484, joining the `_at` faulting-index family with `CharAt`):
    /// evaluates `base`, `index` and pushes the *updated* array with the
    /// element at `index` removed (shifts later elements left,
    /// `Vec::remove`). Array-only: a non-array `base` is a runtime fault
    /// (`NotIndexable`). `index` must be `< len` — out-of-range faults
    /// (`IndexOutOfBounds`). Never produced by ordinary expression
    /// lowering; only by the mutator-statement desugaring.
    SeqRemoveAt {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `[s, i]` → single-character `String`. The `char_at(s, i)` stdlib pure
    /// function (T1b stdlib slice 1 completion, issue #857): `i` indexes
    /// Unicode scalar values ("chars"), not UTF-8 bytes — author sanity, per
    /// the issue. Turn-terminating fault on `i` out of `[0, char_count)`
    /// (value-model-spec §11c: no silent garbage), matching `Index`'s
    /// out-of-bounds fault posture.
    CharAt {
        s: Box<Expr>,
        index: Box<Expr>,
    },

    // ── NS-A1: Option[T] + the ruled stdlib flips (`docs/stdlib-spec.md`
    // §1.4, §§3-5; issue #1107) ─────────────────────────────────────────
    /// The bare `none` Option literal — `Opcode::PushNone`. Produced by an
    /// unresolved single-segment `none` path in the brink dialect (an
    /// author symbol of the same name always wins, E035-warned).
    OptionNone,
    /// `some(x)` — `Opcode::MakeSome`, total over every value.
    OptionSome(Box<Expr>),
    /// The `as` binding's condition (B1b, issue #1475) —
    /// `Opcode::OptionBind(slot)`. Evaluates `value` (an `Option[T]`) and
    /// yields the **bool** the enclosing `if`/`while`/`{if}` branches on,
    /// writing the unwrapped payload into temp `slot` on the `some` path
    /// and leaving the slot untouched on `none`.
    ///
    /// The test and the bind are one opcode on purpose: it keeps the
    /// binding entirely inside condition evaluation, so `while EXPR as n`
    /// rebinds per iteration for free (the condition is re-evaluated), an
    /// inline `{if EXPR as n: …}` needs no statement hoisted out of its
    /// content line, and `value` is evaluated exactly once in every form.
    /// The only producer is `as`-binding lowering (`lir::lower::blocks`,
    /// `lir::lower::content`); nothing in the ink/brink dialects can reach
    /// it.
    OptionBind {
        value: Box<Expr>,
        slot: u16,
        name: NameId,
    },
    /// `[s, sub]` → `Option[int]`. The `find(s, sub)` stdlib pure function
    /// (§3, martyr #1 redeemed): USV index of the first occurrence, `none`
    /// when absent.
    StrFind {
        s: Box<Expr>,
        sub: Box<Expr>,
    },
    /// `[a, x]` → `Option[int]`. The `index_of(a, x)` stdlib pure function
    /// (§4, martyr #2 redeemed): structural-equality scan, `none` absent.
    SeqIndexOf {
        seq: Box<Expr>,
        needle: Box<Expr>,
    },
    /// `[a]` → `Option[T]`. The `min(a)` stdlib pure function (§4/§4b —
    /// empty → `none`; float NaN is mode-dependent since NS-A4: dev-mode
    /// fault / prod-mode pinned placement, decided at the runtime knob).
    SeqMin(Box<Expr>),
    /// `[a]` → `Option[T]`. The `max(a)` stdlib pure function.
    SeqMax(Box<Expr>),
    /// `[a]` → `Option[T]`. The `first(a)` stdlib pure function (§4).
    SeqFirst(Box<Expr>),
    /// `[a]` → `Option[T]`. The `last(a)` stdlib pure function (§4).
    SeqLast(Box<Expr>),
    /// `pop(a)` (§4): mutates its bare-lvalue receiver in place AND
    /// produces `Option[T]` (the removed last element; empty → `none`) —
    /// the one A1 verb that is both mutator and expression. Codegen emits
    /// the take → `SeqPop` → store-back bracket against `root` directly
    /// (`TakeGlobal`/`TakeTemp` … `SetGlobal`/`SetTemp`), so the shrunk
    /// array writes back to the root cell and the Option remains on the
    /// stack as the expression's value. Restricted at lowering to a bare
    /// variable/temp receiver (a chained lvalue like `pop(grid[0])` is the
    /// E055-family error for now — scope fence, see the A1 PR notes).
    SeqPop {
        root: AssignTarget,
    },
    /// `[m, k]` → `Option[V]`. The `get(m, k)` stdlib pure function (§5,
    /// martyr #3 redeemed): missing key → `none`; the faulting `m[k]`
    /// (`Index`) stays the "I expect it there" read.
    MapGetOpt {
        map: Box<Expr>,
        key: Box<Expr>,
    },
    /// `[m, v]` → `Bool`. The `contains_value(m, v)` stdlib pure function
    /// (§5): content-equality scan over values, honest O(n).
    MapContainsValue {
        map: Box<Expr>,
        value: Box<Expr>,
    },
    /// `CollectionRemove`'s sibling for the `clear` stdlib mutator (§5):
    /// evaluates `base` (a map) and pushes the *emptied* container. Never
    /// produced by ordinary expression lowering; only by the
    /// mutator-statement desugaring (`lir::lower::blocks`), same RMW
    /// write-back discipline as `CollectionInsert`/`CollectionRemove`.
    MapClear(Box<Expr>),

    // ── NS-A4: the ordering verbs (`docs/stdlib-spec.md` §4b, issue
    // #1110) ────────────────────────────────────────────────────────────
    /// `Opcode::SeqSorted`: evaluates an array, pushes it sorted ascending
    /// by the §4b doctrine order (stable; dev-mode NaN fault / prod pinned
    /// placement at the runtime knob). Two surfaces share it: `sorted(a)`
    /// (functional, ordinary expression lowering) and `sort(a)`
    /// (statement-only mutator, RMW write-back via `lir::lower::blocks`) —
    /// the `RandShuffle` precedent.
    SeqSorted(Box<Expr>),
    /// `Opcode::SeqSortedBy`: evaluates an array and a comparator function
    /// value (`fn(T, T): int`, F0), pushes the array sorted by the
    /// comparator (stable; re-entrant VM evaluation at the op). Two
    /// surfaces: `sorted_by(a, cmp)` (functional) and `sort_by(a, cmp)`
    /// (statement-only mutator, RMW write-back).
    SeqSortedBy {
        seq: Box<Expr>,
        cmp: Box<Expr>,
    },

    // ── The fn-value verb layer (`docs/stdlib-spec.md` §4, issue #1679) ──
    /// `Opcode::SeqVerb(Map)`: evaluates an array and a transform function
    /// value (`fn(T): U`), pushes the array of results in iteration order.
    /// The callback is pure-required (RULED 2026-07-18) — which is exactly
    /// what makes "one logical pass, order unobservable" true, so nothing
    /// downstream may observe how many passes the runtime actually makes.
    SeqMap {
        seq: Box<Expr>,
        f: Box<Expr>,
    },
    /// `Opcode::SeqVerb(Filter)`: evaluates an array and a predicate
    /// function value (`fn(T): bool`), pushes the retained elements in
    /// iteration order. Pure-required callback, as [`Expr::SeqMap`].
    SeqFilter {
        seq: Box<Expr>,
        pred: Box<Expr>,
    },
    /// `Opcode::SeqVerb(Fold)`: evaluates an array, an initial accumulator,
    /// and a combining function value (`fn(U, T): U`), pushes the final
    /// accumulator. Left fold in iteration order; pure-required callback.
    /// Operands are pushed `seq`, `init`, `f` — the runtime pops in
    /// reverse.
    SeqFold {
        seq: Box<Expr>,
        init: Box<Expr>,
        f: Box<Expr>,
    },

    // ── NS-A8: the numeric tower (`docs/tower-mini-spec.md`, issue
    // #1114) ────────────────────────────────────────────────────────────
    /// One node for the whole tower family — `Opcode::Tower(op)` after the
    /// args are pushed left-to-right. Constructors (`vec2(x, y)` …
    /// `mat4(c0, c1, c2, c3)`), `dot`/`cross`, and the tower-wide
    /// two-arg `min`/`max` plus `clamp(x, lo, hi)`/`lerp(a, b, t)`. All
    /// pure; arity is checked at lowering (E031), operand *kinds* at
    /// runtime (`StdlibWrongType` — a malformed question faults, per the
    /// ruled doctrine). The `+`/`-`/`*` operator family lowers through the
    /// ordinary binary ops, not this node.
    Tower {
        op: brink_format::TowerOp,
        args: Vec<Expr>,
    },

    // ── NS-A7: `Weighted[T]` + the humble heap (`docs/stdlib-spec.md`
    // §8, issue #1113) ──────────────────────────────────────────────────
    /// `weighted(w1, v1, w2, v2, …)` → `Weighted[T]` —
    /// `Opcode::Collect(WeightedNew)` after the flattened pair row is
    /// pushed and gathered by an `ArrayNew(2n)` (a transient codegen
    /// artifact, never observable). The compile-classifiable refusals
    /// (empty/odd row, literal non-positive-int weight) are E120 at
    /// lowering; computed weights carry the construction-fault residual at
    /// the op (`WeightedBadWeight`) — the E078-style split, so a table
    /// that exists is always rollable. Pairs are `(weight, value)` in
    /// construction order (order is semantic for display and the roll
    /// walk; equality alone is the F17 multiset).
    WeightedNew {
        pairs: Vec<(Expr, Expr)>,
    },
    /// `roll(w)` → `T` — `Opcode::Collect(RandRoll)`: one weighted draw
    /// from a `Weighted[T]` table. Total over any table that exists
    /// (construction is the validator); a draw, so its row writes the RNG
    /// cell like the NS-A6 verbs below.
    RandRoll(Box<Expr>),
    /// `Opcode::Collect(HeapPush)`: evaluates an array and an element,
    /// pushes the array with the element sifted into the §4b min-heap
    /// position (dev-mode NaN entry fault / prod pinned placement at the
    /// runtime knob). Statement-only mutator surface (`heap_push(a, x)`,
    /// RMW write-back via `lir::lower::blocks`) — never produced by
    /// ordinary expression lowering (E056 there).
    HeapPush {
        seq: Box<Expr>,
        value: Box<Expr>,
    },
    /// `heap_pop(a)` (§8): mutates its bare-lvalue receiver in place AND
    /// produces `Option[T]` (the extracted minimum; empty → `none`) — the
    /// `SeqPop` shape exactly: codegen emits the take →
    /// `Collect(HeapPop)` → store-back bracket against `root`, the op
    /// pushes the Option under the re-heapified array, and the store
    /// leaves the Option as the expression value. Same bare-receiver
    /// restriction (E055 on anything else — the A1 scope fence).
    HeapPop {
        root: AssignTarget,
    },
    /// `heap_peek(a)` → `Option[T]` — `Opcode::Collect(HeapPeek)`: the
    /// minimum without extraction (`none` on empty). Pure read.
    HeapPeek(Box<Expr>),

    // ── NS-A6: the `std::rand` draw verbs (`docs/stdlib-spec.md` §7,
    // issue #1112). Every one is a write to the RNG cell in the effect
    // row; `seed(n)` has no variant here — it lowers to the frozen
    // `CallBuiltin(SeedRandom)` (one cell, two surfaces, no drift). ─────
    /// `float()` (nullary) → `Float` in `[0,1)` — `Opcode::RandFloat`. One
    /// draw. The unary `float(x)` spelling stays `ConvertFloat`;
    /// disambiguated by arity at lowering (F4, resolved in-wave).
    RandFloat,
    /// `chance(p)` → `Bool` — `Opcode::RandChance`. `p` clamped to
    /// `[0,1]`, NaN → `false` (F3, ruled 2026-07-19); one draw always.
    RandChance(Box<Expr>),
    /// `pick(coll)` → `Option[T]` — `Opcode::RandPick`. Uniform draw from
    /// an array, flags subset, or range (NS-A5); empty → `none`.
    RandPick(Box<Expr>),
    /// The Fisher–Yates primitive — `Opcode::RandShuffle`: evaluates an
    /// array, pushes the shuffled array. Two surfaces share it:
    /// `shuffled(a)` (functional, ordinary expression lowering) and
    /// `shuffle(a)` (statement-only mutator, RMW write-back via
    /// `lir::lower::blocks` exactly like `MapClear`).
    RandShuffle(Box<Expr>),

    // ── NS-A5: range values + the inhabited-range refinement
    // (`docs/stdlib-spec.md` §7, F7/F8, issue #1111). ────────────────
    /// `start..end` / `start..=end` — `Opcode::RangeMakeExcl`/
    /// `RangeMakeIncl`: evaluates both bounds (ints — the op faults on
    /// anything else), pushes the range value. `int(range)` has no
    /// variant of its own — the unary `int(x)` spelling stays
    /// `ConvertInt`, whose VM op dispatches on the operand (range →
    /// draw, else conversion).
    RangeMake {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    /// `non_empty(r)` → `Option[Range]` — `Opcode::RangeNonEmpty`: the
    /// inhabited-range validator (S2), `some(r)` iff `r` denotes at least
    /// one element. Pure — no draw.
    RangeNonEmpty(Box<Expr>),

    // ── Records (TM-4c, `docs/typed-mode-spec.md` §6) ───────────────
    /// `Name#{field: expr, …}` construction. `fields` is in the exact order
    /// `RecordNew`'s VM opcode expects the values pushed — the shape's
    /// *declaration* order for a well-formed literal, or source order for
    /// the construction-fault sentinel path (`lower_struct_literal`'s doc).
    /// `prelude` runs first (source order — the author's left-to-right
    /// order), staging every supplied initializer's value into a synthetic
    /// temp slot *before* any `fields` entry is pushed — codegen emits each
    /// `prelude` triple as `<expr bytecode>; DeclareTemp(slot)` in order,
    /// then `fields` (which, on the well-formed path, are `GetTemp` reads
    /// of those slots reordered into shape order). This decouples
    /// evaluation order from placement order (issue #676): shape order is
    /// a memory-layout concern for `fields`, never an evaluation-order one.
    /// Empty on the construction-fault path, where `fields` already *is*
    /// source order and no reordering — hence no staging — is needed.
    RecordNew {
        shape_id: u32,
        fields: Vec<Expr>,
        prelude: Vec<(u16, NameId, Expr)>,
    },
    /// `base.field` (read). `static_offset: Some(offset)` when
    /// `brink-ir`'s LIR lowering proved `base`'s shape at compile time
    /// (construction-literal chains, or a `types = strict` project's
    /// struct-typed `VAR`/`temp` annotation — see typed-mode-spec §6);
    /// codegen emits the static `RecordGet` offset op then, the by-name
    /// `RecordGetDyn` otherwise. `field` is always populated (even when
    /// `static_offset` is `Some`) so codegen/tooling never needs the
    /// `struct_shapes` table just to describe this node.
    RecordGet {
        base: Box<Expr>,
        field: NameId,
        static_offset: Option<u16>,
    },
    /// `RecordGet`'s RMW write-back sibling — the primitive single-level
    /// `p.field = expr` lowering composes (mirrors `IndexSet`): evaluates
    /// `base`, `value` and pushes the *updated* record. Never produced by
    /// ordinary expression lowering; only by field-assignment desugaring.
    RecordSet {
        base: Box<Expr>,
        field: NameId,
        static_offset: Option<u16>,
        value: Box<Expr>,
    },

    // ── TM-3 completion: conversion intrinsics (typed-mode-spec §4,
    // maintainer ruling 2026-07-13, issue #659) ─────────────────────────
    /// `int(x)` pure conversion intrinsic. Domains: `Int` (identity),
    /// `Float` (truncate toward zero, matching vanilla ink's `INT()`
    /// exactly), `Bool` (`true` → 1, `false` → 0), `String` (parse).
    /// Turn-terminating fault on parse failure or an out-of-domain input
    /// (divert/LIST/array/map/record) — value-model-spec §11c.
    ConvertInt(Box<Expr>),
    /// `float(x)` pure conversion intrinsic. Domains: `Float` (identity),
    /// `Int` (widen), `Bool` (`true` → 1.0, `false` → 0.0), `String`
    /// (parse). Same fault domain as `ConvertInt`.
    ConvertFloat(Box<Expr>),
    /// `string(x)` pure conversion intrinsic — display form, identical to
    /// interpolation. Total over every value; never faults.
    ConvertString(Box<Expr>),
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
