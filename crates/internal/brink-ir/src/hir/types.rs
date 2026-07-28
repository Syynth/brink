use rowan::TextRange;

use crate::provenance::Provenance;

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
    pub ptr: Provenance,
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
    /// Source-level `@[allow(Exxx, …)]` suppression scopes (issue #1161):
    /// one entry per well-formed `@[allow(…)]` annotation, carrying the
    /// annotated declaration's own span and the codes it silences inside
    /// it. Populated by the native frontend's annotation channel
    /// (`hir::lower_native::annotation`); always empty for the ink
    /// frontend, whose `@[…]` channel has no `allow` tenant yet.
    ///
    /// Consumed by [`crate::suppressions::apply_suppressions`] — the same
    /// filter the `//brink-disable` comment channel already flows through
    /// — via `brink-db`'s `suppressions_query`, so every diagnostic
    /// consumer (CLI, LSP, wasm) gets the filtering from one seam.
    pub allow_scopes: Vec<crate::suppressions::AllowScope>,
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

/// A knot definition (or a top-level stitch promoted to knot status).
///
/// A `Knot` can originate from either a `== knot` definition or a
/// top-level `= stitch` (promoted to knot status during HIR lowering).
/// The origin is preserved in `ptr`'s node class — [`NodeClass::Knot`]
/// vs [`NodeClass::Stitch`] — the former `ContainerPtr` discrimination
/// (F-I#5); see [`Knot::symbol_kind`].
///
/// [`NodeClass::Knot`]: crate::provenance::NodeClass::Knot
/// [`NodeClass::Stitch`]: crate::provenance::NodeClass::Stitch
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knot {
    pub ptr: Provenance,
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
    /// Inline `///` doc-comment metadata, if any (B0.4,
    /// docs/hir-admission-contract.md Q3(b)). Additive — carries what
    /// `declare_full`'s `doc` parameter used to route straight into the
    /// manifest, so [`crate::symbols::project_manifest`] can rebuild the
    /// same `SymbolManifest.docs` entry from the HIR node alone.
    pub doc: Option<crate::host_manifest::DocBlock>,
    /// Explicit `#@private`/`#@public` visibility override, if any (M-2,
    /// docs/modules-spec.md §4). Additive for B0.4 — mirrors
    /// `DeclaredSymbol.visibility`.
    pub visibility: Option<crate::VisibilityMark>,
    /// `#@was(old_name)` rename record, if any (M-3,
    /// docs/modules-spec.md §5). Additive for B0.4 — mirrors
    /// `DeclaredSymbol.was`.
    pub was: Option<(String, TextRange)>,
}

impl Knot {
    /// The [`crate::SymbolKind`] this container was indexed under:
    /// `Stitch` for a top-level stitch promoted to knot status
    /// (provenance class [`crate::provenance::NodeClass::Stitch`]), `Knot`
    /// otherwise — the former `ContainerPtr` variant discrimination
    /// (F-I#5, the #626 floating-stitch trap).
    #[must_use]
    pub fn symbol_kind(&self) -> crate::SymbolKind {
        if self.ptr.class() == crate::provenance::NodeClass::Stitch {
            crate::SymbolKind::Stitch
        } else {
            crate::SymbolKind::Knot
        }
    }
}

