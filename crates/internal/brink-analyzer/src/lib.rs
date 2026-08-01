//! Cross-file semantic analysis for inkle's ink narrative scripting language.
//!
//! The analyzer merges per-file `SymbolManifest`s from `brink-ir` into a
//! unified `SymbolIndex`, then runs validation passes (name resolution,
//! duplicate detection, type checking). Both `brink-compiler` and `brink-lsp`
//! consume the analysis result.

mod admission;
mod annotations;
mod anonymous_stateful;
mod await_purity;
mod coalesce;
mod comparator_contract;
mod contains_domain;
mod conventions_confinement;
mod conversions;
mod determinism;
mod dialect_gate;
mod effects_assertions;
mod external_check;
mod fn_values;
mod infer;
mod manifest;
mod map_keys;
mod markup_check;
mod modules;
mod native_admission;
mod native_choice_dead_end;
mod option_conditions;
mod option_rules;
mod protocols;
mod range_refinement;
mod ref_projection;
mod resolve;
mod signature;
mod strict;
mod structs;
mod type_resolution;
mod ufcs;
mod validate;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use admission::validate_admission;
pub use annotations::{
    check as check_annotations, mismatches as annotation_mismatches, resolve as resolve_annotation,
};
pub use anonymous_stateful::check as check_anonymous_stateful;
pub use await_purity::{
    check as await_purity_diagnostics, condition_callees as await_condition_callees, hir_has_await,
};
pub use brink_ir::FileId;
pub use brink_ir::ResolutionMap;
pub use brink_project_config::ProjectConfig;
pub use coalesce::{
    CoalesceChain, CoalesceShape, CoalesceStep, CoalesceTable, project_has_coalesce,
    to_lir_lookup as coalesce_lir_lookup,
};
pub use comparator_contract::{
    check as comparator_contract_diagnostics, comparator_callees, hir_has_comparator_site,
};
pub use conventions_confinement::conventions_module_diagnostics;
pub use dialect_gate::Dialect;
pub use effects_assertions::{
    assertion_defs as effects_assertion_defs, check as effects_assertion_diagnostics,
};
pub use external_check::{
    ExternalCheckSeverity, InferredType, ResolvedParam, ResolvedType,
    SemanticTypeDiagnosticSeverity, SymbolMeta, ValueMeta,
};
pub use infer::{
    BodyTypes, CallGraph, CoalesceError, Def, DirectCallArgMismatch, EffectAtoms, EffectRow, FnRow,
    InferenceResult, InferredSig, SccGraph, Ty, ValueCallFact, ValueCallKind, assignable,
    call_edges, coalesce, collect_external_sigs, def_body, def_effect_atoms, effects_project,
    erase_fn_rows, infer_project, inferable_defs, inferable_defs_from_index, referenced_globals,
    scc_graph, solve_scc, solve_scc_effects, unify, unify_all,
};
pub use manifest::{ModuleMap, ResolvedModule};
pub use native_admission::validate_native_accept_list;
pub use native_choice_dead_end::check as check_native_choice_dead_end;
pub use protocols::{
    Protocol, ProtocolImplDecl, check_protocol_impls, check_reserved_names,
    is_reserved_protocol_name, iterate_element_ty, iterate_val_ty,
};
pub use resolve::ImportScope;
pub use signature::{Sig, local_signature, signature};
pub use strict::{
    LintLevel, LintPolicy, TypePolicy, effective_severity, native_strict_only_error,
    resolve_type_policy,
};
pub use structs::{ShapeInfo, declared_shapes};
pub use ufcs::{
    NodeKey, SideTable, UfcsTable, UfcsVerdict, project_has_ufcs_call,
    resolve as resolve_ufcs_calls, to_lir_lookup as ufcs_lir_lookup,
};

use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DiagnosticCode, DocBlock, HirFile, HostManifest, ManifestExternal, SemanticTypeDef,
    Severity, SymbolIndex, SymbolKind, SymbolManifest,
};
use brink_project_config::ConfigWarning;

/// Tooling options for analysis: the registered host manifest and the
/// severity policy for its external checks. Defaults to no manifest.
///
/// `PartialEq`/`Eq` + serde are the #1306 requirement: `AnalysisOptions` is
/// the resolved-policy slot of the serializable, content-addressed
/// [`Environment`](../brink_environment/struct.Environment.html) input value,
/// so the whole `Environment` can be hashed, cached on, and diffed.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AnalysisOptions {
    /// The registered host-capability manifest, if any.
    pub host_manifest: Option<HostManifest>,
    /// Severity policy for manifest-driven external diagnostics.
    pub external_check: ExternalCheckSeverity,
    /// Severity policy for unknown-semantic-type diagnostics (`E040`).
    /// Defaults to `Tolerant` (the #339/#527 default-tolerant path); raise to
    /// `Error` to re-enable strict checking with no manifest registered
    /// (#532).
    pub semantic_type_check: SemanticTypeDiagnosticSeverity,
    /// T1b compiler dialect: gates brink-extension syntax (blocks, sigil
    /// literals, indexing). Defaults to `StrictInk` — an authoring-time/
    /// tooling input only, mount-time (CLI flag) in T1b-1; project-file
    /// config is out of scope (docs/t1b-surface-spec.md §1, #368 precedent).
    pub dialect: Dialect,
    /// TM-3 typed-mode policy (docs/typed-mode-spec.md §1). `None` means
    /// "the project never said" — the effective policy is then keyed on the
    /// dialect via [`resolve_type_policy`] (issue #1127, ruled 2026-07-19):
    /// `Brink` → `Strict`, `StrictInk` → `Gradual` (forever — the oracle
    /// corpus is anchored to it). `Some(_)` is an explicit choice (CLI flag,
    /// `brink.toml`, editor API) and always wins. Read the effective policy
    /// via [`AnalysisOptions::type_policy`], never this field directly.
    ///
    /// `Strict` requires `dialect = Brink` (a config error otherwise,
    /// `E064`) and turns on `Unknown`/`Conflicted`-escape errors, the
    /// boundary annotation-firewall exemption, and auto-wires `E063`
    /// (annotation-vs-inference mismatch) into production. Authoring-time/
    /// tooling input only — never embedded in `.inkb`, mirroring `dialect`.
    pub types: Option<TypePolicy>,
    /// Resolved `[lints]` policy (issue #1160): per-code severity overrides
    /// plus `deny-warnings`. `LintPolicy::default()` (empty overrides,
    /// `deny_warnings: false`) is a no-op — every diagnostic keeps its
    /// [`brink_ir::DiagnosticCode::severity`] default, byte-identical to
    /// pre-#1160 behavior. Resolved once, here, via
    /// [`AnalysisOptions::apply_project_config`] — read through
    /// [`effective_severity`], never this field directly.
    pub lints: LintPolicy,
    /// `brink.toml`'s `[project] elements` pointer (docs/prose-dialect-spec.md
    /// §3.4), if set: a built-in preset name or a project-relative path to
    /// the project's conventions module. `None` means no conventions
    /// module is configured. Consumed by the confinement check (issue
    /// #1844, `E169`) that requires pattern-claiming `@[element(claims =
    /// "…")]` handlers to live in the one file this names — resolving the
    /// pointer against real project/module identity needs `brink-db`'s
    /// path machinery, so this crate only carries the raw string through,
    /// the same posture [`Self::types`]/[`Self::dialect`] have toward their
    /// own project-file-authored values. Authoring-time/tooling input
    /// only, mirroring every other `AnalysisOptions` field — never embedded
    /// in `.inkb`.
    pub elements: Option<String>,
}

impl AnalysisOptions {
    /// The effective `types` policy for this options set — the one
    /// resolution seam (issue #1127): an explicit [`Self::types`] wins;
    /// otherwise the dialect-keyed default from [`resolve_type_policy`].
    #[must_use]
    pub fn type_policy(&self) -> TypePolicy {
        resolve_type_policy(self.dialect, self.types)
    }

