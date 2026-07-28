//! Diagnostic codes, severities, and the [`Diagnostic`] record.
//!
//! Split out of [`super::types`] (issue #652). The stable [`DiagnosticCode`]
//! catalogue and its lookup tables are touched by every diagnostic-adding
//! change, while the HIR node definitions next door are touched by every
//! language-feature change; keeping the two in separate files keeps those
//! streams of work from colliding.
//!
//! Everything here is re-exported through `hir::*`, so consumers keep
//! importing these names from `brink_ir::hir` exactly as before.

use rowan::TextRange;

use super::types::FileId;

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

/// How seriously a diagnostic should be treated by a consumer (CLI renderer,
/// LSP client, editor diagnostics panel).
///
/// No `DiagnosticCode`'s *default* severity ([`DiagnosticCode::severity`])
/// is `Info` or `Hint` today — the two advisory tiers exist so a project's
/// `[lints]` table (`brink-project-config`'s `LintLevel::Info`/`LintLevel::Hint`,
/// resolved through `brink_analyzer::effective_severity`) can opt a
/// `Warning`-default code down to one when a squiggle is too loud (issue
/// #1162). Moving any *existing* code's default into one of these tiers is a
/// separate decision, deliberately not made by the issue that introduced the
/// tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Blocks compilation / is surfaced as a hard failure.
    Error,
    /// Non-fatal; the default tier for advisory diagnostics until a
    /// `[lints]` override says otherwise.
    Warning,
    /// Advisory, LSP `DiagnosticSeverity::INFORMATION` — worth telling the
    /// author about, but not something they need to act on.
    Info,
    /// Advisory and quiet, LSP `DiagnosticSeverity::HINT` — the tier IDEs use
    /// for things like unused-symbol dimming, where even an info-level
    /// squiggle is too loud.
    Hint,
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
    /// A struct construction literal used as a `VAR`/`CONST` declaration
    /// default doesn't match its declared shape: it omits a declared field,
    /// or supplies one the shape doesn't declare.
    ///
    /// A *well-formed* construction literal is a legal declaration default
    /// (issue #1530): `eval_const_struct_literal` folds it into
    /// `lir::ConstValue::Record`, which is what makes a struct-typed durable
    /// global — and therefore the T1e projection-receiver path
    /// (`docs/t1e-spec.md` §2, which requires a durable root) — spellable at
    /// all. Before #1530 this code was the blanket refusal of *every* struct
    /// literal in that position, because `ConstValue` had no record-carrying
    /// variant.
    ///
    /// Mid-story `p = Point#{…}` construction with a mismatched shape is a
    /// runtime construction fault (`RecordNew` against an invalid shape id,
    /// value-model-spec §11c's gradual path); a declaration default is baked
    /// into `StoryData` with no runtime construction step to fault at, so
    /// this is the compile-time equivalent — a real, non-suppressible error,
    /// never a half-built record. Under `types = strict` `brink-analyzer`'s
    /// `structs::check` reports the more precise [`Self::E069`]/
    /// [`Self::E070`] for the same literal; this backstop is
    /// policy-independent.
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
    /// `is_const_foldable_decl_default`'s top-level twin (`E083`). (Since
    /// #1530 a struct literal at this position folds for real, so a
    /// never-foldable *field* of a nested construction literal reaches this
    /// arm exactly as an array element or map value does; before #1530 the
    /// whole literal was unconditionally `E075` regardless of field
    /// content.)
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
    /// A bare-form `IMPORT { name } FROM mod` / native `use mod::name;`
    /// whose trailing segment `name` names neither a definition `mod`
    /// publicly exports **nor a declared submodule of `mod`** (dual-reading,
    /// issue #1592 — a trailing segment that resolves to a module licenses
    /// it instead, matching Rust's `use`; §13.2). Only enforced against
    /// *declared* modules — an import naming an unknown/undeclared module is
    /// not itself flagged by this code, since that module's export/submodule
    /// set isn't visible to the check (modules-spec §2/§7).
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
    /// An `@[…]` annotation line naming something outside the channel's
    /// closed name set: `effects` on the ink surface, `effects` or the
    /// file-level `was` on the native `.brink` surface. Tag-channel
    /// directive names do not alias into it.
    E111,
    /// An `@[…]` annotation line outside a recognized placement — ink's
    /// leading run at the top of a knot/stitch body, or native's Rust-shaped
    /// position directly above a `flow`/`fn` declaration (issue #1563; the
    /// file-level `@[was]` record for native modules). Never a silent drop,
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
    /// `== none` / `== some(x)`, or the `as`-binding (B1b, issue #1475,
    /// `brink-analyzer::option_conditions::check_binding_condition`); a
    /// bound condition never fires this check. Strict-mode-only,
    /// best-effort static (the "Unknown never disagrees"
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

    // ── NS-A4: the ordering doctrine (docs/stdlib-spec.md §4b, issue
    // #1110) ─────────────────────────────────────────────────────────────
    /// A `sort_by`/`sorted_by` comparator provably breaks the pure·silent
    /// contract (§4b: "the comparator falls under the trio's pure·silent
    /// rule plus the consistent-total-order LAW"). Exceedance-only, the
    /// E114 posture: flagged when the comparator is a statically-named
    /// `#fn(target)` whose inferred row shows a global read/write, an
    /// external call, a content emission, or a tag touch — an opaque or
    /// unresolvable comparator is not *proven* in violation and passes
    /// (the gradual posture; the VM's isolation and
    /// `ComparatorEscaped` fault are the runtime residual).
    E119,
    /// NS-A7 `Weighted[T]` construction refusal (`docs/stdlib-spec.md` §8,
    /// issue #1113): the compile-classifiable half of the E078-style
    /// evidence-by-construction split. Fired by the `weighted(…)` lowering
    /// for a statically-malformed table — an empty pair row, an odd
    /// (dangling-weight) argument count, or a **literal** weight that is
    /// not a positive int (zero, negative, float/string/bool). Computed
    /// weights are not classifiable here; they carry the construction
    /// *fault* residual instead (`RuntimeError::WeightedBadWeight`), so a
    /// table that exists is always rollable.
    E120,

    // ── B0.3 HIR admission validator (docs/hir-admission-contract.md §4.2) ──
    //
    // Reserved range for the loud, non-suppressible `validate_admission`
    // pass wired at the AST→HIR seam (issue #1172, docs/b0-sequencing.md
    // §B0.3). Each check is a hard error — a malformed `(HirFile,
    // SymbolManifest)` triple is a frontend bug, not a story-author mistake,
    // so these never carry the warning-severity carve-out other codes do.
    /// Contract §4.2 check 1a (manifest ⇄ HIR agreement): an
    /// `UnresolvedRef.range` in the manifest has no matching
    /// referencing-expression range anywhere in the file's HIR body — the
    /// range-equality resolution join (Q2(a)) would silently fail to find
    /// this reference at all.
    E121,
    /// Contract §4.2 check 1b (manifest ⇄ HIR agreement): a manifest-declared
    /// symbol has no corresponding HIR declaration node of the same name —
    /// the manifest and the HIR body have drifted apart.
    E122,
    /// Contract §4.2 check 1c (manifest ⇄ HIR agreement, F-I#4): a `Knot`'s
    /// `is_function` flag disagrees with whether its declared symbol carries
    /// the `"function"` detail sentinel.
    E123,
    /// Contract §4.2 check 2a (range well-formedness): a HIR node's source
    /// range is empty or extends past the end of the source file — ranges
    /// are resolution join keys and IDE geometry, so a garbage range would
    /// otherwise corrupt resolution silently instead of erroring loudly.
    /// Exempts the `Option<Provenance>`-carrying synthesized nodes
    /// (`Content.ptr`/`Divert.ptr`/`Return.ptr`) when `None` (B0.1 finding
    /// F-B2) — this fires only on a range that is present but malformed.
    E124,
    /// Contract §4.2 check 2b (join-key uniqueness, Q2(a)): two distinct
    /// `UnresolvedRef` entries in the manifest share an identical source
    /// range — the range-equality join can no longer distinguish them.
    E125,
    /// Contract §4.2 check 3 (name-convention conformance, F-I#3): a
    /// declared symbol's qualified name does not match the dot-qualification
    /// shape its `SymbolKind` requires (bare for knots/globals, `knot.stitch`
    /// for stitches, `List.item` for list items, `knot[.stitch].label` for
    /// labels).
    E126,
    /// Contract §4.2 check 4 (control-flow classification, F-I#7): a
    /// terminal statement (`Divert`/`Return`) is not the last statement in
    /// an inline conditional or sequence branch.
    E127,
    /// Contract §4.2 check 5 (provenance-kind ⇄ `SymbolKind` consistency,
    /// F-I#5, the #626 floating-stitch trap): a `Knot`/`Stitch` HIR node's
    /// provenance class disagrees with the `SymbolKind` bucket its declared
    /// symbol was indexed under in the manifest.
    E128,

    // ── B0.6 native frontend (docs/b0-sequencing.md §B0.6) ──
    //
    // The native `.brink` declaration lowering (`hir::lower_native`) is
    // deliberately partial — bodies are B0.7/B0.8, and a handful of
    // declaration-layer constructs (nested modules, `fn` nested below top
    // level, the `@[…]` annotation channel, lambda expressions in value
    // position) have no HIR representation yet. Per the contract's §4.4
    // additive-open/closed-to-silent-extension posture, every such
    // construct is a loud diagnostic, never a silent drop.
    /// A native construct parses cleanly but has no HIR lowering yet in
    /// this slice (a nested `module { … }` block, a `fn` declared below top
    /// level, an `@[…]` annotation line, a lambda expression in value
    /// position, or any other CST shape `hir::lower_native` does not yet
    /// recognize). The construct is skipped — not silently: this diagnostic
    /// names exactly what was skipped and why.
    ///
    /// Also raised by `brink_analyzer::modules::check` (issue #1592,
    /// #1686 review) for the whole-project-only instance of the same gap:
    /// a bare `use`/`IMPORT` item's trailing segment that is both aliased
    /// and — only knowable once whole-project module data resolves the
    /// dual-reading — a declared **submodule**. Aliasing an entire
    /// imported module's export set has no `Import`/`ImportItem`
    /// representation, same as the single-segment `use a as m;` form
    /// `lower_native::import::lower_use_decl` already rejects with this
    /// code; this later firing exists only because that verdict isn't
    /// decidable until the analyzer's whole-project pass.
    E129,
    /// A native `flow` is declared more than two levels deep (a `flow`
    /// nested inside another nested `flow`'s body) — the contract's Q4(b)
    /// fence (`docs/hir-admission-contract.md` §5 Q4): exactly two
    /// container levels for v1, addressing model written to generalize.
    /// Depth-3+ nesting parses and is rejected here, never silently
    /// flattened into a 2-level shape.
    E130,
    /// `<-` (splice) used outside a choice point (issue #1263, ruled
    /// #1260 on #1256): charter §11 narrows threads to scoped splices
    /// inside `{? … }` choice points, so this has no structural meaning —
    /// but `<-` can also be literal dialogue punctuation, so this is
    /// **warning severity, never blocking** (see `DiagnosticCode::severity`
    /// below). The construct still parses as ordinary text; nothing is
    /// dropped or rejected. `brink-syntax-native`'s
    /// `parser::choice::splice_outside_choice_point` raises the
    /// `ParseSeverity::Warning` diagnostic this code carries once it
    /// reaches `brink-db`'s `lower_native_file`.
    E131,
    /// A native file-level `@[was(…)]` rename record (issue #1286) carries no
    /// quoted old module path — a missing argument, or one that is not a
    /// string literal. Native module paths are `::`-separated and travel as a
    /// string (`::` is not annotation-argument grammar), so the migration
    /// target must be spelled `@[was("story::old::path")]`. **Warning
    /// severity, never blocking** (see `DiagnosticCode::severity`): the
    /// malformed directive is skipped — no alias is produced — but the file
    /// still compiles. `brink-ir::hir::lower_native::module::lower_file_module`
    /// raises it rather than silently dropping the authored record.
    E132,

    // ── B0.9 native accept-list admission gate (docs/hir-admission-contract.md
    // §4.4/§5 Q6, docs/b0-sequencing.md §B0.9, issue #1179) ──
    //
    // The inverse of the ink `dialect_gate` reject-list: `brink_analyzer::
    // validate_native_accept_list` enumerates the HIR shapes a well-formed
    // native lowering is allowed to produce and refuses everything else,
    // loudly, at the same non-suppressible seam B0.3's `validate_admission`
    // runs at. Native-only — never raised against ink-produced HIR.
    /// A native file's `root_content` carries something other than the one
    /// documented shape a native lowering may leave there: empty, or the
    /// single synthesized `flow main()` entry divert (maintainer-ruled
    /// 2026-07-21, `docs/decision-log.md` "Native story-entry convention").
    /// Anything else — real weave content, more than one statement, a
    /// source-backed divert — is ink-only baggage: ink's pre-first-knot root
    /// weave has no native equivalent.
    E133,
    /// A native file's HIR carries an `IncludeSite` — native has no textual
    /// `INCLUDE` graph (charter §13.2, "the tree is the compilation
    /// universe"); `hir::lower_native::lower` always leaves `includes`
    /// empty, so any entry here is ink-only baggage that reached native HIR
    /// some other way.
    E134,
    /// A `ThreadStart` (`<- target`) appears somewhere other than the two
    /// legal native splice positions B0.7's choice-point lowering produces:
    /// immediately preceding the `ChoiceSet` it preambles, or as the
    /// trailing statement(s) of a `Choice`'s own body
    /// (`hir::lower_native::choice::lower_choice_point`). An "ambient"
    /// thread start anywhere else has no structural meaning on the native
    /// surface (charter §11 narrows threads to scoped splices inside `{?
    /// … }` choice points).
    E135,
    /// A native `ChoiceSet` carries a `depth`/`context` other than the
    /// B0.7-documented neutral values (`depth = 0`, `context = Inline`,
    /// `docs/hir-admission-contract.md` §3 D4) every native choice set
    /// stamps uniformly — native has no weave fold to report a real value
    /// from, so any other value means a weave-fold concept leaked in from
    /// somewhere it shouldn't have.
    E136,
    /// The B0.9 native strict-only enforcement point (docs/b0-sequencing.md
    /// §B0.9, decision-log 2026-07-19 "Typing posture ruled"): a native
    /// `.brink` file was compiled with an explicit `types = gradual` knob.
    /// Gradual typing does not exist on the native surface — `types` is not
    /// a project knob there the way it is for the transitional brink
    /// dialect, so an explicit `gradual` setting reaching a `.brink` compile
    /// is refused, loudly, rather than silently accepted.
    E137,

    // ── B5: the construction initializer (issue #1464, #1103 RULED
    //    2026-07-23, `docs/stdlib-spec.md` §9.6) ────────────────────────
    /// A map literal supplies the same key twice (`Map { k: 1, k: 2 }`).
    /// The E076-lineage cascade ruling (A) of #1103: a duplicate key is a
    /// **compile error**, consistent with a struct literal's duplicate
    /// field ([`Self::E084`]) — last-wins would silently swallow the typo.
    /// Only *statically comparable* literal keys can collide here
    /// (int/string/bool, the `E106` key domain); a dynamic key is left to
    /// the runtime, exactly as the key-domain check leaves it.
    E138,
    /// A construction literal's entries are not in the form its target type
    /// constructs from — `Map { a }` (element form for a key/value target)
    /// or `Flags { A: 1 }` (key/value form for an element target). The
    /// brace *tokens* are one fixed grammar; the entry form each type
    /// consumes is the `construct` protocol's business
    /// ([`crate::hir::construct::ConstructTarget::form`]), so a mismatch is
    /// caught at dispatch rather than by the parser.
    E139,

    // ── B3a: UFCS resolution (issue #1482, D1–D5 RULED 2026-07-26,
    //    `docs/decision-log.md` "UFCS resolution pass designed") ────────
    /// **D1**: `recv.name(args)`'s receiver type declares a field `name`,
    /// but that field is not function-typed. Field access *wins outright* —
    /// a matching-but-non-callable field is a hard error, never a silent
    /// fall-through to a free function of the same name, so that a call's
    /// meaning can never hinge on a field's type.
    E140,
    /// `recv.name(args)` resolved as neither: the receiver's type declares
    /// no field `name`, **and** no free function `name` is visible in
    /// ordinary lexical scope (D4 — the candidate set is lexical scope only;
    /// there are no method sets or inherent impls). One diagnostic naming
    /// both attempts, so the author sees the whole search that failed.
    E141,
    /// **D3**: `recv.name(args)`'s receiver type is not known at the
    /// resolution point, so field-access-wins is unanswerable. An annotation
    /// is demanded rather than the resolution being deferred (E107-family
    /// posture). Explicitly a *for now* trade — smarter inference ordering
    /// is planned and additive when it lands.
    E142,
    /// **D5**: `recv.name(args)` resolved to a free function whose first
    /// parameter is declared `ref`, so the receiver is auto-ref'd
    /// (`party.leader.heal(5)` → `heal(ref party.leader, 5)`, issue
    /// #1462) — but *this* receiver cannot be written through: a `CONST`, or
    /// a projection whose root is a frame-local (T1e's durable-root rule,
    /// `docs/t1e-spec.md` §2), or — once the grammar can spell them — an
    /// rvalue such as `[1,2].push(3)`. Refused rather than silently
    /// desugared by value, which would drop the mutation. A non-`ref` first
    /// parameter never reaches this code: the by-value desugar puts no
    /// lvalue requirement on its receiver.
    E143,
    /// A UFCS call site that `brink-analyzer::ufcs` **resolved** cleanly has
    /// reached LIR lowering, which does not consume the verdict side table
    /// yet. Refused loudly rather than lowered: the callee path's resolution
    /// record names the *receiver* (the D2 side table is what names the real
    /// target), so lowering it as an ordinary call would emit a call against
    /// a local's id and silently produce a wrong program. Same "parses/
    /// resolves but has no lowering yet" posture as [`Self::E129`], one
    /// layer further down.
    E144,

    // ── B1b: the `as` binding (issue #1475, RULED `docs/decision-log.md`
    //    2026-07-26 "The `as` binding") ─────────────────────────────────
    /// The v1 whole-condition restriction: an `as` binding was written over
    /// a `&&`/`||` composition (`if a && find(x) as s { … }`). The ruling
    /// fixes the binding as the **entire** condition for v1 — let-chains
    /// can land later, additively — so a boolean composition under the
    /// binding is refused rather than silently binding the composite (which
    /// is never an `Option[T]` anyway). The mirror spelling, an operator
    /// *after* the binding (`if find(x) as s && …`), is caught one layer
    /// earlier as a parse error (`brink-syntax-native::parser::binding`).
    E145,
    /// An `as` binding in a **choice guard** (`* {if EXPR as name} [text]`).
    /// Ruled admissible with capture-at-presentation, by-value COW
    /// semantics (`docs/decision-log.md` 2026-07-26, "Choice-guard `as`
    /// un-deferred"), but **not yet implemented**: the captured value has
    /// to ride the pending choice across saves, which needs the `.inkb` v6
    /// Choice record. Diagnosed by name so the construct never half-works
    /// — this is "not yet", not "not a thing".
    E146,
    /// An `as` binding whose condition is a statically-known **non-Option**
    /// type (`if 5 as n { … }`). The binding unwraps `Option[T]` to `T`;
    /// there is nothing to unwrap here. Strict-mode-only and
    /// classification-gated, exactly like its F27 twin [`Self::E116`]: an
    /// `Unknown`/`Conflicted` condition stays unjudged rather than
    /// guessing.
    E147,
    /// A write to an `as` binding — `if find(s) as i { i = 0; }`, `pop(i)`,
    /// `i[0] = x`, `bump(ref i)`, … The binding is **immutable** by ruling
    /// (`docs/decision-log.md` 2026-07-26): it names the unwrapped payload
    /// the condition proved present, and rebinding it would make the
    /// narrowing guarantee a lie. Raised at the LIR write-target choke
    /// point (`lir::lower::stmts::lower_assign_target`) for assignment,
    /// compound assignment, an indexed/field assignment root, and an
    /// in-place mutator; and separately at the `ref`-argument choke points
    /// (`lir::lower::expr::lower_ref_path_call_arg`,
    /// `lower_ref_projection_arg`), since passing the binding by `ref`
    /// hands the callee a raw pointer to the slot without ever routing
    /// through ordinary assignment lowering. Every write shape is covered
    /// by construction across the two.
    E148,
    /// A `remove(a, i)` call whose first argument is statically known to be
    /// an array (issue #1532, the #1501 review's migration-tail finding):
    /// `remove` went map-only in #1484 (identity-based, idempotent-total
    /// key removal; `docs/t1b-surface-spec.md` §5), and the array-index leg
    /// it used to also serve moved to its own verb, `remove_at(a, i)`. With
    /// no compatibility shim, an un-migrated `remove(array, i)` call site
    /// still parses and type-checks as a call to the (now map-only)
    /// builtin — `infer::body`'s `remove` arm already has `Ty::Array` in
    /// hand at the call site — and previously reached codegen clean, only
    /// faulting at runtime against `MapRemove`'s domain check. Strict-mode-
    /// only (`infer::body::InferPass::array_remove_calls`,
    /// `strict::check_array_remove_calls`), matching every other TM-3
    /// typed-mismatch check in this range — the brink dialect's own
    /// implicit default is `types = strict` (issue #1127), so this fires
    /// for the common case; under `types = gradual` the `MapRemove`
    /// runtime fault stays the backstop, same posture as the rest of TM-3.
    E149,
    /// A def (function or value-returning flow/stitch) declares a non-`void`
    /// return type but its body may fall through without ever executing a
    /// value-carrying `return <expr>` (issue #1551, `docs/decision-log.md`
    /// 2026-07-22 implicit-end ruling item 3: "a flow that declares a
    /// return type must produce a value... falling through without a value
    /// is a checker error", ratified for a return-typed flow/stitch and now
    /// extended to the identical `fn` shape). Strict-mode-only
    /// (`strict::check_def`'s escape check, extended by #1551 to run for
    /// any def carrying a declared return type, not just `is_function`);
    /// deliberately distinct from [`Self::E065`] Unknown-escape — the
    /// annotation-fallback in `infer::body::infer_def_body` backfills a
    /// no-return body's inferred return type from the annotation itself,
    /// so the type comes out concrete (`Clean`, not `Unknown`) and E065's
    /// classification can never see this mistake; only a direct
    /// `has_value_return` check catches it. An implicit `-> DONE` is never
    /// treated as satisfying this — DONE ends the *turn*, not the value
    /// contract.
    E150,

    // ── Native lint: asymmetric choice-branch dead-end (issue #1219,
    //    decision-log 2026-07-22 "Flows end implicitly (native)" item 4) ──
    /// A native `{? … }` choice's own body falls through (no divert/return)
    /// while a sibling choice in the same set diverts onward, at a genuine
    /// dead end (nothing follows the choice point to reconverge into) — the
    /// residual value of ink's retired "ran out of content" error,
    /// relocated to a narrow, **opt-in, warning-severity** lint
    /// (`brink_analyzer::native_choice_dead_end`) rather than a blocking
    /// runtime fault. Fires only for the *mixed* case — some siblings
    /// divert, at least one doesn't — never for a choice set where every
    /// branch falls through (an ordinary menu that ends) or where the
    /// choice set's `continuation` is non-empty (native has no gather,
    /// `docs/native-surface-charter.md` §5 — a non-empty continuation is
    /// the dissolved gather, and every falling-through branch reconverging
    /// there is ordinary weave structure, not a mistake).
    E151,

    /// A `contains(m, needle)` call whose `needle` argument is statically
    /// visible as outside the map key domain (int/string/bool) while `m`
    /// is statically visible as a map — companion to the #580 ruling
    /// (`docs/decision-log.md` 2026-07-12 "contains(map, non-key-domain
    /// needle) returns false"): the call can never do anything but return
    /// `false` at runtime, so the always-false result is a compile-time
    /// warning rather than a silent footgun. Strict-mode-only
    /// (`brink_analyzer::contains_domain`, wired into `strict::check`
    /// alongside `conversions`/`range_refinement` — the same
    /// inference-substrate-backed domain-check family): needs the
    /// project's whole-program `InferenceResult`
    /// (`structs::classify_expr_ty`) to classify a variable/call/
    /// index-valued needle, which is only ever computed under `types =
    /// strict`. Under `types = gradual` this stays silent and the
    /// runtime's total `false` return is the sole (correct, non-faulting)
    /// backstop. `Warning`-severity like `E106`'s map-literal-key sibling
    /// check, so it flows through the ordinary suppressible `diagnostics`
    /// channel and is re-levelable via the project's `[lints]` table.
    E152,

    // ── `@[allow(…)]` source-level suppression (issue #1161) ───────
    /// An `@[allow(…)]` argument is not a diagnostic code this compiler
    /// knows (`DiagnosticCode::from_str_code` says no) — a typo like
    /// `@[allow(E1511)]` or a name like `@[allow(dead_code)]`.
    ///
    /// A hard error by construction, and deliberately so: the whole point
    /// of a suppression directive is that the author believes a diagnostic
    /// is being silenced, so a misspelled code that silently does nothing
    /// is the worst possible outcome (the #1374 reserved-keys lesson, and
    /// the `@`-namespace rule in `docs/directive-annotations-spec.md` §1.1
    /// — every `@`-mark is a valid directive in a valid placement or a hard
    /// error).
    E153,

    /// An `@[allow(…)]` names a real diagnostic code that is **not
    /// suppressible**: one whose default severity
    /// ([`DiagnosticCode::severity`]) is `Error`.
    ///
    /// Source-level suppression only ever reaches the warning/lint tier. An
    /// error means the compiler cannot produce a correct artifact, so
    /// letting an annotation silence one would be a way to ship broken
    /// code; the B0.3 admission-validator family (`E121`–`E128`) is covered
    /// by the same rule (all `Error`-severity) *and* structurally, since
    /// admission diagnostics never route through
    /// [`crate::suppressions::apply_suppressions`] at all. This mirrors the
    /// `[lints]` table's own hard-error exemption (issue #1160, step 2 of
    /// `brink_analyzer::effective_severity`): rather than curating which
    /// `Error` codes are "safe" to relax, none of them are reachable.
    E154,

    /// An `@[allow(…)]` whose argument list is missing, empty, or not a
    /// flat list of bare code identifiers (`@[allow]`, `@[allow()]`,
    /// `@[allow("E151")]`, `@[allow(reads(x))]`).
    ///
    /// The grammar counterpart of `E100` on the `@[effects(…)]` channel:
    /// the annotation parses as an annotation but declares nothing this
    /// channel can act on.
    E155,

    // ── Lambdas (native surface, issue #1685) ──────────────────────
    /// A lambda body assigns to a **captured binding** — a `let`/param
    /// binding declared outside the lambda and read inside it.
    ///
    /// A hard error by the 2026-07-19 ruling ("assignment to a captured
    /// binding is a compile error"): brink lambdas capture BY VALUE always
    /// (Rust's `move` as the only mode, no keyword, no ref captures in v1),
    /// so the binding a lambda body writes to is its own *snapshot* — the
    /// write can never be observed by the enclosing scope. A snapshot write
    /// is always a lost write, and this kills the closure-mutation
    /// confusion structurally rather than letting authors discover it as a
    /// silent no-op at runtime.
    ///
    /// Writes to a *global* (a module-level `var` cell) are not captures
    /// and are not flagged: a global is a durable cell reached by name, not
    /// a snapshotted binding.
    E156,
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
            Self::E119 => "E119",
            Self::E120 => "E120",
            Self::E121 => "E121",
            Self::E122 => "E122",
            Self::E123 => "E123",
            Self::E124 => "E124",
            Self::E125 => "E125",
            Self::E126 => "E126",
            Self::E127 => "E127",
            Self::E128 => "E128",
            Self::E129 => "E129",
            Self::E130 => "E130",
            Self::E131 => "E131",
            Self::E132 => "E132",
            Self::E133 => "E133",
            Self::E134 => "E134",
            Self::E135 => "E135",
            Self::E136 => "E136",
            Self::E137 => "E137",
            Self::E138 => "E138",
            Self::E139 => "E139",
            Self::E140 => "E140",
            Self::E141 => "E141",
            Self::E142 => "E142",
            Self::E143 => "E143",
            Self::E144 => "E144",
            Self::E145 => "E145",
            Self::E146 => "E146",
            Self::E147 => "E147",
            Self::E148 => "E148",
            Self::E149 => "E149",
            Self::E150 => "E150",
            Self::E151 => "E151",
            Self::E152 => "E152",
            Self::E153 => "E153",
            Self::E154 => "E154",
            Self::E155 => "E155",
            Self::E156 => "E156",
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
                "struct construction literal in a VAR/CONST declaration default does not match its declared shape"
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
            Self::E111 => {
                "unknown annotation name (the `@[…]` channel recognizes `effects`, plus `was` and `allow` on the native surface)"
            }
            Self::E112 => {
                "annotation line outside a recognized placement (ink: top of a knot/stitch body; native: directly above a `flow`/`fn`, or above any declaration or statement for `allow`)"
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
            Self::E119 => "sort comparator must be a pure, silent function",
            Self::E120 => "`weighted` requires weight/value pairs with positive int weights",
            Self::E121 => {
                "admission: unresolved reference has no matching referencing expression in the HIR body"
            }
            Self::E122 => "admission: declared symbol has no corresponding HIR declaration node",
            Self::E123 => {
                "admission: knot's `is_function` disagrees with its indexed function sentinel"
            }
            Self::E124 => "admission: node range is empty or extends past the end of the file",
            Self::E125 => "admission: two references share an identical source range",
            Self::E126 => {
                "admission: declared symbol's name does not match its kind's qualification shape"
            }
            Self::E127 => {
                "admission: divert or return is not the last statement in an inline conditional/sequence branch"
            }
            Self::E128 => {
                "admission: container's provenance kind disagrees with its indexed symbol kind"
            }
            Self::E129 => "native: construct parses but has no HIR lowering yet",
            Self::E130 => "native: `flow` nested more than two levels deep is not yet supported",
            Self::E131 => "native: `<-` (splice) used outside a choice point has no effect",
            Self::E132 => {
                "native: `@[was]` needs a quoted old module path, e.g. `@[was(\"story::old::path\")]`"
            }
            Self::E133 => {
                "native accept-list: root_content must be empty or the synthesized `flow main()` entry divert"
            }
            Self::E134 => {
                "native accept-list: INCLUDE sites are ink-only baggage, never legal in native HIR"
            }
            Self::E135 => "native accept-list: thread-start outside choice-point splice position",
            Self::E136 => "native accept-list: choice set carries a non-neutral weave-fold value",
            Self::E137 => "native .brink compile requires types = strict",
            Self::E138 => "map construction literal supplies a duplicate key",
            Self::E139 => "construction literal entries do not match the target type's form",
            Self::E140 => "method-call syntax matched a field that is not callable",
            Self::E141 => "method-call syntax matched neither a field nor a free function",
            Self::E142 => "method-call receiver type is unknown — annotate it",
            Self::E143 => "method-call auto-ref needs a receiver that can be written through",
            Self::E144 => "native: method call resolves but has no LIR lowering yet",
            Self::E145 => {
                "the `as` binding must be the entire condition (no `&&`/`||` composition)"
            }
            Self::E146 => "the `as` binding in a choice guard is not yet supported",
            Self::E147 => "the `as` binding requires an `Option[T]` condition",
            Self::E148 => "an `as` binding is immutable and cannot be assigned to",
            Self::E149 => "`remove` is map-only — did you mean `remove_at`?",
            Self::E150 => {
                "declares a return type but the body may fall through without returning a value"
            }
            Self::E151 => {
                "native: this choice branch falls through while a sibling diverts — did you mean to add `-> …`, or `-> DONE` to end deliberately?"
            }
            Self::E152 => {
                "`contains`'s needle is statically outside the map key domain — this call always returns `false`"
            }
            Self::E153 => "`@[allow(…)]` names a diagnostic code this compiler does not know",
            Self::E154 => {
                "`@[allow(…)]` names a non-suppressible diagnostic — only warning-severity codes can be silenced at the source"
            }
            Self::E155 => {
                "`@[allow(…)]` needs at least one bare diagnostic code, e.g. `@[allow(E151)]`"
            }
            Self::E156 => {
                "a lambda cannot assign to a captured binding — captures are by value, so the write would be lost"
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
            | Self::E110
            | Self::E131
            | Self::E132
            | Self::E151
            | Self::E152 => Severity::Warning,
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
            "E119" => Some(Self::E119),
            "E120" => Some(Self::E120),
            "E121" => Some(Self::E121),
            "E122" => Some(Self::E122),
            "E123" => Some(Self::E123),
            "E124" => Some(Self::E124),
            "E125" => Some(Self::E125),
            "E126" => Some(Self::E126),
            "E127" => Some(Self::E127),
            "E128" => Some(Self::E128),
            "E129" => Some(Self::E129),
            "E130" => Some(Self::E130),
            "E131" => Some(Self::E131),
            "E132" => Some(Self::E132),
            "E133" => Some(Self::E133),
            "E134" => Some(Self::E134),
            "E135" => Some(Self::E135),
            "E136" => Some(Self::E136),
            "E137" => Some(Self::E137),
            "E138" => Some(Self::E138),
            "E139" => Some(Self::E139),
            "E140" => Some(Self::E140),
            "E141" => Some(Self::E141),
            "E142" => Some(Self::E142),
            "E143" => Some(Self::E143),
            "E144" => Some(Self::E144),
            "E145" => Some(Self::E145),
            "E146" => Some(Self::E146),
            "E147" => Some(Self::E147),
            "E148" => Some(Self::E148),
            "E149" => Some(Self::E149),
            "E150" => Some(Self::E150),
            "E151" => Some(Self::E151),
            "E152" => Some(Self::E152),
            "E153" => Some(Self::E153),
            "E154" => Some(Self::E154),
            "E155" => Some(Self::E155),
            "E156" => Some(Self::E156),
            _ => None,
        }
    }
}
