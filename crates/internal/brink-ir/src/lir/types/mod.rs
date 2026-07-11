//! LIR type definitions, split along the logic/narrative waist (#397).
//!
//! The LIR visibly divides into a *logic* half — a small procedural IR of
//! expressions, assignments, conditionals, and calls that is barely
//! ink-specific — and a *narrative* half of weave concepts: choices,
//! diverts, tunnels, threads, sequences, and content lines. The logic half
//! is the natural waist for any future second frontend; the hygiene rule is
//! that no weave concept leaks into [`Expr`]/[`Stmt`]'s logic vocabulary as
//! the logic subset grows.
//!
//! This module keeps the spine that joins the halves — [`Program`],
//! [`Container`], and the [`Stmt`] enum — and re-exports both halves
//! flatly, so consumers keep importing everything from `lir::*` unchanged.

mod logic;
mod narrative;

pub use logic::*;
pub use narrative::*;

use brink_format::{CountingFlags, DefinitionId, NameId};

use crate::AssignOp;

// ─── Program ─────────────────────────────────────────────────────────

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
}

// ─── Containers ──────────────────────────────────────────────────────

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