    /// Apply a parsed `brink.toml` [`ProjectConfig`] onto these options,
    /// honoring the #1005 precedence rule: **explicit API calls / CLI flags
    /// override the file.** `dialect_overridden`/`types_overridden` tell this
    /// whether the caller already has an explicit value for that field (a CLI
    /// flag the user actually passed, an editor session's own
    /// `set_language_dialect`/`set_type_policy` call, …) — when true, that
    /// field is left untouched regardless of what the file says. The file only
    /// ever supplies a *default*.
    ///
    /// For `dialect`/`types`, fields the file doesn't set are also left
    /// untouched, so `self` should already carry whatever it would have
    /// without a config file (typically [`AnalysisOptions::default()`]).
    /// `lints` does not follow this rule — see below.
    ///
    /// `lints`/`deny-warnings` (issue #1160) have their own override
    /// mechanism — [`Self::apply_lint_overrides`], the CLI-flag/editor-API
    /// tier used by `brink compile`, `brink ide`, `brink-lsp`'s
    /// `initializationOptions`, and the wasm `EditorSession` — but unlike
    /// `dialect`/`types` that tier is applied as a *second, separate call*
    /// rather than an `_overridden` parameter here, so this call always
    /// resolves `[lints]` from `config` first: the file's `[lints]` table
    /// is the sole source of truth for what this call sets on
    /// [`AnalysisOptions::lints`], and it **replaces** `self.lints` wholesale
    /// with the policy resolved from
    /// `config` (a code missing from `config.lints`, or an absent `[lints]`
    /// table entirely, resolves to no override for that code; a missing
    /// `deny-warnings` resolves to `false`) rather than merging `config`'s
    /// entries key-by-key into whatever `self.lints` already held.
    ///
    /// This differs from `dialect`/`types`' "unset means untouched" rule
    /// above deliberately (issue #1397): those fields are one-shot,
    /// CLI-flag-style choices where "unset" genuinely means "the file
    /// doesn't have an opinion, leave whatever's already resolved alone".
    /// `[lints]`, in contrast, is a *table* a long-lived caller (the editor
    /// session re-applies `brink.toml` on every change; see
    /// `brink-web`'s `EditorSession::apply_parsed_config`) re-resolves from
    /// scratch each time it calls this — merge semantics meant a code
    /// deleted from `brink.toml` (or an editor-supplied config) left its
    /// previously-applied override permanently stuck, since nothing ever
    /// removed it from [`AnalysisOptions::lints`]. Replacing wholesale is
    /// safe for every caller: `apply_lint_overrides` (the CLI/API tier) is
    /// always documented to run *after* this, on top of whatever it just
    /// resolved, and no caller relies on this call preserving lint state
    /// this one didn't itself just set — `self.lints` at call time is
    /// always a fresh [`AnalysisOptions::default()`] (CLI, LSP, `brink ide`,
    /// the editor session's own throwaway `AnalysisOptions`, or `bevy-brink`
    /// via `brink-environment::resolve_options`); see the Invariant section
    /// below for why every caller constructs fresh rather than reusing a
    /// prior call's output.
    ///
    /// `brink-project-config` doesn't know the real `DiagnosticCode` set
    /// (kept dependency-free, #1234), so it accepts any string key under
    /// `[lints]` without validation. **This is the point that resolves a
    /// key against the real code set** (this crate owns `DiagnosticCode`)
    /// and decides which codes are actually overridable: a key that isn't a
    /// real code, or names a code whose default severity isn't `Warning`
    /// (never reachable through [`effective_severity`]'s hard-error
    /// exemption anyway — see its doc comment), is *not* included in the
    /// replaced [`AnalysisOptions::lints`] and instead earns a returned
    /// [`ConfigWarning`], the same "warn, never silently drop" channel
    /// unknown top-level/`[project]` keys already use. Every call site that
    /// already loops over `brink_project_config::parse_str`'s own warnings
    /// should loop over these the same way.
    ///
    /// Lives here rather than in `brink-project-config` so that crate needs no
    /// workspace dependencies and can publish standalone (#1234) — it owns the
    /// policy *types*, this crate owns applying them to its own options.
    ///
    /// ## Invariant: `self` must be fresh
    ///
    /// `[lints]` would be safe to apply onto a `self` mutated by a prior
    /// call — full replace, not merge, is exactly what makes that safe (see
    /// above). `dialect`/`types` are **not**: their "unset means untouched"
    /// rule means whatever `self.dialect`/`self.types` already held before
    /// this call would silently survive untouched if `config` (and the
    /// `_overridden` flags) don't set them. No caller relies on that today
    /// — there is no exception. Every production call site starts each call
    /// from a **freshly-constructed** [`AnalysisOptions::default()`]:
    /// `brink-cli`'s `brink ide`; `brink-lsp`'s `resolve_language_options`
    /// (called fresh both from `initialize` *and* repeatedly from
    /// `Backend::reload_brink_toml` on every `brink.toml` edit — the
    /// repeat-call case this invariant is actually about); `brink-web`'s
    /// `EditorSession::apply_parsed_config`, via its own throwaway
    /// `AnalysisOptions::default()` (it never reuses a mutated `self` —
    /// `dialect`/`types` are applied directly to the session elsewhere, not
    /// through this method); and — the one every mount funnels through —
    /// `brink-environment::resolve_options`, called fresh inside every
    /// [`Project::load`](../brink_environment/struct.Project.html#method.load)).
    /// `bevy-brink` never calls this method directly; it reaches it solely
    /// through `resolve_options`. Reusing a mutated `self` would let a
    /// later, unrelated compile silently inherit an earlier one's resolved
    /// `dialect`/`types` whenever its own `brink.toml` doesn't set them,
    /// breaking the determinism a caller doing repeat compiles (e.g.
    /// `bevy-brink`'s `InkLoader` on every asset (re)load) depends on.
    /// Nothing in this method's signature enforces starting fresh — it
    /// takes `&mut self`, so it can't tell "fresh" apart from "reused".
    /// This is a documented invariant rather than a compiler-checked one
    /// because enforcing it in the type (e.g. an associated constructor
    /// like `fn from_project_config(config, dialect_overridden,
    /// types_overridden) -> (Self, Vec<ConfigWarning>)` that owns
    /// construction) would mean touching all four production call sites
    /// plus the ~15 `brink-analyzer` unit tests that call
    /// `apply_project_config` directly on an already-constructed `options`
    /// — not because any caller needs `&mut self` reuse; see
    /// `resolve_options`/`repeat_compiles_do_not_leak_options_across_project_load_calls`
    /// in `brink-environment` for where the fresh-start invariant is
    /// actually pinned end-to-end.
    pub fn apply_project_config(
        &mut self,
        config: &ProjectConfig,
        dialect_overridden: bool,
        types_overridden: bool,
    ) -> Vec<ConfigWarning> {
        if !dialect_overridden && let Some(dialect) = config.dialect {
            self.dialect = dialect;
        }
        if !types_overridden && let Some(types) = config.types {
            self.types = Some(types);
        }
        // `elements` (issue #1844) follows `dialect`/`types`' "unset means
        // untouched" rule — no `_overridden` tier exists for it yet (no
        // caller today sets it any way but through this file), so there is
        // nothing for an explicit override to win over.
        if config.elements.is_some() {
            self.elements.clone_from(&config.elements);
        }
        let mut warnings = Vec::new();
        let mut overrides = BTreeMap::new();
        for (code, level) in &config.lints {
            match validate_lint_code(code) {
                Ok(()) => {
                    overrides.insert(code.clone(), *level);
                }
                Err(warning) => warnings.push(warning),
            }
        }
        // Replace, not merge (issue #1397) — see the doc comment above for
        // why: a code (or `deny-warnings`) omitted from `config` must
        // resolve to its base severity, not whatever a prior call left in
        // place.
        self.lints.overrides = overrides;
        self.lints.deny_warnings = config.deny_warnings.unwrap_or(false);
        warnings
    }

    /// Apply explicit CLI/API per-code lint-level overrides on top of
    /// whatever [`Self::apply_project_config`] already resolved (the
    /// default, then a discovered `brink.toml`) — the top of the `CLI/API >
    /// file > default` precedence stack (#1005), completing the "natural
    /// follow-up" [`Self::apply_project_config`]'s own doc comment flags:
    /// `[lints]`/`deny-warnings` previously had no override source at all
    /// (issue #1373). Call this *after* `apply_project_config`, if the
    /// caller applies both — an entry here replaces whatever the file set
    /// for the same code, and `deny_warnings: Some(_)` replaces the file's
    /// `deny-warnings` wholesale, mirroring `dialect`/`types`' own
    /// `*_overridden` handling above.
    ///
    /// Runs every code through the exact same [`validate_lint_code`] gate
    /// `apply_project_config`'s `[lints]` handling uses — a key that isn't a
    /// real [`DiagnosticCode`], or names a code whose *default* severity
    /// isn't `Warning`, is never merged into [`Self::lints`] and instead
    /// earns a returned [`ConfigWarning`] on the same "warn, never silently
    /// drop" channel (#1160's overridability constraint applies identically
    /// to a CLI/API-set code as to a `brink.toml`-set one).
    pub fn apply_lint_overrides(
        &mut self,
        overrides: &BTreeMap<String, LintLevel>,
        deny_warnings: Option<bool>,
    ) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();
        for (code, level) in overrides {
            match validate_lint_code(code) {
                Ok(()) => {
                    self.lints.overrides.insert(code.clone(), *level);
                }
                Err(warning) => warnings.push(warning),
            }
        }
        if let Some(deny_warnings) = deny_warnings {
            self.lints.deny_warnings = deny_warnings;
        }
        warnings
    }
}

/// Validate `code` against the real [`DiagnosticCode`] set (#1160's "resolve
/// a key against the real code set" channel, shared by
/// [`AnalysisOptions::apply_project_config`]'s `[lints]` handling and
/// [`AnalysisOptions::apply_lint_overrides`] — #1373): `Ok(())` if `code` is
/// overridable, otherwise the same-shaped [`ConfigWarning`] both call sites
/// surface, keeping the wording byte-identical regardless of which tier the
/// code came from.
///
/// Overridable means "not `Error`-by-default" — originally just the
/// `Warning`-base set (#1160's "conservative overridable set": a hard error
/// can never be downgraded by `[lints]`, so it is never even looked up).
/// Issue #1674 widens this past `Warning` to `Info`/`Hint`-base codes too
/// (today, only `E157`): the exemption `effective_severity` actually
/// enforces is about `Error`, never reachable through `[lints]` regardless of
/// what this function allows, not about `Warning` being the only overridable
/// base — see that function's own doc for the resolution order this mirrors.
fn validate_lint_code(code: &str) -> Result<(), ConfigWarning> {
    match DiagnosticCode::from_str_code(code) {
        Some(parsed) if parsed.severity() != Severity::Error => Ok(()),
        Some(_) => Err(ConfigWarning(format!(
            "[lints] `{code}` is not overridable (its default severity is `Error`); ignored"
        ))),
        None => Err(ConfigWarning(format!(
            "[lints] `{code}` is not a recognized diagnostic code; ignored"
        ))),
    }
}

/// The output of cross-file semantic analysis.
///
/// `PartialEq` supports early-cutoff backdating when the result is produced
/// by the salsa `analysis` query in `brink-db`.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisResult {
    /// The unified symbol index.
    pub index: Arc<SymbolIndex>,
    /// Resolved references: maps source range → definition id.
    pub resolutions: ResolutionMap,
    /// Diagnostics produced during analysis (duplicate definitions, unresolved refs, etc.).
    pub diagnostics: Vec<Diagnostic>,
    /// Per-symbol metadata enrichment (docs, resolved types, initializer
    /// values), keyed by `DefinitionId`. Empty when no host manifest is
    /// registered and no inline `///` docs are present.
    pub symbol_meta: BTreeMap<DefinitionId, SymbolMeta>,
}

/// Build the project-wide declaration index from per-file symbol manifests.
///
/// Query-shaped seam for the scripting substrate (spec §4, layer 2 —
/// `symbol_index()`): a pure function of the per-file manifests, returning
/// the merged index plus indexing diagnostics (duplicate definitions,
/// built-in shadowing). Declarations only — no body analysis happens here,
/// though the index does include body-declared locals (params/temps), which
/// hierarchical resolution needs.
#[must_use]
pub fn symbol_index(files: &[(FileId, &SymbolManifest)]) -> (Arc<SymbolIndex>, Vec<Diagnostic>) {
    let (index, diagnostics) = manifest::merge_manifests(files);
    (Arc::new(index), diagnostics)
}

/// The merged symbol index with `DefinitionId`s qualified by each file's
/// **declared** module (M-1, docs/modules-spec.md §5).
///
/// Identical to [`symbol_index`] for undeclared stem-modules (the entire
/// pre-modules corpus) — byte-identical `DefinitionId`s — and qualifies
/// names by module only for files carrying `#@module`. `brink-db`'s
/// `symbol_index_query` builds the [`ModuleMap`] from file stems,
/// `#@module` declarations, and the INCLUDE graph, then calls this.
///
/// `dialect` gates the M-2c cross-declared-module duplicate escalation
/// (issue #784): see [`manifest::merge_manifests_with_modules`].
///
/// `is_native` (issue #1562 review finding) widens that same gate past the
/// ink-only `dialect` axis for a native `.brink` project, exactly as
/// [`strict_diagnostics`]'s own `is_native` widens `E064`'s dialect check —
/// see [`manifest::merge_manifests_with_modules`]'s doc. Callers with no
/// `Language` classification of their own pass `false`, unchanged from
/// before this parameter existed.
#[must_use]
pub fn symbol_index_with_modules(
    files: &[(FileId, &SymbolManifest)],
    modules: &ModuleMap,
    dialect: Dialect,
    is_native: bool,
) -> (Arc<SymbolIndex>, Vec<Diagnostic>) {
    let (index, diagnostics) =
        manifest::merge_manifests_with_modules(files, modules, dialect, is_native);
    (Arc::new(index), diagnostics)
}

/// Resolve one file's references against the project-wide symbol index.
///
/// Query-shaped seam for the scripting substrate (spec §4, layer 2 —
/// `resolve(FileId)`): a pure function of the symbol index and this file's
/// own manifest. It never reads another file's content, so a body edit in
/// file B can only affect file A's resolutions by way of the shared index.
#[must_use]
pub fn resolve(
    file: FileId,
    manifest: &SymbolManifest,
    index: &SymbolIndex,
    scope: &ImportScope,
) -> (Arc<ResolutionMap>, Vec<Diagnostic>) {
    let (map, diagnostics) = resolve::resolve_file(index, scope, file, manifest);
    (Arc::new(map), diagnostics)
}

