use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag, NameId};
use rowan::TextRange;

use crate::FileId;
use crate::determinism::{LookupMap, LookupSet};
use crate::symbols::{ResolutionMap, SymbolIndex, SymbolInfo};

use super::structs::{GlobalShapeMap, ShapeTable};

// ─── Resolution lookup ──────────────────────────────────────────────

/// O(1) lookup from `(FileId, TextRange)` to the resolved `DefinitionId`.
pub struct ResolutionLookup {
    map: LookupMap<(FileId, TextRange), DefinitionId>,
}

impl ResolutionLookup {
    pub fn build(resolutions: &ResolutionMap) -> Self {
        let map = resolutions
            .iter()
            .map(|r| ((r.file, r.range), r.target))
            .collect();
        Self { map }
    }

    pub fn resolve(&self, file: FileId, range: TextRange) -> Option<DefinitionId> {
        self.map.get(&(file, range)).copied()
    }
}

// ─── B3a UFCS verdict lookup (issue #1506) ─────────────────────────

/// Mirror of `brink_analyzer::ufcs::UfcsVerdict`, narrowed to what LIR
/// lowering needs to pick the right call shape
/// (`lir::lower::expr::lower_ufcs_call`). `brink-ide`'s hover/go-to-def
/// wiring (issue #1507, `ufcs_hover` module) is a second reader of this same
/// mirror — it reads verdicts through `brink_db::ProjectDb::ufcs_verdict`
/// rather than lowering-specific data, so nothing here is LIR-lowering-only
/// even though lowering is still this type's original consumer.
///
/// `brink-ir` sits below `brink-analyzer` in the crate graph
/// (`brink-analyzer` depends on `brink-ir`, never the reverse — see
/// [`TypeMode`](super::TypeMode)'s doc for the established precedent), so it
/// cannot name the analyzer's own `UfcsVerdict` directly.
/// `brink_analyzer::ufcs_lir_lookup` is the one translation point (it can
/// see both crates, since `brink-analyzer` already depends on `brink-ir`):
/// `brink-db`'s `ufcs_resolution_query` (the production path) and
/// `brink_analyzer::assemble_analyzer_tables` (the salsa-free path used by
/// `brink-test-harness` and any other caller with no salsa layer of its own)
/// both call it to build a [`UfcsLookup`] from `brink_analyzer::UfcsTable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UfcsVerdict {
    /// The call's final path segment names a function-typed field on the
    /// receiver's type — lower as a call *through* that field's value
    /// (`lir::Expr::CallValue`). No further data is needed: the field name
    /// and the receiver chain are already carried structurally by the HIR
    /// `Path` this verdict is keyed against.
    FieldCall,
    /// The call's final path segment names a free function in ordinary
    /// lexical scope — lower as `target(receiver, args…)`.
    FreeFnDesugar { target: DefinitionId },
    /// **D5 auto-ref** (issue #1462): as [`Self::FreeFnDesugar`], but
    /// `target`'s first declared parameter is `ref`, so the receiver is
    /// passed *by reference* — lower as `target(ref receiver, args…)`, the
    /// projection spelled explicitly through the same T1e ref-argument
    /// machinery an explicitly written `ref` argument reaches
    /// (`lir::lower::expr::lower_call_args`). The analyzer has already
    /// checked that the receiver can be written through (`E143` otherwise),
    /// so lowering never re-derives that rule.
    FreeFnAutoRef { target: DefinitionId },
    /// The call's final path segment names a T1b/NS stdlib prelude verb (or
    /// a classic ink builtin) with no index symbol of its own — lower as
    /// `name(receiver, args…)` through the same builtin/stdlib dispatch an
    /// ordinary bare call of that name already reaches.
    PreludeDesugar { name: String },
}

/// Project-wide `(file, range) → verdict` lookup — the `brink-ir`-facing
/// counterpart of `brink_analyzer::UfcsTable`. Built once (`brink-db`'s
/// `ufcs_resolution_query`) and shared read-only across every `LowerCtx` in
/// a `lower_to_program`/incremental-chunk call, exactly like
/// [`ResolutionLookup`] above — plus, since issue #1507, `brink-ide`'s
/// hover/go-to-def wiring reads individual verdicts out of the same
/// memoized table via `brink_db::ProjectDb::ufcs_verdict`, so this is no
/// longer an LIR-lowering-exclusive structure.
///
/// Empty by construction for every caller that never ran the analyzer's
/// `ufcs` pass (this crate's own tests, `compile_bench`, `golden_i078.rs`) —
/// see `lower_ufcs_call`'s fallback doc for what an empty table means at a
/// UFCS-shaped call site reached there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UfcsLookup {
    map: LookupMap<(FileId, TextRange), UfcsVerdict>,
}

impl UfcsLookup {
    /// The empty table — every lookup misses.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from `(file, range, verdict)` rows — `brink-db`'s translation
    /// of `brink_analyzer::UfcsTable::iter()`.
    #[must_use]
    pub fn from_entries(entries: Vec<(FileId, TextRange, UfcsVerdict)>) -> Self {
        Self {
            map: entries.into_iter().map(|(f, r, v)| ((f, r), v)).collect(),
        }
    }

    /// The verdict recorded for the UFCS call site at `range` in `file`, if
    /// any.
    #[must_use]
    pub fn get(&self, file: FileId, range: TextRange) -> Option<&UfcsVerdict> {
        self.map.get(&(file, range))
    }

