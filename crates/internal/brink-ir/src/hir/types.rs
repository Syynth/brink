use brink_syntax::ast::{self, AstPtr, SyntaxNodePtr};
use rowan::TextRange;

// ─── File identity ──────────────────────────────────────────────────

/// Opaque identifier for a source file within a multi-file project.
///
/// `Ord` orders by the underlying `u32`, giving deterministic iteration when
/// `FileId`s are collected into a `BTreeSet`/`BTreeMap` (e.g. the include
/// graph's `reachable_from`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// `STRUCT` declarations (TM-4b, docs/typed-mode-spec.md §6).
    pub structs: Vec<StructDecl>,
    /// `EXTERNAL` declarations.
    pub externals: Vec<ExternalDecl>,
    /// `INCLUDE` sites (for cross-file resolution by the analyzer).
    pub includes: Vec<IncludeSite>,
    /// The file's explicit `#@module(name)` declaration, if any (M-1,
    /// docs/modules-spec.md §1). `None` means the file is an *undeclared*
    /// stem-module — its module name is its file stem and identity hashing
    /// stays byte-identical to the pre-modules derivation. `Some` names the
    /// module explicitly and opts the file into the declared-module world
    /// (module-qualified `DefinitionId`s, §5). The name argument is
    /// validated (non-empty, single occurrence) during lowering; the range
    /// covers the whole directive tag for the dialect gate (`#@module` is
    /// brink-only) and diagnostics.
    pub module: Option<ModuleDecl>,
    /// `IMPORT` statements (M-2, docs/modules-spec.md §2). Brink-dialect
    /// only — the dialect gate rejects each under strict-ink. Empty for the
    /// entire pre-modules world.
    pub imports: Vec<Import>,
    /// Every `#@private` / `#@public` visibility directive occurrence in the
    /// file (M-2, docs/modules-spec.md §4), for the dialect gate (E051 under
    /// strict-ink). The *effective* per-definition visibility travels the
    /// manifest path (`DeclaredSymbol::visibility`) to the symbol index.
    pub visibility: Vec<VisibilityDirective>,
    /// Range of every `#@was(…)` directive tag in the file — module-level
    /// and definition-level alike (M-3, docs/modules-spec.md §5), for the
    /// dialect gate (E051 under strict-ink). The *effective* rename travels
    /// separately: `ModuleDecl::was` for the module, `DeclaredSymbol::was`
    /// for a definition.
    pub was_directives: Vec<TextRange>,
}

/// A file's explicit `#@module(name)` declaration (M-1, modules-spec §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDecl {
    /// The declared module name (the argument to `#@module(…)`).
    pub name: String,
    /// Source range of the whole `#@module(…)` directive tag.
    pub range: TextRange,
    /// The module's old name and directive range, from a file-level
    /// `#@was(old_name)` (M-3, docs/modules-spec.md §5). `None` means no
    /// rename recorded. Scoped to *declared* modules only — an undeclared
    /// stem-module's identity is its file stem, not a `#@was` target (a
    /// stray `#@was` with no `#@module` diagnoses `E049` instead of
    /// silently attaching here).
    pub was: Option<(String, TextRange)>,
}

/// An `IMPORT` statement (M-2, docs/modules-spec.md §2), both forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The imported module name.
    pub module: String,
    /// Source range of the module-name token.
    pub module_range: TextRange,
    /// The bare-form name list (`IMPORT { a, b AS c } FROM mod`). Empty for
    /// the qualified form (`IMPORT mod`), which brings only the module name
    /// into scope for `module.name` access.
    pub items: Vec<ImportItem>,
    /// `true` for the bare form (has a `{ … }` list, even if empty);
    /// distinguishes an empty bare list from the qualified form.
    pub bare: bool,
    /// Source range of the whole `IMPORT …` statement.
    pub range: TextRange,
}

/// One `name` or `name AS alias` entry in a bare-form import list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportItem {
    /// The imported definition's own (source-module) name.
    pub name: String,
    /// The local alias (`AS gt`), if any. Absent means the name is bound
    /// under its own spelling.
    pub alias: Option<String>,
    /// Source range of the item.
    pub range: TextRange,
}

impl ImportItem {
    /// The name this import binds locally — the alias if present, else the
    /// imported name.
    #[must_use]
    pub fn local_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

/// A `#@private` / `#@public` directive occurrence (M-2, modules-spec §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibilityDirective {
    /// Which visibility the directive requests.
    pub mark: crate::VisibilityMark,
    /// Source range of the directive tag.
    pub range: TextRange,
}

/// A `#@effects(…)` author-facing assertion (T2-2, docs/effects-spec.md
/// §10, issue #861) — an upper bound on the definition's inferred effect
/// row. `#@effects(pure)` sets `pure` and leaves the three lists empty (the
/// empty-row sugar); otherwise `pure` is `false` and `reads`/`writes`/
/// `calls` hold the raw identifier text of each clause's declared names —
/// global `VAR`/`CONST` names for `reads`/`writes`, `EXTERNAL` names for
/// `calls`. Resolving those names against the project symbol index (and the
/// exceedance check itself: inferred row ⊄ this bound) is
/// `brink-analyzer`'s job — this struct only carries what the directive
/// text said. Brink-dialect-gated syntax (`E051` under strict-ink, per
/// `dialect_gate`), same superset-parse-then-reject shape as `#@module`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectsAssertion {
    /// `@[effects(pure)]` — asserts the empty state row (no reads, writes,
    /// or calls). NS-A2 note: `pure` deliberately does NOT bound the output
    /// dimensions — purity ≠ silence (issue #1087's motivating case).
    pub pure: bool,
    /// `@[effects(silent)]` (NS-A2, issue #1108): asserts the definition
    /// produces no content — the `emits` row dimension stays false. Tags
    /// are NOT bounded by `silent` (they are the separate metadata channel;
    /// a no-tags assertion arg has no ruled spelling v1).
    pub silent: bool,
    /// `@[effects(total)]` (NS-A2, issue #1108): asserts the definition
    /// raises no turn-terminating fault — the `faults` row dimension stays
    /// false.
    pub total: bool,
    /// Declared `reads:` clause names, in source order.
    pub reads: Vec<String>,
    /// Declared `writes:` clause names, in source order.
    pub writes: Vec<String>,
    /// Declared `calls:` clause names, in source order.
    pub calls: Vec<String>,
    /// Source range of the whole `#@effects(…)` directive tag.
    pub range: TextRange,
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
    /// Marked flow-private via a `#@local` directive line at the top of
    /// the body. Covers the whole definition subtree at runtime policy
    /// resolution (`docs/directive-annotations-spec.md`).
    pub is_local: bool,
    /// The `#@effects(…)` assertion directive line at the top of the body,
    /// if any (T2-2, docs/effects-spec.md §10, issue #861).
    pub effects_assertion: Option<EffectsAssertion>,
    /// The function-header return type annotation (TM-2, docs/typed-mode-spec.md
    /// §3: `): type ===`), brink-dialect-gated syntax. `None` when absent —
    /// not the same as an explicit `void`, which lowers as
    /// `TypeExpr::Named { name: "void" }` like every other nominal.
    pub return_type: Option<TypeExpr>,
}

/// A stitch definition within a knot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stitch {
    pub ptr: AstPtr<ast::StitchDef>,
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Block,
    /// Marked flow-private via a `#@local` directive line at the top of
    /// the body (`docs/directive-annotations-spec.md`).
    pub is_local: bool,
    /// The `#@effects(…)` assertion directive line at the top of the body,
    /// if any (T2-2, docs/effects-spec.md §10, issue #861).
    pub effects_assertion: Option<EffectsAssertion>,
}

/// A parameter on a knot, stitch, or function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Name,
    /// `ref` parameter — passed by reference.
    pub is_ref: bool,
    /// `->` parameter — tunnel return divert target.
    pub is_divert: bool,
    /// The parameter's type annotation (TM-2, docs/typed-mode-spec.md §3:
    /// `name: type`), brink-dialect-gated syntax.
    pub annotation: Option<TypeExpr>,
}

// ─── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ──────
//
// Superset grammar surface — always lowered to HIR regardless of dialect
// (mirrors the T1b pattern); `brink-analyzer::dialect_gate` is where
// `strict-ink` rejection (E051) happens. Nominal grammar only: no struct
// names yet (TM-4), `Fn` parses but types as reserved until T1c.

/// A parsed type annotation expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    /// A bare nominal name: `int`, `float`, `bool`, `string`, `divert`,
    /// `void`, or an unrecognized identifier (flagged by a targeted
    /// diagnostic — declared struct names arrive in TM-4).
    Named { name: String, range: TextRange },
    /// `name<args…>` — `list<L>`, `array<T>`, `map<K, V>`, or an
    /// unrecognized generic head.
    Generic {
        name: String,
        args: Vec<TypeExpr>,
        range: TextRange,
    },
    /// `fn(params…): ret` — a function type (unfrozen with T1c-1, #699:
    /// resolves to the checker's `Ty::Fn`; the row is val-only — refs are
    /// bound away at `#fn` creation, docs/t1c-spec.md §4).
    Fn {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
        range: TextRange,
    },
}

