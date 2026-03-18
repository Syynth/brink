use brink_syntax::ast::{self, AstPtr, SyntaxNodePtr};
use rowan::TextRange;

// ─── File identity ──────────────────────────────────────────────────

/// Opaque identifier for a source file within a multi-file project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

// ─── Source provenance ──────────────────────────────────────────────

/// A named identifier with provenance back to the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub text: String,
    pub range: TextRange,
}

/// A dotted path (e.g. `knot.stitch.label`), unresolved at the HIR level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub segments: Vec<Name>,
    pub range: TextRange,
}

/// A tag attached to content — may contain dynamic inline expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub parts: Vec<ContentPart>,
    pub ptr: AstPtr<ast::Tag>,
}

// ─── Root ───────────────────────────────────────────────────────────

/// The HIR of a single `.ink` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirFile {
    /// Top-level content before the first knot.
    pub root_content: Block,
    /// All knot definitions in the file.
    pub knots: Vec<Knot>,
    /// `VAR` declarations.
    pub variables: Vec<VarDecl>,
    /// `CONST` declarations.
    pub constants: Vec<ConstDecl>,
    /// `LIST` declarations.
    pub lists: Vec<ListDecl>,
    /// `EXTERNAL` declarations.
    pub externals: Vec<ExternalDecl>,
    /// `INCLUDE` sites (for cross-file resolution by the analyzer).
    pub includes: Vec<IncludeSite>,
}

// ─── Containers ─────────────────────────────────────────────────────

/// Pointer back to the AST node that defined a knot-level container.
///
/// A `Knot` can originate from either a `== knot` definition or a
/// top-level `= stitch` (which is promoted to knot status during HIR
/// lowering). This enum preserves the original syntax kind so we can
/// resolve the pointer back to the correct AST node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerPtr {
    Knot(AstPtr<ast::KnotDef>),
    Stitch(AstPtr<ast::StitchDef>),
}

impl ContainerPtr {
    /// The text range of the originating AST node.
    pub fn text_range(&self) -> TextRange {
        match self {
            Self::Knot(p) => p.text_range(),
            Self::Stitch(p) => p.text_range(),
        }
    }
}

/// A knot definition (or a top-level stitch promoted to knot status).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knot {
    pub ptr: ContainerPtr,
    pub name: Name,
    pub is_function: bool,
    pub params: Vec<Param>,
    /// Content before the first stitch, or the full body if no stitches.
    pub body: Block,
    pub stitches: Vec<Stitch>,
}

/// A stitch definition within a knot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stitch {
    pub ptr: AstPtr<ast::StitchDef>,
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Block,
}

/// A parameter on a knot, stitch, or function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Name,
    /// `ref` parameter — passed by reference.
    pub is_ref: bool,
    /// `->` parameter — tunnel return divert target.
    pub is_divert: bool,
}

// ─── Block and statements ───────────────────────────────────────────

/// A sequence of statements — the universal body type.
///
/// When `label` is set, the block represents a named container (e.g. a labeled
/// gather point). LIR planning allocates a container ID for labeled blocks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Block {
    pub label: Option<Name>,
    pub stmts: Vec<Stmt>,
}

/// A single statement within a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// Text output with inline elements and tags.
    Content(Content),
    /// `-> target`
    Divert(Divert),
    /// `->-> target` or tunnel chain
    TunnelCall(TunnelCall),
    /// `<- target`
    ThreadStart(ThreadStart),
    /// `~ temp x = expr`
    TempDecl(TempDecl),
    /// `~ x = expr` or `~ x += expr`
    Assignment(Assignment),
    /// `~ return expr`
    Return(Return),
    /// A weave-folded group of choices with continuation.
    ChoiceSet(Box<ChoiceSet>),
    /// A labeled block — a named scope that becomes a container in LIR.
    /// Used for opening gathers (`- (label) * choice`) and standalone
    /// labeled gathers that need to be embedded mid-flow.
    LabeledBlock(Box<Block>),
    /// Multiline `{ - cond: ... }`
    Conditional(Conditional),
    /// Multiline `{stopping: - ... - ...}`
    Sequence(Sequence),
    /// `~ expr` — expression evaluated for side effects (e.g. function call).
    ExprStmt(Expr),
    /// End-of-line marker — marks the end of a content output line.
    EndOfLine,
}