    /// Whether this table carries any recorded verdict at all — a project
    /// with no UFCS-shaped call anywhere stays at the empty table
    /// ([`Self::new`]'s doc). Exists so a caller assembling this table can
    /// assert it actually got populated instead of silently staying empty
    /// (issue #1528's coverage test) without needing a specific `(file,
    /// range)` key to probe with.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Every call site whose verdict is a [`UfcsVerdict::FreeFnDesugar`] or
    /// [`UfcsVerdict::FreeFnAutoRef`] targeting `target` — issue #1539: the
    /// project-wide enumeration `find_references`/`rename` need to also
    /// rewrite/report a renamed free function's UFCS-desugared call sites,
    /// not just its plain `ResolutionMap` references. `FieldCall`/
    /// `PreludeDesugar` verdicts carry no `DefinitionId` and never match.
    ///
    /// Sorted by `(file, range.start(), range.end())` — this crate's
    /// determinism rule (see `crate::determinism`'s doc): the underlying
    /// `map` is an audited `HashMap`, so an iteration reaching output (an
    /// edit list, a reference list) must sort first.
    #[must_use]
    pub fn call_sites_for_target(&self, target: DefinitionId) -> Vec<(FileId, TextRange)> {
        let mut sites: Vec<(FileId, TextRange)> = self
            .map
            .iter()
            .filter_map(|(&(file, range), verdict)| match *verdict {
                UfcsVerdict::FreeFnDesugar { target: t }
                | UfcsVerdict::FreeFnAutoRef { target: t }
                    if t == target =>
                {
                    Some((file, range))
                }
                _ => None,
            })
            .collect();
        sites.sort_by_key(|&(file, range)| (file, range.start(), range.end()));
        sites
    }
}

// ─── B1 `or`-coalescing shape lookup (issue #1492) ─────────────────

/// Mirror of `brink_analyzer::CoalesceShape`, narrowed to exactly what LIR
/// lowering needs to pick one `or` step's code shape
/// (`lir::lower::expr::lower_coalesce_chain`).
///
/// RULED (maintainer, 2026-07-26, `docs/decision-log.md` "Lowering consumes
/// analyzer types"): **typing verdicts belong to the analyzer; lowering
/// consumes recorded types, never re-derives them.** A syntactic shape-sniff
/// here cannot see through an `Expr::Call` to its declared return type, nor
/// through a bare `Path` to a `VAR`/temp declared `Option[T]` — both are type
/// questions, and the answer already exists in `brink-analyzer::coalesce`.
///
/// `brink-ir` sits below `brink-analyzer` in the crate graph
/// (`brink-analyzer` depends on `brink-ir`, never the reverse — see
/// [`UfcsVerdict`]'s doc for the established precedent), so it cannot name
/// the analyzer's own `CoalesceShape` directly.
/// `brink_analyzer::coalesce_lir_lookup` is the one translation point:
/// `brink-db`'s `coalesce_types_query` (the production path) and
/// `brink_analyzer::assemble_analyzer_tables` (the salsa-free path used by
/// `brink-test-harness` and any other caller with no salsa layer of its own)
/// both call it to build a [`CoalesceLookup`] from
/// `brink_analyzer::CoalesceTable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoalesceShape {
    /// `Option[T] or Option[U]` — optionality survives the step, so codegen
    /// re-wraps the unwrapped `some(v)` branch with a `MakeSome` at the
    /// join point, keeping both branches the same shape.
    PreserveOption,
    /// `Option[T] or U` — the step collapses to the plain value type, so
    /// the unwrapped `v` stands as-is and no `MakeSome` is emitted.
    Collapse,
    /// The left-hand type is not statically pinned (gradual mode, or a
    /// strict escape already reported by `E065`/`E066`), or the analyzer
    /// recorded no verdict for this chain at all — absence and this verdict
    /// mean the same thing, which is why it is also the [`Default`].
    ///
    /// **The runtime check is the semantics here** (RULED 2026-07-26, issue
    /// #1492; documented on `brink_format::Opcode::CoalesceSome`): an
    /// `Option` value coalesces, a plain value faults, exactly like every
    /// other gradual runtime check. Codegen emits no `MakeSome` — with `rhs`
    /// possibly never evaluated there is no value to read a shape off, and
    /// the unwrapped collapse form is the one shape that stays sound for
    /// the `(Option[T],T)->T` reading the runtime check admits.
    #[default]
    RuntimeCheck,
}

/// Project-wide `(file, range) → per-step shapes` lookup for `or`-coalescing
/// chains — the LIR-lowering-facing counterpart of
/// `brink_analyzer::CoalesceTable`. Built once (`brink-db`'s
/// `coalesce_types_query`) and shared read-only across every `LowerCtx` in a
/// `lower_to_program`/incremental-chunk call, exactly like [`UfcsLookup`]
/// above.
///
/// Keyed at the **chain root** by [`crate::hir::expr_span`] — the derivation
/// both sides share, which since issue #1517 is the root infix node's own
/// [`crate::Provenance`] range — carrying every step's shape innermost-first.
/// One chain is one recorded fold, so lowering looks it up exactly once, at
/// its root; a spine node's key is a *different* key and simply misses.
///
/// Empty by construction for every caller that never ran the analyzer's
/// `coalesce` pass (this crate's own tests, `compile_bench`,
/// `golden_i078.rs`) — a miss means [`CoalesceShape::RuntimeCheck`], which
/// is always sound.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoalesceLookup {
    map: LookupMap<(FileId, TextRange), Vec<CoalesceShape>>,
}