impl TypeExpr {
    /// The full source range of this type expression.
    #[must_use]
    pub fn range(&self) -> TextRange {
        match self {
            Self::Named { range, .. } | Self::Generic { range, .. } | Self::Fn { range, .. } => {
                *range
            }
        }
    }
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
    /// Pre-assigned container ID for blocks that become LIR containers
    /// (gather continuations, labeled blocks). Stamped by [`super::stamp_container_ids`].
    pub container_id: Option<brink_format::DefinitionId>,
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
    /// `~ { … }` — a T1b multi-line logic block (brink extension; parse-only
    /// in T1b-1, docs/t1b-surface-spec.md §2). Never lowers to LIR — gated
    /// out by `brink-analyzer`'s dialect check under both dialects.
    LogicBlock(LogicBlock),
    /// `~ await <cond>` — a `FlowFrame` suspension point at logic-line position
    /// (docs/flow-suspension-spec.md §3). Brink extension; the condition is
    /// checked effect-free (the purity gate, E105) and lowering to the VM is
    /// fenced (E052) until the runtime spill/restore slice (FS-3) lands.
    Await(AwaitStmt),
}

// ─── T1b superset: multi-line `~ { … }` blocks ──────────────────────
//
// Deliberately a CLOSED set of statement kinds with no variant for any
// weave concept (content, choices, diverts, gathers, threads) — the seam
// rule from docs/t1b-surface-spec.md §2 is enforced by construction here,
// not by a runtime check: `BlockStmt` simply has nowhere to put a weave
// node.

/// A `~ { … }` multi-line logic block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicBlock {
    pub ptr: SyntaxNodePtr,
    pub stmts: Vec<BlockStmt>,
}

/// A single statement inside a `~ { … }` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockStmt {
    TempDecl(TempDecl),
    Assignment(Assignment),
    Return(Return),
    If(IfStmt),
    While(WhileStmt),
    For(ForStmt),
    Break(SyntaxNodePtr),
    Continue(SyntaxNodePtr),
    /// A bare expression statement (function/external calls).
    ExprStmt(Expr),
    /// `await <cond>` — a `FlowFrame` suspension point inside a `~ { … }` block
    /// (docs/flow-suspension-spec.md §3).
    Await(AwaitStmt),
}

/// `if cond { … } (else …)?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub ptr: SyntaxNodePtr,
    pub condition: Expr,
    pub body: Vec<BlockStmt>,
    pub else_branch: Option<ElseBranch>,
}

/// The `else` arm of an [`IfStmt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElseBranch {
    /// `else if cond { … }` — a nested `if`.
    ElseIf(Box<IfStmt>),
    /// `else { … }`.
    Else(Vec<BlockStmt>),
}

/// `while cond { … }`, or the persistent-await form `while await cond { … }`
/// (docs/flow-suspension-spec.md §3) when [`Self::is_await`] is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhileStmt {
    pub ptr: SyntaxNodePtr,
    pub condition: Expr,
    pub body: Vec<BlockStmt>,
    /// `while await cond { … }`: yield-with-policy — waking IS condition-true
    /// (docs/flow-suspension-spec.md §3, the wake contract). A plain `while`
    /// loop leaves this `false`. Set, the condition rides the same effect-free
    /// purity gate (E105) a bare `await` condition does, and lowering is fenced
    /// (E052) until FS-3.
    pub is_await: bool,
}

/// `await <cond>` — a `FlowFrame` suspension point
/// (docs/flow-suspension-spec.md §3). The condition is captured as a
/// compiler-synthesized *pure* function per §5: its identity is the await
/// site's synthesized resume-container path (site-stable), and its effect row
/// must be read-only (the purity gate, E105) — the row IS the wake map's
/// dependency set. Mid-expression `await` is permanently out (§3): this only
/// ever appears at statement/logic position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwaitStmt {
    pub ptr: SyntaxNodePtr,
    /// The condition expression. `None` only for a malformed bare `await`
    /// (the parser already diagnosed the missing expression).
    pub condition: Option<Expr>,
}

/// `for name in expr { … }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForStmt {
    pub ptr: SyntaxNodePtr,
    pub var_name: Name,
    pub iterable: Expr,
    pub body: Vec<BlockStmt>,
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
    /// Pre-assigned container ID for the gather target.
    /// Stamped by [`super::stamp_container_ids`].
    pub gather_id: Option<brink_format::DefinitionId>,
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
    /// Pre-assigned container ID for this choice's target container.
    /// Stamped by [`super::stamp_container_ids`].
    pub container_id: Option<brink_format::DefinitionId>,
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
    /// Word-break spring — conditional space resolved by the runtime.
    Spring,
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
    /// Pre-assigned container ID for this branch's container.
    /// Stamped by [`super::stamp_container_ids`].
    pub container_id: Option<brink_format::DefinitionId>,
}

/// A sequence block (stopping, cycle, once, shuffle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub ptr: SyntaxNodePtr,
    pub kind: SequenceType,
    pub branches: Vec<Block>,
    /// Pre-assigned container ID for the sequence wrapper container.
    /// Stamped by [`super::stamp_container_ids`].
    pub container_id: Option<brink_format::DefinitionId>,
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

    /// `#[expr, …]` — array sigil literal (brink extension, T1b §3).
    ArrayLiteral(ArrayLiteral),
    /// `#{key: expr, …}` — map sigil literal (brink extension, T1b §3).
    MapLiteral(MapLiteral),
    /// `base[index]` — postfix indexing (brink extension, T1b §4).
    Index(IndexExpr),
    /// `start..end` / `start..=end` — range literal (brink extension,
    /// NS-A5, docs/stdlib-spec.md §7, F7).
    Range(RangeExpr),
    /// `Name#{field: expr, …}` — struct construction literal (brink
    /// extension, TM-4b, docs/typed-mode-spec.md §6).
    StructLiteral(StructLiteral),
    /// `base.field` — postfix field access (brink extension, TM-4b,
    /// docs/typed-mode-spec.md §6). Only produced for the unambiguous
    /// grammar shape (a non-`Path` base); a bare `ident.ident` chain still
    /// lowers as `Expr::Path` — the resolution fallback that disambiguates
    /// "static path" from "field access on a variable" is
    /// `brink-analyzer`'s job (§6: "ink's static dotted paths... resolved
    /// first and win").
    FieldAccess(FieldAccessExpr),
    /// `#fn(target, args…)` — function-value creation (brink extension,
    /// T1c, docs/t1c-spec.md §2): partial application over the statically
    /// named function `target`, binding a prefix of its declared params.
    FnLiteral(FnLiteral),
    /// `ref lvalue-path` — path-projection creation (brink extension, T1e,
    /// docs/t1e-spec.md §2): a symbolic `(root cell, path segments)` value.
    /// Legal only in ref-argument position (calls, `#fn(…)`, `bind(…)`);
    /// anywhere else is `brink-analyzer`'s E097 (icebox #825). The segment
    /// expressions (index subexpressions inside a nested `Index`) are
    /// captured whole as part of `operand`'s own tree — "index expressions
    /// snapshot at `ref` creation" (t1e-spec §1) falls out of this shape for
    /// free: there is no lazy re-evaluation path, only this one owned tree,
    /// evaluated once at the creation site.
    RefArg(RefArgExpr),
}

/// `#fn(target, args…)`. `ptr` lets the dialect gate point its diagnostic
/// at the exact literal, matching the sibling sigil-literal shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnLiteral {
    pub ptr: SyntaxNodePtr,
    /// The static target path — a name (possibly dotted), never an
    /// expression. Whether it resolves to a function definition is
    /// `brink-analyzer`'s creation-site check (E079), not this shape's.
    pub target: Path,
    /// Bound-argument expressions, in source order — a prefix of the
    /// target's declared param row (over-binding is E081; `ref`-param
    /// binding discipline is E080).
    pub args: Vec<Expr>,
}

/// `ref lvalue-path` — path-projection creation (brink extension, T1e,
/// docs/t1e-spec.md §2). `ptr` lets the dialect gate point its diagnostic at
/// the exact `ref` expression, matching the sibling sigil-literal shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefArgExpr {
    pub ptr: SyntaxNodePtr,
    /// The lvalue-shaped operand: a `Path` (plain or dotted), an `Index`, a
    /// `FieldAccess`, or a mix of the two nested arbitrarily deep. Any other
    /// expression kind is not an lvalue at all — `brink-analyzer`'s E080
    /// ("this argument is not an lvalue"), same message class T1c's
    /// unmarked-`ref` discipline already uses.
    pub operand: Box<Expr>,
}

