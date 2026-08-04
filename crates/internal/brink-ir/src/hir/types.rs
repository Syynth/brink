use std::collections::BTreeMap;

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
    /// Every prose line the natural-notation element dispatcher claimed
    /// (issue #1838), in source order — the per-line classification record
    /// the no-invisible-expansion ruling requires. Populated by the native
    /// frontend (`hir::lower_native::element`); always empty for the ink
    /// frontend, whose grammar has no `@[element]` channel.
    pub element_matches: Vec<ElementMatch>,
    /// Every `@NAME` cue payload written in this file, harvested
    /// independent of whether any conventions handler claims the line
    /// (issue #2114, `docs/prose-dialect-spec.md` §5's "harvest by
    /// default" ruling — see [`CueSite`]'s own doc for why this cannot be
    /// derived from `element_matches`). Populated by the native frontend;
    /// always empty for the ink frontend, whose grammar has no cue
    /// channel.
    pub cue_names: Vec<CueSite>,
    /// Which frontend produced this file: `true` for the native (`.brink`)
    /// surface (`hir::lower_native`), `false` for the ink one
    /// (`hir::lower::structure`).
    ///
    /// The two surfaces share every HIR shape below this struct, so a
    /// downstream pass that must decide a question the *surface* answers
    /// differently has nowhere else to ask. Today that is exactly one
    /// question — the 2026-08-01 ruling that a statically-named function in
    /// expression position **is** a fn value in native (`handler(scene)`,
    /// no sigil) while the same bare name in ink is a knot's visit count
    /// (`docs/t1c-spec.md` §2a). `lir::lower::expr::lower_path` and
    /// `brink-analyzer`'s `fn_values`/`infer` read this flag; see those
    /// sites for why a per-*file* answer (rather than the project-wide
    /// `is_native` flag `brink_analyzer::analyze_with_modules` takes) is the
    /// right granularity: the fact travels with the HIR it describes, so
    /// every pass that already holds an `HirFile` gets it for free.
    pub native: bool,
    /// Every natural-notation claiming handler *declared* in this file
    /// (issue #1844), in ascending `@[convention]` `order` (issue #2164,
    /// `docs/decision-log.md` 2026-08-03 — declaration position no longer
    /// has any bearing on precedence) — independent of whether it ever
    /// claimed a line; see [`ClaimHandlerDecl`]'s own doc for why this is
    /// not derivable from `element_matches`. Populated by the native
    /// frontend (`hir::lower_native::element::collect`); always empty for
    /// the ink frontend.
    pub claim_handlers: Vec<ClaimHandlerDecl>,
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

/// An `@[element(args = "…", block)]` per-declaration annotation (issue
/// #1719, `docs/prose-dialect-spec.md` §3.5b) — the **declaration
/// surface** for a `!name`-dispatched handler: a dispatched content line
/// rewrites to a call on the annotated `fn`/`flow`, with the pattern's
/// named captures binding params by name.
///
/// Until issue #2164, this struct also carried the natural-notation
/// `claims = "…"` spelling (a `claims: bool` field distinguished the two).
/// The 2026-08-03 ruling ("Claiming and `!name` dispatch split into two
/// annotations: `@[convention]` and `@[element]`", `docs/decision-log.md`)
/// splits that claiming half out into [`ConventionAnnotation`], declared
/// under its own `@[convention(claims = "…", order = N)]` name. What
/// remains here is `!name` dispatch only: self-announcing, legal
/// anywhere, and carrying **no `order`** — a handler that names itself
/// never competes for a line, so it needs no precedence over anything.
/// `§9.1` item 1's "there is ONE element mechanism, not two" still holds
/// at the *lowering* layer (both annotations still lower to a matched
/// line, captures bound by name, and exactly one handler call) — only the
/// authoring surface split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementAnnotation {
    /// The portable-regex source text of the `args = "…"` clause, anchored
    /// against the dispatched line's remainder (after the `!name ` prefix
    /// is stripped).
    pub pattern: String,
    /// `pattern`'s named capture groups, in the order `regex::Regex::
    /// capture_names` yields them — the set a paired `@[style(…)]`'s keys
    /// are validated against (`E162`), and the set that binds the
    /// annotated declaration's params by name.
    pub captures: Vec<String>,
    /// An explicit dispatch-name alias (`name = "…"` in the same
    /// annotation), overriding the fn/flow's own name as the `!`-sigil
    /// dispatch key. `None` when the declaration's own name is the
    /// dispatch name.
    pub alias: Option<String>,
    /// The bare `block` clause (issue #1839, `docs/decision-log.md`
    /// 2026-07-31 "Conventions are annotated handlers"): `@[element(args =
    /// "…", block)]` declares that the handler captures the **following
    /// run** — terminated by a blank line or any element-level line — into
    /// a trailing `content`-typed parameter, the same first-class
    /// fragment-capture path (`BeginFragment`…`EndFragment` →
    /// `Value::FragmentRef`, `brink_format::Opcode`) an ordinary call
    /// expression already uses, widened in scope rather than a new
    /// mechanism. The handler **wraps** the block (receives the content,
    /// decides emission) and does not tag it — an ambient "current
    /// speaker" was considered and rejected as implicit state.
    ///
    /// `true` only when the declaration also has a qualifying trailing
    /// `content`-typed parameter (`E166` otherwise — see
    /// [`crate::hir::lower_native::annotation::parse_element`]). The
    /// terminator search, the fragment capture itself, and the dispatch
    /// call are `hir::lower_native::element`'s `try_claim`/`try_dispatch`
    /// (issue #1839) — this field is what tells them to run it.
    pub block: bool,
    /// Source range of the whole `@[element(…)]` annotation line.
    pub range: TextRange,
}

/// An `@[convention(claims = "…", order = N)]` per-declaration annotation
/// (issue #2164, `docs/decision-log.md` 2026-08-03) — the **declaration
/// surface** for a pattern-claiming handler: a claimed prose line (one
/// that announces nothing of its own — no `!name` sigil) rewrites to a
/// call on the annotated top-level `fn`, with the pattern's named
/// captures binding params by name.
///
/// Split out of the old `@[element(claims = "…")]` spelling (issue #1838)
/// by the 2026-08-03 ruling: a claiming handler *competes* for lines it
/// did not announce, so it needs an explicit precedence (`order`) and it
/// stays **confined** to the `brink.toml`-named conventions module (§9.1
/// item 4, issue #1844's `E169`) — properties [`ElementAnnotation`]'s
/// `!name` dispatch does not share, since a self-announcing handler never
/// competes for anything. Scene heading, cue, parenthetical are
/// conventions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionAnnotation {
    /// The portable-regex source text of the `claims = "…"` clause,
    /// anchored against the claimed line's whole text.
    pub pattern: String,
    /// The claiming precedence (`order = N`) — a bare integer, RULED
    /// (`docs/decision-log.md` 2026-08-03 "`order` is REQUIRED on
    /// `@[convention]`…") over a sparse-integer convention or named
    /// anchors. **Required**: a `@[convention]` with no `order` clause is
    /// `E178` and yields no `ConventionAnnotation` at all — there is no
    /// default, because `order` can never be absent. Two declarations in
    /// one module carrying the same `order` is `E179` (both declarations
    /// named), enforced across the module's collected handlers
    /// (`hir::lower_native::element::diagnose_duplicate_order`) rather
    /// than here, where only one declaration is in view at a time. Lower
    /// values are tried first — the walk still tries every registered
    /// pattern against every line, uses the first match, and records the
    /// rest as shadowed (2026-08-02 match-ordering ruling, unaffected);
    /// only the *source* of that trial order changed, from declaration
    /// position to this field.
    pub order: i64,
    /// `pattern`'s named capture groups, in the order `regex::Regex::
    /// capture_names` yields them — the set a paired `@[style(…)]`'s keys
    /// are validated against (`E162`), and the set every argument of the
    /// rewritten call comes from (`E167`: a claimed line has no other
    /// source of arguments).
    pub captures: Vec<String>,
    /// The bare `block` clause (issue #1839) — identical meaning to
    /// [`ElementAnnotation::block`]'s own doc, for a claiming handler
    /// instead of a `!name`-dispatched one (the built-in screenplay
    /// preset's `cue`/`parenthetical` handlers both declare it).
    pub block: bool,
    /// The `attach = StructName` clause (issue #2178, split from #2164's
    /// backport comment "item 2"), if declared — the handler's attached
    /// output **schema**: a declared `struct`'s name, naming which keys
    /// the handler attaches to the run that follows and their types.
    /// `docs/decision-log.md` 2026-08-03 "The element output model": *"A
    /// `struct` is already declarative, statically known, serialized, and
    /// understood by compiler + editor + host — so the projection
    /// carries a type and no new declarative sub-language exists."*
    /// **Declared** here (the schema); the handler body **computes** the
    /// values — the governing split the same ruling states: "keys are
    /// declared, values are computed." A handler may never attach a
    /// computed key *name*: that isn't a check this field needs, because
    /// nothing about a plain `struct` return type lets a handler invent
    /// one — the field names are fixed by the struct's own declaration.
    ///
    /// `None` when absent: `attach` is optional, unlike `order` — a
    /// claiming handler that only ever emits text (the pre-#2178 shape)
    /// still needs no declared schema.
    ///
    /// Validated against [`Knot::return_type`]/[`Stitch::return_type`] by
    /// [`crate::hir::lower_native::annotation::parse_convention`]: the
    /// declared return type's bare name must equal this clause's name, or
    /// the mismatch is `E180` and this annotation is not attached at all
    /// (the same "never a partial one" posture `E159`/`E178` already
    /// take). This module never resolves whether a struct of this name is
    /// actually *declared* — the same shallow, declaration-surface-only
    /// posture the rest of the annotation-lowering module takes for every
    /// other clause here — that is real name resolution's job, not this
    /// one's.
    pub attach: Option<Name>,
    /// Source range of the whole `@[convention(…)]` annotation line.
    pub range: TextRange,
}