impl CoalesceLookup {
    /// The empty table — every lookup misses.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from `(file, range, shapes)` rows — `brink-analyzer`'s
    /// translation of `brink_analyzer::CoalesceTable::iter()`. `shapes` is
    /// innermost-step-first, matching `CoalesceChain::steps`.
    #[must_use]
    pub fn from_entries(entries: Vec<(FileId, TextRange, Vec<CoalesceShape>)>) -> Self {
        Self {
            map: entries.into_iter().map(|(f, r, v)| ((f, r), v)).collect(),
        }
    }

    /// The per-step shapes recorded for the coalescing chain rooted at
    /// `range` in `file`, innermost first, if any.
    #[must_use]
    pub fn get(&self, file: FileId, range: TextRange) -> Option<&[CoalesceShape]> {
        self.map.get(&(file, range)).map(Vec::as_slice)
    }

    /// Whether this table carries any recorded chain at all —
    /// [`UfcsLookup::is_empty`]'s sibling, same rationale (issue #1528's
    /// coverage test).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ─── Analyzer side-table bundle (issue #1527) ──────────────────────

/// Every analyzer-produced side-table LIR lowering reads to make a
/// resolution-dependent codegen choice, bundled into one value instead of
/// one `&Lookup` parameter per table.
///
/// Before this bundle, each new side-table ([`UfcsLookup`] for B3a UFCS,
/// then [`CoalesceLookup`] for B1 `or`-coalescing) added its own parameter
/// to every lowering signature between
/// [`super::lower_to_program_with_type_mode`] and [`LowerCtx`] itself —
/// eight signatures deep, forcing
/// `lower_root_content_chunks` to carry
/// `#[expect(clippy::too_many_arguments)]`. Two independent reviewers
/// (#1471, #1479) hit the same pain point and deliberately deferred the fix
/// to avoid mid-PR churn — this bundle is that fix. A future table (the
/// v6/Step work) now means adding one field here, not touching every
/// signature between the entry points and `LowerCtx` again.
///
/// A future table also means adding one field to
/// `brink_analyzer::AnalyzerTablesOwned` and wiring it into
/// `brink_analyzer::assemble_analyzer_tables` (issue #1528) — the one place
/// a caller with no salsa layer of its own (`brink-test-harness`'s
/// `corpus.rs`) assembles the owned tables this struct then borrows from;
/// forgetting that step is what let the harness silently lower with an
/// empty table for a table the production `brink-db` path already had.
///
/// `Copy` — it's two references, cheap to pass by value everywhere instead
/// of threading yet another `&`.
#[derive(Debug, Clone, Copy)]
pub struct AnalyzerTables<'a> {
    /// B3a UFCS (issue #1506) — see [`UfcsLookup`]'s own doc.
    pub ufcs: &'a UfcsLookup,
    /// B1 `or`-coalescing (issue #1492) — see [`CoalesceLookup`]'s own doc.
    pub coalesce: &'a CoalesceLookup,
}

// ─── Name table ─────────────────────────────────────────────────────

/// Intern strings to `NameId`. Deduplicates identical strings.
pub struct NameTable {
    map: LookupMap<String, NameId>,
    entries: Vec<String>,
}

impl NameTable {
    pub fn new() -> Self {
        Self {
            map: LookupMap::new(),
            entries: Vec::new(),
        }
    }

    pub fn intern(&mut self, name: &str) -> NameId {
        if let Some(&id) = self.map.get(name) {
            return id;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "name table won't exceed u16::MAX"
        )]
        let id = NameId(self.entries.len() as u16);
        self.entries.push(name.to_string());
        self.map.insert(name.to_string(), id);
        id
    }

    pub fn into_entries(self) -> Vec<String> {
        self.entries
    }

    /// Rebuild a table pre-seeded with `entries` (first-occurrence order),
    /// so subsequent [`intern`](Self::intern) calls dedup against them and
    /// append after. The inverse of [`into_entries`](Self::into_entries) —
    /// the FG-4d link phase captures the decl+struct seed as a `Vec<String>`
    /// (a cutoff-friendly, `Eq`-able value) and reconstitutes the table here
    /// before merging the per-chunk local names, so the assembled ids are
    /// byte-identical to the single shared-table walk.
    pub fn from_entries(entries: Vec<String>) -> Self {
        let map = entries
            .iter()
            .enumerate()
            .map(|(i, s)| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "name table won't exceed u16::MAX"
                )]
                (s.clone(), NameId(i as u16))
            })
            .collect();
        Self { map, entries }
    }
}

/// The root container's `DefinitionId` — the hash-of-empty-path address every
/// lowering phase (prelude, per-chunk, assembly) uses as the `root_id`
/// fallback. Content-derived (a fixed hash), so a fresh [`IdAllocator`] in a
/// per-chunk salsa memo yields the same value as the whole-project walk
/// (FG-4d history-independence — `docs/fine-grained-salsa-proposal.md`
/// appendix).
#[must_use]
pub fn root_definition_id() -> DefinitionId {
    IdAllocator::new().alloc_address("")
}

// ─── Id allocator ───────────────────────────────────────────────────