/// A stitch definition within a knot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stitch {
    pub ptr: Provenance,
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Block,
    /// Marked flow-private via a `#@local` directive line at the top of
    /// the body (`docs/directive-annotations-spec.md`).
    pub is_local: bool,
    /// The `#@effects(…)` assertion directive line at the top of the body,
    /// if any (T2-2, docs/effects-spec.md §10, issue #861).
    pub effects_assertion: Option<EffectsAssertion>,
    /// The stitch-header return type annotation (NG-C, issue #1489, widened
    /// to stitches by #1509: `= name(params): type` for ink, `flow
    /// name(params): type { … }` for a nested native flow). `None` when
    /// absent — same "no annotation" vs. explicit-`void` distinction as
    /// [`Knot::return_type`], and the same coroutine-vs-state toggle: a
    /// nested native flow that declares one is exempted from the
    /// implicit-`-> DONE` grace (`lower_native::container::lower_stitch`).
    pub return_type: Option<TypeExpr>,
    /// Inline `///` doc-comment metadata, if any (B0.4). See [`Knot::doc`].
    pub doc: Option<crate::host_manifest::DocBlock>,
    /// Explicit `#@private`/`#@public` visibility override, if any (B0.4).
    /// See [`Knot::visibility`].
    pub visibility: Option<crate::VisibilityMark>,
    /// `#@was(old_name)` rename record, if any (B0.4). See [`Knot::was`] —
    /// note the *stored* old name is already fully qualified
    /// (`knot.old_stitch_name`) for a nested stitch, matching
    /// `DeclaredSymbol.was`'s convention (`lower_stitch`'s qualification).
    pub was: Option<(String, TextRange)>,
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
    /// `name<args…>` — `List<L>`, `Array<T>`, `Map<K, V>`, or an
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
    /// The block's value/control-flow shape, derived from `stmts`' final
    /// statement (docs/block-effect-model.md §2, §10 row j — S1 of the
    /// block/effect migration). **Expand-phase groundwork only:** both
    /// frontends populate this field, but `stmts` remains the sole source of
    /// truth for every consumer (analyzer, HIR→LIR lowering, codegen) — `tail`
    /// is redundant-but-correct data, not yet read by anything. A later
    /// migrate/contract slice cuts consumers over and lets `stmts` stop
    /// carrying the terminator. Use [`Block::tail`] to read it and
    /// [`tail_from_stmts`]/[`Block::recompute_tail`] to (re)derive it.
    pub tail: Tail,
}

impl Block {
    /// Body block with no label/container, `tail` derived from `stmts`.
    #[must_use]
    pub fn from_stmts(stmts: Vec<Stmt>) -> Self {
        let tail = tail_from_stmts(&stmts);
        Self {
            label: None,
            stmts,
            container_id: None,
            tail,
        }
    }

    /// The block's [`Tail`] — see the field doc for the migration status
    /// (S1, docs/block-effect-model.md §10 row j: populated, unconsumed).
    #[must_use]
    pub fn tail(&self) -> &Tail {
        &self.tail
    }

    /// Recompute `tail` from the current `stmts`. Frontends call this after
    /// mutating an already-built block's `stmts` in place (e.g. appending a
    /// synthesized divert, or splicing weave-fold content into a choice
    /// body) so `tail` doesn't go stale relative to the new final statement.
    pub fn recompute_tail(&mut self) {
        self.tail = tail_from_stmts(&self.stmts);
    }
}

/// The value or control-flow shape a [`Block`] resolves to (the "tail"
/// taxonomy, docs/block-effect-model.md §2): a value-yielding expression, a
/// terminator that diverts control away, or neither ("falls through").
///
/// S1 of the block/effect migration (docs/block-effect-model.md §10 row j):
/// populated by both frontends from `stmts`' final statement, consumed by
/// nothing yet — `stmts` stays authoritative until a later slice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Tail {
    /// A value-yielding tail (a block-as-expression, e.g. the eventual
    /// interpolation/value-block case, docs/block-effect-model.md §3). Not
    /// produced by any construct this slice populates — defined ahead of
    /// its consumer per the model's tail taxonomy (§2).
    Value(Expr),
    /// A terminator: the block's last statement transfers control and
    /// execution never falls through to whatever follows. Carries the same
    /// terminator data already recorded on the final `Stmt` — not a
    /// parallel representation (docs/block-effect-model.md §2).
    Diverge(Terminator),
    /// No terminating tail — execution falls through to whatever follows.
    #[default]
    Unit,
}