/// The prose shape a claimed line was written as — the "matched kind" half
/// of the per-line classification record (issue #1838).
///
/// Deliberately structural, not a preset vocabulary: it names the *grammar*
/// node the line came from, so an editor can say "this is a scene heading
/// that matched handler `scene`" without the compiler owning a closed list
/// of element names. The element's *name* is the handler's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    /// An ordinary prose line (`CONTENT_LINE`) with no structural sigil.
    ContentLine,
    /// A scene heading (`SCENE_HEADING`, `docs/prose-dialect-spec.md`
    /// §8b.2/.3) — the `INT.`/`EXT.` prefixed header line.
    SceneHeading,
    /// A `!name` sigil dispatch (`BANG_DISPATCH`, §3.5b, issue #2004) —
    /// self-announcing, unlike the other two variants above (both of
    /// which are *claimed*, i.e. matched without any structural marker of
    /// their own).
    BangDispatch,
    /// A block cue (`CUE`, `docs/prose-dialect-spec.md` §8b.9/§8d.4) — a
    /// bare `@NAME` speaker line with no tag extension (a cue carrying a
    /// tag, e.g. `@VENDOR #(v.o.)`, is declined the same way a
    /// slug/tag-carrying [`Self::SceneHeading`] is — see
    /// [`crate::hir::lower_native::element::candidate`]'s own doc).
    /// Issue #1720: the built-in screenplay preset's `cue` handler is the
    /// first consumer.
    Cue,
    /// A parenthetical delivery line (`PARENTHETICAL`,
    /// `docs/prose-dialect-spec.md` §8.), chain-gated by the parser (only
    /// recognized directly after a live cue — `brink-syntax-native`'s
    /// `at_parenthetical`). Issue #1720: the built-in screenplay preset's
    /// `parenthetical` handler is the first consumer.
    Parenthetical,
}

/// One named capture bound by a claimed line, as a **span into real
/// source** rather than a copied string alone (issue #1838's
/// no-invisible-expansion guard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementCapture {
    /// The capture group's name — also the handler parameter it binds.
    pub name: String,
    /// The captured text.
    pub text: String,
    /// Where in the claimed line the capture came from.
    pub range: TextRange,
}

/// What the compiler did with a claimed line — the "disposition" column of
/// the classification record.
///
/// One variant today: the ruled rewrite is *exactly one call*. It is an
/// enum rather than a bare marker because the ruling names the other
/// dispositions a handler expresses by what its body does ("content" is a
/// line with no handler; "nothing" is a handler that emits nothing), and a
/// tooling consumer reading this record should not have to re-derive which
/// of those it is looking at from the absence of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementDisposition {
    /// The line was rewritten to exactly one call on the matched handler,
    /// emitted in the line's own position.
    Call,
}

/// One line the natural-notation element dispatcher claimed, recorded so
/// nothing the compiler rewrote is invisible to tooling (issue #1838; the
/// no-invisible-expansion guard, `docs/prose-dialect-spec.md` §3.5b
/// "Tooling transparency").
///
/// Every field points at real source: the claimed line's own range, the
/// handler's name *and the range of its declaration*, and each capture as a
/// span. That is what lets `LineContext`/the IDE query family answer "what
/// happened to this line, and where is the code that did it" without
/// re-running the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMatch {
    /// The claimed line's own source range.
    pub line: TextRange,
    /// The prose shape the line was written as.
    pub kind: ElementKind,
    /// The matched handler's name, carrying its own declaration-site range.
    pub handler: Name,
    /// The range of the `@[element(claims = "…")]` annotation that claimed
    /// the line — the declaration a hover jumps to.
    pub annotation: TextRange,
    /// The captures bound into the call, in parameter order.
    pub captures: Vec<ElementCapture>,
    /// What the compiler did with the line.
    pub disposition: ElementDisposition,
    /// The captured block's source range (issue #1839's `block` clause) —
    /// `None` for an ordinary (non-`block`) match. Covers every source line
    /// the terminator search absorbed into the trailing `content`-typed
    /// argument, so a tooling consumer can show "this whole span feeds
    /// `handler`'s `content` parameter" without re-running the terminator
    /// search itself — the same no-invisible-expansion guarantee `line`
    /// already gives the claimed header line alone.
    pub content: Option<TextRange>,
}

/// One natural-notation claiming handler *declared* in a file — recorded
/// regardless of whether it ever won a claim (issue #1844, the module half
/// of the §9.1 confinement ruling: "pattern-claiming is confined to ONE
/// module — the conventions module named in `brink.toml`"). [`ElementMatch`]
/// only records lines a handler actually claimed, which is the wrong ground
/// truth for this check — a claiming handler that matches nothing in its
/// *own* file (because the lines it targets live elsewhere, or simply don't
/// occur here) is still a declared claim, and still misplaced if this file
/// isn't the configured conventions module.
///
/// `params`/`pattern` are this file's own CST-derived claiming-handler
/// payload — [`crate::hir::lower_native::element::collect`]'s output,
/// consumed directly by [`crate::ConventionsProjection::from_decls`] (issue
/// #2111) now that precedence is a static `order` property rather than a
/// comptime-evaluated identity list (issue #2165 deleted the cross-file
/// injection seam this doc used to describe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimHandlerDecl {
    /// The handler's own name, carrying its declaration-site range.
    pub name: Name,
    /// Range of the `@[convention(claims = "…", order = N)]` annotation
    /// line itself — the confinement diagnostic's anchor, matching
    /// `E112`'s own placement diagnostic (the annotation line, not the
    /// declaration body).
    pub annotation: TextRange,
    /// Parameter names in declaration order — the argument order a
    /// rewritten call uses. Guaranteed by `E160`/`E167` to be exactly the
    /// pattern's named-capture set.
    pub params: Vec<String>,
    /// The claiming pattern's regex source (uncompiled — `regex::Regex`
    /// has no `Eq`, which this struct's derive needs).
    pub pattern: String,
    /// The bare `block` clause (issue #1839): `true` when the trailing
    /// declared parameter is a `content`-typed block-capture receiver, not
    /// a regex-bound capture — see
    /// [`crate::hir::lower_native::element`]'s `ClaimHandler::block` doc
    /// for what that changes about argument binding.
    pub block: bool,
    /// The claiming precedence (`order = N`), issue #2164 — see
    /// [`ConventionAnnotation::order`]'s own doc. Required on every
    /// `@[convention]`, so every `ClaimHandlerDecl` carries one (there is
    /// no "no order" case to represent).
    pub order: i64,
    /// The `attach = StructName` clause (issue #2178), if declared — see
    /// [`ConventionAnnotation::attach`]'s own doc. This is the field a
    /// future NS-T projection (#2111, blocked on this landing) reads to
    /// surface a handler's declared output schema to the editor/host,
    /// same "compiler reads the conventions module's CST" role
    /// `params`/`pattern` already play for this struct.
    pub attach: Option<String>,
}

// ─── The serialized conventions projection (issue #2111) ────────────

/// The attachment mode a `@[convention]` handler declares — issue #2164's
/// 2026-08-03 design-backport comment names this axis "mode (attach/wrap)",
/// tracked separately from the `attach = StructName` schema ruling at
/// icebox issue #2169 ("the wrap mode… is the natural home for the title
/// page once claiming can be region-scoped"). It is not a new concept: it is
/// [`ClaimHandlerDecl::block`] read as a two-value enum for projection
/// purposes, rather than a bare `bool` a tooling consumer would have to
/// remember the polarity of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConventionMode {
    /// The ordinary case: the handler's pattern matches exactly the one
    /// claimed line. Nothing following it is captured.
    Attach,
    /// The `block` clause (issue #1839): the handler's trailing `content`-
    /// typed parameter captures the run *following* the matched line,
    /// terminated by a blank line or any element-level line. The handler
    /// **wraps** the captured run — receives it, decides its emission.
    Wrap,
}