/// Allocates new `DefinitionId`s for containers not in the symbol index
/// (root, choice targets, unlabeled gathers).
pub struct IdAllocator {
    used: LookupMap<String, DefinitionId>,
    /// Global counter for conditionals and sequences. Never resets —
    /// shared between the plan phase and lowering phase to ensure
    /// unique container paths across all sub-scopes.
    seq_counter: usize,
    /// Prefix every allocated path is qualified with (#1504). Set to the
    /// owning file's `root_content_scope_path` for root-content chunks and
    /// — since issue #2229 — for knot chunks too: a knot-name qualifier
    /// alone stops being unique the moment two files legitimately declare a
    /// same-named knot (M-2d, #790). See
    /// [`set_path_prefix`](Self::set_path_prefix).
    path_prefix: String,
}

impl IdAllocator {
    pub fn new() -> Self {
        Self {
            used: LookupMap::new(),
            seq_counter: 0,
            path_prefix: String::new(),
        }
    }

    /// Qualify every subsequently allocated path with `prefix` (#1504).
    ///
    /// `lower_root_content_chunks` shares one allocator across every file's
    /// root weave, and those paths (`s-0`, `#root-terminus`, …) restart per
    /// file — so without a per-file prefix two files' root content mints the
    /// same `DefinitionId`. `lower_knot_chunk` needs the identical
    /// qualifier for the same reason one level down (#2229): its paths are
    /// knot-name-qualified, and two files' same-named knots (M-2d, #790)
    /// restart them per knot. Callers set
    /// [`hir::root_content_scope_path`](crate::hir::root_content_scope_path)
    /// here, the same qualifier the HIR stamping pass gives the anonymous
    /// choice/gather containers of the same file.
    pub fn set_path_prefix(&mut self, prefix: String) {
        self.path_prefix = prefix;
    }

    /// Allocate an address id from a path string (e.g. `""`, `"knot.c0"`),
    /// qualified by the current [`set_path_prefix`](Self::set_path_prefix).
    pub fn alloc_address(&mut self, path: &str) -> DefinitionId {
        let qualified = qualify_path(&self.path_prefix, path);
        if let Some(&id) = self.used.get(qualified.as_ref()) {
            return id;
        }
        let hash = hash_path(&qualified);
        let id = DefinitionId::new(DefinitionTag::Address, hash);
        self.used.insert(qualified.into_owned(), id);
        id
    }

    /// Qualify a lambda-lifted function's scope-relative path with the
    /// current [`set_path_prefix`](Self::set_path_prefix) prefix — the
    /// **name** string codegen reads off `Container::name` to derive
    /// `path_hash` and its address path (issue #1709).
    ///
    /// RULED 2026-08-02 (`docs/decision-log.md`, issue #1727): this
    /// allocator no longer *mints* a lifted lambda's `DefinitionId` as its
    /// primary source of truth — it only re-derives the display-name
    /// spelling here. The identity itself is minted once, upstream, by
    /// `hir::stamp::stamp_container_ids` (`hir::LambdaExpr::container_id`);
    /// the caller (`lir::lower::lambda::lower_lambda`) reads that id
    /// directly rather than asking this allocator to hash a path again for
    /// every lambda the stamping pass reaches. Before this ruling, that
    /// second hash used the *live* `ctx.scope_path` — which mutates while
    /// descending into a `Conditional`/`Sequence`/`ChoiceSet` body — so a
    /// lambda nested inside one got a path the HIR-time stamping pass
    /// (which, pre-#1727, never walked expressions at all) could never
    /// reproduce byte-for-byte. Removing that as the *primary* derivation
    /// removes the id-parity problem outright; see `lower_lambda`'s own doc
    /// for the one narrow, structurally-safe position (a file-scope decl
    /// default) that still falls back to hashing `ctx.scope_path` here.
    #[must_use]
    pub fn qualify_lambda_path(&self, path: &str) -> String {
        qualify_path(&self.path_prefix, path).into_owned()
    }

    /// Allocate the next sequential index for a conditional or sequence scope.
    /// This counter never resets when entering sub-scopes within a knot/stitch,
    /// ensuring unique paths like `b-0`, `b-1`, etc. It resets at knot/stitch
    /// boundaries via [`reset_seq_counter`], since scope paths are qualified
    /// by knot name (e.g., `"start.b-0"` can't collide with `"waited.b-0"`).
    pub fn next_seq_index(&mut self) -> usize {
        let idx = self.seq_counter;
        self.seq_counter += 1;
        idx
    }

    /// Reset the sequential index counter. Called at knot/stitch boundaries
    /// where the scope path prefix changes.
    pub fn reset_seq_counter(&mut self) {
        self.seq_counter = 0;
    }
}

/// Join an [`IdAllocator`] path prefix with a scope-relative path (#1504).
///
/// Borrows when there is no prefix, so the overwhelmingly common case (knot
/// chunks, whose paths are already knot-qualified) allocates nothing.
fn qualify_path<'a>(prefix: &str, path: &'a str) -> Cow<'a, str> {
    if prefix.is_empty() {
        Cow::Borrowed(path)
    } else if path.is_empty() {
        Cow::Owned(prefix.to_string())
    } else {
        Cow::Owned(format!("{prefix}.{path}"))
    }
}