/// Run cross-file semantic analysis with default options (no host manifest).
///
/// Each entry is a `(FileId, HirFile, SymbolManifest)` tuple produced by
/// per-file HIR lowering.
pub fn analyze(files: &[(FileId, &HirFile, &SymbolManifest)]) -> AnalysisResult {
    analyze_with_options(files, &AnalysisOptions::default())
}

/// Run cross-file semantic analysis with explicit tooling options, including
/// an optional host-capability manifest and its external-check severity.
///
/// **Module-blind** (issue #1526): with no [`ModuleMap`] there is nothing to
/// qualify identity by, so every symbol hashes by bare name (`module: None`)
/// and the import scope is inert. That is byte-identical to `brink-db` for
/// the undeclared-stem-module world — the entire ink corpus that carries no
/// `#@module` — and *diverges* for any file whose real module is declared:
/// an ink file with `#@module(name)`, and **every** native `.brink` file
/// (whose module is its path, `story::…`, always declared — see
/// `brink_db::modules::native_module_path`). A `DefinitionId` minted here
/// for such a file does not match the one `brink-db`'s queries mint for the
/// same declaration, so it cannot be used as a key into `db.effects` /
/// `db.signature` / `db.infer_body`.
///
/// Callers that hold a `ProjectDb` — every IDE/LSP path — must use
/// [`analyze_with_modules`] with `ProjectDb::module_map()` instead.
pub fn analyze_with_options(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    opts: &AnalysisOptions,
) -> AnalysisResult {
    // No `Language` classification exists at this layer (issue #1348 /
    // #1562) — see `analyze_with_modules`'s own `is_native` doc.
    analyze_with_modules(files, &ModuleMap::new(), opts, false)
}

/// Run cross-file semantic analysis against the project's resolved
/// [`ModuleMap`] — the module-aware form of [`analyze_with_options`]
/// (issue #1526).
///
/// `modules` is the map `brink-db`'s `module_map_query` computes (file stems,
/// `#@module` declarations, the INCLUDE graph, and — for native `.brink`
/// files — the path-derived `story::…` identity). Feeding it here makes this
/// path mint the *same* `DefinitionId`s as `brink-db`'s
/// `symbol_index_query`/`resolve_query`, which is what lets an IDE feature
/// use a symbol from this result as a key into the db's per-def queries
/// (`effects`/`signature`/`infer_body`).
///
/// Module identity is never recomputed here: the map is an input, minted by
/// the one layer that knows file paths, so a native file's save-key-critical
/// identity stays a pure function of its path and cannot drift between the
/// two paths.
///
/// An empty map reproduces [`analyze_with_options`] exactly.
///
/// `is_native` declares that every file in `files` is native (`.brink`)
/// source, and selects the native arm of **every** analyzer pass this
/// function composes (issue #1358) — not just the symbol index it originally
/// reached (issue #1562):
///
/// - [`symbol_index_with_modules`] — M-2d cross-declared-module coexistence
///   stops depending on `opts.dialect` being `Dialect::Brink`.
/// - [`per_file_diagnostics`], via [`finish_analysis`] — the ink-only T1b
///   dialect gate (`E051`) is skipped, and the construction-literal checks
///   (`E084`/`E106`/`E138`) widen past the brink-only block.
/// - [`native_strict_only_error`] (`E137`), via [`finish_analysis`] — the
///   B0.9 strict-only gate, which has no meaning for ink source at all.
/// - [`strict_diagnostics`], via [`whole_project_diagnostics`] — the ink-only
///   `types = strict` config error (`E064`) is skipped (issue #1348).
///
/// This is the whole point of the flag: before #1358 it reached only the
/// first bullet, so a caller analyzing native source off-db still got the
/// ink arm of the per-file and whole-project passes — spurious `E051`/`E064`
/// on every editor surface, and `E137` unreachable there. `brink-db`'s
/// salsa queries have always selected these arms from their own
/// `Language` classification; this makes the pure path able to express the
/// same combination.
///
/// Callers that know the project's `Language` from a `ProjectDb`
/// (`brink-lsp`'s `analysis_loop` via `ProjectDb::is_native`, `IdeSession`
/// via `ProjectDb::is_all_native`) pass the real value; every other caller
/// passes `false`, unchanged from before this parameter existed —
/// [`analyze_with_options`] always does.
///
/// It is a whole-project flag, so `true` is only correct for a file set that
/// is *entirely* native; a mixed set must pass `false` (the analyzer has no
/// file paths and so cannot classify per file itself).
pub fn analyze_with_modules(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    modules: &ModuleMap,
    opts: &AnalysisOptions,
    is_native: bool,
) -> AnalysisResult {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> = files
        .iter()
        .map(|&(id, _hir, manifest)| (id, manifest))
        .collect();

    let (index, mut diagnostics) =
        symbol_index_with_modules(&manifest_inputs, modules, opts.dialect, is_native);
    let mut resolutions = ResolutionMap::new();
    for &(file_id, hir, manifest) in files {
        // Import-scoped resolution (M-2d, issue #790), matching
        // `brink-db`'s `resolve_query`: the resolved **declared** module
        // scopes the file's references. The map is authoritative when it
        // covers this file — notably for a native file, whose `hir.module`
        // carries a deliberately empty `name` (it exists only to hold the
        // authored `@[was]`; see `brink_ir::hir::lower_native::module`) and
        // would otherwise scope the file to the module named `""`.
        //
        // Falling back to the file's own HIR keeps the map-free
        // (`analyze_with_options`) path byte-identical to what it was before
        // this parameter existed.
        let declared_module = match modules.get(&file_id) {
            Some(resolved) => resolved.declared.then(|| resolved.name.clone()),
            None => hir.module.as_ref().map(|m| m.name.clone()),
        };
        let scope = ImportScope::new(declared_module, &hir.imports);
        let (file_map, file_diags) = resolve(file_id, manifest, &index, &scope);
        resolutions.extend(Arc::unwrap_or_clone(file_map));
        diagnostics.extend(file_diags);
    }

    finish_analysis(
        files,
        index,
        resolutions,
        diagnostics,
        opts,
        is_native,
        None,
    )
}

/// Per-file diagnostic contributors (issue #632 / FG-3,
/// `docs/fine-grained-salsa-proposal.md` §1 item 4): structural validation,
/// the dialect gate, and (brink dialect only) annotation-*content* checks —
/// the three passes `finish_analysis` used to run as whole-project loops
/// (`validate::validate`/`dialect_gate::check`/`annotations::check`, each
/// internally iterating every file) even though none of them actually reads
/// another file's state:
///
/// - [`validate::validate`] never reads cross-file state at all.
/// - [`dialect_gate::check`]'s only cross-file-shaped input, the resolution
///   map, is queried only for `(this file, range)` pairs — a reference's
///   resolution record always carries the file the reference itself lives
///   in, never another file's — so `file_resolutions` need only be this
///   file's own slice.
/// - [`annotations::check`]'s cross-file inputs are the project's declared
///   `LIST`/`STRUCT` names (derivable from a range-free index projection —
///   `declared_list_names`/`declared_struct_names` read no symbol's range)
///   and, for `Handle<K>` (T1d-2, docs/t1d-spec.md §3), the registered host
///   manifest — project-wide, host-set config, not file-edit-derived, so
///   reading it here is the same coarse dependency shape `dialect` already
///   is, not a reintroduction of whole-project churn.
///
/// This is the query-shaped seam `brink-db`'s `per_file_diagnostics_query`
/// wraps: a body edit in file Y leaves file X's per-file contributor memo
/// untouched (pinned by `fg3_dependency_edges.rs`).
///
/// `is_native`: the T1b dialect gate (`dialect_gate::check`, issue #1348) is
/// an ink-only axis — a native `.brink` file has no "dialect" concept at all
/// (its own grammar *is* the superset grammar the gate exists to police, so
/// every construct it recognizes is ordinary native syntax, never "brink
/// extension" syntax to reject). `true` skips the gate entirely, regardless
/// of `dialect`'s value; every other per-file contributor is unaffected —
/// this caller-supplied flag never widens what `per_file_diagnostics` itself
/// needs to know (it stays as agnostic to `Language` as `dialect` already
/// was), it only tells this one contributor whether it applies. Callers with
/// no `Language` classification of their own (the pure `analyze_with_options`
/// path, via [`finish_analysis`]) always pass `false`, unchanged from before
/// this parameter existed.
#[must_use]
pub fn per_file_diagnostics(
    file: FileId,
    hir: &HirFile,
    file_resolutions: &ResolutionMap,
    index: &SymbolIndex,
    dialect: Dialect,
    is_native: bool,
    host_manifest: Option<&HostManifest>,
) -> Vec<Diagnostic> {
    let files = [(file, hir)];
    let mut out = validate::validate(&files);
    if !is_native {
        out.extend(dialect_gate::check(&files, file_resolutions, dialect));
    }
    // NS-A1 E107 (bare-`none`-needs-context, docs/stdlib-spec.md §1.4) —
    // dialect-INDEPENDENT, unlike the brink-only block below: the rule is
    // part of the Option package itself, and under `strict-ink` (where
    // `VAR`/`CONST` initializers aren't in the gate's block-tree walk) it
    // is also what keeps `VAR x = none` an error at all. Same per-file
    // argument as `dialect_gate`: the resolution records consulted always
    // carry this file's own id.
    out.extend(option_rules::check(&files, file_resolutions));
    // Annotation *content* checks (E061) run only under the brink
    // dialect: under `strict-ink` the annotation is already rejected whole
    // by `dialect_gate` (E051), and critiquing the inside of rejected
    // syntax is noise (maintainer ruling 2026-07-13).
    if dialect == Dialect::Brink {
        out.extend(annotations::check(&files, index, host_manifest));
        // T1c `#fn` creation-site checks (E079/E080/E081) follow the same
        // brink-only rule: under `strict-ink` the literal is already
        // rejected whole (E051). Per-file by the same argument as
        // `dialect_gate`: the resolution records consulted always carry
        // this file's own id.
        out.extend(fn_values::check(&files, file_resolutions, index));
        // T1e-1 `ref lvalue-path` creation-site checks (E080 durable root,
        // E097 standalone position, docs/t1e-spec.md §2/§6, issue #831) —
        // same brink-only rule, same per-file argument as `fn_values`'s own
        // comment just above (a reference's resolution record always
        // carries the file the reference itself lives in).
        out.extend(ref_projection::check(&files, file_resolutions, index));
        // NS-A3 protocol-registry name reservation (E113, F6 ruled
        // 2026-07-19, docs/stdlib-spec.md §9.6): `display`/`compare`/`next`
        // are reserved method names under the brink dialect — an author
        // declaration is a hard error, not an E035 warning. Brink-only:
        // under strict-ink there is no protocol registry and vanilla ink
        // identifiers stay untouched (the oracle corpus is out of reach by
        // construction).
        out.extend(protocols::check_reserved_names(&files));
    }
    // The three construction-literal checks below are wired WIDER than the
    // brink-only block above on purpose (B5, issue #1464, #1103 cascade
    // ruling (A), docs/stdlib-spec.md §9.6): `TypeName { … }` construction
    // reaches `StructLiteral`/`MapLiteral` through the native surface
    // (`Map { k: v }`, `Point { x: 1 }`) regardless of the (ink-only)
    // `dialect` axis a native project happens to carry — a `.brink` file
    // compiled under the default `strict-ink` dialect must still get these
    // errors. Under `strict-ink` *ink* the literal sigils (`#{…}`) are
    // already rejected whole by `dialect_gate` (E051), so nothing new fires
    // there.
    if dialect == Dialect::Brink || is_native {
        // Struct construction-literal duplicate-field check (E084, issue
        // #675) — unlike `structs::check`'s missing/extra/mistyped trio
        // this runs under *both* `types` policies (see `structs`' module
        // doc): a repeated field name is a structural mistake detectable
        // from the literal alone, with no shape resolution or
        // whole-project inference needed.
        out.extend(structs::check_duplicates(&files));
        // Map-literal key-domain warning (E106, issue #598,
        // docs/t1b-surface-spec.md §3) — same policy-independence
        // `structs::check_duplicates` documents: a statically-visible
        // non-key-domain literal key is a structural authoring mistake
        // detectable from the literal alone, no shape resolution or
        // whole-project inference needed.
        out.extend(map_keys::check(&files));
        // Map-literal duplicate-key error (E138, B5 issue #1464, #1103
        // cascade ruling (A)).
        out.extend(map_keys::check_duplicate_keys(&files));
    }
    // Native bare-name fn values (issue #1862): the `.brink` half of the
    // T1c creation-site discipline. Keyed off `is_native` alone rather than
    // the block above's `dialect == Brink || is_native`, because the rule
    // it enforces only exists on the native surface — see
    // [`fn_values::check_native_bare_refs`]'s own doc. (`check` above stays
    // where it is: `#fn` is the brink-*dialect* spelling and is not
    // reachable from `.brink` source at all.)
    if is_native {
        out.extend(fn_values::check_native_bare_refs(
            &files,
            file_resolutions,
            index,
        ));
    }
    // Inline-markup vocabulary checks (E164/E165, issue #1733,
    // docs/prose-dialect-spec.md §4.2). Wired *outside* every dialect
    // branch above on purpose: markup spans are a native-grammar
    // construct, so the ink-only `dialect` axis has nothing to say about
    // them, and the pass is inert for ink source by construction (no
    // `ContentPart::Span` can exist there). Inert for native source too
    // unless the host manifest actually declares a markup vocabulary —
    // freeform is the default (§4.2), and `markup_check::check` returns
    // before touching the HIR when nothing is declared.
    out.extend(markup_check::check(&[(file, hir)], host_manifest));
    out
}