/// A field type's structural shape, resolved and span-free — issue #2111's
/// continuation, finding 1: "the schema is a NAME, not fields... resolved
/// FIELDS AND TYPES, never a value a handler computed." Mirrors
/// [`TypeExpr`]'s three shapes, with source ranges stripped: a `.inkb` has
/// no source text to point a range at, and this is schema, not a
/// jump-to-declaration target (that's still [`ConventionAttachField::name`]'s
/// job — it keeps its own field name as a plain `String` because a
/// `.inkb`-loaded host has no source to carry a [`Name`]'s range against
/// either; the in-process editor consumer that wants a field's declaration
/// site re-resolves the owning [`StructDecl`] itself, the same way it
/// resolves the schema's own name today).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaTypeShape {
    /// A bare nominal name: `int`, `float`, `bool`, `string`, or a declared
    /// struct name. Not recursively expanded — a field typed as another
    /// struct carries that struct's bare name here, one level deep, matching
    /// [`TypeExpr::Named`]'s own "declared struct names arrive in TM-4"
    /// note. A consumer that needs the nested struct's own fields resolves
    /// that name exactly the way this type's own name was resolved.
    Named(String),
    /// `name<args…>` — `List<L>`, `Array<T>`, `Map<K, V>`.
    Generic {
        name: String,
        args: Vec<SchemaTypeShape>,
    },
    /// `fn(params…): ret`.
    Fn {
        params: Vec<SchemaTypeShape>,
        ret: Box<SchemaTypeShape>,
    },
}

impl From<&TypeExpr> for SchemaTypeShape {
    fn from(ty: &TypeExpr) -> Self {
        match ty {
            TypeExpr::Named { name, .. } => Self::Named(name.clone()),
            TypeExpr::Generic { name, args, .. } => Self::Generic {
                name: name.clone(),
                args: args.iter().map(Self::from).collect(),
            },
            TypeExpr::Fn { params, ret, .. } => Self::Fn {
                params: params.iter().map(Self::from).collect(),
                ret: Box::new(Self::from(&**ret)),
            },
        }
    }
}

/// One resolved field of an `attach = StructName` schema (issue #2111
/// finding 1): the field's declared name and type, read straight off the
/// struct's own [`StructFieldDecl`] — never a value any handler computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionAttachField {
    pub name: String,
    pub ty: SchemaTypeShape,
}

/// The `attach = StructName` clause's resolution outcome (issue #2111
/// finding 1, superseding the pre-continuation `Option<String>` shape that
/// carried only the name). Resolved against the conventions module's own
/// file plus its transitive `IMPORT` closure (finding 3 —
/// `brink_db::queries::analysis::import_closure_query`), since
/// `attach = StructName` may legally name a struct declared in an imported
/// module, not only the conventions module's own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConventionAttachSchema {
    /// The struct resolved: its declared name plus every field, in
    /// declaration order, with each field's declared type. **Schema, never
    /// values** — nothing here is computed by running the handler's body;
    /// an editor or host consuming this never runs brink code.
    Resolved {
        name: String,
        fields: Vec<ConventionAttachField>,
    },
    /// `attach = StructName` named a struct that does not exist anywhere in
    /// the conventions module's own file or its import closure. Carries the
    /// declared name rather than dropping it silently (house rule: flag
    /// silent data drops) — a consumer can still report *what* was
    /// declared even though it could not be resolved to a schema.
    /// [`ClaimHandlerDecl::attach`]'s own doc: HIR lowering never checks
    /// struct *existence* itself ("real name resolution's job, not this
    /// one's"), so the projection query is the first layer that can even
    /// notice — it warns through the same `tracing::warn!` channel its
    /// unresolvable-pointer sibling case uses, never silently.
    Unresolved(String),
}

/// One `@[convention]` handler's projected, purely declarative shape — the
/// editor/host-readable half of a claiming declaration (issue #2111).
///
/// **Schema, never values** (`docs/decision-log.md` 2026-08-03 "The element
/// output model"): every field here is read straight off the declaration's
/// own annotation and return-type syntax. Nothing here is computed by
/// running the handler's body — an editor consuming this projection never
/// runs brink code, matching the "the editor reads data and never executes
/// brink code" boundary the no-world-reads fence (issue #2179, not yet
/// built) will eventually enforce on the *body* side of the same split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionProjectionEntry {
    /// The handler's own name, carrying its declaration-site range — what a
    /// hover/explain-match consumer jumps to.
    pub name: Name,
    /// The claiming pattern's regex source, as declared.
    pub pattern: String,
    /// The claiming precedence — see [`ConventionAnnotation::order`]'s own
    /// doc. Every entry in a [`ConventionsProjection`] carries one (`order`
    /// is required, never absent), and the projection's own `entries` are
    /// already sorted ascending by it — the same resolution order the
    /// classification walk uses.
    pub order: i64,
    /// Attach (ordinary) or wrap (`block`) — see [`ConventionMode`]'s own
    /// doc.
    pub mode: ConventionMode,
    /// What a match on this handler produces — reuses [`ElementDisposition`]
    /// verbatim rather than inventing a parallel "resulting kind" concept:
    /// every claimed line still rewrites to exactly one call, so this is
    /// [`ElementDisposition::Call`] for every entry today. Carried as a
    /// real field (not simply omitted because there is only one variant)
    /// for the same reason [`ElementMatch::disposition`] is: a tooling
    /// consumer should read what happened, not infer it from a field's
    /// absence, and this stays exact if a second disposition is ever added.
    pub disposition: ElementDisposition,
    /// The `attach = StructName` clause's resolution outcome, if the clause
    /// was declared at all — see [`ConventionAnnotation::attach`]'s own doc.
    /// `None` for a handler that only ever emits text (the pre-#2178
    /// shape). See [`ConventionAttachSchema`]'s own doc for the
    /// `Resolved`/`Unresolved` split — issue #2111 finding 1's resolved
    /// field list, superseding the pre-continuation `Option<String>` shape
    /// that carried only the struct's bare name.
    pub attach: Option<ConventionAttachSchema>,
}

/// The conventions projection (issue #2111): every `@[convention]` handler
/// declared in the project's one configured conventions module, in
/// ascending `order` — the flat, ordered, purely-declarative record an
/// editor reads instead of tracing execution.
///
/// # What #2111 does NOT yet deliver
///
/// Recorded here rather than left to be re-discovered, per the house rule
/// that a doc must state current state upfront. The 2026-08-04 continuation
/// closed two of the three gaps the original slice left open:
///
/// 1. ~~The attachment schema is a NAME, not a struct.~~ **CLOSED.**
///    [`ConventionProjectionEntry::attach`] now carries a
///    [`ConventionAttachSchema`] — the resolved field list (names + declared
///    types), not merely the struct's bare name. Resolution walks the
///    conventions module's own file plus its transitive `IMPORT` closure
///    (item 3, now also closed) — see [`Self::from_decls`]'s doc.
/// 2. **Still open, deliberately.** Nothing here derives a wire encoding and
///    nothing emits it into `.inkb`/`StoryData` yet. The wire SHAPE this
///    projection would serialize to now exists
///    (`brink_format`'s hand-rolled `.inkb`-section-codec idiom, matching
///    `StructShapeDef`/`FrameShapeDef` rather than `serde` — this crate's
///    `.inkb` format is not serde-based anywhere), with a `to_wire()`
///    conversion (every field the wire shape carries round-trips exactly;
///    see [`Self::to_wire`]'s own doc for the one field it deliberately
///    does not carry) and round-trip tests. What does **not** yet
///    exist is a `StoryData` field / `SectionKind` tag / codegen population —
///    that requires threading a whole-project claim-handler join
///    (`brink_analyzer::conventions_registry`) through LIR lowering, which
///    `brink-compiler`'s production pipeline does not currently do (it never
///    runs through `brink-db`'s salsa layer at all), and is left to whoever
///    wires the actual #2108 host binding join — allocating a `.inkb`
///    section tag ahead of that consumer settling its exact needs would risk
///    locking in the wrong shape. Tracked as a follow-up, not silently
///    dropped.
/// 3. ~~No reusable import closure is computed.~~ **CLOSED.**
///    `brink_db::queries::analysis::import_closure_query` computes the
///    transitive `IMPORT` closure of any entry file, generically — not
///    conventions-specific — so #2167's `E169` confinement relaxation can
///    call the exact same query.
///
/// # Why there is no comptime-fault / last-good-value case here
///
/// The issue this type answers was filed against the `fn conventions()`
/// registration design (issue #1840), where `order` came from
/// comptime-evaluating a registration function — so the projection could
/// fault (a step-limit hit, a panic) and the editor needed a last-good
/// fallback (`docs/decision-log.md` 2026-08-01 "Conventions comptime… Q2").
/// **That mechanism no longer exists.** The 2026-08-03 ruling "`fn
/// conventions()` is DISSOLVED" moved `order` onto the `@[convention]`
/// annotation itself, so every field on [`ConventionProjectionEntry`] is
/// now read straight off HIR — a pure, total function of source text (now
/// spanning the conventions module's own file plus its import closure, for
/// `attach` resolution), with no VM execution anywhere in the path. There is
/// nothing left to fault: a malformed declaration (a missing `order`, a
/// duplicate one, an `attach` name/return-type mismatch) is an ordinary
/// compile diagnostic (`E178`/`E179`/`E180`) reported by the existing
/// per-file lowering pass; an `attach` name that simply does not resolve to
/// any declared struct becomes [`ConventionAttachSchema::Unresolved`] plus a
/// `tracing::warn!`, not a fault this type needs to degrade under.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConventionsProjection {
    /// Every declared `@[convention]` handler, ascending by `order`.
    pub entries: Vec<ConventionProjectionEntry>,
}