/// Hash a path string using `DefaultHasher`, matching the converter/linker convention.
///
/// Collisions between container IDs and other definition types are already
/// impossible because `DefinitionId` encodes the tag in its top 8 bits.
fn hash_path(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

// ─── TM-4c struct-shape context ─────────────────────────────────────

/// The project's `types` policy, as seen by LIR lowering (TM-4c,
/// `docs/typed-mode-spec.md` §6/§1). A local, minimal mirror of
/// `brink-analyzer`'s `TypePolicy` — `brink-ir` sits *below*
/// `brink-analyzer` in the crate graph (`brink-analyzer` depends on
/// `brink-ir`, never the reverse), so it cannot name that type directly;
/// `brink-db`'s `lir_query` maps `TypePolicy` to this enum at the one call
/// site that threads it into [`lower_to_program`](super::lower_to_program).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeMode {
    #[default]
    Gradual,
    Strict,
}

/// Whole-program struct-shape data, built once before any container is
/// lowered and shared (read-only) by every [`LowerCtx`] — see
/// `lower::structs`' module doc for what each field is and the soundness
/// argument for why [`TypeMode::Strict`] gates static-offset emission.
pub struct StructCtx<'a> {
    pub shapes: &'a ShapeTable,
    pub global_shapes: &'a GlobalShapeMap,
    pub type_mode: TypeMode,
}

// ─── Lower context ──────────────────────────────────────────────────