/// The terminator shapes a [`Tail::Diverge`] carries — the existing
/// `Divert`/`Return` statement data (DONE/END ride `Divert`'s
/// `DivertPath::Done`/`End`; explicit-return vs. tunnel-redirect ride
/// `Return`'s `ReturnKind`), reused verbatim per
/// docs/block-effect-model.md §2's "already exists" `!`-terminator note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminator {
    /// `-> target`, `-> DONE`, `-> END`.
    Divert(Divert),
    /// `~ return expr` or bare `->->` (tunnel return) — see [`ReturnKind`].
    Return(Return),
}

/// Compute the [`Tail`] a block with these statements should carry:
/// `Tail::Diverge` when the last statement is a terminator (`Stmt::Divert`
/// or `Stmt::Return`), `Tail::Unit` otherwise. `Stmt::TunnelCall` is
/// deliberately not a terminator here — a tunnel call returns control to the
/// statement after it once the tunnel pops, so a block ending in one still
/// falls through (docs/block-effect-model.md §2).
///
/// Both frontends call this at construction time; call sites that mutate an
/// already-built block's `stmts` afterward should call
/// [`Block::recompute_tail`] instead of re-deriving this inline.
#[must_use]
pub fn tail_from_stmts(stmts: &[Stmt]) -> Tail {
    match stmts.last() {
        Some(Stmt::Divert(d)) => Tail::Diverge(Terminator::Divert(d.clone())),
        Some(Stmt::Return(r)) => Tail::Diverge(Terminator::Return(r.clone())),
        _ => Tail::Unit,
    }
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
    pub ptr: Provenance,
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
    Break(Provenance),
    Continue(Provenance),
    /// A bare expression statement (function/external calls).
    ExprStmt(Expr),
    /// `await <cond>` — a `FlowFrame` suspension point inside a `~ { … }` block
    /// (docs/flow-suspension-spec.md §3).
    Await(AwaitStmt),
}

/// `if cond { … } (else …)?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub ptr: Provenance,
    pub condition: Expr,
    /// `if EXPR as NAME { … }` — the condition-position Option binding
    /// (B1b, issue #1475). Immutable, typed `T` from the condition's
    /// `Option[T]`, and scoped strictly to [`Self::body`]: an
    /// [`ElseBranch`] never sees it. Native-surface-only — the ink/brink
    /// dialect `~ { if … }` grammar has no `as` and always leaves this
    /// `None`.
    pub binding: Option<Name>,
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
    pub ptr: Provenance,
    pub condition: Expr,
    /// `while EXPR as NAME { … }` — the same binding [`IfStmt::binding`]
    /// carries, **rebound on every iteration** (B1b, issue #1475): the
    /// condition is re-evaluated per pass, so the binding tracks each
    /// pass's own `some` payload rather than snapshotting the first.
    pub binding: Option<Name>,
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
    pub ptr: Provenance,
    /// The condition expression. `None` only for a malformed bare `await`
    /// (the parser already diagnosed the missing expression).
    pub condition: Option<Expr>,
}

/// `for name in expr { … }` — or, on the native surface only, `for key,
/// val in expr { … }` (B2, issue #1461, docs/stdlib-spec.md §5/§9's F10
/// ruling: two-binding map iteration replaces `entries()`; no pair shape
/// ever materializes). `val_name` is the one additive HIR field the B0
/// fence reserved (docs/b0-sequencing.md:356) — no new node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForStmt {
    pub ptr: Provenance,
    pub var_name: Name,
    /// The second binding (`for k, v in m`'s `v`) — always `None` for the
    /// ink `~ { for … }` grammar, which has no two-binding syntax; native
    /// `.brink` sets this from `for k, v in …`'s comma-separated form.
    pub val_name: Option<Name>,
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
    pub ptr: Provenance,
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
    pub ptr: Option<Provenance>,
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
    pub ptr: Provenance,
    pub kind: CondKind,
    pub branches: Vec<CondBranch>,
}