/// `Name#{field: expr, …}`. `ptr` lets the dialect gate point its
/// diagnostic at the exact literal, matching `ArrayLiteral`/`MapLiteral`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructLiteral {
    pub ptr: SyntaxNodePtr,
    pub shape: Name,
    /// Field initializers, in source order — construction validity
    /// (missing/extra/mistyped fields) is `brink-analyzer`'s job, not this
    /// shape's.
    pub fields: Vec<(Name, Expr)>,
}

/// `base.field`, chainable (`base.field.field2` lowers as nested
/// `FieldAccessExpr`, same pattern as [`IndexExpr`]'s `grid[y][x]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAccessExpr {
    pub ptr: SyntaxNodePtr,
    pub base: Box<Expr>,
    pub field: Name,
}

/// `#[expr, …]` — carries a `ptr` (unlike the plain literal variants above)
/// so the T1b dialect gate can point its diagnostic at the exact literal,
/// not just the enclosing statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayLiteral {
    pub ptr: SyntaxNodePtr,
    pub elements: Vec<Expr>,
}

/// `#{key: expr, …}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLiteral {
    pub ptr: SyntaxNodePtr,
    pub entries: Vec<(Expr, Expr)>,
}

/// `base[index]`, chainable (`grid[y][x]` lowers as nested `IndexExpr`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexExpr {
    pub ptr: SyntaxNodePtr,
    pub base: Box<Expr>,
    pub index: Box<Expr>,
}

/// `start..end` / `start..=end` — range literal (brink extension, NS-A5,
/// docs/stdlib-spec.md §7, F7). `ptr` lets the dialect gate point its
/// diagnostic at the exact literal, matching the sibling extension shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeExpr {
    pub ptr: SyntaxNodePtr,
    /// The start bound (always an element when the range is non-empty).
    pub start: Box<Expr>,
    /// The written end bound.
    pub end: Box<Expr>,
    /// `true` for the `..=` form.
    pub inclusive: bool,
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
        Expr::ArrayLiteral(_) => "#[...]".to_string(),
        Expr::MapLiteral(_) => "#{...}".to_string(),
        Expr::Index(idx) => format!("{}[{}]", display_expr(&idx.base), display_expr(&idx.index)),
        Expr::StructLiteral(sl) => format!("{}#{{...}}", sl.shape.text),
        Expr::FieldAccess(fa) => format!("{}.{}", display_expr(&fa.base), fa.field.text),
        Expr::FnLiteral(fl) => {
            let mut name = String::new();
            for (i, seg) in fl.target.segments.iter().enumerate() {
                if i > 0 {
                    name.push('.');
                }
                name.push_str(&seg.text);
            }
            if fl.args.is_empty() {
                format!("#fn({name})")
            } else {
                format!("#fn({name}, ...)")
            }
        }
        Expr::RefArg(ra) => format!("ref {}", display_expr(&ra.operand)),
        Expr::Range(r) => {
            let op = if r.inclusive { "..=" } else { ".." };
            format!("{}{op}{}", display_expr(&r.start), display_expr(&r.end))
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
    /// Marked flow-private via a `#@local` directive line above the
    /// declaration (`docs/directive-annotations-spec.md`).
    pub is_local: bool,
    /// The declared type annotation (TM-2, docs/typed-mode-spec.md §3:
    /// `VAR name: type = expr`), brink-dialect-gated syntax.
    pub annotation: Option<TypeExpr>,
}

/// `CONST x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDecl {
    pub ptr: AstPtr<ast::ConstDecl>,
    pub name: Name,
    pub value: Expr,
    /// The declared type annotation (TM-2, docs/typed-mode-spec.md §3:
    /// `CONST name: type = expr`), brink-dialect-gated syntax.
    pub annotation: Option<TypeExpr>,
}

/// `~ temp x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempDecl {
    pub ptr: AstPtr<ast::TempDecl>,
    pub name: Name,
    pub value: Option<Expr>,
    /// The ascription's type annotation (TM-2, docs/typed-mode-spec.md §3:
    /// `~ temp name: type = expr`), brink-dialect-gated syntax. HIR/parse
    /// surface only in this slice — not yet wired into body inference (that
    /// would touch `infer::body::BodyCtx`, out of scope per #638).
    pub annotation: Option<TypeExpr>,
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

// ─── TM-4b structs (docs/typed-mode-spec.md §6) ─────────────────────
//
// Superset grammar surface — always lowered to HIR regardless of dialect
// (mirrors the T1b/TM-2 pattern); `brink-analyzer::dialect_gate` is where
// `strict-ink` rejection (E051) happens. LIR lowering rejects every
// construct below with a targeted diagnostic — codegen lands with TM-4c.

/// `STRUCT Name = #{ field: type, … }`. Top-level only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDecl {
    pub ptr: AstPtr<ast::StructDecl>,
    pub name: Name,
    /// Declared fields, in source order — the same order
    /// `brink_format`'s `Value::Record` flat field vector will follow once
    /// TM-4c's codegen assigns a `ShapeId`.
    pub fields: Vec<StructFieldDecl>,
}