impl ConventionsProjection {
    /// Build a projection from one file's already-collected claim-handler
    /// declarations (`ClaimHandlerDecl`, ordered ascending by `order` —
    /// [`crate::hir::lower_native::element::collect`]'s own contract).
    /// Order is preserved, not re-sorted: this function trusts its input's
    /// ordering rather than re-deriving it, so a caller that hands in an
    /// unsorted slice gets an unsorted projection back — re-sorting here
    /// would silently mask a caller bug instead of surfacing it.
    ///
    /// `structs` is the caller-resolved struct-name → field-list map (issue
    /// #2111 finding 1 + finding 3): every struct visible from the
    /// conventions module's own file plus its transitive `IMPORT` closure,
    /// keyed by bare struct name. Building that map — walking the closure,
    /// breaking name collisions deterministically — is the caller's job
    /// (`brink_db::queries::analysis::conventions_projection_query`); this
    /// function only does the per-entry lookup, so it stays a pure function
    /// of its two inputs with no salsa/project awareness of its own. An
    /// `attach` name absent from `structs` becomes
    /// [`ConventionAttachSchema::Unresolved`] — never silently dropped, and
    /// never a fabricated empty field list.
    #[must_use]
    pub fn from_decls(
        decls: &[ClaimHandlerDecl],
        structs: &BTreeMap<String, Vec<ConventionAttachField>>,
    ) -> Self {
        Self {
            entries: decls
                .iter()
                .map(|decl| ConventionProjectionEntry {
                    name: decl.name.clone(),
                    pattern: decl.pattern.clone(),
                    order: decl.order,
                    mode: if decl.block {
                        ConventionMode::Wrap
                    } else {
                        ConventionMode::Attach
                    },
                    disposition: ElementDisposition::Call,
                    attach: decl.attach.as_ref().map(|name| match structs.get(name) {
                        Some(fields) => ConventionAttachSchema::Resolved {
                            name: name.clone(),
                            fields: fields.clone(),
                        },
                        None => ConventionAttachSchema::Unresolved(name.clone()),
                    }),
                })
                .collect(),
        }
    }

    /// Convert to the `.inkb`-section-codec wire shape (issue #2111
    /// finding 2) — a mirror with source spans stripped and (deliberately)
    /// `disposition` not carried, since every entry is the single existing
    /// `ElementDisposition::Call` case today; see
    /// `brink_format::conventions::ConventionEntryDef`'s own doc for that
    /// omission's reasoning. See `brink_format::conventions`'s own module
    /// doc for why this exists but is not yet wired into `StoryData`.
    #[must_use]
    pub fn to_wire(&self) -> brink_format::ConventionsProjectionDef {
        brink_format::ConventionsProjectionDef {
            entries: self
                .entries
                .iter()
                .map(ConventionProjectionEntry::to_wire)
                .collect(),
        }
    }
}

impl ConventionProjectionEntry {
    /// See [`ConventionsProjection::to_wire`]'s doc.
    #[must_use]
    pub fn to_wire(&self) -> brink_format::ConventionEntryDef {
        brink_format::ConventionEntryDef {
            name: self.name.text.clone(),
            pattern: self.pattern.clone(),
            order: self.order,
            mode: match self.mode {
                ConventionMode::Attach => brink_format::ConventionModeDef::Attach,
                ConventionMode::Wrap => brink_format::ConventionModeDef::Wrap,
            },
            attach: self.attach.as_ref().map(ConventionAttachSchema::to_wire),
        }
    }
}

impl ConventionAttachSchema {
    /// See [`ConventionsProjection::to_wire`]'s doc.
    #[must_use]
    pub fn to_wire(&self) -> brink_format::ConventionAttachDef {
        match self {
            Self::Resolved { name, fields } => brink_format::ConventionAttachDef::Resolved {
                name: name.clone(),
                fields: fields.iter().map(ConventionAttachField::to_wire).collect(),
            },
            Self::Unresolved(name) => brink_format::ConventionAttachDef::Unresolved(name.clone()),
        }
    }
}

impl ConventionAttachField {
    /// See [`ConventionsProjection::to_wire`]'s doc.
    #[must_use]
    pub fn to_wire(&self) -> brink_format::ConventionAttachFieldDef {
        brink_format::ConventionAttachFieldDef {
            name: self.name.clone(),
            ty: self.ty.to_wire(),
        }
    }
}

impl SchemaTypeShape {
    /// See [`ConventionsProjection::to_wire`]'s doc.
    #[must_use]
    pub fn to_wire(&self) -> brink_format::SchemaTypeDef {
        match self {
            Self::Named(name) => brink_format::SchemaTypeDef::Named(name.clone()),
            Self::Generic { name, args } => brink_format::SchemaTypeDef::Generic {
                name: name.clone(),
                args: args.iter().map(SchemaTypeShape::to_wire).collect(),
            },
            Self::Fn { params, ret } => brink_format::SchemaTypeDef::Fn {
                params: params.iter().map(SchemaTypeShape::to_wire).collect(),
                ret: Box::new(ret.to_wire()),
            },
        }
    }
}

/// One `@NAME` cue occurrence, recorded regardless of whether any
/// conventions handler ever claims the line (`docs/prose-dialect-spec.md`
/// §5, issue #2114: "harvest by default, declaration upgrades").
///
/// This is the harvest counterpart to [`ElementMatch`]: `element_matches`
/// only records a cue line a claiming handler actually won, so a project
/// with no conventions module — where every `@NAME` line reports the loud
/// `E129` and produces no `ElementMatch`/`Stmt` at all — would otherwise
/// have no record of its own cue names for cross-file completion to read.
/// [`HirFile::cue_names`] is populated by a whole-tree scan for every
/// `CUE_NAME` node (nested inside both the block `CUE` and the fused
/// `COMPACT_CUE`, `docs/prose-dialect-spec.md` §8b.9/§8d.4), independent of
/// `element`'s claim/dispatch machinery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueSite {
    /// The speaker name text, as `ast::CueName::text()` yields it (source
    /// whitespace trimmed, a recognized inline escape's backslash
    /// stripped).
    pub name: String,
    /// Source range of the `CUE_NAME` node itself.
    pub range: TextRange,
}

/// A built-in editor-presentation token (`docs/prose-dialect-spec.md`
/// §3.5b addenda 3–4) — the closed, LSP-semantic-token-style vocabulary
/// every conforming editor implements natively. Anything outside this
/// closed set is [`StyleToken::Custom`], never a diagnostic: "any other
/// name is a custom hook emitting a stable `brink-*` class for host CSS."
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleToken {
    /// `left` / `center` / `right` — line alignment.
    AlignLeft,
    AlignCenter,
    AlignRight,
    /// `bold` / `italic` / `dim` / `mono` — emphasis.
    Bold,
    Italic,
    Dim,
    Mono,
    /// `uppercase` — the one built-in case transform.
    Uppercase,
    /// `conceal` — rides the shipped hidden-span/atomic-range machinery;
    /// also the declared spelling for hiding the `!name` dispatch prefix.
    Conceal,
    /// A raw hex color (`#rgb` or `#rrggbb`) — "a basic theme-overridable
    /// default." The narrow, unambiguous shape this v1 recognizes; any
    /// other spelling (a named CSS color, a preset reference) is a
    /// [`StyleToken::Custom`] hook instead, per the spec's own fallback
    /// rule — no closed color-keyword list is invented here.
    Color(String),
    /// Any name outside the built-in vocabulary: "a custom hook emitting a
    /// stable `brink-*` class for host CSS." Never a diagnostic.
    Custom(String),
}