/// Shared context threaded through all lowering functions.
pub struct LowerCtx<'a> {
    pub file: FileId,
    /// Whether the file being lowered came from the native (`.brink`)
    /// frontend — [`crate::HirFile::native`], carried down from the
    /// chunk-entry `HirFile` so expression lowering can reach it.
    ///
    /// Read by exactly one lowering decision today: a bare `Expr::Path`
    /// resolving to a statically-named function is a **fn value** on the
    /// native surface (`handler(scene)`, RULED 2026-08-01, issue #1862)
    /// but a knot's **visit count** in ink — see
    /// [`super::expr::lower_path`].
    pub native: bool,
    pub resolutions: &'a ResolutionLookup,
    pub index: &'a SymbolIndex,
    pub temps: &'a TempMap,
    pub names: &'a mut NameTable,
    pub ids: &'a mut IdAllocator,
    /// Current container path prefix (e.g. `"knot"`, `"knot.stitch"`).
    pub scope_path: String,
    /// Whether this lowering call is inside a file's root content (as
    /// opposed to a knot or stitch body). Fixed for the lifetime of one
    /// `lower_root_content_chunks`/`lower_knot`/`lower_stitch` call — unlike
    /// `scope_path`, nested conditionals/sequences/choice bodies never
    /// change it, so it reliably answers "are we at the story's top level"
    /// regardless of how deeply nested the current statement is.
    ///
    /// Used to gate the implicit `-> DONE` a `ChoiceSet`'s empty, unlabeled
    /// continuation gets (`build_continuation_container`): falling off the
    /// end of the *root* content is a safe implicit end (ink's own
    /// behavior), but falling off the end of a knot/stitch without an
    /// explicit `-> DONE`/`-> END` is a genuine `RanOutOfContent` runtime
    /// error in real ink — see issue #1503.
    pub is_root_content_scope: bool,
    /// Child containers created during content lowering (inline sequences).
    /// Drained by the caller after each statement.
    pub pending_children: Vec<super::lir::Container>,
    /// Temps that have been declared so far in source order.
    /// Forward-referenced temps (used before declaration) should resolve as
    /// globals, matching inklecate's behavior.
    pub visible_temps: LookupSet<String>,
    /// Mapping from `FileId` to source file path, for populating `SourceLocation`.
    pub file_paths: &'a LookupMap<FileId, String>,
    /// The root container ID — used as fallback when a stamped container ID
    /// is missing (should not happen in well-formed HIR).
    pub root_id: brink_format::DefinitionId,
    /// The gather target of the enclosing choice set, if any. Set when
    /// lowering a choice body so that labeled containers within it can
    /// include an explicit `goto gather` — making them self-sufficient
    /// regardless of whether they're entered via `enter_container`
    /// (structured) or `goto` (ink divert).
    pub choice_gather_target: Option<brink_format::DefinitionId>,
    /// Next free temp slot for T1b block-scoped locals (explicit `temp`
    /// declarations inside `~ { … }`, `for` loop variables, and
    /// compiler-synthesized temps for indexed-assignment/for-loop
    /// desugaring). Shared by mutable reference across an entire frame
    /// (a knot body + all its stitches, or the whole root scope across
    /// files) — the same threading discipline as `ids` — so two block
    /// scopes that share a call frame never collide on a slot number.
    /// Seeded to the classic `TempMap`'s `total_slots()` for that frame.
    pub next_block_slot: &'a mut u16,
    /// Stack of open `~ { … }` lexical scopes. Each frame holds
    /// `(name, slot)` pairs for locals declared in that scope, innermost
    /// last. [`LowerCtx::temp_slot`] searches this stack (innermost first)
    /// before falling back to the classic flat `temps` map, so a
    /// block-scoped `temp` correctly shadows an outer temp of the same
    /// name (docs/t1b-surface-spec.md §2) without disturbing the outer
    /// slot's storage.
    pub block_scopes: Vec<Vec<(String, u16)>>,
    /// Temp slots that hold an `as` binding (B1b, issue #1475). The
    /// binding is **immutable** by ruling, and this is what makes that
    /// enforceable: every write path — plain assignment, compound `+=`,
    /// an indexed-assignment root, a bare in-place mutator like
    /// `pop`/`clear` — resolves its target through
    /// [`super::stmts::lower_assign_target`], which refuses a slot in this
    /// set (`E148`) via the shared [`super::stmts::reject_as_binding_write`]
    /// check. A single-level struct-field write/mutator
    /// (`super::blocks::lower_single_level_field_write`,
    /// `super::blocks::lower_field_mutator`, issue #2122) resolves a
    /// `Param`/`Temp` root's slot independently of `lower_assign_target`
    /// (their root is the *head* of a two-segment path, not the whole
    /// target), so those two call `reject_as_binding_write` directly
    /// instead of routing through `lower_assign_target` itself — the same
    /// set, the same diagnostic, a different call site. `ref` arguments
    /// never route through either choke point — they hand the callee a raw
    /// pointer to the slot instead — so
    /// [`super::expr::lower_ref_path_call_arg`] and
    /// [`super::expr::lower_ref_projection_arg`] separately consult this
    /// set at their own root. Likewise, the UFCS frame-local auto-ref
    /// recognizer ([`super::blocks::try_lower_frame_local_auto_ref_stmt`],
    /// `g.hp.heal(5)`-shaped calls) writes the receiver back into the
    /// root slot via its own `Assign`, bypassing both choke points above,
    /// so it also consults this set inline at its own root. Entries are
    /// never removed: a slot is allocated fresh per binding and never
    /// reused, so membership is a permanent property of the slot, not of
    /// the scope being open.
    pub as_binding_slots: crate::determinism::LookupSet<u16>,
    /// Every name ever declared via [`LowerCtx::declare_block_local`] in
    /// this frame — i.e. every T1b block-scoped `temp`/`for`-loop-variable
    /// name, whether or not its `~ { … }` block is still open. Unlike
    /// `block_scopes`, entries are never removed on `pop_block_scope`.
    ///
    /// Distinguishes, at the point `lower_path`/`lower_call_args` fall
    /// through to the "temp not currently visible" case, a genuine classic
    /// (non-block) forward reference (used before its declaring `~ temp`
    /// statement — inklecate-compat: emits a phantom `get_global` that
    /// faults at link time) from a block-scoped temp referenced *after* its
    /// own block has already closed (#680 RCA) — the latter is a real
    /// authoring error and gets its own diagnostic (E082) instead of
    /// silently resolving to the wrong global slot.
    pub block_scoped_temp_names: LookupSet<String>,
    /// Diagnostics produced during lowering — historically just warnings
    /// (the T1b block-scoped-temp shadow warning, E054), but also
    /// Error-severity ones now (E055/E056 mutator checks, E057
    /// break/continue-outside-loop, E058 mutator arity). Severity is read
    /// from `DiagnosticCode::severity()`, not from which list a diagnostic
    /// was pushed to — `brink-db`'s `lir_query` partitions this Vec by
    /// severity and refuses to hand back a `Program` (gates `program:
    /// None`, bypassing `// brink-disable-all` suppression entirely, unlike
    /// analysis-phase diagnostics) when any Error-severity one is present,
    /// so pushing here is a real, non-suppressible compile error, not a
    /// cosmetic note. Shared by mutable reference across an entire
    /// `lower_to_program` call — the same threading discipline as
    /// `ids`/`next_block_slot` — and returned alongside the program.
    pub diagnostics: &'a mut Vec<crate::Diagnostic>,
    /// Depth of `while`/`for` loop nesting the current T1b block-statement
    /// lowering position is inside — incremented/decremented around
    /// `while`/`for` body lowering in `blocks::lower_block_stmt`. Zero
    /// outside any loop; used to reject `break`/`continue` at depth 0
    /// (E057) instead of emitting an unguarded `LogicBreak`/`LogicContinue`
    /// that codegen has no jump target for (see #577 review).
    pub loop_depth: u32,
    /// TM-4c (`docs/typed-mode-spec.md` §6): the whole-program struct-shape
    /// data (shape table, `types` policy, global struct-typed VAR/CONST
    /// annotations) — shared, read-only, identical across every `LowerCtx`
    /// in a single `lower_to_program` call.
    pub structs: &'a StructCtx<'a>,
    /// TM-4c: temp slots (this frame only — reset per `LowerCtx`, exactly
    /// like `visible_temps`) whose declaring `TempDecl`'s TM-2 annotation
    /// names a declared struct, resolved once (issue #2238) to that
    /// shape's own `DefinitionId` (referrer = this frame's own file, always
    /// correct since a temp's annotation and every read of it share one
    /// file) — the temp-local half of the "compile-time known shape" story
    /// `expr::known_shape` chases (`structs::GlobalShapeMap` is the global
    /// half). Keyed by slot, not name, so a block-scoped shadow of an outer
    /// temp of the same name still maps to its own (correct) shape.
    pub temp_shapes: LookupMap<u16, DefinitionId>,
    /// The analyzer-produced side-tables (B3a UFCS, B1 `or`-coalescing,
    /// issue #1527) — shared, read-only, identical across every `LowerCtx`
    /// in a single `lower_to_program`/incremental-chunk call, the same
    /// threading discipline as `structs`. Empty for every caller that never
    /// ran the corresponding analyzer pass — see [`AnalyzerTables`]'s doc.
    pub tables: AnalyzerTables<'a>,
    /// Lambda-lifted function containers synthesized while lowering this
    /// scope (issue #1709, `lower::lambda`). Shared by mutable reference
    /// across an entire chunk — the same threading discipline as
    /// `ids`/`next_block_slot`/`diagnostics` — because a lambda can appear
    /// anywhere an expression can, including inside a stitch that shares
    /// the knot's frame. The chunk's caller drains this into the chunk's
    /// top-level containers, so every lifted function ends up a sibling of
    /// the project's function knots rather than nested inside the frame
    /// that created it (see `lower::lambda`'s module doc for why that
    /// placement is the safe one).
    pub lifted: &'a mut Vec<crate::lir::types::Container>,
}