// ─── Weave structure ────────────────────────────────────────────────

/// The structural context in which a choice set was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceSetContext {
    /// Normal weave context — choices are at the top level of a knot/stitch
    /// body or nested within other weave structures. Codegen handles loose
    /// ends (choices without explicit diverts fall through to the gather or
    /// the next weave level).
    Weave,
    /// Inside a conditional or sequence branch body. Choices here are
    /// inline — they don't participate in weave folding and may lack a
    /// natural continuation path.
    Inline,
}

/// A group of choices at the same weave depth, with a continuation block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceSet {
    pub choices: Vec<Choice>,
    /// The continuation block after all choices converge. Contains the
    /// gather's content/divert/tags as statements, with the gather's label
    /// on the block. An empty continuation with no label means choices have
    /// no explicit gather (loose ends for codegen to wire up).
    pub continuation: Block,
    /// Where this choice set was created — weave folding or inline content.
    pub context: ChoiceSetContext,
    /// The weave depth at which this choice set was folded.
    /// `0` for inline choice sets (inside conditionals/sequences).
    pub depth: u32,
}

/// A single choice in a choice set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub ptr: AstPtr<ast::Choice>,
    /// `+` (sticky) vs `*` (once-only).
    pub is_sticky: bool,
    /// Invisible default choice (fallback).
    pub is_fallback: bool,
    /// Optional label `(label_name)`.
    pub label: Option<Name>,
    /// Condition expression `{cond}`.
    pub condition: Option<Expr>,
    /// Text before `[` — appears in both choice list and output.
    pub start_content: Option<Content>,
    /// Text inside `[...]` — appears only in the choice list.
    pub bracket_content: Option<Content>,
    /// Text after `]` — appears only after selection.
    pub inner_content: Option<Content>,
    pub tags: Vec<Tag>,
    /// Nested content after this choice is selected.
    pub body: Block,
}

// ─── Content and inline elements ────────────────────────────────────

/// A line of text output with inline elements and associated tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub ptr: Option<SyntaxNodePtr>,
    pub parts: Vec<ContentPart>,
    pub tags: Vec<Tag>,
}

/// A fragment within a content line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    /// Plain text.
    Text(String),
    /// `<>` — glue (suppresses line break).
    Glue,
    /// `{expr}` — expression interpolation.
    Interpolation(Expr),
    /// `{cond: a | b}` — inline conditional.
    InlineConditional(Conditional),
    /// `{&a|b|c}` — inline sequence.
    InlineSequence(Sequence),
}

// ─── Sequence types ─────────────────────────────────────────────────

bitflags::bitflags! {
    /// Sequence type as a bitmask. The reference ink compiler supports
    /// combining flags (e.g., `shuffle stopping`).
    ///
    /// Symbols: `$` = stopping, `&` = cycle, `!` = once, `~` = shuffle.
    /// Default (no annotation) = stopping.
    ///
    /// Valid combinations: each standalone, `shuffle | stopping`, `shuffle | once`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct SequenceType: u8 {
        /// `$` — stops at the last element (default).
        const STOPPING = 0x01;
        /// `&` — loops back to the first element.
        const CYCLE    = 0x02;
        /// `!` — shows each element once, then nothing.
        const ONCE     = 0x04;
        /// `~` — random order.
        const SHUFFLE  = 0x08;
    }
}

// ─── Block-level conditional and sequence ───────────────────────────