/// One `key = "value"` clause of an `@[style(…)]` annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleEntry {
    /// The clause's key: `"line"`, `"dispatch"`, or the name of a capture
    /// group declared by the paired [`ElementAnnotation::pattern`] or
    /// [`ConventionAnnotation::pattern`] (validated at lowering time —
    /// `E162`).
    pub key: String,
    pub value: StyleToken,
    /// Source range of this one clause (not the whole annotation line).
    pub range: TextRange,
}

/// An `@[style(…)]` per-declaration annotation (issue #1719,
/// `docs/prose-dialect-spec.md` §3.5b addenda 3–4) — declared editor
/// presentation, mapping a paired [`ElementAnnotation`]'s or
/// [`ConventionAnnotation`]'s captures (plus the two special keys
/// `line`/`dispatch`) to [`StyleToken`]s.
///
/// **Editor-presentation only.** The consumer of this data is the editor
/// track (NS-T, issues #1131/#1350), which is held — this struct exists so
/// the declaration is parsed, validated, and not silently dropped; nothing
/// in the compiler or runtime reads it yet. Buffer decoration is firmly
/// distinct from the runtime markup layer (§4) — output styling is the
/// handler's own emitted markup spans, not this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleAnnotation {
    /// Clauses in source order. Never empty — an empty argument list is
    /// `E161` and produces no [`StyleAnnotation`] at all.
    pub entries: Vec<StyleEntry>,
    /// Source range of the whole `@[style(…)]` annotation line.
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
    /// The `@[element(args = "…")]` annotation, if any (issue #1719,
    /// `docs/prose-dialect-spec.md` §3.5b). Native-only — ink has no
    /// equivalent tag.
    pub element_annotation: Option<ElementAnnotation>,
    /// The `@[convention(claims = "…", order = N)]` annotation, if any
    /// (issue #2164, split out of `@[element]` by the 2026-08-03 ruling).
    /// Native-only, and legal only above a top-level `fn` (`E112`
    /// otherwise) — see [`ConventionAnnotation`]'s own doc. Mutually
    /// exclusive with `element_annotation` in practice (a declaration
    /// names one mechanism or the other), but nothing enforces that here;
    /// both are independent declaration-surface reads.
    pub convention_annotation: Option<ConventionAnnotation>,
    /// The `@[style(…)]` annotation, if any (issue #1719, same spec
    /// section). Requires a paired `element_annotation` OR
    /// `convention_annotation` on the same declaration (`E163`) — see
    /// [`StyleAnnotation`].
    pub style_annotation: Option<StyleAnnotation>,
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
    /// The `@[element(args = "…")]` annotation, if any (issue #1719). See
    /// [`Knot::element_annotation`].
    pub element_annotation: Option<ElementAnnotation>,
    /// The `@[convention(claims = "…", order = N)]` annotation, if any
    /// (issue #2164). See [`Knot::convention_annotation`].
    pub convention_annotation: Option<ConventionAnnotation>,
    /// The `@[style(…)]` annotation, if any (issue #1719). See
    /// [`Knot::style_annotation`].
    pub style_annotation: Option<StyleAnnotation>,
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
    /// How this block's T1b lexical scope relates to its neighbors in the
    /// enclosing `Vec<Stmt>` — always `Standalone` except a code-ground
    /// body split by a `> text` prose-line escape (issue #1992 review
    /// finding F1; see `hir::lower_native::body`'s
    /// `mark_split_logic_block_scopes` doc).
    pub scope: LogicBlockScope,
}

/// How a `Stmt::LogicBlock`'s block-scope push/pop bracket relates to its
/// neighbors in the same statement stream. `lower_stmt_block_as_body`
/// (`hir::lower_native::body`) splits one code-ground `STMT_BLOCK` into
/// more than one `LogicBlock` around a `> text` prose-line escape — but a
/// `let`/`temp` declared in an earlier run must stay visible, for both
/// reads and writes, in every run after it (and in the `Stmt::Content`
/// siblings a `> text` line lowers to, including any trailing content
/// *after* the last split run), so the split runs still need to share
/// **one** T1b lexical scope spanning the whole body rather than each
/// opening and closing its own.
///
/// `lir::lower::blocks::lower_logic_block` reads this to decide whether to
/// push a scope for a given run; the matching pop is **not** attached to
/// any particular run (a `Stmt::Content` sibling can legally come after the
/// last one and still needs the scope open) — instead
/// `lir::lower::lower_block_with_children`, which lowers a whole
/// `hir::Block`'s statements in one call, pops it once after processing
/// every statement in the block, if and only if an `Opens` was seen. This
/// is sound because splitting is scoped to *this* function's caller only:
/// a `PROSE_LINE` nested inside an `if`/`while`/`for` body or a lambda's
/// braced body never reaches `lower_stmt_block_as_body` and so never
/// produces an `Opens`/`Continues` tag that could leak into some other,
/// recursively-processed block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogicBlockScope {
    /// Push a new scope on entry, pop it on exit — every `LogicBlock`
    /// except a split code-ground run.
    #[default]
    Standalone,
    /// The first of several runs split from one code-ground body: push a
    /// new scope on entry. The scope stays open for every later
    /// `Continues` sibling (and any interleaved `Stmt::Content`) — popped
    /// once, by the enclosing block's own lowering, after every statement
    /// in the block has been processed.
    Opens,
    /// A run (other than the first) split from one code-ground body:
    /// neither push nor pop — continues the scope an earlier `Opens`
    /// sibling pushed.
    Continues,
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
    /// `* {if EXPR as NAME} [text]` — the guard's Option binding (B1b,
    /// issue #1475/#1508, decision log 2026-07-26 "Choice-guard `as`
    /// un-deferred"). Capture-at-presentation, by-value COW: the guard's
    /// `OptionBind` runs (and writes the payload into its frame-local
    /// slot) at presentation time, in the same frame `BeginChoice`'s
    /// `fork_thread` snapshots into the pending choice — so the slot
    /// value rides the pending choice through selection (even across a
    /// `StorySnapshot` round trip) with no separate wire-level capture:
    /// it is the same mechanism that already restores tunnel/function
    /// temps across a pick. Scoped strictly to [`Self::body`], same as
    /// [`IfStmt::binding`]. Native-surface-only — the ink/brink dialect
    /// choice-guard grammar has no `as` and always leaves this `None`.
    pub binding: Option<Name>,
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
    /// `<name attr="v">…</name>` — an inline markup span
    /// (`docs/prose-dialect-spec.md` §4, issue #1716). Genuinely nested —
    /// `children` is the span's own content run, which may itself contain
    /// another `Span`, `Interpolation`, or (per the nesting doctrine, §4.3)
    /// a fully-closed `InlineConditional`/`InlineSequence`. Presentational
    /// only: `attrs` and `name` are never part of a line's translation
    /// identity (§4.4's hash-transparency ruling — see
    /// `lir::lower::recognize`, the one place that matters).
    Span(SpanPart),
}

/// The payload of a [`ContentPart::Span`] — also the shape
/// `lir::lower::recognize` mirrors onto the wire `LinePart::Span` when a
/// span is admitted to line recognition (§4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanPart {
    /// This span's own provenance (issue #1782) — the whole `<name …>…
    /// </name>` (or self-closing `<name …/>`) node's range, `NodeClass::Span`.
    /// Lets a diagnostic (`E164`/`E165`) anchor to the exact span instead of
    /// its enclosing content line, so several spans on one line — even
    /// repeats of the same undeclared tag — each get their own,
    /// distinguishable range. Never resolved outside IDE tooling (contract
    /// §4.3): analysis/LIR/codegen consume it as plain range data.
    pub ptr: Provenance,
    /// The tag name — freeform (§4.2): never validated against a fixed set
    /// at this layer. Manifest validation, when a host declares one, is a
    /// separate, later pass over the same tree.
    pub name: String,
    /// `name="value"` pairs, in source order. Static text only — see
    /// `SyntaxKind::SPAN_ATTR_VALUE`'s doc for why attribute values don't
    /// support `{expr}` interpolation.
    pub attrs: Vec<(String, String)>,
    /// The span's content — empty for a self-closing / point-marker span
    /// (`<pause/>`, `<sfx name="bell"/>`, §8b.11).
    pub children: Vec<ContentPart>,
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
    /// `|x| expr` / `|g: Guest|: bool { … }` — an anonymous fn value
    /// (native surface, RULED 2026-07-19 "Lambdas ruled: Rust pipes under
    /// the `RustScript` north star"; issue #1685). Only the native frontend
    /// produces this shape — ink's grammar cannot spell a lambda at all.
    ///
    /// Boxed: a lambda carries params, an annotation and a whole body, and
    /// leaving it inline would make every `Expr` (and so every `Stmt`,
    /// `Content`, …) pay for the largest variant in the tree.
    Lambda(Box<LambdaExpr>),
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
    /// An internal fragment-capture expression — **not constructible from
    /// surface syntax** (`docs/decision-log.md` 2026-08-01 "Content-as-value:
    /// an internal `Expr::Fragment` lowering form, turn-scoped — no new
    /// primitive, no surface syntax"). Evaluates `stmts` through the normal
    /// statement-lowering path *inside* a `BeginFragment`/`EndFragment`
    /// bracket and yields the resulting `Value::FragmentRef` — the same
    /// composition `brink-codegen-inkb::content::emit_slot_expr` already
    /// uses for a call's side-effect output, widened to hold an arbitrary
    /// captured run rather than one call's output alone.
    ///
    /// The only producer is the block-capture machinery
    /// (`hir::lower_native::element`, issue #1839): a `@[element(…, block)]`
    /// handler's trailing `content`-typed parameter binds one of these,
    /// built from the source lines the terminator search captured. Each
    /// statement inside keeps its own `Stmt::Content`/`Stmt::EndOfLine`
    /// shape (and its own line-table entry once lowered) — a captured block
    /// is never flattened to a single string, which is the whole point of
    /// `content` staying translation-resident (§3.5b's capture contract).
    Fragment(Vec<Stmt>),
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