impl<'a> LowerCtx<'a> {
    /// Resolve a HIR path at the given range. Returns the resolved `SymbolInfo`.
    pub fn resolve_path(&self, range: TextRange) -> Option<&'a SymbolInfo> {
        let id = self.resolutions.resolve(self.file, range)?;
        self.index.symbols.get(&id)
    }

    /// Resolve a HIR path to its `DefinitionId`.
    pub fn resolve_id(&self, range: TextRange) -> Option<DefinitionId> {
        self.resolutions.resolve(self.file, range)
    }

    /// Look up a name in the temp map for the current scope.
    /// Only returns a slot if the temp has been declared (is visible).
    ///
    /// Checks open T1b block scopes first (innermost frame first, most
    /// recent declaration within a frame first) so a block-scoped `temp`
    /// correctly shadows an outer temp of the same name; outside any block
    /// (`block_scopes` empty) this is exactly the pre-T1b lookup.
    pub fn temp_slot(&self, name: &str) -> Option<u16> {
        if let Some(slot) = self.lookup_block_local(name) {
            return Some(slot);
        }
        if self.visible_temps.contains(name) {
            self.temps.get(name)
        } else {
            None
        }
    }

    /// Search the open block-scope stack for `name`, innermost first.
    fn lookup_block_local(&self, name: &str) -> Option<u16> {
        for frame in self.block_scopes.iter().rev() {
            if let Some(&(_, slot)) = frame.iter().rev().find(|(n, _)| n == name) {
                return Some(slot);
            }
        }
        None
    }

    /// Open a new T1b lexical block scope (`~ { … }`, or an `if`/`while`/
    /// `for` body nested within one). Must be paired with
    /// [`pop_block_scope`](Self::pop_block_scope).
    pub fn push_block_scope(&mut self) {
        self.block_scopes.push(Vec::new());
    }

    /// Close the innermost T1b lexical block scope. Locals declared in it
    /// stop shadowing once popped.
    pub fn pop_block_scope(&mut self) {
        self.block_scopes.pop();
    }

    /// Allocate a fresh temp slot for a T1b block-scoped local (explicit or
    /// compiler-synthesized). Never reused/deduped by name — that's exactly
    /// what makes shadowing safe.
    pub fn alloc_block_slot(&mut self) -> u16 {
        let slot = *self.next_block_slot;
        *self.next_block_slot += 1;
        slot
    }

    /// Whether `name` is already visible as a temp/param — either an open
    /// T1b block scope (innermost first) or the classic flat scope (knot/
    /// stitch params + `~ temp` declarations seen so far in source order).
    /// The shadow-warning check (`docs/t1b-surface-spec.md` §2, E054) uses
    /// this: declaring a block-scoped `temp` with a name that's already
    /// visible — from an enclosing block *or* an outer classic temp — is a
    /// shadow.
    pub fn is_name_visible(&self, name: &str) -> bool {
        self.lookup_block_local(name).is_some() || self.visible_temps.contains(name)
    }

    /// Bind `name` to `slot` in the innermost open block scope.
    ///
    /// Every T1b local is declared while lowering a `~ { … }`/`if`/`while`/
    /// `for` body, all of which push a scope first, so `block_scopes` is
    /// never empty in practice; self-heals (opens a scope) rather than
    /// using a denied `expect`/`unwrap` if that invariant is ever violated.
    pub fn declare_block_local(&mut self, name: String, slot: u16) {
        self.block_scoped_temp_names.insert(name.clone());
        if self.block_scopes.is_empty() {
            self.block_scopes.push(Vec::new());
        }
        if let Some(frame) = self.block_scopes.last_mut() {
            frame.push((name, slot));
        }
    }

    /// Look up a temp slot by name, bypassing visibility checks.
    /// Used for `DeclareTemp` lowering where the slot must exist even
    /// though the temp hasn't been marked visible yet.
    pub fn temp_slot_raw(&self, name: &str) -> Option<u16> {
        self.temps.get(name)
    }

    /// TM-4c: record that temp `slot` was declared with a TM-2 annotation
    /// naming struct `shape_def` (its own `DefinitionId`, issue #2238).
    pub fn set_temp_shape(&mut self, slot: u16, shape_def: DefinitionId) {
        self.temp_shapes.insert(slot, shape_def);
    }

    /// TM-4c: if `annotation` names a declared struct, record it as `slot`'s
    /// known shape (`set_temp_shape`). The one call every `TempDecl`
    /// lowering site (classic and block-scoped) makes right after
    /// allocating/resolving the temp's own slot, so `expr::known_shape` can
    /// later chase a struct-typed `temp`'s reads/writes to a static offset
    /// under `types = strict`.
    ///
    /// Issue #2249: `annotation`'s own span is a `RefKind::Type` reference
    /// the analyzer already resolved (`symbols::project`'s walk +
    /// `resolve::resolve_type_ref`, referrer = this frame's own file, same
    /// correctness argument issue #2238 gave for the prior
    /// `ShapeTable::resolve` call this replaces: a temp's annotation and
    /// every read of it share one file) — consume that recorded resolution
    /// directly instead of re-deriving it through `ShapeTable::resolve`'s
    /// own narrower primitive. Mirrors `lir::lower::structs::
    /// record_global_annotation`'s identical migration.
    pub fn record_temp_annotation(&mut self, slot: u16, annotation: Option<&crate::hir::TypeExpr>) {
        let shape = annotation
            .and_then(|ann| self.resolutions.resolve(self.file, ann.range()))
            .and_then(|id| self.structs.shapes.get_by_def(id));
        if let Some(shape) = shape {
            self.set_temp_shape(slot, shape.definition_id);
        }
    }

    /// TM-4c: the declared struct shape's own `DefinitionId` for temp
    /// `slot`, if its `TempDecl` carried a struct-typed TM-2 annotation.
    pub fn temp_shape(&self, slot: u16) -> Option<DefinitionId> {
        self.temp_shapes.get(&slot).copied()
    }

    /// TM-4c: the declared struct shape's own `DefinitionId` for a resolved
    /// global `VAR`/`CONST`, if its TM-2 annotation named a declared struct
    /// — already resolved once, at declaration time (issue #2238,
    /// `structs::record_global_annotation`).
    pub fn global_shape(&self, id: DefinitionId) -> Option<DefinitionId> {
        self.structs.global_shapes.get(&id).copied()
    }

    /// Qualify a label name with the current scope path.
    pub fn qualify_label(&self, label: &str) -> String {
        if self.scope_path.is_empty() {
            label.to_string()
        } else {
            format!("{}.{label}", self.scope_path)
        }
    }

    /// Allocate a `DefinitionId` for a sequence wrapper container.
    pub fn alloc_sequence_id(&mut self, counter: usize) -> DefinitionId {
        let path = if self.scope_path.is_empty() {
            format!("s-{counter}")
        } else {
            format!("{}.s-{counter}", self.scope_path)
        };
        self.ids.alloc_address(&path)
    }

    /// Look up an address `DefinitionId` by qualifying a label name with the
    /// current scope.
    ///
    /// **File-scoped, preferring `self.file`** (issue #2215 review): mirrors
    /// `lower::lookup_container_id`'s file-scoped lookup for the exact same
    /// M-2d collision (#2197) — with same-named labels/gathers/knots
    /// coexisting across two declared modules, an unscoped first-match
    /// would silently pick whichever candidate happens to sort first for
    /// *every* file querying that name. Preferring the entry declared in
    /// `self.file` is the correct self-identity semantic; falling back to
    /// the first match preserves prior behavior when there is no collision
    /// (`by_name` holding only one container candidate for the name).
    pub fn lookup_address_id(&self, label: &str) -> Option<DefinitionId> {
        use crate::symbols::SymbolKind;
        fn is_container(info: &SymbolInfo) -> bool {
            matches!(
                info.kind,
                SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label
            )
        }
        let qualified = self.qualify_label(label);
        self.index.by_name.get(&qualified).and_then(|ids| {
            ids.iter()
                .find(|&&id| {
                    self.index
                        .symbols
                        .get(&id)
                        .is_some_and(|info| is_container(info) && info.file == self.file)
                })
                .or_else(|| ids.first())
                .copied()
        })
    }
}