/// A branch within a multiline conditional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondBranch {
    /// This branch's own source span — condition (if any) plus body —
    /// distinct from the enclosing [`Conditional::ptr`]'s whole-construct
    /// span (issue #404). Lets a diagnostic or editor decoration (e.g. a
    /// `- else:` fold anchor) point at this branch specifically instead of
    /// the entire conditional. Best-effort: a branch shape with no
    /// dedicated source node (e.g. the branchless-body form's implicit
    /// first branch) falls back to the narrowest real span available.
    pub ptr: Provenance,
    /// `None` for the else branch.
    pub condition: Option<Expr>,
    /// The `as` binding of the template condition form `{if EXPR as NAME:
    /// … else: …}` (B1b, issue #1475, ruled `docs/decision-log.md`
    /// 2026-07-26). Native-only: the ink/brink-dialect conditional
    /// lowerings never set it. Immutable, typed `T` from the condition's
    /// `Option[T]`, and visible **only** in this branch's own `body` — an
    /// `else` branch (`condition: None`) always carries `None` here.
    pub binding: Option<Name>,
    pub body: Block,
    /// Pre-assigned container ID for this branch's container.
    /// Stamped by [`super::stamp_container_ids`].
    pub container_id: Option<brink_format::DefinitionId>,
}

/// A sequence block (stopping, cycle, once, shuffle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub ptr: Provenance,
    pub kind: SequenceType,
    pub branches: Vec<SequenceBranch>,
    /// Pre-assigned container ID for the sequence wrapper container.
    /// Stamped by [`super::stamp_container_ids`].
    pub container_id: Option<brink_format::DefinitionId>,
}

/// A branch (alternative) within a sequence, paired with its own source
/// span (issue #404) — mirrors [`CondBranch::ptr`]. Best-effort: native's
/// pipe-separated inline alternatives (`{~ a|b|c}`) have no dedicated
/// per-alternative CST node, so their span is the union of the
/// alternative's own child nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceBranch {
    pub ptr: Provenance,
    pub body: Block,
}

// ─── Control flow ───────────────────────────────────────────────────

/// `-> target` — simple divert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divert {
    pub ptr: Option<Provenance>,
    pub target: DivertTarget,
}

/// `->-> target` or chained tunnel calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelCall {
    pub ptr: Provenance,
    pub targets: Vec<DivertTarget>,
}

/// `<- target` — fork execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadStart {
    pub ptr: Provenance,
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
///
/// The explicit-vs-tunnel distinction is carried by [`ReturnKind`] — never
/// by `ptr` presence. `ptr` is uniform carrying-or-not provenance with no
/// semantic load: a frontend may attach provenance to a tunnel return (or
/// synthesize an explicit return without one) freely (contract D5 / F-I#6,
/// retired by B0.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    pub ptr: Option<Provenance>,
    /// Explicit `~ return` vs tunnel `->->` — the semantic bit formerly
    /// smuggled through `ptr` presence.
    pub kind: ReturnKind,
    pub value: Option<Expr>,
    /// Arguments for `->-> target(args)` tunnel onwards — pushed before the
    /// divert target on the value stack so the redirect target can pop them.
    pub onwards_args: Vec<Expr>,
}

/// Whether a [`Return`] is an explicit `~ return` or a tunnel return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnKind {
    /// `~ return [expr]` — return from a function knot. `E032` outside one.
    Explicit,
    /// `->->` / `->-> target(args)` — pop the tunnel frame, optionally
    /// redirecting onwards. Never `E032`; lowers to LIR `is_tunnel`. The
    /// future native `return -> x` respell stamps this explicitly.
    TunnelRedirect,
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
    /// Infix operation (`x + y`, `x == y`, etc.). Carries its own
    /// [`Provenance`] (issue #1517) — see [`InfixExpr`].
    Infix(InfixExpr),
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
    pub ptr: Provenance,
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
    pub ptr: Provenance,
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
    pub ptr: Provenance,
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
    pub ptr: Provenance,
    pub base: Box<Expr>,
    pub field: Name,
}