/// `|x, y| expr` / `|g: Guest|: bool { … }` / `||` — an anonymous fn value
/// (RULED 2026-07-19, `docs/decision-log.md` "Lambdas ruled: Rust pipes
/// under the `RustScript` north star"; issue #1685).
///
/// This is the "real anonymous-body node" the native lowering's `E129`
/// fence used to wait for. What the ruling fixes and this shape records:
/// params are optionally annotated (mono-HM infers the rest at concrete
/// call sites, so an unannotated param is `None`, never a fabricated type);
/// the return annotation is the ruled colon spelling (`|g|: bool { … }`),
/// not `->`, which stays purely a divert; the body is either a single
/// expression or a braced block whose **last expression is the value**
/// ([`LambdaBody`]).
///
/// What this shape deliberately does *not* carry:
/// - **Captures.** Capture is BY-VALUE always (no `move` keyword, no ref
///   captures in v1), so there is no capture *mode* to record; *which*
///   names a body captures is a resolution fact, not a syntax one, and is
///   left to the layer that resolves names. The one capture rule that is
///   decidable lexically — assignment to a captured binding is a compile
///   error, since a snapshot write is always a lost write — is enforced at
///   lowering (`hir::lower_native::lambda`, `E156`).
/// - **An effect row.** Lambdas are fn-colored always, and rows compose
///   through captures (#872). `Ty::Fn` carries an effect row since #1680
///   step 3, but that row names creation targets by `DefinitionId` — this
///   node now carries one ([`Self::container_id`]), stamped at HIR time
///   (#1727), so the identity half of the gap is closed; joining the
///   effect fixpoint on that identity is #1770's job, not this shape's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaExpr {
    /// The whole `|…| …` expression's own source range — the identity key a
    /// side table addresses this node by, same convention as
    /// [`InfixExpr`]'s `ptr`.
    pub ptr: Provenance,
    /// The pipe-delimited parameter row, in source order. Empty for the
    /// zero-arg form `||`. `is_ref`/`is_divert` are always `false`: the
    /// native grammar accepts neither on a lambda parameter (ref captures
    /// do not exist in v1, and there is no divert-typed lambda param).
    pub params: Vec<Param>,
    /// The `: type` return annotation, if written — the ruled colon
    /// spelling. `None` means "infer", not "void".
    pub return_type: Option<TypeExpr>,
    /// The body: one expression, or a braced block.
    pub body: LambdaBody,
    /// This lambda's lifted-function identity (issue #1727), stamped by
    /// [`super::stamp_container_ids`] from the exact same content-derived
    /// scheme `IdAllocator::alloc_lambda_address` used to derive
    /// independently: `{enclosing scope path}.#lambda-{source start
    /// offset}`.
    ///
    /// RULED 2026-08-02 (`docs/decision-log.md`): HIR **mints** this id;
    /// LIR lowering (`lir::lower::lambda::lower_lambda`) only **consumes**
    /// it. Before this ruling, LIR derived the same identity a second time
    /// from its own live `ctx.scope_path` — which is mutated while
    /// descending into a `Conditional`/`Sequence`/`ChoiceSet` body, so a
    /// lambda nested inside one got a path the HIR-time stamping pass (which
    /// walked only `root_content`/knot/stitch bodies, never expressions)
    /// could not reproduce. Removing the second derivation removes the
    /// id-parity problem outright rather than trying to keep two
    /// derivations in sync.
    ///
    /// `None` for a `LambdaExpr` that never ran
    /// [`super::stamp_container_ids`] — a hand-built test fixture, or a
    /// file-scope `VAR`/`CONST` lambda default folded from `brink-db`'s
    /// decl-only projection (issue #1774), which deliberately skips
    /// stamping to backdate across body-only edits. `lower_lambda` falls
    /// back to re-deriving the id from the live `ctx.scope_path` in that
    /// case (not `ctx.root_id`, unlike every other pre-stamped container id
    /// — see that function's own doc for why a lambda's fallback needs to
    /// differ).
    pub container_id: Option<brink_format::DefinitionId>,
}

/// A [`LambdaExpr`]'s body — the two ruled spellings ("single-expression or
/// braced-block bodies; `return` leaves the lambda; last expression is the
/// value").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LambdaBody {
    /// `|g| g.awake` — the expression *is* the value.
    Expr(Box<Expr>),
    /// `|g|: bool { let a = f(g); a }` — statements, then the block's
    /// trailing expression (the grammar's own blocks-as-values tail,
    /// `brink_syntax_native::ast::StmtBlock::tail`).
    Block {
        /// The `;`-terminated statements, in source order.
        stmts: Vec<BlockStmt>,
        /// The block's trailing unterminated expression — "last expression
        /// is the value". `None` when the body ends in a statement, in
        /// which case the value comes from an explicit `return` (which
        /// leaves the lambda, not the enclosing function) or the lambda
        /// yields nothing.
        tail: Option<Box<Expr>>,
    },
}

impl LambdaBody {
    /// The body's direct **expression** children: the single-expression
    /// body, or a braced body's trailing value expression ("last expression
    /// is the value"). Empty for a braced body that ends in a statement.
    ///
    /// A braced body's *statements* are deliberately not exposed here —
    /// they are statements, not expressions. Consumers that can handle
    /// statements descend into [`LambdaBody::Block`]'s `stmts` explicitly
    /// (see [`crate::hir::visit`]'s `walk_expr`, which walks them with the
    /// same `walk_block_stmt` every code-ground block gets). This helper
    /// exists so the many walkers that genuinely want only the *value*
    /// position — a return-type/tail question — spell it one way instead of
    /// eleven.
    ///
    /// **This is not the right helper for a walker that is looking for a
    /// construct *anywhere* in the body** (issue #1749, #1764). Such a
    /// walker under-reports on every block-bodied lambda if it stops at the
    /// tail; [`Self::all_exprs`] is its helper.
    #[must_use]
    pub fn value_exprs(&self) -> Vec<&Expr> {
        match self {
            Self::Expr(e) => vec![e],
            Self::Block { tail, .. } => tail.as_deref().into_iter().collect(),
        }
    }

    /// Every expression reachable from the body, in source order: a braced
    /// body's statement-embedded expressions first, then its trailing value
    /// expression — or, for a single-expression body, just that expression.
    ///
    /// This is [`Self::value_exprs`]'s counterpart for the walkers that ask
    /// "does this construct occur *anywhere* inside?" rather than "what is
    /// the body's value?". Statements are flattened to the expressions they
    /// contain (recursively through `if`/`while`/`for` bodies, mirroring
    /// [`crate::hir::visit`]'s `walk_block_stmt` seam for seam), so an
    /// expression-only walker can consume them without growing a statement
    /// vocabulary: an expression does not change meaning for such a walker
    /// because it sits in statement rather than tail position.
    ///
    /// Issue #1764 (the audit umbrella) and #1749 (the effect-row instance
    /// that proved the shape unsound) are why this exists: eight walkers had
    /// independently stopped at [`Self::value_exprs`] and so silently skipped
    /// everything a block-bodied lambda does before its last expression.
    ///
    /// The returned expressions are *roots* to recurse from, exactly like
    /// [`Self::value_exprs`]' — nothing here descends into a nested
    /// [`Expr`]'s own children.
    #[must_use]
    pub fn all_exprs(&self) -> Vec<&Expr> {
        match self {
            Self::Expr(e) => vec![e],
            Self::Block { stmts, tail } => {
                let mut out = Vec::new();
                for s in stmts {
                    push_block_stmt_exprs(s, &mut out);
                }
                out.extend(tail.as_deref());
                out
            }
        }
    }
}