/// One `field: type` pair inside a [`StructDecl`]'s body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructFieldDecl {
    pub name: Name,
    pub ty: TypeExpr,
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
    /// RETIRED (lane-A audit, #709) — the parser always materializes a
    /// `FILE_PATH` node inside `INCLUDE_STMT` (possibly empty) and reports
    /// missing path as E037 (`parser/declaration.rs::include_statement`).
    /// Code kept reserved, not reused.
    E011,

    // ── Control flow ────────────────────────────────────────────
    /// Divert is missing a target.
    E012,
    /// RETIRED (lane-A audit, #709) — `parser/divert.rs::path` always creates
    /// a `PATH` node (empty on error + E037), so `ThreadStart::target()` is
    /// never `None`. Code kept reserved, not reused.
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
    /// RETIRED (lane-A audit, #709) — `parser/divert.rs::path` always creates
    /// a `PATH` node (empty on error + E037), so `DivertTargetExpr::target()`
    /// is never `None`. Code kept reserved, not reused.
    E018,

    // ── Choices ─────────────────────────────────────────────────
    /// RETIRED (lane-A audit, #709) — the parser only builds a `CHOICE` node
    /// after seeing a bullet token, so a bullet-less choice CST cannot exist.
    /// Code kept reserved, not reused.
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
    /// RETIRED (lane-A audit, #709) — circular INCLUDE is detected at the
    /// discovery phase and surfaces as `CompileError::CircularInclude`, not as
    /// a per-construct diagnostic. Code kept reserved, not reused.
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
    /// Syntax error reported by the parser (malformed source).
    E037,
    /// Malformed `///` doc-comment tag on a declaration.
    E038,

    // ── Host manifest (external-function vocabulary) ─────────────
    /// Registered host manifest disagrees with the ink `EXTERNAL` arity.
    E039,
    /// Doc-comment / manifest references an unknown semantic type.
    E040,
    /// External call argument type mismatches the manifest signature.
    E041,
    /// External call argument violates a closed-domain constraint.
    E042,
    /// Well-formed `///` doc-comment tag that doesn't apply to this
    /// declaration kind (e.g. `@kind` on a knot, `@param` on a VAR).
    E043,

    // ── Directives (`#@…` — docs/directive-annotations-spec.md) ──
    /// Unknown directive name (e.g. `#@locale`).
    E044,
    /// Directive has no valid target in this position.
    E045,
    /// Directive contains dynamic inline logic — directives are static text.
    E046,
    /// Directive must be the only tag on its line.
    E047,
    /// Duplicate directive on one target.
    E048,
    /// Directive not supported on this target (e.g. `@local` on CONST).
    E049,
    /// Directive does not take arguments or trailing text.
    E050,

    // ── T1b dialect gate (docs/t1b-surface-spec.md §1) ────────────
    /// A brink-extension construct (block, sigil literal, indexing) was
    /// used under the `strict-ink` dialect.
    E051,
    /// A brink-extension construct parses and analyzes cleanly under the
    /// `brink` dialect, but its LIR lowering hasn't landed yet. Originally
    /// minted for T1b-1 (every T1b construct lowers since T1b-2, #570), then
    /// revived by T1c-1 (#699) as the `#fn(…)` lowering fence, retired again by
    /// T1c-2 (#700). **Now the `await` fence** (FS-2,
    /// docs/flow-suspension-spec.md §3, issue #928): `await <cond>` /
    /// `while await <cond>` parse to HIR and pass the effect-free purity gate
    /// (E105), but their runtime spill/restore semantics are FS-3 — every
    /// `await` construct is fenced here at LIR lowering until that lands. The
    /// code stays a general "parses/analyzes before its lowering lands" fence,
    /// reused as each new extension needs it.
    E052,
    /// RETIRED (T1b-2, #570) — previously a non-suppressible backstop
    /// rejecting T1b brink-extension HIR nodes (`LogicBlock`, `ArrayLiteral`,
    /// `MapLiteral`, `Index`) at LIR lowering. T1b-2 completed real lowering
    /// for all such constructs, making the backstop obsolete. Code kept
    /// reserved, not reused, for diagnostic-code stability.
    E053,
    /// A block-scoped `temp` (`~ { … }`, docs/t1b-surface-spec.md §2) or
    /// `for` loop variable shadows an already-visible temp/param — either an
    /// enclosing `~ { … }` block scope or an outer classic `~ temp`.
    E054,

    // ── T1b stdlib slice 1 (docs/t1b-surface-spec.md §5) ──────────────
    /// `push`/`insert`/`remove`'s first argument is not an lvalue (a
    /// variable, temp, or indexed path) — mutators require a place to
    /// write the mutated container back into.
    E055,
    /// `push`/`insert`/`remove` was used in expression position — they
    /// return nothing and are only valid as a statement.
    E056,

    // ── T1b logic blocks (docs/t1b-surface-spec.md §2) ────────────────
    /// `break`/`continue` used outside any enclosing `while`/`for` loop.
    E057,
    /// Collection mutator (`push`/`insert`/`remove`) called with the wrong
    /// number of arguments — a targeted compile error naming the expected
    /// signature (replaces the generic `E031` warning + silently-dropped
    /// RMW lowering, RULED 2026-07-12, see `docs/decision-log.md`).
    E058,

    // ── Weave-in-inline-content backstop (sibling of #578, #585) ──────
    /// A choice set, labeled gather block, multi-line conditional, or
    /// sequence was found nested inside inline content (e.g. a choice's own
    /// display/bracket/inner text) where it would need a child container
    /// that position structurally cannot hold.
    E059,

    // ── Codegen defense-in-depth backstop (#586) ──────────────────────
    /// `brink-codegen-inkb` refused to emit bytecode for a `Program` that
    /// violates an invariant an earlier, non-suppressible compiler stage is
    /// supposed to guarantee (currently: an out-of-loop `LogicBreak`/
    /// `LogicContinue`, normally rejected at `E057`). Reaching this from a
    /// normal compile is a compiler bug, not an authoring mistake — this
    /// code exists so that bug fails loudly instead of silently corrupting
    /// bytecode.
    E060,

    // ── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ────
    /// A type annotation names something that isn't a recognized nominal
    /// type (`int`/`float`/`bool`/`string`/`divert`/`void`), a `list<L>`
    /// naming a declared `LIST`, `array<T>`, or `map<K, V>` — declared
    /// struct names arrive in TM-4.
    E061,
    /// RETIRED (T1c-1, #699): previously "`fn(T…): R` function-type
    /// annotation used — parses, but types as reserved until T1c". T1c
    /// unfroze the form (docs/t1c-spec.md §4: "boundary annotations gain
    /// the `fn(T…): R` form"), so it now resolves to a real checker type.
    /// Code kept reserved, not reused, for diagnostic-code stability — no
    /// longer emitted by any pass.
    E062,
    /// A param/return/`VAR` type annotation disagrees with the type
    /// TM-1's body inference would otherwise derive. Advisory only in this
    /// slice (gradual policy) — strict-mode severity is TM-3's call.
    E063,

    // ── TM-3 strict typed-mode policy (docs/typed-mode-spec.md §1/§9-3) ──
    /// `types = strict` was requested but the project's dialect isn't
    /// `brink` — strict typing is a brink-dialect extension (its annotation
    /// syntax is extension syntax), so `types = strict` + `dialect =
    /// strict-ink` is a config error, not a per-construct diagnostic.
    E064,
    /// Under `types = strict`, a def's inferred signature or body slot
    /// (param, return, or temp) resolved to `Unknown` after the SCC
    /// fixpoint with no annotation to supply a concrete type — "annotate or
    /// restructure" (spec §1). Legal under `types = gradual`.
    E065,
    /// Under `types = strict`, a def's inferred signature or body slot
    /// resolved to `Ty::Conflicted` (#627) — the body's own uses disagree
    /// on the slot's type. Legal (advisory-only, unreported) under `types =
    /// gradual`.
    E066,
    /// Under `types = strict`, a `~ x = f()` / `~ temp x = f()` assigns the
    /// result of a call whose resolved def is a `void`-returning function
    /// (docs/typed-mode-spec.md §3: "assigning a `void` call is an error in
    /// strict mode"). Only the assignment/temp-decl's RHS *root* call is
    /// checked — a statement-position call (`~ f()`) or a call nested inside
    /// interpolation is never flagged. Never emitted under `types = gradual`.
    E067,

    // ── TM-4b structs (docs/typed-mode-spec.md §6) ────────────────────
    /// A struct construction literal's leading shape name (`Name#{…}`)
    /// doesn't name any declared `STRUCT`.
    E068,
    /// Under `types = strict`, a struct construction literal omits a
    /// declared field — names the missing field.
    E069,
    /// A struct construction literal supplies a field the shape doesn't
    /// declare — names the extra field.
    E070,
    /// Under `types = strict`, a struct construction literal's field
    /// initializer disagrees with the field's declared type — names the
    /// field.
    E071,
    /// RETIRED (TM-4c, #666): previously a non-suppressible backstop
    /// rejecting *every* struct construct/field access reaching LIR
    /// lowering, back when codegen for structs didn't exist yet. Structs
    /// now lower for real (`E073` is TM-4c's narrower replacement
    /// backstop). Code kept reserved, not reused, for diagnostic-code
    /// stability — no longer emitted by any pass.
    E072,
    /// Non-suppressible defense-in-depth backstop, mirroring `E053`/`E060`/
    /// (former) `E072`: a struct construction literal referencing a shape
    /// name that doesn't resolve to any declared `STRUCT` reached LIR
    /// lowering. Reaching this from a normal compile means
    /// `brink-analyzer`'s `resolve::resolve_struct_ref` diagnostic (`E068`)
    /// was suppressed (`// brink-disable-all`), not a compiler bug on its
    /// own — `RecordNew` needs a real `ShapeId` at compile time; there is no
    /// dynamic "construct with unknown shape" concept in this design.
    E073,
    /// A field-write target (`p.field = expr`) is a *chained* projection —
    /// `p.a.b = v` or a mixed `p.a[i].b = v` — never a bare `ident.field`
    /// on a resolvable root. TM-4c ships single-level field writes only
    /// (mirrors `lower_indexed_assignment`'s `n == 1` fast path); chained
    /// writes are an explicit, permanent T1e boundary (`docs/
    /// typed-mode-spec.md` §6), not a "not implemented yet" gap — this is a
    /// real, reachable, non-suppressible diagnostic authors can hit by
    /// writing ordinary (if currently unsupported) ink, not a defensive
    /// backstop for a suppressed analysis diagnostic.
    E074,

    // ── decls constant-folding backstops (#673) ───────────────────────
    /// A struct construction literal (`Name#{…}`) appears as a `VAR`/`CONST`
    /// declaration's default. `eval_const_expr` (`brink-ir::lir::lower::
    /// decls`) has no compile-time representation for a record value — a
    /// global's default is baked into `StoryData` at compile time, so there
    /// is no runtime construction path to defer to the way a mid-story
    /// `p = Point#{…}` assignment has. A real, non-suppressible compile
    /// error (never a silent `Null`) until declaration-default structs get
    /// a design (`ConstValue` would need a struct-carrying variant).
    E075,
    /// A map literal used as a `VAR`/`CONST` declaration default has a key
    /// that isn't a compile-time-constant scalar in the ratified map-key
    /// domain (int/string/bool — value-model-spec §4). Mid-story map
    /// construction (`MapNew`) faults on this at runtime
    /// (`InvalidMapKeyType`); a declaration default has no runtime
    /// construction step to fault at, so this is the compile-time
    /// equivalent — a real error, never a silent `Null`.
    E076,
    /// An array element, map value, struct field, or `#fn` bound `val` arg
    /// nested inside a `VAR`/`CONST` declaration default has a source
    /// expression kind that can never constant-fold — a function call,
    /// postfix indexing, field access, `++`/`--`, or (#743) a bare
    /// reference to another `VAR`. A declaration default is baked into
    /// `StoryData` at compile time, so there is no runtime construction
    /// step left to evaluate the element at; without this diagnostic the
    /// element recursed into `eval_const_expr`'s `Path`
    /// (`SymbolKind::Variable`) arm or catch-all and silently became `Null`
    /// — #673's silent-`Null` bug one level down, inside the literal (#679
    /// review; the `Path`-to-`Variable` case one level in was left
    /// deliberately unchanged there and closed by #743). Keyed off the
    /// source expression *kind*, never the folded result: an `Expr::Null`
    /// produced by HIR error recovery must not double-report, and a `Path`
    /// resolving to a `CONST`/list item/knot/stitch/function still folds
    /// for real and is not flagged — only a resolved `SymbolKind::Variable`
    /// (or an unresolved path, left to the analyzer's own diagnostic) is
    /// exempt from the fold-for-real behavior, matching
    /// `is_const_foldable_decl_default`'s top-level twin (`E083`). (A
    /// struct literal nested at this position is unconditionally `E075`
    /// regardless of field content — `ConstValue` has no record variant at
    /// all — so a bad field inside it never reaches this arm.)
    E077,
    // ── TM-3 completion: conversion intrinsics (docs/typed-mode-spec.md
    // §4, maintainer ruling 2026-07-13, issue #659) ──────────────────────
    /// Under `types = strict`, an unresolved (builtin, not author-shadowed)
    /// call to `int(x)`/`float(x)` where `x` is statically a divert-target,
    /// LIST, array, map, or struct construction literal — outside the
    /// permissive numeric+bool domain (ruling 2: "compile error under
    /// `types = strict`, runtime fault under gradual"). `string(x)` accepts
    /// every type and is never checked here.
    E078,

    // ── T1c function values (docs/t1c-spec.md §2/§8, issue #699) ─────
    /// `#fn(name, …)`'s target does not resolve to a statically-named
    /// function definition (`=== function name ===`) — it resolved to a
    /// variable/list/external/label/non-function knot or stitch, or it
    /// names a builtin/stdlib intrinsic (which has no definition to take a
    /// token of). Only fires under `dialect = brink` — under `strict-ink`
    /// the whole literal is already rejected as extension syntax (E051),
    /// and content diagnostics on rejected syntax are noise (the TM-2
    /// suppression precedent, maintainer ruling 2026-07-13).
    E079,
    /// A `ref` param of a `#fn` target is not bound in the creation-site
    /// prefix, or is bound to a non-durable lvalue. All `ref` params must
    /// be bound at creation, and each must capture a durable cell — a
    /// global `VAR` (flow-local `#@local` VARs included); a `temp`/param
    /// is a compile error (temps die with the frame, value-model §11), a
    /// `CONST` is not a mutable cell, and a bare (unmarked) rvalue/field
    /// reference is not a cell at all.
    ///
    /// T1e (docs/t1e-spec.md §2/§6, issue #831) extends this same code —
    /// "reuse the E080-family message shape" — to the explicit `ref
    /// lvalue-path` projection form (`heal(ref npc.hp, 5)`,
    /// `#fn(heal, ref party[leader].hp)`, `bind(f, ref inventory[idx])`):
    /// the *root* of the path (the innermost variable the segments walk
    /// from) must still be a durable global `VAR`, by the same rule —
    /// `temp`/param roots remain a compile error, a `CONST` root is not a
    /// mutable cell. A projection's own *segments* (dotted fields, `[…]`
    /// indices) are a separate check (`E098`, strict-mode statically-known
    /// shapes only) — this code is the root-durability obligation alone.
    E080,
    /// `#fn(name, args…)` binds more arguments than the target declares —
    /// the bound-arg row is a *prefix* of the declared param row
    /// (docs/t1c-spec.md §2: "binding more args than the target declares
    /// is a compile error").
    E081,

    // ── T1b block-temp scoping (docs/t1b-surface-spec.md §2, issue #680) ──
    /// A T1b block-scoped `temp` (`~ { … }`) — or a `for`-loop variable,
    /// which desugars the same way — was referenced (by value or by `ref`
    /// argument) after its own `~ { … }`/`while`/`for`/`if` block already
    /// closed. Root-caused for #680: LIR lowering's fallback for "temp not
    /// currently visible" (used for inklecate-compat forward-reference
    /// emulation of *classic* temps) previously also caught this case,
    /// silently emitting a phantom hashed `GetGlobal`/`RefGlobal` id that
    /// was never registered as a real global — a runtime-only
    /// `UnresolvedGlobal` fault with no compile diagnostic.
    E082,

    // ── Declaration-default constness, top level (issue #692, sibling to
    // #673/#679's collection-element E075/E076/E077) ─────────────────────
    /// A scalar `VAR`/`CONST` declaration default whose *source expression
    /// kind* can never be a compile-time constant — a bare reference to
    /// another `VAR` (`VAR x = someOtherVar`) or a function call
    /// (`VAR x = f()`), including either wrapped in a prefix/infix
    /// operation. `eval_const_expr`'s `Path` arm (`SymbolKind::Variable`)
    /// and its catch-all previously folded both silently to `Null` with no
    /// diagnostic — the same silent-fold bug #673/#679 fixed one level
    /// down, inside array/map/struct literals, left unfixed at this top
    /// level. Keyed off the source expression kind, never the folded
    /// result, same as `E077`. Does not fire for a `Path` nested inside a
    /// collection/struct/fn literal (array element, map value, struct
    /// field, `#fn` argument) — those recurse through their own existing
    /// `E075`/`E076`/`E077` per-element checks one level in, which
    /// deliberately still leave a `VAR`-reference gap unchanged (#679 scope
    /// notes) pending its own follow-up.
    E083,

    // ── TM-5 struct construction literals (docs/typed-mode-spec.md §6,
    // decision-log "Struct construction literals: source-order evaluation,
    // duplicate field is a compile error" 2026-07-14, issues #675/#676) ──
    /// A struct construction literal (`Name#{…}`) supplies the same field
    /// name more than once. Previously a silent last-wins: only the final
    /// initializer's value was placed, and — because the well-formed
    /// `RecordNew` lowering path discarded every non-placed lowered
    /// expression tree wholesale — an earlier duplicate's initializer
    /// (including any observable side effect, e.g. a function call) never
    /// actually ran at all, with no diagnostic (#675's RCA). Now a real
    /// compile error naming the repeated field, under both
    /// `types = gradual` and `types = strict` — unlike `E069`/`E070`/
    /// `E071` (which need a resolved shape to check missing/extra/mistyped
    /// fields against, and are strict-mode-only), a duplicate field is a
    /// structural authoring mistake detectable from the literal alone,
    /// independent of type-checking policy or whether the shape name even
    /// resolves.
    E084,

    // ── M-1 modules (docs/modules-spec.md §1/§5) ──────────────────
    /// An *undeclared* file whose module (its file stem) collides with a
    /// *declared* module's name (`#@module(name)` elsewhere). Accidental
    /// membership with mixed visibility defaults is the one footgun the
    /// module model forbids (modules-spec §1). Fix: declare the file with
    /// the same `#@module(name)`, or rename it.
    E085,
    /// A malformed `#@module(…)` directive: a missing or empty name
    /// argument, or a second `#@module` in the same file. `#@module`
    /// takes exactly one non-empty module name and appears at most once
    /// per file (modules-spec §1).
    E086,

    // ── M-2 imports + visibility (docs/modules-spec.md §2/§4/§7) ───
    /// A reference resolves to a `#@private` definition in another module.
    /// Private names are module-internal; the referrer is outside that
    /// module. Fix: make the definition `#@public` and `IMPORT` it, or move
    /// the reference into the module (modules-spec §4/§7).
    E087,
    /// A bare-form `IMPORT { name } FROM mod` names a definition that the
    /// *declared* module `mod` does not publicly export. Only enforced
    /// against declared modules — an import naming an unknown/undeclared
    /// module is not itself flagged by this code, since this module's
    /// export set isn't visible to the check (modules-spec §2/§7).
    E088,
    /// An `IMPORT` brings the same local name into scope twice (a repeated
    /// bare import, or two imports whose names/aliases collide) — the
    /// reference would be ambiguous (modules-spec §2/§7).
    E089,
    /// An `IMPORT` names the importing file's own module — a module cannot
    /// import itself; its own names are already bare (modules-spec §2/§7).
    E090,
    /// A qualified access `a.b` is ambiguous: `a` is both a module imported
    /// in this file and a visible definition. Fix with an `AS` alias — no
    /// silent precedence (modules-spec §2/§7).
    E091,
    /// A `#@public`/`#@private` override that restates the module's default
    /// (e.g. `#@public` in an undeclared module, `#@private` in a declared
    /// one) — redundant, no effect (warning, modules-spec §4/§7).
    E092,
    /// Conflicting or repeated visibility directives on one declaration
    /// (both `#@private` and `#@public`, or the same one twice). A
    /// declaration takes at most one visibility directive (modules-spec §4).
    E093,

    // ── M-3 renames (docs/modules-spec.md §5/§7) ────────────────────
    /// A malformed `#@was(…)` directive: a missing or empty old-name
    /// argument (`#@was`, `#@was()`). `#@was` takes exactly one non-empty
    /// name (modules-spec §5).
    E094,
    /// `#@was(name)` names the thing's own *current* name — a self-alias
    /// that would be a no-op entry in the compiled alias table. Nothing to
    /// migrate; likely a stale directive left over from a previous rename
    /// (warning, modules-spec §5/§7).
    E095,

    // ── M-2c cross-module collisions (issue #784, decision-log
    // "Cross-module name collisions" 2026-07-14) ────────────────────────
    /// Two *declared* modules (`#@module(name)`, different names) each
    /// define a same-name, same-kind symbol. Escalated from the
    /// `E022`/`E023`/`E026` inklecate-compat duplicate warning to a hard
    /// error under `dialect = brink` only: flat resolution (unchanged by
    /// this stopgap — true import-scoped resolution is #790's job) binds a
    /// bare name to whichever declared-module definition merge happens to
    /// see first, so two declared modules sharing a name make that binding
    /// silently order-dependent for one of them. A duplicate *within* one
    /// module (same declared module name across its files, or any
    /// undeclared/legacy file) keeps the existing warning — this code
    /// fires only when both colliding definitions' owning files declared
    /// *different* modules. Reported once per colliding definition (both
    /// spans), under `strict-ink` this code never fires (compat corpus
    /// untouched).
    E096,

    // ── T1e-1 path projections (docs/t1e-spec.md §2/§6, issue #831,
    // tracking #828) ──────────────────────────────────────────────────
    /// A `ref lvalue-path` projection expression (`ref npc.hp`,
    /// `ref inventory[idx]`) appears somewhere other than ref-argument
    /// position (a direct argument of a call, `#fn(…)`, or `bind(…)`) — a
    /// standalone projection value (`temp r = ref a[0]`), one nested inside
    /// another expression, or any other position. Deliberate v1 posture
    /// (t1e-spec §2: "projections exist only where `ref` already exists:
    /// argument binding"); first-class standalone projection values are a
    /// future round, tracked as icebox #825 — not a permanent rejection.
    E097,
    /// A `ref lvalue-path` projection's segment (a dotted field, or a
    /// `[…]` index) disagrees with the root's statically-known shape, under
    /// `types = strict` only — a dotted field the declared `STRUCT` shape
    /// doesn't have, or a `[…]` index against a declared shape that isn't a
    /// collection (mirrors `structs::check`'s missing/extra-field
    /// machinery, `E069`–`E071`, applied to path segments instead of
    /// construction-literal fields; "Unknown never disagrees" for any
    /// segment whose base type isn't statically known this way — silently
    /// unchecked, same spirit as `E071`).
    E098,
    /// A `ref lvalue-path` projection with at least one path segment
    /// (dotted field or `[…]` index — a *real* projection, not a bare
    /// single-name `ref`) reached LIR lowering. T1e-1 (docs/t1e-spec.md §8
    /// sequencing item 1) ships grammar + HIR + analyzer only — the
    /// `MakeProjection`/`ProjRead`/`ProjWrite` opcodes a projection needs to
    /// actually run land in T1e-2 (tracking #828). The E052-fence pattern:
    /// every other check (`E080` durable root, `E097` position, `E098`
    /// strict segment shape) already ran and passed, so this is a clean,
    /// deliberate "not yet lowerable" stop, not a silent drop or a
    /// miscompile — see `brink-ir::lir::lower::mod`'s backstop doctrine. A
    /// bare single-name `ref x` (zero segments) never hits this — it lowers
    /// exactly like today's unmarked ref-argument binding.
    E099,

    // ── T2-2 `#@effects(…)` assertion surface (docs/effects-spec.md §10,
    // issue #861) ──────────────────────────────────────────────────
    /// `#@effects` with no argument at all (`#@effects`, `#@effects()`, or
    /// an argument that parses to nothing) — the directive always requires
    /// either `pure` or at least one `reads:`/`writes:`/`calls:` clause.
    E100,
    /// A malformed `#@effects(…)` argument: an unrecognized clause keyword
    /// (only `reads`/`writes`/`calls` are valid), a value that isn't a bare
    /// identifier, or a bare value with no preceding clause to attach to.
    E101,
    /// A `#@effects(…)` clause names an identifier that isn't a declared
    /// global `VAR`/`CONST` (for `reads`/`writes`) or a declared `EXTERNAL`
    /// (for `calls`) anywhere in the project.
    E102,
    /// **The exceedance error** (docs/effects-spec.md §10, sitting 2,
    /// 2026-07-14 ruling): the definition's inferred effect row is not
    /// covered by (`⊄`) its `#@effects(…)` assertion's declared upper
    /// bound. Per the ruling, this is the *only* diagnostic the assertion
    /// surface ever produces — an inferred row that is narrower than the
    /// bound is silent; there is no drift policy.
    E103,

    // ── Computed-callee call attempt (docs/t1c-spec.md §3/§10, issue #869) ──
    /// A call `expr(args…)` whose callee isn't a bare variable/temp/param
    /// name (an `INDEX_EXPR`, `FIELD_ACCESS_EXPR`, chained call result,
    /// parenthesized expr, …). Direct-call syntax is RULED (t1c-spec §3) to
    /// a bare-name callee only; "method-call syntax" through a computed
    /// callee is explicitly out of T1c (§10). Always rejected — every
    /// dialect, every mode — pointing at the ratified `call(f, args…)`
    /// form, which already dispatches through exactly this class of
    /// expression correctly. Replaces the pre-existing silent drop (the
    /// parser used to leave `(args…)` unconsumed, so it resurfaced as
    /// trailing prose text on the content line and the call itself
    /// vanished) with a loud, unconditional compile error.
    E104,

    // ── `await` condition purity gate (docs/flow-suspension-spec.md §3/§5, ──
    // ── issue #928, FS-2) ─────────────────────────────────────────────────
    /// An `await <cond>` / `while await <cond>` condition is not effect-free.
    /// The condition is captured as a compiler-synthesized *pure* function
    /// (docs/flow-suspension-spec.md §5): its effect row must be read-only —
    /// reads are the wake map's dependency set, but a transitive **write** to a
    /// global cell, or an effectful host **call**, makes the condition
    /// re-evaluation itself observable, which the wake contract forbids. Built
    /// on the effects machinery (#859): the condition's transitive effect row
    /// (via the whole-project [`crate`]-level effect table) must have empty
    /// `writes`/`calls` and not be opaque. Brink-only (under strict-ink the
    /// whole `await` is already `E051`); a bare fn-value reference used as a
    /// dynamic condition (`await some_fn_value`, no call syntax) is read-only
    /// by construction and never flagged.
    E105,

    // ── T1b map-literal key-domain warning (docs/t1b-surface-spec.md §3,
    // issue #598) ──────────────────────────────────────────────────────
    /// A `#{key: expr, …}` map-literal key is a statically-classifiable
    /// literal outside the ratified int/string/bool key domain — a float,
    /// array (`#[...]`), nested map (`#{...}`), struct (`Name#{...}`),
    /// function-value (`#fn(...)`), ink `LIST`, or divert-target literal
    /// used directly as a key. §3 rules the key domain to
    /// int/string/bool at runtime (`RuntimeError::InvalidMapKeyType`) and
    /// says the analyzer warns on statically-visible non-key types; this was
    /// the missing half (`MapLiteral` lowering did zero key-domain checking).
    /// A dynamic key (a variable, call, index, or any other non-literal
    /// expression) is not statically visible and is never flagged here —
    /// the runtime fault remains the sole backstop for those.
    E106,

    // ── NS-A1 Option[T] (docs/stdlib-spec.md §1.4, issue #1107) ────────
    /// A fresh, un-annotated declaration (`VAR x = none`, `CONST x = none`,
    /// `~ temp x = none`) whose initializer is the bare `none` Option
    /// literal. §1.4's ruled rule: "a bare `none` needs a type from
    /// context (concrete sites fine; a fresh un-annotated `var x = none`
    /// errors — the empty-collection posture)." A declaration site IS the
    /// slot's type origin, so there is no context to take the element type
    /// from — the fix is to initialize from a real Option-producing
    /// expression (`some(x)`, or an Option-returning verb like
    /// `find`/`get`/`pop`). Error in both dialects and both `types`
    /// policies: the rule is part of the Option package itself, not a
    /// strict-mode refinement.
    E107,

    // ── NS-A2 effect-row extension (issue #1108; docs/stdlib-spec.md
    // §1.2/§9.2, issues #1087/#1097) ───────────────────────────────────
    /// `@[effects(silent)]` exceedance: the definition's inferred row can
    /// produce content (`emits`, incl. transitively through callees, or an
    /// opaque/unbounded row). Exceedance-only, like `E103` — asserting less
    /// than reality is legal, asserting more is not.
    E108,
    /// `@[effects(total)]` exceedance: the definition's inferred row can
    /// raise a turn-terminating fault (`faults`, incl. transitively, or an
    /// opaque/unbounded row). Exceedance-only, like `E103`.
    E109,
    /// The deprecated `#@effects(…)` tag-channel spelling — superseded by
    /// the `@[effects(…)]` annotation final form (stdlib-spec §9.2, ruled
    /// 2026-07-18). Warning: the alias keeps parsing (it shipped in
    /// released surface, `@brink-lang/web@0.11.1`).
    E110,
    /// An `@[…]` annotation line naming anything other than `effects` —
    /// the annotation channel's recognized name set is closed (v1: exactly
    /// one member). Tag-channel directive names do not alias into it.
    E111,
    /// An `@[…]` annotation line outside its one recognized placement (the
    /// leading run at the top of a knot/stitch body). Never a silent drop,
    /// never content — the `E045` posture, on the annotation channel.
    E112,

    // ── NS-A3 protocol registry (issue #1109; docs/stdlib-spec.md §9.6)
    /// A declaration named after a registry protocol method — `display`,
    /// `compare`, or `next` (F6, ruled 2026-07-19): the names are RESERVED
    /// under the brink dialect, and an author declaration of any callable
    /// or value-bindable kind (knot/stitch/function, param, temp, VAR,
    /// CONST, EXTERNAL, for-loop variable) is a **hard error**, not an
    /// E035-lineage shadowing warning — a shadowed `display` would make
    /// interpolation untrustworthy.
    E113,
    /// A registered protocol impl's inferred effect row exceeds its
    /// protocol's effect contract (`display`/`compare`: pure·silent·total;
    /// `iterate`'s `next`: writes-receiver·silent·total — the receiver is
    /// a `ref` param, invisible to the global row, so every v1 contract
    /// bounds the *global* row at empty). Exceedance-only, the
    /// `E103`/`E108`/`E109` posture; an opaque row exceeds every contract.
    E114,
    /// An ill-formed protocol impl registration: the named type isn't a
    /// declared `STRUCT`, the impl target isn't a declared function, the
    /// signature shape is wrong (arity, `ref`-ness, or a contradicting
    /// type annotation), or the (protocol, type) pair is already
    /// registered.
    E115,

    // ── F27: Option has no truthiness (docs/stdlib-spec.md §1.6, ruled
    // 2026-07-19, issue #1120) ─────────────────────────────────────────
    /// A condition-position expression (an `if`/`while` condition, a
    /// `{cond: …}` conditional branch, a choice guard, an `await`
    /// condition) whose statically-known type is `Option[T]`. Option has
    /// **no** truthiness — truthiness is a quiet coercion of exactly the
    /// kind `Option[T] ≠ T` exists to ban — so a strict-mode author writes
    /// `== none` / `== some(x)` (or, post-B1, the `as`-binding).
    /// Strict-mode-only, best-effort static (the "Unknown never disagrees"
    /// posture: an unclassifiable condition stays silently unchecked);
    /// under `types = gradual` the same condition is the
    /// `RuntimeError::OptionTruthiness` turn-terminating fault — the
    /// runtime backstop that catches every case either way. Supersedes
    /// NS-A1's shipped falsy-none truthiness.
    E116,
    // ── NS-A5 the inhabited-range refinement (issue #1111;
    // docs/stdlib-spec.md §7, F7/F8 ruled 2026-07-19) ──────────────────
    /// A range-refinement violation under `types = strict` (the E078
    /// precedent — strict-only; gradual mode is inert and leaves the
    /// runtime fault residual, F8's general rule): `int(r)` demands
    /// `NonEmptyRange` evidence, and either (a) the range literal in
    /// argument position is **provably empty** (`0..0`, `5..=2` — bounds
    /// fold statically, CONST refs included), or (b) the argument's type
    /// carries no inhabitedness evidence (a possibly-empty range — route
    /// computed bounds through `non_empty(r)`, parse-don't-validate).
    E117,

    // ── NS-A8: the numeric tower (docs/tower-mini-spec.md, issue #1114) ──
    /// A protocol impl registration named a numeric-tower kind
    /// (`vec2`/`vec3`/`vec4`/`quat`/`mat2`/`mat3`/`mat4`) as its type.
    /// Tower kinds are compiler-known value kinds, not user structs: their
    /// `display` is the fixed structural form, their equality is
    /// componentwise IEEE (T4), and they are NOT orderable — a `compare`
    /// impl for a tower kind would contradict the ruled §4b doctrine, and
    /// `display`/`iterate` impls would shadow compiler-owned behavior. The
    /// rejection is unconditional — it wins even over a user STRUCT
    /// declared with the same name (tower type names are global like
    /// `int`).
    E118,
}