/// Collect inline `///` docs across all files, keyed by `(kind, declared
/// name)` — the project-wide doc merge feeding the external/callable/value
/// enrichment passes. Exposed as its own seam (issue #750 / FG-3
/// completion) so `brink-db` can memoize it behind an `Eq`-cutoff query:
/// [`DocBlock`] carries no ranges, so any edit that leaves every `///`
/// block's parsed content intact backdates the memo even though the pass
/// reads every file's manifest.
#[must_use]
pub fn project_inline_docs(
    files: &[(FileId, &SymbolManifest)],
) -> BTreeMap<(SymbolKind, String), DocBlock> {
    collect_inline_docs(files)
}

/// The index-driven half of the external-check family (issue #750 / FG-3
/// completion): host-manifest enrichment + checks for `EXTERNAL`s
/// ([`external_check::analyze_externals`] — arity `E039`, unknown semantic
/// types `E040`) followed by knot/stitch doc enrichment
/// ([`external_check::enrich_callables`], same `E040` vocabulary), in
/// exactly that order for both the diagnostics and the `symbol_meta`
/// merge. Reads the index and the merged inline docs only — never any
/// file's HIR — which is what lets `brink-db` memoize it separately from
/// the per-file HIR walks ([`file_value_meta`] /
/// [`file_call_site_diagnostics`]).
#[must_use]
pub fn external_meta_diagnostics(
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
    opts: &AnalysisOptions,
) -> (BTreeMap<DefinitionId, SymbolMeta>, Vec<Diagnostic>) {
    let (types, registered) = manifest_maps(opts.host_manifest.as_ref());
    let has_manifest = opts.host_manifest.is_some();
    // Unknown-semantic-type checking (`E040`) is on when a manifest is
    // registered, or when the severity lever is explicitly raised to `Error`
    // (#532) — a host can opt back into strict checking with no manifest.
    let check_unknown_types =
        has_manifest || opts.semantic_type_check == SemanticTypeDiagnosticSeverity::Error;
    let (mut symbol_meta, mut diagnostics) = external_check::analyze_externals(
        index,
        inline_docs,
        &types,
        &registered,
        opts.external_check,
        check_unknown_types,
    );

    // Knot/stitch doc enrichment (presentational; shares the semantic-type
    // vocabulary, so unknown types still diagnose — but only once a manifest
    // is registered, or the severity lever is raised (#339/#532); see
    // `resolve_type`).
    let (callable_meta, callable_diags) = external_check::enrich_callables(
        index,
        inline_docs,
        &types,
        opts.external_check,
        check_unknown_types,
    );
    diagnostics.extend(callable_diags);
    symbol_meta.extend(callable_meta);

    (symbol_meta, diagnostics)
}

/// Project the external-kind entries of an enrichment map to a name-keyed
/// map for the call-site checks (issue #750 / FG-3 completion). Range-free
/// by construction ([`SymbolMeta`] carries no spans), so `brink-db` can put
/// an `Eq`-cutoff memo between the (often-invalidated, full-ranged-index-
/// reading) enrichment pass and every file's call-site walk — the
/// `resolution_index` playbook.
///
/// Fed [`external_meta_diagnostics`]'s output, this is identical to the
/// pre-split filter over the *fully merged* `symbol_meta`: the callable
/// ([`external_check::enrich_callables`]) and value
/// ([`external_check::infer_value_meta`]) passes only ever key
/// `Knot`/`Stitch` and `Variable`/`Constant`/`List` ids respectively, so no
/// entry they add can pass the `SymbolKind::External` filter here.
/// Same-name duplicates resolve identically too: iteration is in
/// `DefinitionId` order in both shapes, later entries overwriting.
#[must_use]
pub fn call_site_metas(
    index: &SymbolIndex,
    metas: &BTreeMap<DefinitionId, SymbolMeta>,
) -> BTreeMap<String, SymbolMeta> {
    metas
        .iter()
        .filter_map(|(id, meta)| {
            index.symbols.get(id).and_then(|s| {
                (s.kind == SymbolKind::External).then(|| (s.name.clone(), meta.clone()))
            })
        })
        .collect()
}