/// Distinguishes the semantic forms of conditional blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CondKind {
    /// The condition belongs to the conditional itself (inklecate's
    /// `initialCondition`). The first branch's condition is the initial
    /// condition; it is emitted flat. Produced by `{expr: body}` and
    /// `{expr: body | else_body}` inline syntax, and `{expr:\n  body\n-
    /// else:\n  body2}` branchless-body syntax.
    InitialCondition,
    /// Each branch has an independent boolean condition evaluated inside its
    /// own container (inklecate's `ownExpression`). Produced by multiline
    /// `{ - cond1: ... - cond2: ... }` syntax without a switch expression.
    IfElse,
    /// One expression evaluated once; each branch is a case value compared with `==`.
    /// Produced by `{expr: - val: ...}` syntax (`ConditionalWithExpr` with multiline branches).
    Switch(Expr),
}

/// A multiline conditional block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conditional {
    pub ptr: SyntaxNodePtr,
    pub kind: CondKind,
    pub branches: Vec<CondBranch>,
}

/// A branch within a multiline conditional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondBranch {
    /// `None` for the else branch.
    pub condition: Option<Expr>,
    pub body: Block,
}

/// A sequence block (stopping, cycle, once, shuffle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub ptr: SyntaxNodePtr,
    pub kind: SequenceType,
    pub branches: Vec<Block>,
}

// ─── Control flow ───────────────────────────────────────────────────

/// `-> target` — simple divert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divert {
    pub ptr: Option<SyntaxNodePtr>,
    pub target: DivertTarget,
}

/// `->-> target` or chained tunnel calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelCall {
    pub ptr: AstPtr<ast::DivertNode>,
    pub targets: Vec<DivertTarget>,
}

/// `<- target` — fork execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadStart {
    pub ptr: AstPtr<ast::ThreadStart>,
    pub target: DivertTarget,
}

/// A divert destination with optional arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivertTarget {
    pub path: DivertPath,
    pub args: Vec<Expr>,
}

/// The target of a divert — either a named path or a special keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DivertPath {
    /// A named path (knot, stitch, label, variable).
    Path(Path),
    /// `-> DONE`
    Done,
    /// `-> END`
    End,
}

/// `~ return expr` or bare `->->` (tunnel return).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    pub ptr: Option<AstPtr<ast::ReturnStmt>>,
    pub value: Option<Expr>,
    /// Arguments for `->-> target(args)` tunnel onwards — pushed before the
    /// divert target on the value stack so the redirect target can pop them.
    pub onwards_args: Vec<Expr>,
}

// ─── Expressions ────────────────────────────────────────────────────

/// An expression tree — preserved as-is, not lowered to stack operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Integer literal.
    Int(i32),
    /// Float literal (stored as bits for Eq).
    Float(FloatBits),
    /// Boolean literal.
    Bool(bool),
    /// String literal, possibly with interpolation.
    String(StringExpr),
    /// `null` / uninitialized.
    Null,

    /// Variable or path reference (unresolved).
    Path(Path),
    /// `-> target` as a value (divert target expression).
    DivertTarget(Path),
    /// List literal `(item1, item2)`.
    ListLiteral(Vec<Path>),

    /// Prefix operation (`-x`, `not x`).
    Prefix(PrefixOp, Box<Expr>),
    /// Infix operation (`x + y`, `x == y`, etc.).
    Infix(Box<Expr>, InfixOp, Box<Expr>),
    /// Postfix operation (`x++`, `x--`).
    Postfix(Box<Expr>, PostfixOp),

    /// Function call (`func(args)`).
    Call(Path, Vec<Expr>),
}

/// Float stored as raw bits so it can derive Eq.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FloatBits(pub u64);

impl FloatBits {
    pub fn from_f64(f: f64) -> Self {
        Self(f.to_bits())
    }