/// `#[expr, …]` — carries a `ptr` (unlike the plain literal variants above)
/// so the T1b dialect gate can point its diagnostic at the exact literal,
/// not just the enclosing statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayLiteral {
    pub ptr: Provenance,
    pub elements: Vec<Expr>,
}

/// `#{key: expr, …}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLiteral {
    pub ptr: Provenance,
    pub entries: Vec<(Expr, Expr)>,
}

/// `lhs op rhs` — one infix (binary) operation.
///
/// `ptr` is the whole operation's own source range, and it is what makes an
/// infix node **separately addressable** (issue #1517): before it existed,
/// a side table could only identify an infix node by the union of the
/// ranges reachable in its subtree, so a left-associative chain and its own
/// left spine collided whenever the trailing operand carried no range of
/// its own (`a or b or 99`). Consumers that record a per-node verdict
/// (`brink_analyzer::coalesce`, `lir::lower::expr::lower_coalesce_chain`)
/// key on this range, and a chain root's range strictly contains its left
/// spine's, so the two can never be confused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfixExpr {
    /// The whole `lhs op rhs` operation's provenance.
    pub ptr: Provenance,
    /// The left-hand operand.
    pub lhs: Box<Expr>,
    /// The operator.
    pub op: InfixOp,
    /// The right-hand operand.
    pub rhs: Box<Expr>,
}

impl InfixExpr {
    /// Build an infix node from its parts, boxing the operands.
    #[must_use]
    pub fn new(ptr: Provenance, lhs: Expr, op: InfixOp, rhs: Expr) -> Self {
        Self {
            ptr,
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
        }
    }
}

/// `base[index]`, chainable (`grid[y][x]` lowers as nested `IndexExpr`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexExpr {
    pub ptr: Provenance,
    pub base: Box<Expr>,
    pub index: Box<Expr>,
}

/// `start..end` / `start..=end` — range literal (brink extension, NS-A5,
/// docs/stdlib-spec.md §7, F7). `ptr` lets the dialect gate point its
/// diagnostic at the exact literal, matching the sibling extension shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeExpr {
    pub ptr: Provenance,
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
    /// `or`-coalescing (B1, `docs/stdlib-spec.md` §1.6a, issue #1460):
    /// `x or default`. Distinct from [`Or`](Self::Or) — ink's boolean
    /// `||`, oracle-frozen — because the two mean different things on the
    /// same textual keyword: `Or` is condition-position boolean
    /// disjunction, `Coalesce` is value-position Option unwrapping
    /// (`(Option[T],T)->T`, `(Option[T],Option[T])->Option[T]`, ruled
    /// `docs/decision-log.md` 2026-07-18). Only native lowering produces
    /// this variant (`hir::lower_native::expr::infix_op`); the legacy
    /// ink/brink lowering path never does, so it is unreachable from the
    /// oracle-covered dialects.
    ///
    /// **Evaluation strictness: eager, both operands always evaluated —
    /// an unruled implementation decision** (review finding on PR
    /// #1469/#1460, raised on #1460 for a ruling). This lowers through the
    /// same `infix_op_to_opcode` path every other `InfixOp` does, so
    /// there is no codegen-level short-circuit the way condition-position
    /// `And`/`Or` get: `x or rand::int(1, 10)` always draws (advancing RNG
    /// state) and `x or pop(ref s)` always mutates `s`, even when `x` is
    /// `some(_)` and the fallback's value is discarded. Every convention
    /// this operator's precedence placement cites (C# `??`, Kotlin `?:`)
    /// short-circuits the fallback; this implementation does not.
    Coalesce,
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
        Expr::Infix(ie) => {
            format!(
                "{} {} {}",
                display_expr(&ie.lhs),
                ie.op.as_str(),
                display_expr(&ie.rhs)
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
            Self::Coalesce => "or",
        }
    }
}

// ─── Declarations ───────────────────────────────────────────────────