/// One file's VAR/CONST/LIST initializer/doc enrichment (issue #750 / FG-3
/// completion — the per-file slice of [`external_check::infer_value_meta`],
/// which `whole_project_diagnostics` used to run as one loop over every
/// file's HIR). Purely presentational — never produces diagnostics. A
/// declaration's initializer lives in exactly one file, so the per-file
/// split is behavior-neutral: the whole-project result is the file-order
/// merge of the per-file maps (later files overwrite on the — deliberately
/// deterministic — duplicate-name id collision, exactly as the single loop
/// did). Reads no symbol ranges from `index` (only `by_name` + `kind`), so
/// a range-zeroed index projection serves it.
#[must_use]
pub fn file_value_meta(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> BTreeMap<DefinitionId, SymbolMeta> {
    external_check::infer_value_meta(&[(file, hir)], index, inline_docs)
}

/// One file's external call-site literal checks (`E041` type mismatch,
/// `E042` closed domain) — the per-file slice of
/// [`external_check::check_call_sites`] (issue #750 / FG-3 completion).
/// The checker only ever reads the file it is visiting plus the name-keyed
/// external metas, so the per-file split is behavior-neutral; the caller
/// owns both the [`ExternalCheckSeverity`] gate and the file-order
/// concatenation the single whole-project walk produced.
#[must_use]
pub fn file_call_site_diagnostics(
    file: FileId,
    hir: &HirFile,
    metas: &BTreeMap<String, SymbolMeta>,
) -> Vec<Diagnostic> {
    let name_to_meta: BTreeMap<&str, &SymbolMeta> = metas
        .iter()
        .map(|(name, meta)| (name.as_str(), meta))
        .collect();
    external_check::check_call_sites(&[(file, hir)], &name_to_meta)
}

/// The M-2 module import + visibility checks (docs/modules-spec.md
/// §2/§4/§7): import well-formedness and cross-module `#@private`
/// reference enforcement. Purely additive — every trigger needs an
/// `IMPORT`/`#@private`/`#@public` construct absent from the pre-modules
/// world, so the oracle/tier1 corpus is untouched. Genuinely whole-project
/// (reads every file's HIR plus the project-wide resolutions to walk
/// cross-module references), so it stays a whole-project pass in
/// `brink-db`'s decomposed `whole_project_diagnostics_query` rather than
/// gaining a per-file split here (issue #750 / FG-3 completion rebase note;
/// a per-file slice is possible FG-4-era work if module churn is ever hot).
#[must_use]
pub fn module_diagnostics(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    modules::check(files, index, resolutions)
}

/// The strict typed-mode pass (docs/typed-mode-spec.md §1/§9-step-3),
/// extracted from `whole_project_diagnostics`'s body (issue #750 / FG-3
/// completion) so `brink-db` can run it without also paying for the
/// external-check family's inputs. Returns empty under `types = gradual` —
/// byte-identical, forever.
///
/// `types = strict` requires `dialect = brink` — a config error (`E064`)
/// otherwise, reported alone (nothing else strict-specific runs against a
/// project whose dialect already rejects the annotation syntax strict mode
/// needs). Under `dialect = brink`, runs inference (reusing
/// `strict_inference` when the caller already computed one — see
/// [`whole_project_diagnostics`]'s doc) and wires in Unknown/Conflicted-
/// escape (`E065`/`E066`) plus `E063` mismatches.
///
/// `is_native` (issue #1348): `E064` is [`strict::config_error`]'s dialect
/// check, and `dialect` is an ink-only axis — a native `.brink` project has
/// no dialect to be wrong about, so `true` skips the `config_error` call
/// entirely and always proceeds straight to the inference-driven checks
/// below (never a config error for native, regardless of `opts.dialect`).
/// Same "caller-supplied, never widens this function's own knowledge" shape
/// as [`per_file_diagnostics`]'s own `is_native` — the pure path (via
/// [`whole_project_diagnostics`]) always passes `false`.
///
/// `inline_docs` (issue #805): forwarded to [`infer::infer_project`]'s own
/// `EXTERNAL`-signature seeding when `strict_inference` isn't already
/// supplied — the pure/self-contained fallback path only; `brink-db`'s
/// production seam always supplies `strict_inference` (its FG-narrowed
/// `type_inference_query`, which reads `inline_docs_query` itself through
/// `solve_scc_query`), so this parameter is inert there. Kept required
/// (rather than defaulted away) so the pure path stays composed-equals-
/// monolithic with the salsa one for every caller, not just the memoized
/// production one.
#[must_use]
pub fn strict_diagnostics(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    opts: &AnalysisOptions,
    is_native: bool,
    strict_inference: Option<&InferenceResult>,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if opts.type_policy() == TypePolicy::Strict {
        let config_err = if is_native {
            None
        } else {
            strict::config_error(opts.dialect, files.first().map(|&(f, _)| f))
        };
        if let Some(diag) = config_err {
            diagnostics.push(diag);
        } else {
            let owned_inference;
            let inference = if let Some(inf) = strict_inference {
                inf
            } else {
                owned_inference = infer::infer_project(
                    files,
                    index,
                    resolutions,
                    opts.host_manifest.as_ref(),
                    inline_docs,
                );
                &owned_inference
            };
            diagnostics.extend(strict::check(
                files,
                index,
                inference,
                resolutions,
                opts.host_manifest.as_ref(),
            ));
            // Issue #1004: escape-check each registered `EXTERNAL`
            // declaration's own param types against the manifest/inline-doc
            // signatures. `strict::check` above only walks `hir.knots`, so a
            // manifest-typed external param would otherwise never be verified
            // — resolved types stay clean, an unresolvable `ManifestParam.ty`
            // reports `E065` at the external's own declaration span. Seeded
            // from the same `collect_external_sigs` resolution that feeds
            // call-site argument checking; runs on this shared
            // `strict_diagnostics` seam so the pure `analyze_with_options`
            // path and `brink-db`'s query path get byte-identical output.
            let external_sigs =
                infer::collect_external_sigs(index, opts.host_manifest.as_ref(), inline_docs);
            diagnostics.extend(strict::check_external_escapes(index, &external_sigs));
        }
    }
    diagnostics
}

/// Whole-project diagnostic contributors that genuinely need cross-file
/// state (issue #632 / FG-3 design doc §1), now composed of the same
/// per-pass seams `brink-db`'s decomposed queries wrap (issue #750 / FG-3
/// completion) — [`module_diagnostics`], [`strict_diagnostics`],
/// [`external_meta_diagnostics`], per-file [`file_value_meta`], and per-file
/// [`file_call_site_diagnostics`] behind [`call_site_metas`] — in exactly
/// the pre-split order, so the query-composed result is identical to this
/// monolithic one by construction (pinned by `query_equivalence.rs`).
///
/// `strict_inference`: TM-3's strict pass needs a whole-project
/// [`InferenceResult`] (docs/typed-mode-spec.md §9-step-3 — E063 auto-wiring
/// "must run inference anyway"). Pass `None` to have this function compute
/// its own via [`infer_project`] (the self-contained default —
/// [`analyze_with_options`]'s pure, non-salsa path). Pass `Some` to reuse an
/// already-computed result instead — `brink-db` supplies its FG-narrowed,
/// per-SCC-memoized `type_inference_query` here so strict mode's
/// warm-reanalyze cost is the incremental one the FG spine exists for, not a
/// from-scratch whole-project solve on every keystroke. Ignored entirely
/// under `types = gradual` or when the dialect makes strict mode a config
/// error. The `types = strict` + wrong-dialect config error (`E064`) is
/// computed exactly once, inside [`strict_diagnostics`] (issue #632's
/// TM-3-interaction fence).
///
/// `is_native` (issue #1358): every file is native (`.brink`) source, so the
/// ink-only `E064` config error is skipped — forwarded verbatim to
/// [`strict_diagnostics`], whose own `is_native` doc has the reasoning
/// (issue #1348). `brink-db`'s `whole_project_diagnostics_query` passes its
/// own `project_is_native` answer at the same seam, but that answer is
/// entry-anchored — it reads `false` whenever the db has no entry set — so
/// it does not automatically agree with a caller-computed `is_native` for a
/// db that never calls `set_entry` (e.g. `IdeSession`'s editor/LSP analysis
/// path, as opposed to `IdeSession::compile`). Callers of this function are
/// responsible for supplying an `is_native` that actually matches their file
/// set.
#[must_use]
pub fn whole_project_diagnostics(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    opts: &AnalysisOptions,
    is_native: bool,
    strict_inference: Option<&InferenceResult>,
) -> (Vec<Diagnostic>, BTreeMap<DefinitionId, SymbolMeta>) {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> = files
        .iter()
        .map(|&(id, _hir, manifest)| (id, manifest))
        .collect();
    let hir_inputs: Vec<(FileId, &HirFile)> = files.iter().map(|&(id, hir, _)| (id, hir)).collect();

    // Computed once, up front (moved ahead of `strict_diagnostics`, issue
    // #805): both the TM-3 strict pass's `EXTERNAL`-signature seeding and
    // the host-manifest enrichment pass below need the project-wide merged
    // `///` doc map.
    let inline_docs = collect_inline_docs(&manifest_inputs);

    // M-2 module import + visibility checks (docs/modules-spec.md
    // §2/§4/§7), first in diagnostic order.
    let mut diagnostics = module_diagnostics(&hir_inputs, index, resolutions);

    // TM-3 strict typed-mode policy. Gradual mode returns empty here —
    // byte-identical, forever.
    diagnostics.extend(strict_diagnostics(
        &hir_inputs,
        index,
        resolutions,
        opts,
        is_native,
        strict_inference,
        &inline_docs,
    ));

    // Host-manifest enrichment + checks (tooling/author-time only) — the
    // index-driven half: externals (E039/E040), then callables.
    let (mut symbol_meta, ext_diags) = external_meta_diagnostics(index, &inline_docs, opts);
    diagnostics.extend(ext_diags);

    // Name-keyed external metas for the call-site checks — built before the
    // value-meta merge, which is identical to the pre-split post-merge
    // filter (see `call_site_metas`'s doc for the argument).
    let cs_metas = call_site_metas(index, &symbol_meta);

    // VAR/CONST initializer info + LIST docs (presentational, no
    // diagnostics), merged in file order.
    for &(file_id, hir, _) in files {
        symbol_meta.extend(file_value_meta(file_id, hir, index, &inline_docs));
    }

    // Call-site literal checks (type mismatch, closed domain) over the HIR,
    // in file order. Externals only — knot/stitch metadata is
    // presentational, not binding.
    if opts.external_check != ExternalCheckSeverity::Off {
        for &(file_id, hir, _) in files {
            diagnostics.extend(file_call_site_diagnostics(file_id, hir, &cs_metas));
        }
    }

    // T2-2 `#@effects(…)` exceedance check (docs/effects-spec.md §10, issue
    // #861) — brink-only, same TM-2 "content checks skip strict-ink"
    // precedent `per_file_diagnostics` documents (the directive is already
    // rejected whole by `dialect_gate`'s `E051` under strict-ink). Only pays
    // for `effects_project`'s whole-project inference when at least one
    // assertion actually exists anywhere in the project — an unannotated
    // project stays effects-inference-free, matching T2-1's advisory-only
    // posture.
    //
    // The FS-2 `await`-condition purity gate (E105,
    // docs/flow-suspension-spec.md §3/§5, issue #928) rides the same
    // whole-project effect table and the same brink-only + laziness posture:
    // it needs `effects_project`'s rows to judge a condition's transitive
    // effect, so both passes share one inference when *either* an `#@effects`
    // assertion or an `await` appears anywhere in the project.
    // The NS-A4 comparator-contract gate (E119, docs/stdlib-spec.md §4b,
    // issue #1110 — extended to the fn-value verb trio `map`/`filter`/
    // `fold` by issue #1679, §4) rides the same whole-project effect table
    // with the same brink-only + laziness posture: a project with no
    // `sort_by`/`sorted_by`/`map`/`filter`/`fold`-with-inline-`#fn` site
    // never triggers effect inference for it.
    let needs_effects = hir_inputs.iter().any(|&(_, hir)| {
        hir_has_effects_assertion(hir)
            || await_purity::hir_has_await(hir)
            || comparator_contract::hir_has_comparator_site(hir)
    });
    if opts.dialect == Dialect::Brink && needs_effects {
        let rows =
            infer::effects_project(&hir_inputs, index, resolutions, opts.host_manifest.as_ref());
        for &(file_id, hir) in &hir_inputs {
            // Import-scoped resolution (issue #881, the T2 follow-up to
            // M-2d/#790): the assertion's own `reads`/`writes`/`calls` clause
            // names must resolve through this file's own declared module +
            // imports, exactly like every other reference does — see
            // `effects_assertions::check`'s doc.
            let scope = ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
            diagnostics.extend(effects_assertions::check(
                file_id, hir, index, &scope, &rows,
            ));
            // The `await` purity gate resolves each condition's calls through
            // this file's own resolution records (`resolutions` carries file
            // provenance, filtered inside `await_purity::check`).
            diagnostics.extend(await_purity::check(file_id, hir, index, resolutions, &rows));
            // The NS-A4 comparator-contract gate (E119) — same resolution
            // discipline, judging inline `#fn(target)` comparators of
            // `sort_by`/`sorted_by` and the fn-value verb trio's callbacks
            // (`map`/`filter`/`fold`, issue #1679) against their target's
            // row.
            diagnostics.extend(comparator_contract::check(
                file_id,
                hir,
                index,
                resolutions,
                &rows,
            ));
        }
    }

    // B3a UFCS resolution (issue #1482, D1–D5 RULED 2026-07-26). Only the
    // diagnostics land here; the verdict side table itself is served to LIR
    // lowering and the IDE through [`ufcs_resolution`], which runs the same
    // pass over the same inputs.
    //
    // Dialect-independent, for the reason `ufcs`' module doc gives: a
    // multi-segment `Expr::Call` path can only originate in the native
    // frontend, so the gate is structural rather than policy-driven — the
    // ink corpus never reaches this pass. Lazy on the same argument as
    // `needs_effects` above: a project with no dotted-callee call anywhere
    // pays nothing.
    if hir_inputs
        .iter()
        .any(|&(_, hir)| ufcs::project_has_ufcs_call(hir))
    {
        let owned_inference;
        let inference = if let Some(inf) = strict_inference {
            inf
        } else {
            owned_inference = infer::infer_project(
                &hir_inputs,
                index,
                resolutions,
                opts.host_manifest.as_ref(),
                &inline_docs,
            );
            &owned_inference
        };
        let (_table, ufcs_diags) = ufcs::resolve(&hir_inputs, index, resolutions, inference);
        diagnostics.extend(ufcs_diags);
    }

    (diagnostics, symbol_meta)
}

/// The B3a UFCS verdict side table for a project (issue #1482, D2): the
/// `node → resolved target` channel LIR lowering reads to choose between
/// emitting a call through a field's value and emitting the desugared free
/// call `name(recv, args)`, and that IDE hover/go-to-def reads to name the
/// real target of a method-call-shaped site.
///
/// Split out from [`whole_project_diagnostics`] — which keeps the same
/// pass's *diagnostics* — because the two consumers want opposite halves of
/// one result and neither should pay for the other's.
#[must_use]
pub fn ufcs_resolution(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    inference: &InferenceResult,
) -> (UfcsTable, Vec<Diagnostic>) {
    ufcs::resolve(files, index, resolutions, inference)
}

/// The B1 `or`-coalescing typing side table for a project (issue #1492):
/// the `chain root → per-step operand/result types` channel LIR lowering
/// reads to choose a chain's code shape — "inner stays `Option`" vs
/// "unwrap at the end" — instead of re-deriving the answer from syntax it
/// cannot see through (a call's return type, a `VAR`'s declared type).
///
/// Keyed by [`brink_ir::hir::expr_span`] of the chain root, the derivation
/// both sides share — since issue #1517, the root `Expr::Infix`'s own
/// `Provenance` range, so every chain root in a file is separately
/// addressable. See [`CoalesceChain`] for the step order and `coalesce`'s
/// module doc for why absence (an ill-typed chain the pass abandoned) is
/// always safe — the consumer falls back to the runtime check, which is
/// what gradual mode does regardless.
///
/// Split out from [`whole_project_diagnostics`] — which keeps the same
/// pass's `E066` *diagnostics* — exactly as [`ufcs_resolution`] is, and for
/// the same reason: the two consumers want opposite halves of one result.
///
/// Unlike [`ufcs_resolution`] (whose diagnostics run unconditionally inside
/// [`whole_project_diagnostics`]), the `E066` diagnostics this function
/// also returns are **strict-mode-only by convention, not by construction**:
/// production code reaches them only from `strict::check`, after
/// `strict::config_error` has confirmed `types = strict` + `dialect =
/// brink` (see `coalesce::resolve`'s own doc for that entry condition), but
/// this function itself performs no such gate — it walks every file
/// unconditionally. A caller that surfaces its `Vec<Diagnostic>` without
/// re-checking `type_policy`/`dialect` itself would emit strict-only
/// `E066` under `types = gradual`.
#[must_use]
pub fn coalesce_types(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> (CoalesceTable, Vec<Diagnostic>) {
    coalesce::resolve(files, index, inference, resolutions)
}

/// Owned form of [`brink_ir::lir::AnalyzerTables`] (issue #1527) — every
/// analyzer side-table LIR lowering reads, held by value instead of by the
/// borrowed references `AnalyzerTables` itself carries. A caller builds one
/// of these (via [`assemble_analyzer_tables`]) and then borrows its fields
/// into an `AnalyzerTables` at the lowering call site, exactly as
/// `brink-db`'s two salsa queries already borrow their own owned
/// `UfcsLookup`/`CoalesceLookup` locals.
#[derive(Debug, Clone, Default)]
pub struct AnalyzerTablesOwned {
    /// B3a UFCS (issue #1506) — see [`brink_ir::lir::UfcsLookup`]'s own doc.
    pub ufcs: brink_ir::lir::UfcsLookup,
    /// B1 `or`-coalescing (issue #1492) — see [`brink_ir::lir::CoalesceLookup`]'s own doc.
    pub coalesce: brink_ir::lir::CoalesceLookup,
}

impl AnalyzerTablesOwned {
    /// Borrow this owned bundle into the [`brink_ir::lir::AnalyzerTables`]
    /// lowering actually takes — the one place that borrow is assembled
    /// (issue #1528's review finding). Field-by-field construction at each
    /// call site meant a third `AnalyzerTables` field would compile-error at
    /// the call site instead of here, and the cheapest silencer there is a
    /// throwaway default value rather than actually wiring the new table —
    /// exactly the silent-empty-table failure this whole function exists to
    /// prevent. Keeping the borrow here means a new field's compile error
    /// lands next to this assembly instead.
    #[must_use]
    pub fn as_tables(&self) -> brink_ir::lir::AnalyzerTables<'_> {
        brink_ir::lir::AnalyzerTables {
            ufcs: &self.ufcs,
            coalesce: &self.coalesce,
        }
    }
}

/// Assemble every analyzer side-table LIR lowering needs, from scratch, in
/// one whole-project pass — **the one path a caller with no salsa layer of
/// its own must use** (issue #1528).
///
/// Before this function existed, `brink-test-harness`'s `corpus.rs` hand-
/// rolled this assembly itself: one `if project_has_*` block per table,
/// each independently re-running [`infer_project`] — a *third* parallel
/// implementation of the same gate-then-translate pattern `brink-db`'s two
/// salsa queries (`ufcs_resolution_query`, `coalesce_types_query`) already
/// each implement for their own table. That meant a future side-table (the
/// v6/Step work) had to be *remembered* in three places at once — miss the
/// harness's copy and lowering there silently got an empty table for it: a
/// compiling, green-tested, wrong-coverage bug, the same silent-drop class
/// this repo always treats as a bug. Extending *this* function is the fix
/// for every salsa-free caller: it is the one place such a caller's
/// gate+translate needs adding, mirroring how
/// [`brink_ir::lir::AnalyzerTables`] (issue #1527) is the one place a
/// future table needs adding to lowering's own signature. `brink-db`'s two
/// queries stay separate `#[salsa::tracked]` functions on purpose — each
/// needs its own independent memoization/backdating cutoff, which a single
/// bundled query would collapse — but both continue to call the exact same
/// translation primitives this function composes
/// ([`ufcs_resolution`]/[`coalesce_types`]/[`ufcs_lir_lookup`]/
/// [`coalesce_lir_lookup`]), so the two paths can't drift on *how* a table
/// is computed, only on *when* (memoized vs. every call).
///
/// Lazy exactly like each table already was individually: [`infer_project`]
/// runs at most once — shared across every table that needs it, unlike the
/// old per-table harness blocks which each ran their own copy — and only if
/// some table's structural gate ([`project_has_ufcs_call`] or
/// [`project_has_coalesce`]) found something to resolve. A project using
/// neither feature (every ink-dialect project, by construction — both
/// features are native-frontend-only) pays nothing and returns the
/// all-empty default.
#[must_use]
pub fn assemble_analyzer_tables(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    host_manifest: Option<&HostManifest>,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> AnalyzerTablesOwned {
    let needs_ufcs = files
        .iter()
        .any(|&(_, hir)| ufcs::project_has_ufcs_call(hir));
    let needs_coalesce = files
        .iter()
        .any(|&(_, hir)| coalesce::project_has_coalesce(hir));

    let inference = if needs_ufcs || needs_coalesce {
        Some(infer::infer_project(
            files,
            index,
            resolutions,
            host_manifest,
            inline_docs,
        ))
    } else {
        None
    };

    let ufcs = match (&inference, needs_ufcs) {
        (Some(inference), true) => {
            let (table, _ufcs_diagnostics) = ufcs_resolution(files, index, resolutions, inference);
            ufcs_lir_lookup(&table)
        }
        _ => brink_ir::lir::UfcsLookup::new(),
    };

    let coalesce = match (&inference, needs_coalesce) {
        (Some(inference), true) => {
            let (table, _e066_diagnostics) = coalesce_types(files, index, inference, resolutions);
            coalesce_lir_lookup(&table)
        }
        _ => brink_ir::lir::CoalesceLookup::new(),
    };

    AnalyzerTablesOwned { ufcs, coalesce }
}

/// Cheap structural scan: does any knot/stitch in `hir` carry a
/// `#@effects(…)` assertion? The laziness gate for
/// [`whole_project_diagnostics`]'s exceedance pass — avoids running
/// [`infer::effects_project`] at all for a project that never uses the
/// directive.
fn hir_has_effects_assertion(hir: &HirFile) -> bool {
    hir.knots.iter().any(|k| {
        k.effects_assertion.is_some() || k.stitches.iter().any(|s| s.effects_assertion.is_some())
    })
}

/// Assemble the final [`AnalysisResult`] from the already-computed layer-2
/// pieces (index + per-file resolutions), running the remaining passes:
/// per-file diagnostic contributors ([`per_file_diagnostics`]) for every
/// file, then the whole-project contributors
/// ([`whole_project_diagnostics`]).
///
/// Query-shaped seam for the scripting substrate: `brink-db`'s salsa
/// `analysis_query` composes [`symbol_index`] and per-file [`resolve`]
/// queries, then the decomposed per-file/whole-project queries this
/// function's two halves wrap (issue #632 / FG-3) — the same sequence this
/// function runs, in the same order, so the query-composed result is
/// identical to the monolithic one by construction (pinned by
/// `query_equivalence.rs`).
///
/// `is_native`: every file in `files` is native (`.brink`) source — see
/// [`analyze_with_modules`]'s own `is_native` doc for the full list of arms
/// it selects (issue #1358). Forwarded to [`per_file_diagnostics`] and
/// [`whole_project_diagnostics`], and it is what makes the B0.9 strict-only
/// gate ([`native_strict_only_error`], `E137`) reachable from this path at
/// all. The analyzer has no file paths of its own, so this is a caller-
/// supplied classification: a caller with a `ProjectDb` reads it from there,
/// and one without passes `false` (the ink arm, byte-identical to this
/// function before the parameter existed).
///
/// `strict_inference`: see [`whole_project_diagnostics`]'s doc — forwarded
/// unchanged.
pub fn finish_analysis(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    index: Arc<SymbolIndex>,
    resolutions: ResolutionMap,
    mut diagnostics: Vec<Diagnostic>,
    opts: &AnalysisOptions,
    is_native: bool,
    strict_inference: Option<&infer::InferenceResult>,
) -> AnalysisResult {
    for &(file_id, hir, _manifest) in files {
        let file_resolutions: ResolutionMap = resolutions
            .iter()
            .filter(|r| r.file == file_id)
            .cloned()
            .collect();
        diagnostics.extend(per_file_diagnostics(
            file_id,
            hir,
            &file_resolutions,
            &index,
            opts.dialect,
            is_native,
            opts.host_manifest.as_ref(),
        ));
        if is_native {
            // The B0.9 native strict-only gate, in the same per-file
            // position `brink-db`'s `per_file_diagnostics_query` runs it
            // (right after the per-file contributors for that file), so the
            // composed and monolithic paths stay order-identical.
            diagnostics.extend(native_strict_only_error(file_id, opts.types));
        }
    }

    let (whole_diagnostics, symbol_meta) = whole_project_diagnostics(
        files,
        &index,
        &resolutions,
        opts,
        is_native,
        strict_inference,
    );
    diagnostics.extend(whole_diagnostics);

    AnalysisResult {
        index,
        resolutions,
        diagnostics,
        symbol_meta,
    }
}

/// Collect inline `///` docs across all files, keyed by `(kind, declared name)`.
fn collect_inline_docs(
    files: &[(FileId, &SymbolManifest)],
) -> BTreeMap<(SymbolKind, String), DocBlock> {
    let mut out = BTreeMap::new();
    for &(_id, manifest) in files {
        for (key, doc) in &manifest.docs {
            out.insert(key.clone(), doc.clone());
        }
    }
    out
}

/// Build lookup maps from the registered manifest: semantic types by name and
/// registered externals by name.
fn manifest_maps(
    manifest: Option<&HostManifest>,
) -> (
    BTreeMap<String, SemanticTypeDef>,
    BTreeMap<String, &ManifestExternal>,
) {
    let mut types = BTreeMap::new();
    let mut registered = BTreeMap::new();
    if let Some(manifest) = manifest {
        for ty in &manifest.types {
            types.insert(ty.name.clone(), ty.clone());
        }
        for ext in &manifest.externals {
            registered.insert(ext.name.clone(), ext);
        }
    }
    (types, registered)
}

#[cfg(test)]
mod tests {
    //! End-to-end coverage for #339: host semantic types (`///` `@param`
    //! tags referencing host vocabulary, e.g. `actor_id`) must not block
    //! compilation when no `HostManifest` is registered, while a registered
    //! manifest keeps full checking (a genuinely unknown type still errors).

    use std::collections::BTreeMap;

    use brink_ir::{BaseType, HostManifest, SemanticTypeDef};

    use super::{
        AnalysisOptions, Dialect, FileId, ImportScope, LintLevel, LintPolicy, ModuleMap,
        ProjectConfig, SemanticTypeDiagnosticSeverity, TypePolicy, analyze, analyze_with_modules,
        analyze_with_options, per_file_diagnostics, resolve, symbol_index,
    };

    /// ink with an `EXTERNAL` whose param is typed with a host semantic type
    /// (`actor_id`) — exactly the `host.ink`-generated shape from the issue.
    const SRC: &str = "\
/// @param who {actor_id}
EXTERNAL add_state(who)
";

    fn lower(src: &str) -> (brink_ir::hir::HirFile, brink_ir::SymbolManifest) {
        let parsed = brink_syntax::parse(src);
        let tree = parsed.tree();
        let (hir, manifest, diags) = brink_ir::hir::lower(FileId(0), &tree);
        assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
        (hir, manifest)
    }

    #[test]
    fn host_semantic_type_compiles_host_free_with_no_manifest() {
        let (hir, manifest) = lower(SRC);
        // `analyze()` uses `AnalysisOptions::default()` — no host manifest —
        // matching the real "no HostManifest registered" consumer path
        // (`compileProject()` with no `setHostManifest` call).
        let result = analyze(&[(FileId(0), &hir, &manifest)]);
        assert!(
            result.diagnostics.is_empty(),
            "host-free compile must not error on unknown semantic types: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn host_semantic_type_still_checked_once_manifest_registered() {
        let (hir, manifest) = lower(SRC);
        let host_manifest = HostManifest {
            markup: Vec::new(),
            externals: Vec::new(),
            types: vec![SemanticTypeDef {
                name: "actor_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            }],
        };
        let opts = AnalysisOptions {
            host_manifest: Some(host_manifest),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "known semantic type resolves cleanly: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn genuinely_unknown_type_still_errors_when_manifest_registered() {
        // Same shape, but the registered manifest does NOT define `actor_id`
        // — a manifest being present makes checking fully binding again.
        let (hir, manifest) = lower(SRC);
        let opts = AnalysisOptions {
            host_manifest: Some(HostManifest::default()),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| d.code == brink_ir::DiagnosticCode::E040)
                .count(),
            1,
            "manifest registered but type unknown: E040 still fires: {:?}",
            result.diagnostics
        );
    }

    /// #532: `semantic_type_check` defaults to `Tolerant`, matching the
    /// #339/#527 default-tolerant behavior — an explicit `Tolerant` opt-in
    /// behaves identically to the unset default.
    #[test]
    fn semantic_type_check_default_is_tolerant() {
        let (hir, manifest) = lower(SRC);
        let opts = AnalysisOptions {
            semantic_type_check: SemanticTypeDiagnosticSeverity::Tolerant,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "Tolerant (default) with no manifest: no E040: {:?}",
            result.diagnostics
        );
    }

    /// #532: raising `semantic_type_check` to `Error` re-enables strict
    /// checking even with no manifest registered — a host can catch typo'd
    /// semantic-type tags before wiring up a full manifest.
    #[test]
    fn semantic_type_check_error_diagnoses_with_no_manifest() {
        let (hir, manifest) = lower(SRC);
        let opts = AnalysisOptions {
            semantic_type_check: SemanticTypeDiagnosticSeverity::Error,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| d.code == brink_ir::DiagnosticCode::E040)
                .count(),
            1,
            "Error with no manifest: E040 still fires: {:?}",
            result.diagnostics
        );
    }

    /// #532: the lever composes with a registered manifest that defines the
    /// type — a known type never diagnoses regardless of severity.
    #[test]
    fn semantic_type_check_error_with_known_type_in_manifest_is_clean() {
        let (hir, manifest) = lower(SRC);
        let host_manifest = HostManifest {
            markup: Vec::new(),
            externals: Vec::new(),
            types: vec![SemanticTypeDef {
                name: "actor_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            }],
        };
        let opts = AnalysisOptions {
            host_manifest: Some(host_manifest),
            semantic_type_check: SemanticTypeDiagnosticSeverity::Error,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "known type resolves cleanly regardless of severity: {:?}",
            result.diagnostics
        );
    }

    // ── TM-3 (#619): strict policy end-to-end through analyze_with_options ──

    fn lower_one(src: &str) -> (brink_ir::hir::HirFile, brink_ir::SymbolManifest) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, diags) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
        (hir, manifest)
    }

    /// The dialect-keyed default (issue #1127, ruled 2026-07-19). Under
    /// `strict-ink`, an unset `types` resolves gradual FOREVER — the same
    /// source, `types` never set, must produce results identical to a build
    /// that predates TM-3 entirely: no `E064`/`E065`/`E066`, and `E063`
    /// stays un-auto-invoked (the #618/PR#640 ruling, untouched — the
    /// oracle corpus is anchored to this). Under `brink`, the same unset
    /// `types` now resolves strict, so the Unknown-escape check fires;
    /// explicit `Gradual` remains the opt-out knob and restores silence.
    #[test]
    fn types_default_is_dialect_keyed() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);

        // strict-ink + unset types: gradual, byte-identical forever.
        let opts = AnalysisOptions {
            dialect: Dialect::StrictInk,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "strict-ink (default types = gradual) must stay silent: {:?}",
            result.diagnostics
        );

        // brink + unset types: strict — the Unknown-escape check fires.
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "brink (default types = strict) must flag the Unknown escape: {:?}",
            result.diagnostics
        );

        // brink + explicit gradual: the opt-out knob restores silence.
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Gradual),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "brink + explicit gradual opt-out must stay silent: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn strict_with_strict_ink_dialect_is_a_config_error_and_nothing_else_runs() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);
        let opts = AnalysisOptions {
            dialect: Dialect::StrictInk,
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        let strict_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                matches!(
                    d.code,
                    brink_ir::DiagnosticCode::E064
                        | brink_ir::DiagnosticCode::E065
                        | brink_ir::DiagnosticCode::E066
                )
            })
            .collect();
        assert_eq!(
            strict_diags.len(),
            1,
            "exactly the one config error, nothing else: {:?}",
            result.diagnostics
        );
        assert_eq!(strict_diags[0].code, brink_ir::DiagnosticCode::E064);
    }

    #[test]
    fn strict_with_brink_dialect_surfaces_unknown_escape_as_a_compile_error() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "{:?}",
            result.diagnostics
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .find(|d| d.code == brink_ir::DiagnosticCode::E065)
                .expect("checked above")
                .code
                .severity(),
            brink_ir::Severity::Error,
            "Unknown-escape is a compile error under strict, not a warning"
        );
    }

    #[test]
    fn strict_clean_project_compiles_with_no_diagnostics() {
        let src =
            "=== function heal(hp: int): int ===\n~ temp bonus: int = 5\n~ return hp + bonus\n";
        let (hir, manifest) = lower_one(src);
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }

    // ── per_file_diagnostics: is_native decouples the T1b dialect gate
    //    (issue #1348) ────────────────────────────────────────────────

    #[test]
    fn per_file_diagnostics_is_native_true_skips_the_dialect_gate() {
        // Postfix indexing is ordinary syntax in the native grammar, but a
        // brink-extension construct the T1b gate flags (`E051`) under ink's
        // default `StrictInk` dialect. Under `is_native = true` the gate must
        // never run, regardless of `dialect`.
        let (hir, manifest) = lower_one("~ x = a[0]\n");
        let (index, _diags) = symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diags) = resolve(FileId(0), &manifest, &index, &ImportScope::default());
        let diags = per_file_diagnostics(
            FileId(0),
            &hir,
            &resolutions,
            &index,
            Dialect::StrictInk,
            true,
            None,
        );
        assert!(
            !diags
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "native must never see the ink-only dialect gate: {diags:?}"
        );
    }

    #[test]
    fn per_file_diagnostics_is_native_false_unaffected_still_flags_extension_syntax() {
        // The `is_native = false` (ink) path is byte-identical to before
        // this parameter existed — same source, same `StrictInk` default,
        // still an `E051` extension-syntax diagnostic.
        let (hir, manifest) = lower_one("~ x = a[0]\n");
        let (index, _diags) = symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diags) = resolve(FileId(0), &manifest, &index, &ImportScope::default());
        let diags = per_file_diagnostics(
            FileId(0),
            &hir,
            &resolutions,
            &index,
            Dialect::StrictInk,
            false,
            None,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "ink must still see the dialect gate: {diags:?}"
        );
    }

    // ── analyze_with_modules: is_native reaches the per-file and
    //    whole-project arms too (issue #1358) ──────────────────────────

    /// The composed pure path — not just the `per_file_diagnostics` seam
    /// directly — must skip the ink-only T1b gate for native source.
    /// Before #1358 `analyze_with_modules`'s `is_native` reached only the
    /// symbol index, so this `E051` leaked into every editor surface that
    /// analyzes off-db.
    #[test]
    fn analyze_with_modules_is_native_true_skips_the_dialect_gate() {
        let (hir, manifest) = lower_one("~ x = a[0]\n");
        let result = analyze_with_modules(
            &[(FileId(0), &hir, &manifest)],
            &ModuleMap::new(),
            &AnalysisOptions::default(),
            true,
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "native must never see the ink-only dialect gate: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn analyze_with_modules_is_native_false_unaffected_still_flags_extension_syntax() {
        let (hir, manifest) = lower_one("~ x = a[0]\n");
        let result = analyze_with_modules(
            &[(FileId(0), &hir, &manifest)],
            &ModuleMap::new(),
            &AnalysisOptions::default(),
            false,
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "ink must still see the dialect gate: {:?}",
            result.diagnostics
        );
    }

    /// `E064` rejects `types = strict` under a non-`brink` **dialect** — an
    /// ink-only axis. A native project carries `StrictInk` by default (it
    /// has no dialect opinion), so before #1358 dialing `types = strict` on
    /// the pure path produced this spurious project-level error.
    #[test]
    fn analyze_with_modules_is_native_true_skips_the_ink_only_config_error() {
        let (hir, manifest) = lower_one("=== start ===\nHello.\n-> DONE\n");
        let opts = AnalysisOptions {
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_modules(
            &[(FileId(0), &hir, &manifest)],
            &ModuleMap::new(),
            &opts,
            true,
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E064),
            "native has no dialect to be wrong about: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn analyze_with_modules_is_native_false_unaffected_still_fires_config_error() {
        let (hir, manifest) = lower_one("=== start ===\nHello.\n-> DONE\n");
        let opts = AnalysisOptions {
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_modules(
            &[(FileId(0), &hir, &manifest)],
            &ModuleMap::new(),
            &opts,
            false,
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E064),
            "ink must still get the config error: {:?}",
            result.diagnostics
        );
    }

    /// The B0.9 strict-only gate (`E137`): explicit `types = gradual` is not
    /// a policy native source can be compiled under. `brink-db`'s
    /// `per_file_diagnostics_query` has always run it; the pure path could
    /// not express it at all before #1358.
    #[test]
    fn analyze_with_modules_is_native_true_reports_the_native_strict_only_error() {
        let (hir, manifest) = lower_one("=== start ===\nHello.\n-> DONE\n");
        let opts = AnalysisOptions {
            types: Some(TypePolicy::Gradual),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_modules(
            &[(FileId(0), &hir, &manifest)],
            &ModuleMap::new(),
            &opts,
            true,
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E137),
            "explicit `types = gradual` is a native config error: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn analyze_with_modules_is_native_false_never_reports_the_native_strict_only_error() {
        let (hir, manifest) = lower_one("=== start ===\nHello.\n-> DONE\n");
        let opts = AnalysisOptions {
            types: Some(TypePolicy::Gradual),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_modules(
            &[(FileId(0), &hir, &manifest)],
            &ModuleMap::new(),
            &opts,
            false,
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E137),
            "`E137` is native-only: {:?}",
            result.diagnostics
        );
    }

    /// The module-blind convenience wrapper stays the ink path, byte for
    /// byte — it has no `Language` classification to offer.
    #[test]
    fn analyze_with_options_stays_the_ink_arm() {
        let (hir, manifest) = lower_one("~ x = a[0]\n");
        let result =
            analyze_with_options(&[(FileId(0), &hir, &manifest)], &AnalysisOptions::default());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "{:?}",
            result.diagnostics
        );
    }

    // ── AnalysisOptions::apply_project_config (moved from
    // brink-project-config with the #1234 dependency inversion) ──────

    #[test]
    fn apply_sets_unset_fields_from_config() {
        let mut options = AnalysisOptions::default();
        let config = ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: Some(TypePolicy::Strict),
            ..ProjectConfig::default()
        };
        options.apply_project_config(&config, false, false);
        assert_eq!(options.dialect, Dialect::Brink);
        assert_eq!(options.types, Some(TypePolicy::Strict));
    }

    #[test]
    fn apply_leaves_overridden_fields_alone() {
        let mut options = AnalysisOptions {
            dialect: Dialect::StrictInk,
            types: Some(TypePolicy::Gradual),
            ..AnalysisOptions::default()
        };
        let config = ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: Some(TypePolicy::Strict),
            ..ProjectConfig::default()
        };
        // Both overridden: explicit calls win, file is ignored entirely.
        options.apply_project_config(&config, true, true);
        assert_eq!(options.dialect, Dialect::StrictInk);
        assert_eq!(options.types, Some(TypePolicy::Gradual));
    }

    #[test]
    fn apply_mixed_override_only_touches_non_overridden_field() {
        let mut options = AnalysisOptions {
            dialect: Dialect::StrictInk,
            types: Some(TypePolicy::Gradual),
            ..AnalysisOptions::default()
        };
        let config = ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: Some(TypePolicy::Strict),
            ..ProjectConfig::default()
        };
        // dialect explicitly overridden (stays StrictInk); types is not
        // (file wins, becomes Strict).
        options.apply_project_config(&config, true, false);
        assert_eq!(options.dialect, Dialect::StrictInk);
        assert_eq!(options.types, Some(TypePolicy::Strict));
    }

    #[test]
    fn apply_with_no_config_values_leaves_options_untouched() {
        let mut options = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        };
        options.apply_project_config(&ProjectConfig::default(), false, false);
        assert_eq!(options.dialect, Dialect::Brink);
        assert_eq!(options.types, Some(TypePolicy::Strict));
    }

    // ── AnalysisOptions::apply_project_config: [lints] (issue #1160) ──

    #[test]
    fn apply_project_config_applies_lint_overrides() {
        let mut options = AnalysisOptions::default();
        let mut config = ProjectConfig::default();
        config.lints.insert("E014".to_owned(), LintLevel::Deny);
        config.lints.insert("E022".to_owned(), LintLevel::Allow);

        options.apply_project_config(&config, false, false);

        assert_eq!(options.lints.overrides.get("E014"), Some(&LintLevel::Deny));
        assert_eq!(options.lints.overrides.get("E022"), Some(&LintLevel::Allow));
    }

    #[test]
    fn apply_project_config_sets_deny_warnings() {
        let mut options = AnalysisOptions::default();
        let config = ProjectConfig {
            deny_warnings: Some(true),
            ..ProjectConfig::default()
        };

        options.apply_project_config(&config, false, false);

        assert!(options.lints.deny_warnings);
    }

    #[test]
    fn apply_project_config_absent_lints_clears_lint_policy() {
        let mut options = AnalysisOptions {
            lints: LintPolicy {
                overrides: BTreeMap::from([("E014".to_owned(), LintLevel::Deny)]),
                deny_warnings: true,
            },
            ..AnalysisOptions::default()
        };

        options.apply_project_config(&ProjectConfig::default(), false, false);

        // Issue #1397: unlike `dialect`/`types`, `[lints]` REPLACES the
        // resolved policy rather than merging into it — an empty (or
        // absent) `[lints]` table resolves to no overrides and
        // `deny-warnings = false`, so a long-lived caller (the editor
        // session) that re-applies `brink.toml` after the table was deleted
        // actually reverts, instead of leaving the previous override stuck.
        assert!(
            options.lints.overrides.is_empty(),
            "an absent [lints] table must clear previously-resolved overrides"
        );
        assert!(!options.lints.deny_warnings);
    }

    #[test]
    fn apply_project_config_omitted_code_reverts_to_base_severity() {
        // Simulates the editor session's live-reapply scenario (#1397): a
        // prior call already resolved E014 and E022 overrides plus
        // deny-warnings; the re-applied config only re-asserts E014 —
        // E022 and deny-warnings were deleted from `brink.toml` in between.
        let mut options = AnalysisOptions {
            lints: LintPolicy {
                overrides: BTreeMap::from([
                    ("E014".to_owned(), LintLevel::Deny),
                    ("E022".to_owned(), LintLevel::Allow),
                ]),
                deny_warnings: true,
            },
            ..AnalysisOptions::default()
        };
        let mut config = ProjectConfig::default();
        config.lints.insert("E014".to_owned(), LintLevel::Deny);

        options.apply_project_config(&config, false, false);

        assert_eq!(
            options.lints.overrides.get("E014"),
            Some(&LintLevel::Deny),
            "a code still present in the re-applied config keeps its override"
        );
        assert!(
            !options.lints.overrides.contains_key("E022"),
            "a code omitted from the re-applied config must revert to its \
             base severity, not stick"
        );
        assert!(
            !options.lints.deny_warnings,
            "deny-warnings omitted from the re-applied config must revert \
             to false, not stick"
        );
    }

    #[test]
    fn apply_project_config_rejects_unknown_lint_code() {
        let mut options = AnalysisOptions::default();
        let mut config = ProjectConfig::default();
        // Not a real `DiagnosticCode` — never parses.
        config.lints.insert("E9999".to_owned(), LintLevel::Deny);

        let warnings = options.apply_project_config(&config, false, false);

        assert!(
            options.lints.overrides.is_empty(),
            "an unknown code must never be merged into the policy"
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("E9999"));
    }

    #[test]
    fn apply_project_config_rejects_misspelled_lint_code_case() {
        let mut options = AnalysisOptions::default();
        let mut config = ProjectConfig::default();
        // `DiagnosticCode::from_str_code` is case-sensitive — a lowercase
        // spelling of a real code is not itself a real code.
        config.lints.insert("e014".to_owned(), LintLevel::Deny);

        let warnings = options.apply_project_config(&config, false, false);

        assert!(options.lints.overrides.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("e014"));
    }

    #[test]
    fn apply_project_config_rejects_non_overridable_lint_code() {
        let mut options = AnalysisOptions::default();
        let mut config = ProjectConfig::default();
        // E001 is a real code, but its default severity is `Error`, not
        // `Warning` — never reachable through `effective_severity`'s
        // hard-error exemption, so `[lints]` must not silently accept it.
        assert_eq!(
            brink_ir::DiagnosticCode::E001.severity(),
            brink_ir::Severity::Error
        );
        config.lints.insert("E001".to_owned(), LintLevel::Deny);

        let warnings = options.apply_project_config(&config, false, false);

        assert!(options.lints.overrides.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("E001"));
    }

    #[test]
    fn apply_project_config_reports_no_warnings_for_valid_overridable_codes() {
        let mut options = AnalysisOptions::default();
        let mut config = ProjectConfig::default();
        config.lints.insert("E014".to_owned(), LintLevel::Deny);

        let warnings = options.apply_project_config(&config, false, false);

        assert!(warnings.is_empty());
    }

    /// Issue #1674: `E157`'s default severity is `Info`, not `Warning` — the
    /// widened `validate_lint_code` gate (anything short of `Error`) must
    /// still accept a `[lints] E157 = "warn"` override rather than rejecting
    /// it the way the pre-#1674 `Warning`-base-only gate would have.
    #[test]
    fn apply_project_config_accepts_info_base_lint_code() {
        let mut options = AnalysisOptions::default();
        let mut config = ProjectConfig::default();
        assert_eq!(
            brink_ir::DiagnosticCode::E157.severity(),
            brink_ir::Severity::Info
        );
        config.lints.insert("E157".to_owned(), LintLevel::Warn);

        let warnings = options.apply_project_config(&config, false, false);

        assert!(warnings.is_empty());
        assert_eq!(options.lints.overrides.get("E157"), Some(&LintLevel::Warn));
    }

    // ── AnalysisOptions::apply_lint_overrides: CLI/API tier (issue #1373) ──

    #[test]
    fn apply_lint_overrides_merges_per_code_overrides() {
        let mut options = AnalysisOptions::default();
        let mut overrides = BTreeMap::new();
        overrides.insert("E014".to_owned(), LintLevel::Deny);

        let warnings = options.apply_lint_overrides(&overrides, None);

        assert!(warnings.is_empty());
        assert_eq!(options.lints.overrides.get("E014"), Some(&LintLevel::Deny));
    }

    #[test]
    fn apply_lint_overrides_sets_deny_warnings() {
        let mut options = AnalysisOptions::default();

        let warnings = options.apply_lint_overrides(&BTreeMap::new(), Some(true));

        assert!(warnings.is_empty());
        assert!(options.lints.deny_warnings);
    }

    #[test]
    fn apply_lint_overrides_none_deny_warnings_leaves_it_untouched() {
        let mut options = AnalysisOptions::default();
        options.lints.deny_warnings = true;

        options.apply_lint_overrides(&BTreeMap::new(), None);

        assert!(options.lints.deny_warnings);
    }

    #[test]
    fn apply_lint_overrides_rejects_unknown_code() {
        let mut options = AnalysisOptions::default();
        let mut overrides = BTreeMap::new();
        overrides.insert("E9999".to_owned(), LintLevel::Deny);

        let warnings = options.apply_lint_overrides(&overrides, None);

        assert!(options.lints.overrides.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("E9999"));
    }

    #[test]
    fn apply_lint_overrides_rejects_non_overridable_code() {
        let mut options = AnalysisOptions::default();
        let mut overrides = BTreeMap::new();
        // E001 is a real code, but its default severity is `Error`, not
        // `Warning` — same non-overridability rule as the file's `[lints]`
        // table (#1160).
        overrides.insert("E001".to_owned(), LintLevel::Deny);

        let warnings = options.apply_lint_overrides(&overrides, None);

        assert!(options.lints.overrides.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("E001"));
    }

    #[test]
    fn apply_lint_overrides_wins_over_a_prior_apply_project_config_for_the_same_code() {
        let mut options = AnalysisOptions::default();
        let mut config = ProjectConfig::default();
        config.lints.insert("E014".to_owned(), LintLevel::Deny);
        options.apply_project_config(&config, false, false);
        assert_eq!(options.lints.overrides.get("E014"), Some(&LintLevel::Deny));

        let mut overrides = BTreeMap::new();
        overrides.insert("E014".to_owned(), LintLevel::Allow);
        options.apply_lint_overrides(&overrides, None);

        // The explicit override replaces the file's value for the same
        // code — #1005/#1373's `CLI/API > file` precedence.
        assert_eq!(options.lints.overrides.get("E014"), Some(&LintLevel::Allow));
    }
}