/// Append every expression `bs` directly contains, descending through nested
/// statement bodies but not into an [`Expr`]'s own children — the flattening
/// [`LambdaBody::all_exprs`] hands to expression-only walkers. Mirrors
/// [`crate::hir::visit`]'s `walk_block_stmt`/`walk_if_stmt` arm for arm; a new
/// [`BlockStmt`] variant must be added to both.
fn push_block_stmt_exprs<'a>(bs: &'a BlockStmt, out: &mut Vec<&'a Expr>) {
    match bs {
        BlockStmt::TempDecl(t) => out.extend(t.value.as_ref()),
        BlockStmt::Assignment(a) => {
            out.push(&a.target);
            out.push(&a.value);
        }
        BlockStmt::Return(r) => {
            out.extend(r.value.as_ref());
            out.extend(r.onwards_args.iter());
        }
        BlockStmt::If(i) => push_if_stmt_exprs(i, out),
        BlockStmt::While(w) => {
            out.push(&w.condition);
            for s in &w.body {
                push_block_stmt_exprs(s, out);
            }
        }
        BlockStmt::For(f) => {
            out.push(&f.iterable);
            for s in &f.body {
                push_block_stmt_exprs(s, out);
            }
        }
        BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
        BlockStmt::ExprStmt(e) => out.push(e),
        BlockStmt::Await(a) => out.extend(a.condition.as_ref()),
    }
}

/// [`push_block_stmt_exprs`]' `if`/`else if`/`else` arm, split out so the
/// `else if` chain recurses the same way `visit::walk_if_stmt` does.
fn push_if_stmt_exprs<'a>(i: &'a IfStmt, out: &mut Vec<&'a Expr>) {
    out.push(&i.condition);
    for s in &i.body {
        push_block_stmt_exprs(s, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => push_if_stmt_exprs(inner, out),
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                push_block_stmt_exprs(s, out);
            }
        }
        None => {}
    }
}

/// Every expression directly reachable from a block-capture's statements
/// (`Expr::Fragment`, issue #1839) — the top-level-[`Stmt`] analogue of
/// [`push_block_stmt_exprs`]'s `BlockStmt` walk, made `pub` (unlike that
/// function) because callers outside this crate need it: every
/// "does this construct occur *anywhere* inside an expression tree" walker
/// in `brink-analyzer` that already flattens a lambda body via
/// [`LambdaBody::all_exprs`] needs the identical flattening for a captured
/// content block — the same audit-driven reason issue #1764 fixed eight
/// walkers that had independently stopped short. Descends through nested
/// statement bodies (`LabeledBlock`, `Conditional`/`Sequence` branches,
/// `ChoiceSet`) but never into an `Expr`'s own children — callers recurse
/// from these roots themselves, exactly like [`LambdaBody::all_exprs`]'s
/// own contract.
#[must_use]
pub fn fragment_stmt_exprs(stmts: &[Stmt]) -> Vec<&Expr> {
    let mut out = Vec::new();
    for s in stmts {
        push_stmt_exprs(s, &mut out);
    }
    out
}