/// `VAR x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub ptr: Provenance,
    pub name: Name,
    pub value: Expr,
    /// Marked flow-private via a `#@local` directive line above the
    /// declaration (`docs/directive-annotations-spec.md`).
    pub is_local: bool,
    /// The declared type annotation (TM-2, docs/typed-mode-spec.md §3:
    /// `VAR name: type = expr`), brink-dialect-gated syntax.
    pub annotation: Option<TypeExpr>,
    /// Inline `///` doc-comment metadata, if any (B0.4). See [`Knot::doc`].
    pub doc: Option<crate::host_manifest::DocBlock>,
    /// Explicit `#@private`/`#@public` visibility override, if any (B0.4).
    pub visibility: Option<crate::VisibilityMark>,
    /// `#@was(old_name)` rename record, if any (B0.4).
    pub was: Option<(String, TextRange)>,
}

/// `CONST x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDecl {
    pub ptr: Provenance,
    pub name: Name,
    pub value: Expr,
    /// The declared type annotation (TM-2, docs/typed-mode-spec.md §3:
    /// `CONST name: type = expr`), brink-dialect-gated syntax.
    pub annotation: Option<TypeExpr>,
    /// Inline `///` doc-comment metadata, if any (B0.4). See [`Knot::doc`].
    pub doc: Option<crate::host_manifest::DocBlock>,
    /// Explicit `#@private`/`#@public` visibility override, if any (B0.4).
    pub visibility: Option<crate::VisibilityMark>,
    /// `#@was(old_name)` rename record, if any (B0.4).
    pub was: Option<(String, TextRange)>,
}

/// `~ temp x = expr`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempDecl {
    pub ptr: Provenance,
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
    pub ptr: Provenance,
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
    pub ptr: Provenance,
    pub name: Name,
    pub members: Vec<ListMember>,
    /// Inline `///` doc-comment metadata, if any (B0.4). See [`Knot::doc`].
    pub doc: Option<crate::host_manifest::DocBlock>,
    /// Explicit `#@private`/`#@public` visibility override, if any (B0.4).
    pub visibility: Option<crate::VisibilityMark>,
    /// `#@was(old_name)` rename record, if any (B0.4).
    pub was: Option<(String, TextRange)>,
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
    pub ptr: Provenance,
    pub name: Name,
    /// Declared fields, in source order — the same order
    /// `brink_format`'s `Value::Record` flat field vector will follow once
    /// TM-4c's codegen assigns a `ShapeId`.
    pub fields: Vec<StructFieldDecl>,
    /// Inline `///` doc-comment metadata, if any (B0.4). See [`Knot::doc`].
    pub doc: Option<crate::host_manifest::DocBlock>,
    /// Explicit `#@private`/`#@public` visibility override, if any (B0.4).
    /// Structs never carry a `#@was` rename (the lowering never parses one
    /// for `STRUCT` — M-2 only wires visibility for this kind), so there is
    /// no `was` field here, unlike the other declaration nodes.
    pub visibility: Option<crate::VisibilityMark>,
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
    pub ptr: Provenance,
    pub name: Name,
    pub param_count: u8,
    /// Per-parameter names (B0.4 addition — `param_count` alone cannot
    /// reconstruct `DeclaredSymbol.params`, which the manifest carries for
    /// hover/signature help; `is_ref`/`is_divert` are always `false` for an
    /// `EXTERNAL` parameter, mirroring the pre-B0.4 manifest population in
    /// `decl::external::declare_and_lower`). Invariant: `params.len() ==
    /// usize::from(param_count)`.
    pub params: Vec<crate::ParamInfo>,
    /// Inline `///` doc-comment metadata, if any (B0.4). See [`Knot::doc`].
    pub doc: Option<crate::host_manifest::DocBlock>,
    /// Explicit `#@private`/`#@public` visibility override, if any (B0.4).
    pub visibility: Option<crate::VisibilityMark>,
    /// `#@was(old_name)` rename record, if any (B0.4).
    pub was: Option<(String, TextRange)>,
}

/// `INCLUDE path/to/file.ink`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeSite {
    pub file_path: String,
    pub ptr: Provenance,
}