    pub fn to_f64(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// A string literal, possibly with interpolated expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringExpr {
    pub parts: Vec<StringPart>,
}

/// A part of a string literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringPart {
    /// Literal text.
    Literal(String),
    /// `{expr}` interpolation within a string.
    Interpolation(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefixOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostfixOp {
    Increment,
    Decrement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InfixOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Intersect,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    Has,
    HasNot,
}

// ─── Expression display ─────────────────────────────────────────────

/// Reconstruct a human-readable name from an HIR expression.
///
/// Used to derive slot names for interpolation slots in template lines.
/// E.g. `Path(["player_name"])` → `"player_name"`, `Infix(x, Add, y)` → `"x + y"`.
#[must_use]
pub fn display_expr(expr: &Expr) -> String {
    match expr {
        Expr::Path(p) => {
            let mut out = String::new();
            for (i, seg) in p.segments.iter().enumerate() {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(&seg.text);
            }
            out
        }
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => f.to_f64().to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::String(_) => "\"...\"".to_string(),
        Expr::Null => "null".to_string(),
        Expr::DivertTarget(p) => {
            let mut out = "-> ".to_string();
            for (i, seg) in p.segments.iter().enumerate() {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(&seg.text);
            }
            out
        }
        Expr::ListLiteral(_) => "(...)".to_string(),
        Expr::Prefix(op, inner) => {
            format!("{}{}", op.as_str(), display_expr(inner))
        }
        Expr::Infix(lhs, op, rhs) => {
            format!(
                "{} {} {}",
                display_expr(lhs),
                op.as_str(),
                display_expr(rhs)
            )
        }
        Expr::Postfix(inner, op) => {
            format!("{}{}", display_expr(inner), op.as_str())
        }
        Expr::Call(path, _) => {
            let mut name = String::new();
            for (i, seg) in path.segments.iter().enumerate() {
                if i > 0 {
                    name.push('.');
                }
                name.push_str(&seg.text);
            }
            format!("{name}(...)")
        }
    }
}

impl PrefixOp {
    /// Operator symbol as a string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Negate => "-",
            Self::Not => "not ",
        }
    }
}

impl PostfixOp {
    /// Operator symbol as a string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Increment => "++",
            Self::Decrement => "--",
        }
    }
}

impl InfixOp {
    /// Operator symbol as a string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Intersect => "^",
            Self::Eq => "==",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::LtEq => "<=",
            Self::GtEq => ">=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Has => "?",
            Self::HasNot => "!?",
        }
    }
}

// ─── Declarations ───────────────────────────────────────────────────

/// `VAR x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub ptr: AstPtr<ast::VarDecl>,
    pub name: Name,
    pub value: Expr,
}

/// `CONST x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDecl {
    pub ptr: AstPtr<ast::ConstDecl>,
    pub name: Name,
    pub value: Expr,
}

/// `~ temp x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempDecl {
    pub ptr: AstPtr<ast::TempDecl>,
    pub name: Name,
    pub value: Option<Expr>,
}

/// `~ x = expr` or `~ x += expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub ptr: AstPtr<ast::Assignment>,
    pub target: Expr,
    pub op: AssignOp,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

/// `LIST name = (item1), item2, (item3 = 5)`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDecl {
    pub ptr: AstPtr<ast::ListDecl>,
    pub name: Name,
    pub members: Vec<ListMember>,
}

/// A single member in a list declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListMember {
    pub name: Name,
    /// Explicit ordinal value (e.g., `item = 5`).
    pub value: Option<i32>,
    /// Whether this member is active by default (wrapped in parens).
    pub is_active: bool,
}

/// `EXTERNAL fn_name(param1, param2)`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDecl {
    pub ptr: AstPtr<ast::ExternalDecl>,
    pub name: Name,
    pub param_count: u8,
}

/// `INCLUDE path/to/file.ink`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeSite {
    pub file_path: String,
    pub ptr: AstPtr<ast::IncludeStmt>,
}

// ─── Diagnostics ────────────────────────────────────────────────────

/// A diagnostic produced during HIR lowering or cross-file analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Which file this diagnostic belongs to.
    pub file: FileId,
    /// The source span this diagnostic points at.
    pub range: TextRange,
    /// Human-readable message describing the problem.
    pub message: String,
    /// Structured error code for documentation and tooling.
    pub code: DiagnosticCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
}