impl DiagnosticCode {
    /// The stable string representation (e.g., `"E001"`).
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "a flat one-arm-per-code table that necessarily grows with the diagnostic set"
    )]
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
            Self::E037 => "E037",
            Self::E038 => "E038",
            Self::E039 => "E039",
            Self::E040 => "E040",
            Self::E041 => "E041",
            Self::E042 => "E042",
            Self::E043 => "E043",
            Self::E044 => "E044",
            Self::E045 => "E045",
            Self::E046 => "E046",
            Self::E047 => "E047",
            Self::E048 => "E048",
            Self::E049 => "E049",
            Self::E050 => "E050",
            Self::E051 => "E051",
            Self::E052 => "E052",
            Self::E053 => "E053",
            Self::E054 => "E054",
            Self::E055 => "E055",
            Self::E056 => "E056",
            Self::E057 => "E057",
            Self::E058 => "E058",
            Self::E059 => "E059",
            Self::E060 => "E060",
            Self::E061 => "E061",
            Self::E062 => "E062",
            Self::E063 => "E063",
            Self::E064 => "E064",
            Self::E065 => "E065",
            Self::E066 => "E066",
            Self::E067 => "E067",
            Self::E068 => "E068",
            Self::E069 => "E069",
            Self::E070 => "E070",
            Self::E071 => "E071",
            Self::E072 => "E072",
            Self::E073 => "E073",
            Self::E074 => "E074",
            Self::E075 => "E075",
            Self::E076 => "E076",
            Self::E077 => "E077",
            Self::E078 => "E078",
            Self::E079 => "E079",
            Self::E080 => "E080",
            Self::E081 => "E081",
            Self::E082 => "E082",
            Self::E083 => "E083",
            Self::E084 => "E084",
            Self::E085 => "E085",
            Self::E086 => "E086",
            Self::E087 => "E087",
            Self::E088 => "E088",
            Self::E089 => "E089",
            Self::E090 => "E090",
            Self::E091 => "E091",
            Self::E092 => "E092",
            Self::E093 => "E093",
            Self::E094 => "E094",
            Self::E095 => "E095",
            Self::E096 => "E096",
            Self::E097 => "E097",
            Self::E098 => "E098",
            Self::E099 => "E099",
            Self::E100 => "E100",
            Self::E101 => "E101",
            Self::E102 => "E102",
            Self::E103 => "E103",
            Self::E104 => "E104",
            Self::E105 => "E105",
            Self::E106 => "E106",
            Self::E107 => "E107",
            Self::E108 => "E108",
            Self::E109 => "E109",
            Self::E110 => "E110",
            Self::E111 => "E111",
            Self::E112 => "E112",
            Self::E113 => "E113",
            Self::E114 => "E114",
            Self::E115 => "E115",
            Self::E116 => "E116",
            Self::E117 => "E117",
            Self::E118 => "E118",
        }
    }

    /// Short human-readable title for this diagnostic code.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "a flat one-arm-per-code message table that necessarily grows with the diagnostic set"
    )]
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
            Self::E011 => "retired (lane-A audit) — parser always creates FILE_PATH",
            Self::E012 => "divert is missing a target",
            Self::E013 | Self::E018 => "retired (lane-A audit) — parser always creates PATH node",
            Self::E014 => "logic line has no effect",
            Self::E015 => "expression is missing an operand",
            Self::E016 => "unknown or unsupported operator",
            Self::E017 => "function call is missing a name",
            Self::E019 => "retired (lane-A audit) — parser guarantees bullet markers",
            Self::E020 => "inline conditional is missing a condition",
            Self::E021 => "inline sequence has no branches",
            Self::E022 => "duplicate knot definition",
            Self::E023 => "duplicate variable/constant definition",
            Self::E024 => "unresolved divert target",
            Self::E025 => "unresolved variable reference",
            Self::E026 => "duplicate list item",
            Self::E027 => "ambiguous bare list item reference",
            Self::E028 => "retired (lane-A audit) — circular INCLUDE surfaces as CompileError",
            Self::E029 => "choice in conditional must explicitly divert",
            Self::E030 => "string interpolation in constant initializer is ignored",
            Self::E031 => "function call argument count mismatch",
            Self::E032 => "return statement outside function",
            Self::E033 => "unreachable code after divert",
            Self::E034 => "choice set has only fallback choices",
            Self::E035 => "name shadows a built-in function",
            Self::E036 => "expected diagnostic not produced",
            Self::E037 => "syntax error",
            Self::E038 => "malformed doc-comment tag",
            Self::E039 => "manifest disagrees with EXTERNAL arity",
            Self::E040 => "unknown semantic type",
            Self::E041 => "external argument type mismatch",
            Self::E042 => "external argument out of domain",
            Self::E043 => "doc-comment tag not applicable to this declaration",
            Self::E044 => "unknown directive",
            Self::E045 => "directive has no valid target here",
            Self::E046 => "directive must be static text",
            Self::E047 => "directive must be the only tag on its line",
            Self::E048 => "duplicate directive",
            Self::E049 => "directive not supported on this target",
            Self::E050 => "directive does not take arguments",
            Self::E051 => "brink extension used under strict-ink dialect",
            Self::E052 => "brink extension not yet implemented",
            Self::E053 => "retired (T1b-2) — T1b extension lowering is complete",
            Self::E054 => "block-scoped temp shadows an already-visible temp",
            Self::E055 => "collection mutator's first argument is not an lvalue",
            Self::E056 => "collection mutator used in expression position",
            Self::E057 => "break/continue outside a loop",
            Self::E058 => "collection mutator argument count mismatch",
            Self::E059 => "choice/gather construct nested inside inline content",
            Self::E060 => "internal codegen error",
            Self::E061 => "unknown type name in annotation",
            Self::E062 => "retired (T1c-1) — fn(T…): R annotations now resolve for real",
            Self::E063 => "type annotation disagrees with inferred type",
            Self::E064 => "strict types require the brink dialect",
            Self::E065 => "type escapes strict inference as Unknown",
            Self::E066 => "type is Conflicted under strict inference",
            Self::E067 => "assigning the result of a void function",
            Self::E068 => "struct construction literal names an undeclared STRUCT",
            Self::E069 => "struct construction literal is missing a declared field",
            Self::E070 => "struct construction literal supplies an undeclared field",
            Self::E071 => "struct construction literal field disagrees with the declared type",
            Self::E072 => "retired (TM-4c) — struct constructs now lower for real",
            Self::E073 => {
                "struct construction literal names an unresolved STRUCT shape at LIR lowering"
            }
            Self::E074 => "chained field-write projection (p.a.b = v) is not supported",
            Self::E075 => {
                "struct construction literal is not supported as a VAR/CONST declaration default"
            }
            Self::E076 => {
                "map literal key in a VAR/CONST declaration default is not a compile-time-constant scalar (int/string/bool)"
            }
            Self::E077 => {
                "array element, map value, or #fn bound value argument in a VAR/CONST declaration default is not a compile-time-constant expression"
            }
            Self::E078 => "int()/float() argument is outside the permissive numeric+bool domain",
            Self::E079 => "#fn target is not a statically-named function definition",
            Self::E080 => {
                "ref-argument (#fn, call, or bind) does not bind a durable cell at creation"
            }
            Self::E081 => "#fn binds more arguments than the target declares",
            Self::E082 => "block-scoped temp referenced after its block has closed",
            Self::E083 => "VAR/CONST declaration default is not a compile-time-constant expression",
            Self::E084 => "struct construction literal supplies a duplicate field",
            Self::E085 => {
                "file's module (its stem) collides with a declared module of the same name"
            }
            Self::E086 => {
                "`#@module` requires exactly one module name and may appear at most once per file"
            }
            Self::E087 => "reference to a `#@private` definition in another module",
            Self::E088 => {
                "bare `IMPORT { name } FROM mod` names a definition the declared module does not export"
            }
            Self::E089 => "`IMPORT` brings the same name into scope more than once",
            Self::E090 => "a module cannot `IMPORT` itself",
            Self::E091 => {
                "qualified access is ambiguous: the name is both an imported module and a definition"
            }
            Self::E092 => "redundant `#@public`/`#@private` — restates the module default",
            Self::E093 => "conflicting or repeated visibility directives on one declaration",
            Self::E094 => "`#@was` requires exactly one non-empty old-name argument",
            Self::E095 => "`#@was` names the definition's own current name — nothing to migrate",
            Self::E096 => "duplicate definition declared in two different modules",
            Self::E097 => "`ref` projection expression outside ref-argument position",
            Self::E098 => "ref-argument path segment disagrees with the statically-known shape",
            Self::E099 => "path-projection ref-argument is not yet lowerable (T1e-2, #828)",
            Self::E100 => "`#@effects` requires `pure` or at least one reads/writes/calls clause",
            Self::E101 => "malformed `#@effects` clause (unknown keyword or non-identifier value)",
            Self::E102 => "`#@effects` clause names an unknown global cell or external",
            Self::E103 => "inferred effects exceed the `#@effects` assertion's declared bound",
            Self::E104 => {
                "direct-call syntax requires a bare variable/temp/param callee — use `call(f, args…)` for a computed callee"
            }
            Self::E105 => {
                "`await` condition must be effect-free (read-only) — it writes a global or performs an effectful call"
            }
            Self::E106 => "map-literal key is outside the int/string/bool key domain",
            Self::E107 => "bare `none` needs a type from context",
            Self::E108 => {
                "inferred effects exceed the `@[effects(silent)]` assertion (the definition can produce content)"
            }
            Self::E109 => {
                "inferred effects exceed the `@[effects(total)]` assertion (the definition can raise a turn-terminating fault)"
            }
            Self::E110 => {
                "`#@effects(…)` is deprecated; use the `@[effects(…)]` annotation spelling"
            }
            Self::E111 => "unknown annotation name (the `@[…]` channel recognizes only `effects`)",
            Self::E112 => {
                "annotation line outside a recognized placement (top of a knot/stitch body)"
            }
            Self::E113 => {
                "reserved protocol method name (`display`/`compare`/`next` belong to the protocol registry)"
            }
            Self::E114 => "protocol impl exceeds its protocol's effect contract",
            Self::E115 => "ill-formed protocol impl registration",
            Self::E116 => {
                "an `Option[T]` has no truthiness — test `== none` / `== some(x)` in the condition"
            }
            Self::E117 => "`int(r)` requires an inhabited range (NonEmptyRange)",
            Self::E118 => {
                "numeric-tower kinds are compiler-known and cannot implement registry protocols"
            }
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
            | Self::E035
            | Self::E038
            | Self::E043
            | Self::E054
            | Self::E063
            | Self::E092
            | Self::E095
            | Self::E106
            | Self::E110 => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// Parse a diagnostic code from its string representation (e.g., `"E027"`).
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "a flat one-arm-per-code table that necessarily grows with the diagnostic set"
    )]
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
            "E037" => Some(Self::E037),
            "E038" => Some(Self::E038),
            "E039" => Some(Self::E039),
            "E040" => Some(Self::E040),
            "E041" => Some(Self::E041),
            "E042" => Some(Self::E042),
            "E043" => Some(Self::E043),
            "E044" => Some(Self::E044),
            "E045" => Some(Self::E045),
            "E046" => Some(Self::E046),
            "E047" => Some(Self::E047),
            "E048" => Some(Self::E048),
            "E049" => Some(Self::E049),
            "E050" => Some(Self::E050),
            "E051" => Some(Self::E051),
            "E052" => Some(Self::E052),
            "E053" => Some(Self::E053),
            "E054" => Some(Self::E054),
            "E055" => Some(Self::E055),
            "E056" => Some(Self::E056),
            "E057" => Some(Self::E057),
            "E058" => Some(Self::E058),
            "E059" => Some(Self::E059),
            "E060" => Some(Self::E060),
            "E061" => Some(Self::E061),
            "E062" => Some(Self::E062),
            "E063" => Some(Self::E063),
            "E064" => Some(Self::E064),
            "E065" => Some(Self::E065),
            "E066" => Some(Self::E066),
            "E067" => Some(Self::E067),
            "E068" => Some(Self::E068),
            "E069" => Some(Self::E069),
            "E070" => Some(Self::E070),
            "E071" => Some(Self::E071),
            "E072" => Some(Self::E072),
            "E073" => Some(Self::E073),
            "E074" => Some(Self::E074),
            "E075" => Some(Self::E075),
            "E076" => Some(Self::E076),
            "E077" => Some(Self::E077),
            "E078" => Some(Self::E078),
            "E079" => Some(Self::E079),
            "E080" => Some(Self::E080),
            "E081" => Some(Self::E081),
            "E082" => Some(Self::E082),
            "E083" => Some(Self::E083),
            "E084" => Some(Self::E084),
            "E085" => Some(Self::E085),
            "E086" => Some(Self::E086),
            "E087" => Some(Self::E087),
            "E088" => Some(Self::E088),
            "E089" => Some(Self::E089),
            "E090" => Some(Self::E090),
            "E091" => Some(Self::E091),
            "E092" => Some(Self::E092),
            "E093" => Some(Self::E093),
            "E094" => Some(Self::E094),
            "E095" => Some(Self::E095),
            "E096" => Some(Self::E096),
            "E097" => Some(Self::E097),
            "E098" => Some(Self::E098),
            "E099" => Some(Self::E099),
            "E100" => Some(Self::E100),
            "E101" => Some(Self::E101),
            "E102" => Some(Self::E102),
            "E103" => Some(Self::E103),
            "E104" => Some(Self::E104),
            "E105" => Some(Self::E105),
            "E106" => Some(Self::E106),
            "E107" => Some(Self::E107),
            "E108" => Some(Self::E108),
            "E109" => Some(Self::E109),
            "E110" => Some(Self::E110),
            "E111" => Some(Self::E111),
            "E112" => Some(Self::E112),
            "E113" => Some(Self::E113),
            "E114" => Some(Self::E114),
            "E115" => Some(Self::E115),
            "E116" => Some(Self::E116),
            "E117" => Some(Self::E117),
            "E118" => Some(Self::E118),
            _ => None,
        }
    }
}