/// [`fragment_stmt_exprs`]'s per-statement worker — mirrors
/// [`crate::hir::visit`]'s `walk_stmt` arm for arm; a new [`Stmt`] variant
/// must be added to both (and to [`push_block_stmt_exprs`] if it is ever
/// added to the closed `BlockStmt` set instead).
fn push_stmt_exprs<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Expr>) {
    match stmt {
        Stmt::Content(c) => {
            for part in &c.parts {
                if let ContentPart::Interpolation(e) = part {
                    out.push(e);
                }
            }
        }
        Stmt::Divert(d) => out.extend(d.target.args.iter()),
        Stmt::TunnelCall(t) => {
            for target in &t.targets {
                out.extend(target.args.iter());
            }
        }
        Stmt::ThreadStart(t) => out.extend(t.target.args.iter()),
        Stmt::TempDecl(t) => out.extend(t.value.as_ref()),
        Stmt::Assignment(a) => {
            out.push(&a.target);
            out.push(&a.value);
        }
        Stmt::Return(r) => {
            out.extend(r.value.as_ref());
            out.extend(r.onwards_args.iter());
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                out.extend(choice.condition.as_ref());
                for c in [
                    &choice.start_content,
                    &choice.bracket_content,
                    &choice.inner_content,
                ]
                .into_iter()
                .flatten()
                {
                    for part in &c.parts {
                        if let ContentPart::Interpolation(e) = part {
                            out.push(e);
                        }
                    }
                }
                for s in &choice.body.stmts {
                    push_stmt_exprs(s, out);
                }
            }
            for s in &cs.continuation.stmts {
                push_stmt_exprs(s, out);
            }
        }
        Stmt::LabeledBlock(b) => {
            for s in &b.stmts {
                push_stmt_exprs(s, out);
            }
        }
        Stmt::Conditional(cond) => {
            if let CondKind::Switch(e) = &cond.kind {
                out.push(e);
            }
            for branch in &cond.branches {
                out.extend(branch.condition.as_ref());
                for s in &branch.body.stmts {
                    push_stmt_exprs(s, out);
                }
            }
        }
        Stmt::Sequence(seq) => {
            for branch in &seq.branches {
                for s in &branch.body.stmts {
                    push_stmt_exprs(s, out);
                }
            }
        }
        Stmt::ExprStmt(e) => out.push(e),
        Stmt::EndOfLine => {}
        Stmt::LogicBlock(lb) => {
            for s in &lb.stmts {
                push_block_stmt_exprs(s, out);
            }
        }
        Stmt::Await(a) => out.extend(a.condition.as_ref()),
    }
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
        Expr::Lambda(l) => {
            let params = l
                .params
                .iter()
                .map(|p| p.name.text.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            match &l.body {
                LambdaBody::Expr(e) => format!("|{params}| {}", display_expr(e)),
                LambdaBody::Block { .. } => format!("|{params}| {{ ... }}"),
            }
        }
        Expr::Range(r) => {
            let op = if r.inclusive { "..=" } else { ".." };
            format!("{}{op}{}", display_expr(&r.start), display_expr(&r.end))
        }
        // Internal-only (issue #1839) — never constructed from surface
        // syntax, so there is no author-facing slot name to reconstruct.
        Expr::Fragment(_) => "{...}".to_string(),
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

#[cfg(test)]
mod lambda_body_tests {
    use super::*;
    use crate::FileId;

    /// The lambda in `src`'s first `var` initializer. Lambdas exist only on
    /// the native surface, so this goes through `lower_native`.
    fn lambda_body_of(src: &str) -> LambdaBody {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let tree = parsed.tree();
        let (hir, _manifest, _diag) = crate::hir::lower_native::lower(FileId(0), &tree);
        let Expr::Lambda(l) = &hir.variables[0].value else {
            unreachable!("fixture's initializer is a lambda: {:?}", hir.variables[0]);
        };
        l.body.clone()
    }

    #[test]
    fn a_single_expression_body_yields_the_same_one_expression_either_way() {
        let body = lambda_body_of("var f = |x| x + 1\n");
        assert_eq!(body.value_exprs().len(), 1);
        assert_eq!(body.all_exprs().len(), 1);
    }

    /// The #1749/#1764 shape: `value_exprs` sees only the tail; `all_exprs`
    /// sees the statements too, statements first and the tail last.
    #[test]
    fn a_braced_body_hides_its_statements_from_value_exprs_but_not_all_exprs() {
        let body = lambda_body_of("var f = ||: int {\n  let a = 1;\n  let b = 2;\n  3\n};\n");
        assert_eq!(body.value_exprs().len(), 1, "just the `3` tail");
        let all = body.all_exprs();
        assert_eq!(all.len(), 3, "{all:?}");
        assert_eq!(all[2], &Expr::Int(3), "the tail comes last: {all:?}");
    }

    /// A statement-terminated body has no value expression at all, so
    /// `value_exprs` is empty — the worst case for a walker that stops there.
    #[test]
    fn a_statement_terminated_body_yields_nothing_from_value_exprs() {
        let body = lambda_body_of("var f = ||: int {\n  return 7;\n};\n");
        assert!(body.value_exprs().is_empty());
        assert_eq!(body.all_exprs().len(), 1, "the `return`'s operand");
    }

    /// Nested statement bodies are flattened too — `walk_block_stmt`'s own
    /// recursion, which is what makes this safe to hand an expression walker.
    #[test]
    fn nested_statement_bodies_are_flattened() {
        let body = lambda_body_of(
            "var f = ||: int {\n  if 1 == 1 {\n    let a = 2;\n  } else {\n    let b = 3;\n  }\n  4\n};\n",
        );
        let all = body.all_exprs();
        // the `if` condition, the two branch initializers, and the tail.
        assert_eq!(all.len(), 4, "{all:?}");
    }
}

#[cfg(test)]
mod conventions_projection_tests {
    use super::*;

    fn name(text: &str) -> Name {
        Name {
            text: text.to_string(),
            range: TextRange::default(),
        }
    }

    fn decl(name_text: &str, order: i64, block: bool, attach: Option<&str>) -> ClaimHandlerDecl {
        ClaimHandlerDecl {
            name: name(name_text),
            annotation: TextRange::default(),
            params: Vec::new(),
            pattern: format!("^{name_text}$"),
            block,
            order,
            attach: attach.map(str::to_string),
        }
    }

    fn no_structs() -> BTreeMap<String, Vec<ConventionAttachField>> {
        BTreeMap::new()
    }

    fn field(name: &str, ty: SchemaTypeShape) -> ConventionAttachField {
        ConventionAttachField {
            name: name.to_string(),
            ty,
        }
    }

    #[test]
    fn from_decls_preserves_input_order() {
        // `collect` hands this function an already-`order`-sorted slice
        // (issue #2164's own contract) — the builder trusts that, so this
        // proves it does not re-sort behind the caller's back.
        let decls = vec![
            decl("exterior", 20, false, None),
            decl("interior", 10, false, None),
        ];
        let projection = ConventionsProjection::from_decls(&decls, &no_structs());
        let names: Vec<&str> = projection
            .entries
            .iter()
            .map(|e| e.name.text.as_str())
            .collect();
        assert_eq!(names, vec!["exterior", "interior"]);
    }

    #[test]
    fn block_flag_becomes_wrap_mode_and_its_absence_becomes_attach_mode() {
        let decls = vec![
            decl("cue", 10, true, None),
            decl("interior", 20, false, None),
        ];
        let projection = ConventionsProjection::from_decls(&decls, &no_structs());
        assert_eq!(projection.entries[0].mode, ConventionMode::Wrap);
        assert_eq!(projection.entries[1].mode, ConventionMode::Attach);
    }

    /// Issue #2111 finding 1: the schema is the RESOLVED FIELD LIST, not
    /// merely the struct's bare name.
    #[test]
    fn attach_schema_resolves_to_the_declared_fields_and_types() {
        let decls = vec![decl("cue", 10, false, Some("Cue"))];
        let mut structs = no_structs();
        structs.insert(
            "Cue".to_string(),
            vec![
                field("speaker", SchemaTypeShape::Named("string".to_string())),
                field("voiceover", SchemaTypeShape::Named("bool".to_string())),
                field("offscreen", SchemaTypeShape::Named("bool".to_string())),
            ],
        );
        let projection = ConventionsProjection::from_decls(&decls, &structs);
        assert_eq!(
            projection.entries[0].attach,
            Some(ConventionAttachSchema::Resolved {
                name: "Cue".to_string(),
                fields: vec![
                    field("speaker", SchemaTypeShape::Named("string".to_string())),
                    field("voiceover", SchemaTypeShape::Named("bool".to_string())),
                    field("offscreen", SchemaTypeShape::Named("bool".to_string())),
                ],
            })
        );
    }

    /// Mutation-style (rule 20a): reversing the field order must be visible
    /// — a projection that silently re-sorted fields would pass a looser
    /// assertion (e.g. a set comparison) but must fail this one.
    #[test]
    fn attach_schema_field_order_is_preserved_not_resorted() {
        let decls = vec![decl("cue", 10, false, Some("Cue"))];
        let mut structs = no_structs();
        structs.insert(
            "Cue".to_string(),
            vec![
                field("b_field", SchemaTypeShape::Named("bool".to_string())),
                field("a_field", SchemaTypeShape::Named("string".to_string())),
            ],
        );
        let projection = ConventionsProjection::from_decls(&decls, &structs);
        let Some(ConventionAttachSchema::Resolved { fields, .. }) = &projection.entries[0].attach
        else {
            unreachable!(
                "expected a resolved schema: {:?}",
                projection.entries[0].attach
            );
        };
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["b_field", "a_field"],
            "field order must not be re-sorted"
        );
    }

    /// Issue #2111 finding 1: an `attach` name that resolves to no known
    /// struct is flagged (`Unresolved`), never silently dropped to `None`
    /// (house rule: flag silent data drops) and never fabricated into an
    /// empty-fields `Resolved`.
    #[test]
    fn attach_schema_unresolved_struct_name_is_flagged_not_dropped() {
        let decls = vec![decl("cue", 10, false, Some("Ghost"))];
        let projection = ConventionsProjection::from_decls(&decls, &no_structs());
        assert_eq!(
            projection.entries[0].attach,
            Some(ConventionAttachSchema::Unresolved("Ghost".to_string()))
        );
    }

    #[test]
    fn no_attach_clause_projects_to_none() {
        let decls = vec![decl("interior", 10, false, None)];
        let projection = ConventionsProjection::from_decls(&decls, &no_structs());
        assert_eq!(projection.entries[0].attach, None);
    }

    #[test]
    fn every_entry_carries_the_call_disposition() {
        // There is only one `ElementDisposition` variant today — this pins
        // that the projection actually reads and carries it, rather than
        // e.g. hardcoding a literal in a way that would silently stay
        // correct even if this field were dropped from the struct.
        let decls = vec![decl("interior", 10, false, None)];
        let projection = ConventionsProjection::from_decls(&decls, &no_structs());
        assert_eq!(projection.entries[0].disposition, ElementDisposition::Call);
    }

    #[test]
    fn an_empty_decl_set_projects_to_an_empty_projection() {
        let projection = ConventionsProjection::from_decls(&[], &no_structs());
        assert!(projection.entries.is_empty());
        assert_eq!(projection, ConventionsProjection::default());
    }

    #[test]
    fn order_and_pattern_are_carried_through_verbatim() {
        let decls = vec![decl("interior", 42, false, None)];
        let projection = ConventionsProjection::from_decls(&decls, &no_structs());
        assert_eq!(projection.entries[0].order, 42);
        assert_eq!(projection.entries[0].pattern, "^interior$");
    }

    // ─── `to_wire` (issue #2111 finding 2) ──────────────────────────────

    /// The wire conversion is lossless for a resolved schema — mutation-style
    /// (rule 20a): forcing a wrong field type on either side would fail this
    /// exact assertion, proving it isn't vacuously true.
    #[test]
    fn to_wire_carries_the_resolved_schema_losslessly() {
        let decls = vec![decl("cue", 5, true, Some("Cue"))];
        let mut structs = no_structs();
        structs.insert(
            "Cue".to_string(),
            vec![field(
                "speaker",
                SchemaTypeShape::Named("string".to_string()),
            )],
        );
        let projection = ConventionsProjection::from_decls(&decls, &structs);
        let wire = projection.to_wire();
        assert_eq!(wire.entries.len(), 1);
        assert_eq!(wire.entries[0].name, "cue");
        assert_eq!(wire.entries[0].mode, brink_format::ConventionModeDef::Wrap);
        assert_eq!(
            wire.entries[0].attach,
            Some(brink_format::ConventionAttachDef::Resolved {
                name: "Cue".to_string(),
                fields: vec![brink_format::ConventionAttachFieldDef {
                    name: "speaker".to_string(),
                    ty: brink_format::SchemaTypeDef::Named("string".to_string()),
                }],
            })
        );
    }

    #[test]
    fn to_wire_carries_unresolved_attach_as_unresolved_not_none() {
        let decls = vec![decl("cue", 5, false, Some("Ghost"))];
        let projection = ConventionsProjection::from_decls(&decls, &no_structs());
        let wire = projection.to_wire();
        assert_eq!(
            wire.entries[0].attach,
            Some(brink_format::ConventionAttachDef::Unresolved(
                "Ghost".to_string()
            ))
        );
    }

    #[test]
    fn to_wire_round_trips_through_the_inkb_codec() {
        let decls = vec![
            decl("cue", 5, true, Some("Cue")),
            decl("interior", 10, false, None),
        ];
        let mut structs = no_structs();
        structs.insert(
            "Cue".to_string(),
            vec![field(
                "speaker",
                SchemaTypeShape::Named("string".to_string()),
            )],
        );
        let wire = ConventionsProjection::from_decls(&decls, &structs).to_wire();
        let mut buf = Vec::new();
        brink_format::write_conventions_projection(&wire, &mut buf);
        let mut offset = 0;
        let decoded = brink_format::read_conventions_projection(&buf, &mut offset)
            .expect("decode the wire form this crate just encoded");
        assert_eq!(decoded, wire);
    }
}