// ─── Temp map ───────────────────────────────────────────────────────

/// Per-scope temp variable slot assignments.
#[derive(Debug, Clone, Default)]
pub struct TempMap {
    slots: LookupMap<String, u16>,
}

impl TempMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: String, slot: u16) {
        self.slots.insert(name, slot);
    }

    pub fn get(&self, name: &str) -> Option<u16> {
        self.slots.get(name).copied()
    }

    pub fn total_slots(&self) -> u16 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "temp count won't exceed u16::MAX"
        )]
        {
            self.slots.len() as u16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::ResolvedRef;

    #[test]
    fn name_table_deduplication() {
        let mut table = NameTable::new();
        let a = table.intern("hello");
        let b = table.intern("world");
        let c = table.intern("hello");
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_eq!(table.into_entries(), vec!["hello", "world"]);
    }

    #[test]
    fn resolution_lookup() {
        let refs = vec![ResolvedRef {
            file: FileId(0),
            range: TextRange::new(10.into(), 15.into()),
            target: DefinitionId::new(DefinitionTag::Address, 42),
        }];
        let lookup = ResolutionLookup::build(&refs);
        assert_eq!(
            lookup.resolve(FileId(0), TextRange::new(10.into(), 15.into())),
            Some(DefinitionId::new(DefinitionTag::Address, 42))
        );
        assert_eq!(
            lookup.resolve(FileId(1), TextRange::new(10.into(), 15.into())),
            None
        );
    }

    #[test]
    fn id_allocator_stable() {
        let mut alloc = IdAllocator::new();
        let a = alloc.alloc_address("knot.c0");
        let b = alloc.alloc_address("knot.c0");
        assert_eq!(a, b);
        let c = alloc.alloc_address("knot.c1");
        assert_ne!(a, c);
    }

    #[test]
    fn temp_map_slots() {
        let mut map = TempMap::new();
        map.insert("x".to_string(), 0);
        map.insert("y".to_string(), 1);
        assert_eq!(map.get("x"), Some(0));
        assert_eq!(map.get("y"), Some(1));
        assert_eq!(map.get("z"), None);
        assert_eq!(map.total_slots(), 2);
    }
}