/// Stable error codes for brink diagnostics.
///
/// Codes are never reused once assigned. Each code has a corresponding
/// explanation file at `docs/diagnostics/Exxx.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    // ── Containers ──────────────────────────────────────────────
    /// Knot definition is missing a name.
    E001,
    /// Stitch definition is missing a name.
    E002,
    /// Knot or stitch parameter is missing a name.
    E003,

    // ── Declarations ────────────────────────────────────────────
    /// `VAR` declaration is missing a name.
    E004,
    /// `VAR` declaration is missing an initializer.
    E005,
    /// `CONST` declaration is missing a name.
    E006,
    /// `CONST` declaration is missing an initializer.
    E007,
    /// `LIST` declaration is missing a name.
    E008,
    /// `LIST` member is missing a name.
    E009,
    /// `EXTERNAL` declaration is missing a name.
    E010,
    /// `INCLUDE` statement is missing a file path.
    E011,

    // ── Control flow ────────────────────────────────────────────
    /// Divert is missing a target.
    E012,
    /// Thread start is missing a target.
    E013,
    /// Logic line has no effect (bare `~`).
    E014,

    // ── Expressions ─────────────────────────────────────────────
    /// Expression is missing an operand.
    E015,
    /// Unknown or unsupported operator.
    E016,
    /// Function call is missing a name.
    E017,
    /// Divert target expression is missing a path.
    E018,

    // ── Choices ─────────────────────────────────────────────────
    /// Choice is missing bullet markers.
    E019,

    // ── Inline logic ────────────────────────────────────────────
    /// Inline conditional is missing a condition.
    E020,
    /// Inline sequence has no branches.
    E021,

    // ── Cross-file analysis ──────────────────────────────────────
    /// Duplicate knot definition.
    E022,
    /// Duplicate variable/constant definition.
    E023,
    /// Unresolved divert target.
    E024,
    /// Unresolved variable reference.
    E025,
    /// Duplicate list item.
    E026,
    /// Ambiguous bare list item reference.
    E027,
    /// Circular INCLUDE dependency.
    E028,

    // ── Compile errors ────────────────────────────────────────────
    /// Choice nested in conditional without explicit divert.
    E029,

    // ── Warnings ─────────────────────────────────────────────────
    /// String interpolation in constant initializer is ignored.
    E030,
    /// Function call argument count mismatch.
    E031,

    // ── Structural validation ───────────────────────────────────
    /// Return statement outside function.
    E032,
    /// Unreachable code after divert.
    E033,
    /// Choice set has only fallback choices.
    E034,
    /// Name shadows a built-in function.
    E035,
    /// Expected diagnostic not produced (`// brink-expect`).
    E036,
}

impl DiagnosticCode {
    /// The stable string representation (e.g., `"E001"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::E001 => "E001",
            Self::E002 => "E002",
            Self::E003 => "E003",
            Self::E004 => "E004",
            Self::E005 => "E005",
            Self::E006 => "E006",
            Self::E007 => "E007",
            Self::E008 => "E008",
            Self::E009 => "E009",
            Self::E010 => "E010",
            Self::E011 => "E011",
            Self::E012 => "E012",
            Self::E013 => "E013",
            Self::E014 => "E014",
            Self::E015 => "E015",
            Self::E016 => "E016",
            Self::E017 => "E017",
            Self::E018 => "E018",
            Self::E019 => "E019",
            Self::E020 => "E020",
            Self::E021 => "E021",
            Self::E022 => "E022",
            Self::E023 => "E023",
            Self::E024 => "E024",
            Self::E025 => "E025",
            Self::E026 => "E026",
            Self::E027 => "E027",
            Self::E028 => "E028",
            Self::E029 => "E029",
            Self::E030 => "E030",
            Self::E031 => "E031",
            Self::E032 => "E032",
            Self::E033 => "E033",
            Self::E034 => "E034",
            Self::E035 => "E035",
            Self::E036 => "E036",
        }
    }

    /// Short human-readable title for this diagnostic code.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::E001 => "knot is missing a name",
            Self::E002 => "stitch is missing a name",
            Self::E003 => "parameter is missing a name",
            Self::E004 => "VAR declaration is missing a name",
            Self::E005 => "VAR declaration is missing an initializer",
            Self::E006 => "CONST declaration is missing a name",
            Self::E007 => "CONST declaration is missing an initializer",
            Self::E008 => "LIST declaration is missing a name",
            Self::E009 => "LIST member is missing a name",
            Self::E010 => "EXTERNAL declaration is missing a name",
            Self::E011 => "INCLUDE statement is missing a file path",
            Self::E012 => "divert is missing a target",
            Self::E013 => "thread start is missing a target",
            Self::E014 => "logic line has no effect",
            Self::E015 => "expression is missing an operand",
            Self::E016 => "unknown or unsupported operator",
            Self::E017 => "function call is missing a name",
            Self::E018 => "divert target expression is missing a path",
            Self::E019 => "choice is missing bullet markers",
            Self::E020 => "inline conditional is missing a condition",
            Self::E021 => "inline sequence has no branches",
            Self::E022 => "duplicate knot definition",
            Self::E023 => "duplicate variable/constant definition",
            Self::E024 => "unresolved divert target",
            Self::E025 => "unresolved variable reference",
            Self::E026 => "duplicate list item",
            Self::E027 => "ambiguous bare list item reference",
            Self::E028 => "circular INCLUDE dependency",
            Self::E029 => "choice in conditional must explicitly divert",
            Self::E030 => "string interpolation in constant initializer is ignored",
            Self::E031 => "function call argument count mismatch",
            Self::E032 => "return statement outside function",
            Self::E033 => "unreachable code after divert",
            Self::E034 => "choice set has only fallback choices",
            Self::E035 => "name shadows a built-in function",
            Self::E036 => "expected diagnostic not produced",
        }
    }

    /// Default severity for this diagnostic code.
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            Self::E014
            | Self::E022
            | Self::E023
            | Self::E026
            | Self::E030
            | Self::E031
            | Self::E033
            | Self::E034
            | Self::E035 => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// Parse a diagnostic code from its string representation (e.g., `"E027"`).
    #[must_use]
    pub fn from_str_code(s: &str) -> Option<Self> {
        match s {
            "E001" => Some(Self::E001),
            "E002" => Some(Self::E002),
            "E003" => Some(Self::E003),
            "E004" => Some(Self::E004),
            "E005" => Some(Self::E005),
            "E006" => Some(Self::E006),
            "E007" => Some(Self::E007),
            "E008" => Some(Self::E008),
            "E009" => Some(Self::E009),
            "E010" => Some(Self::E010),
            "E011" => Some(Self::E011),
            "E012" => Some(Self::E012),
            "E013" => Some(Self::E013),
            "E014" => Some(Self::E014),
            "E015" => Some(Self::E015),
            "E016" => Some(Self::E016),
            "E017" => Some(Self::E017),
            "E018" => Some(Self::E018),
            "E019" => Some(Self::E019),
            "E020" => Some(Self::E020),
            "E021" => Some(Self::E021),
            "E022" => Some(Self::E022),
            "E023" => Some(Self::E023),
            "E024" => Some(Self::E024),
            "E025" => Some(Self::E025),
            "E026" => Some(Self::E026),
            "E027" => Some(Self::E027),
            "E028" => Some(Self::E028),
            "E029" => Some(Self::E029),
            "E030" => Some(Self::E030),
            "E031" => Some(Self::E031),
            "E032" => Some(Self::E032),
            "E033" => Some(Self::E033),
            "E034" => Some(Self::E034),
            "E035" => Some(Self::E035),
            "E036" => Some(Self::E036),
            _ => None,
        }
    }
}
