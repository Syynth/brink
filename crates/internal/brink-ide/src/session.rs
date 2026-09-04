//! Stateful IDE session wrapping `ProjectDb`.
//!
//! `IdeSession` is the single entry point for IDE queries in the wasm
//! bridge. It owns the project database; analysis is the db's
//! `analysis_query` (option A, ruled 2026-08-24 — see
//! `docs/live-typing-diagnostics-divergence.md`), so a keystroke pays one
//! incremental salsa pull instead of the retired snapshot-clone +
//! whole-project off-db re-analysis.

use std::sync::Arc;

use brink_analyzer::{
    AnalysisOptions, AnalysisResult, Dialect, ExternalCheckSeverity, LintPolicy,
    SemanticTypeDiagnosticSeverity, TypePolicy,
};
use brink_db::{CompileProduct, ProjectDb};
use brink_ir::{FileId, HirFile, HostManifest, ResolvedDialect, SymbolManifest};

use crate::hir_projection::Projection;

/// Why [`IdeSession::compile`] could not produce an artifact for the requested
/// entry point. Diagnostics that merely *prevent* a successful compile (parse,
/// lowering, analysis errors) are carried in [`CompileProduct::errors`], not
/// here — this is only the "there is nothing to compile" precondition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompileEntryError {
    /// The requested entry path is not a file loaded in this session.
    #[error("entry file not found in session: {0}")]
    EntryNotFound(String),
}

/// Stateful IDE session — owns `ProjectDb`; analysis is the db's own
/// memoized `analysis_query` (option A, ruled 2026-08-24 — no session-side
/// cache, no off-db snapshot road).
pub struct IdeSession {
    db: ProjectDb,
    /// `[project] drafts` globs, as the author wrote them. Session state
    /// rather than analysis input: drafts are a *reporting* concept
    /// (`crate::drafts`), and changing them needs no re-analyze.
    draft_globs: Vec<String>,
    /// Files mounted from the stdlib rather than authored by the user
    /// ([`Self::mount_stdlib`]). They are tracked because several surfaces
    /// have to exclude them — a mounted file is never the author's draft,
    /// and never theirs to rename or delete.
    mounted_std_ids: std::collections::BTreeSet<FileId>,
    /// The registered host-capability manifest (tooling/author-time), if any.
    host_manifest: Option<HostManifest>,
    /// Host-pushed values for `host`-source semantic types (Tier 3, #174).
    /// Query-time only — not part of analysis, so a push needs no re-analyze.
    host_values: crate::HostValues,
    /// Severity policy for manifest-driven external checks.
    external_check: ExternalCheckSeverity,
    /// Severity policy for unknown-semantic-type diagnostics (`E040`),
    /// parallel to `external_check` (#532).
    semantic_type_check: SemanticTypeDiagnosticSeverity,
    /// The registered dialogue dialect (#368), pre-compiled for
    /// classification. Tooling-only, query-time state consumed by
    /// `line_contexts` — never part of analysis, so registering one needs no
    /// re-analyze. `None` means no dialect is mounted (plain structural
    /// classification only).
    dialect: Option<ResolvedDialect>,
    /// The T1b compiler dialect (docs/t1b-surface-spec.md §1, #589/#600),
    /// set via `set_language_dialect`. Defaults to `Dialect::StrictInk`,
    /// matching `AnalysisOptions::default()`. Unlike `dialect` above (the
    /// #368 dialogue-dialect, query-time-only tooling state), this one feeds
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection` — it
    /// gates the `E051` "brink extension" diagnostic (#611: previously only
    /// `EditorSession`'s local copy gated completions/signature-help, so a
    /// `brink`-dialect project got permanent spurious `E051` from the
    /// background analysis pass regardless of this setting).
    language_dialect: Dialect,
    /// TM-3 typed-mode policy (docs/typed-mode-spec.md §1), set via
    /// `set_type_policy`. `None` until a caller opts in — the effective
    /// policy is then the dialect-keyed default (issue #1127, ruled
    /// 2026-07-19: `brink` → strict, `strict-ink` → gradual), resolved by
    /// `brink_analyzer::resolve_type_policy` via `AnalysisOptions::
    /// type_policy()`. Mirrors `language_dialect` exactly (#660: PR #656
    /// left this hardcoded to `Gradual` in both `snapshot` and
    /// `analysis_options`, so the IDE/LSP/web surface could not reach
    /// `types = strict` at all — the compiler CLI's `--types strict` was the
    /// only path). Authoring-time/tooling input only, feeding
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection`, same as
    /// `language_dialect`.
    type_policy: Option<TypePolicy>,
    /// Resolved `[lints]` policy (issue #1160), set via `set_lint_policy`.
    /// Defaults to `LintPolicy::default()` (a no-op: every diagnostic keeps
    /// its `DiagnosticCode::severity()` default), matching
    /// `AnalysisOptions::default()`. Unlike `language_dialect`/`type_policy`
    /// there is no explicit-vs-file precedence to track here — `[lints]` has
    /// no CLI-flag/editor-API override source of its own yet (see
    /// `AnalysisOptions::apply_project_config`'s doc comment), so the file is
    /// always the source of truth for the *whole table* (issue #1397: a
    /// fresh `apply_project_config` call replaces the resolved policy
    /// wholesale, not just the codes it mentions). Feeds
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection` exactly
    /// like `language_dialect`/`type_policy` (#1366: previously hardcoded to
    /// `LintPolicy::default()` in both `snapshot` and `analysis_options`, so
    /// a project's `[lints]` never reached the IDE/LSP/web surface).
    lints: LintPolicy,
    /// `brink.toml`'s `[project] conventions` pointer (issue #1844; renamed
    /// from `elements` by #2180), set via `set_conventions`. `None` means
    /// "no conventions module configured" — the confinement check
    /// (`E169`) consuming this reads `None` as a real misconfiguration
    /// since #2289, not "nothing to check", so this field must reflect
    /// whatever `brink.toml`'s `[project] conventions` key actually says
    /// rather than a hardcoded default (issue #1880: before this field
    /// existed, every session read as unconfigured, firing `E169` on every
    /// claim handler in every native project opened in the editor).
    /// Authoring-time/tooling input only, mirroring `language_dialect`/
    /// `type_policy`/`lints` — feeds
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection` the
    /// same way.
    conventions: Option<String>,
    /// D6/#3229: whether this session's compiles emit the `DebugInfo`
    /// section (`docs/debugger-spec.md` §1.2's ship policy). **Per-session,
    /// not per-project** — no `brink.toml` key makes it a property of the
    /// project. **Default ON since 2026-08-29** (`docs/decision-log.md`
    /// "debug info on by default"; supersedes the 2026-08-28 default-off
    /// consequence, mechanism unchanged): every studio compile carries the
    /// section so breakpoints bind and stepping resolves mid-play with no
    /// artifact switch; the App-settings opt-out calls
    /// [`Self::set_emit_debug_info`]`(false)`. Release export is
    /// unaffected — `AnalysisOptions::default()` (the CLI/release path)
    /// stays `false`, and only `brink compile --debug-info` or a session
    /// opts in there.
    ///
    /// Unlike `language_dialect`/`type_policy`/`lints`/`conventions` above,
    /// this is NOT an analysis input: it changes only what codegen emits.
    /// `debug_info_policy_query`'s narrow salsa projection gives it a cheap
    /// cutoff, so toggling it re-runs `story_data_query` and leaves every
    /// diagnostic query backdated — which is exactly why flipping it at
    /// debug-start is affordable.
    emit_debug_info: bool,
}

impl IdeSession {
    /// Create an empty session.
    pub fn new() -> Self {
        Self {
            db: ProjectDb::new(),
            draft_globs: Vec::new(),
            mounted_std_ids: std::collections::BTreeSet::new(),
            host_manifest: None,
            host_values: crate::HostValues::new(),
            external_check: ExternalCheckSeverity::default(),
            semantic_type_check: SemanticTypeDiagnosticSeverity::default(),
            dialect: None,
            language_dialect: Dialect::default(),
            type_policy: None,
            lints: LintPolicy::default(),
            conventions: None,
            emit_debug_info: true,
        }
    }

    /// Record that `id` was mounted from the stdlib rather than authored.
    ///
    /// The mounting itself stays with the host: the stdlib's text lives in
    /// `brink-environment`, and making this crate depend on the compiler to
    /// save a four-line loop would push that weight onto every consumer
    /// (`brink-lsp` included). What must live here is the *consequence* —
    /// which files are the author's — because rules are written against it.
    pub fn mark_mounted_std(&mut self, id: FileId) {
        self.mounted_std_ids.insert(id);
    }

    /// Whether `id` is a mounted stdlib file rather than the author's own.
    #[must_use]
    pub fn is_mounted_std(&self, id: FileId) -> bool {
        self.mounted_std_ids.contains(&id)
    }

    /// Every mounted stdlib file, in id order.
    pub fn mounted_std_ids(&self) -> impl Iterator<Item = FileId> + '_ {
        self.mounted_std_ids.iter().copied()
    }

    /// Forget a mounted id — for a host that removes files from the session.
    pub fn unmount_std(&mut self, id: FileId) {
        self.mounted_std_ids.remove(&id);
    }

    /// `[project] drafts` globs (`crate::drafts`).
    pub fn set_draft_globs(&mut self, globs: Vec<String>) {
        self.draft_globs = globs;
    }

    /// The configured draft globs, as authored.
    #[must_use]
    pub fn draft_globs(&self) -> &[String] {
        &self.draft_globs
    }

    /// Register (or replace) the host-capability manifest, then re-analyze.
    pub fn set_host_manifest(&mut self, manifest: HostManifest) {
        self.host_manifest = Some(manifest);
        self.reanalyze();
    }

    /// Clear the registered host manifest, then re-analyze.
    pub fn clear_host_manifest(&mut self) {
        self.host_manifest = None;
        self.reanalyze();
    }

    /// Replace the host-pushed value cache (Tier 3, #174). No re-analyze — these
    /// values are consumed only by the argument picker + value-label inlay
    /// hints at query time, not by analysis.
    pub fn set_host_values(&mut self, values: crate::HostValues) {
        self.host_values = values;
    }

    /// Clear the host-pushed value cache.
    pub fn clear_host_values(&mut self) {
        self.host_values.clear();
    }

    /// The host-pushed value cache (empty when no host is attached).
    #[must_use]
    pub fn host_values(&self) -> &crate::HostValues {
        &self.host_values
    }

    /// Register (or replace) the dialogue dialect (#368). Compiles the
    /// dialect's patterns once, up front, so `line_contexts` never
    /// re-compiles on a hot path. Tooling-only — consumed at query time by
    /// `line_contexts`, never by analysis, so no re-analyze.
    ///
    /// Prefer [`set_dialect_config`](Self::set_dialect_config): it also
    /// writes the db's dialect input (#3064 B1), which the per-segment
    /// editor queries read as a tracked dependency. This compiled-form
    /// setter remains for callers that never touch those queries.
    pub fn set_dialect(&mut self, dialect: ResolvedDialect) {
        self.dialect = Some(dialect);
    }

    /// Register (or replace) the dialect from its pure-JSON config —
    /// compiles once here AND writes the db input (guarded against
    /// no-op rewrites: salsa's setter stamps the revision
    /// unconditionally, the same hazard `sync_db_options` guards).
    pub fn set_dialect_config(&mut self, config: brink_ir::DialogueDialect) {
        if let Ok(resolved) = ResolvedDialect::compile(&config) {
            self.dialect = Some(resolved);
        }
        if self.db.dialect_config() != Some(&config) {
            self.db.set_dialect(Some(config));
        }
    }

    /// Clear the registered dialect. `line_contexts` reverts to plain
    /// structural classification (no `dialect` facet on any line).
    pub fn clear_dialect(&mut self) {
        self.dialect = None;
        if self.db.dialect_config().is_some() {
            self.db.set_dialect(None);
        }
    }

    /// The registered dialect, if any.
    #[must_use]
    pub fn dialect(&self) -> Option<&ResolvedDialect> {
        self.dialect.as_ref()
    }

    /// Set the T1b compiler dialect (docs/t1b-surface-spec.md §1, #589/#600),
    /// then re-analyze — this is the diagnostics-facing counterpart of
    /// completions/signature-help gating (#611). `Dialect::Brink` drops the
    /// `E051` "brink extension" diagnostic for extension syntax that already
    /// lowers and runs under either dialect; `Dialect::StrictInk` (the
    /// default) restores it.
    pub fn set_language_dialect(&mut self, dialect: Dialect) {
        self.language_dialect = dialect;
        self.reanalyze();
    }

    /// The registered T1b compiler dialect (defaults to `Dialect::StrictInk`).
    #[must_use]
    pub fn language_dialect(&self) -> Dialect {
        self.language_dialect
    }

    /// Set the TM-3 typed-mode policy (docs/typed-mode-spec.md §1, #660),
    /// then re-analyze — the diagnostics-facing counterpart of the compiler
    /// CLI's `--types strict`. `TypePolicy::Strict` requires
    /// `language_dialect() == Dialect::Brink`, or `analyze` reports a single
    /// project-level `E064` config-error diagnostic instead of running the
    /// normal passes (see `brink_analyzer::strict::config_error`) — same
    /// caller responsibility as `set_language_dialect`.
    pub fn set_type_policy(&mut self, types: TypePolicy) {
        self.type_policy = Some(types);
        self.reanalyze();
    }

    /// The *effective* TM-3 typed-mode policy: an explicit
    /// `set_type_policy` value if one was ever registered, else the
    /// dialect-keyed default (issue #1127 — `brink` → `Strict`,
    /// `strict-ink` → `Gradual`), resolved through the one
    /// `brink_analyzer::resolve_type_policy` seam.
    #[must_use]
    pub fn type_policy(&self) -> TypePolicy {
        brink_analyzer::resolve_type_policy(self.language_dialect, self.type_policy)
    }

    /// Set the resolved `[lints]` policy (issue #1160/#1366), then
    /// re-analyze. Callers resolve the policy themselves — typically by
    /// running a parsed `brink.toml`'s `[lints]` table through
    /// `AnalysisOptions::apply_project_config`, which **replaces** the
    /// policy wholesale from the file rather than merging onto whatever was
    /// resolved before (issue #1397), and passing the result's `.lints`
    /// back here. `brink-web`'s
    /// `EditorSession::apply_project_config` and the CLI's
    /// `Project::ide_session()` (`brink-cli/src/ide/project.rs`, issue
    /// #1393) both call this — the CLI resolves `[lints]` once, via
    /// `resolve_analysis_options`, into the `Driver`'s `AnalysisOptions` for
    /// compile/analysis, then `Project::ide_session()` forwards that same
    /// resolved policy here so a `brink ide` structural-op safety gate
    /// (`structural_result::gate`) sees it too instead of
    /// `LintPolicy::default()`.
    pub fn set_lint_policy(&mut self, lints: LintPolicy) {
        self.lints = lints;
        self.reanalyze();
    }

    /// The current resolved `[lints]` policy (defaults to
    /// `LintPolicy::default()`, a no-op).
    #[must_use]
    pub fn lint_policy(&self) -> &LintPolicy {
        &self.lints
    }

    /// Set `brink.toml`'s `[project] conventions` pointer (issue #1880),
    /// then re-analyze — the diagnostics-facing counterpart of the compiler
    /// CLI/`brink-db`'s already-working salsa path. Mirrors
    /// `set_type_policy`/`set_lint_policy` exactly: no explicit-vs-file
    /// precedence tier exists for this field yet (see
    /// `AnalysisOptions::apply_project_config`'s doc comment on
    /// `conventions`), so a caller reloading `brink.toml` always passes the
    /// freshly-resolved value — `None` when the file no longer sets the
    /// key, `Some(pointer)` otherwise — and this replaces whatever was
    /// registered before, the same wholesale-replace posture `[lints]` uses
    /// for the same reason (issue #1397).
    pub fn set_conventions(&mut self, conventions: Option<String>) {
        self.conventions = conventions;
        self.reanalyze();
    }

    /// The currently registered `[project] conventions` pointer, if any.
    #[must_use]
    pub fn conventions(&self) -> Option<&str> {
        self.conventions.as_deref()
    }

    /// The raw registered `[project] types` override, if any — `None` means
    /// "never explicitly set", distinct from [`Self::type_policy`] (the
    /// *effective* value, which resolves `None` to the dialect-keyed
    /// default via [`brink_analyzer::resolve_type_policy`]). A producer that
    /// needs to round-trip [`AnalysisOptions::types`] without collapsing an
    /// unset override into an explicitly-chosen one (issue #2334's
    /// `apply_analysis_options` seam — a caller re-deriving a full
    /// `AnalysisOptions` to hand it back must not accidentally freeze
    /// today's dialect-keyed default into a permanent explicit choice) reads
    /// this, never [`Self::type_policy`].
    #[must_use]
    pub fn type_policy_override(&self) -> Option<TypePolicy> {
        self.type_policy
    }

    /// Apply the `[project]`/`[lints]`-derived subset of `options` —
    /// [`AnalysisOptions::dialect`]/`.types`/`.lints`/`.conventions` — onto
    /// this session in one call, then re-analyze at most once (issue #2334).
    ///
    /// This is the **one seam** every `IdeSession` producer that already has
    /// a fully-resolved `AnalysisOptions` in hand (`brink-cli`'s
    /// `Project::ide_session`, `brink-web`'s `EditorSession::
    /// apply_parsed_config`) should call instead of hand-copying individual
    /// `set_language_dialect`/`set_type_policy`/`set_lint_policy`/
    /// `set_conventions` calls. The same field (`conventions`) was dropped
    /// from that hand-written forwarding three times in a row across three
    /// separate producers (#1880 → #2316 fixed two of them, #2317 → #2325
    /// fixed the third) — routing every producer through this one function
    /// means a future field only needs a forwarding decision made *here*,
    /// once, rather than copy-pasted into every call site.
    ///
    /// `host_manifest`/`external_check`/`semantic_type_check` are
    /// deliberately **not** forwarded: those three are tooling-level
    /// concerns with their own dedicated setters
    /// ([`Self::set_host_manifest`]/[`Self::set_external_check`]/
    /// [`Self::set_semantic_type_check`]), sourced independently of
    /// `brink.toml`/`[project]` (no `ProjectConfig` field maps to any of
    /// them) — a producer that owns one of those calls its own setter
    /// directly, exactly as today. Spelled out explicitly below (the same
    /// "not `..Default::default()`" pattern [`IdeSnapshot::analyze`] and
    /// [`Self::analysis_options`] already use) so that a *new*
    /// `AnalysisOptions` field breaks this function's compilation the
    /// moment it's added — forcing an explicit forward-or-exclude decision
    /// here rather than a silent, field-shaped hole a producer might never
    /// notice.
    ///
    /// Change-guarded as a whole (mirrors [`Self::sync_db_options`]'s own
    /// guard): re-analyzes once, only if at least one of the four fields
    /// actually differs from what this session already has registered —
    /// never the up-to-four redundant re-analyses a producer hand-calling
    /// each setter separately would trigger (the "related, same root cause"
    /// half of issue #2334: `Project::ide_session()` previously ran a full
    /// re-analysis once per setter call to build a single session).
    pub fn apply_analysis_options(&mut self, options: &AnalysisOptions) {
        let AnalysisOptions {
            host_manifest: _,
            external_check: _,
            semantic_type_check: _,
            dialect,
            types,
            lints,
            // D6/#3229: a compile flag with no diagnostics-relevant
            // effect — not one of the four fields this method
            // change-detects, same posture as the three ignored fields
            // above. Deliberately ignored rather than honoured: since
            // #3229 the flag is **session-owned state**
            // (`set_emit_debug_info`), and every caller of this method
            // rebuilds an `AnalysisOptions` from a `brink.toml` re-read
            // (`EditorSession::apply_parsed_config`) — honouring it here
            // would let each such re-apply silently switch a live debug
            // session's compiles back off.
            emit_debug_info: _,
            conventions,
        } = options.clone();
        let mut changed = false;
        if self.language_dialect != dialect {
            self.language_dialect = dialect;
            changed = true;
        }
        if self.type_policy != types {
            self.type_policy = types;
            changed = true;
        }
        if self.lints != lints {
            self.lints = lints;
            changed = true;
        }
        if self.conventions != conventions {
            self.conventions = conventions;
            changed = true;
        }
        // Option A (2026-08-24): the `|| self.analysis.is_none()` arm that
        // #2334's review added (re-closing #1393's fresh-session gap) is
        // gone WITH its gap — `analysis()` now always pulls the db's
        // memoized query, so a session that never "analyzed" cannot hold a
        // stale `None`; freshness is the db's job, not a call-ordering
        // obligation.
        if changed {
            self.reanalyze();
        }
    }

    /// Turn the D6 `DebugInfo` section on or off for **this session's**
    /// compiles (`docs/debugger-spec.md` §1.2, issue #3229).
    ///
    /// This is the toggle that makes the debugger reachable at all: D4's
    /// runtime position, D6's section, D7's locals table and D9's
    /// program→source resolver are all inert against an artifact compiled
    /// without it. A studio turns it on for the session it is about to
    /// debug and off again when that session ends, per the 2026-08-28
    /// ruling — per-session rather than always-on (which would pay the
    /// size and time cost on every keystroke of ordinary authoring) or a
    /// `brink.toml` key (which would make debuggability a property of the
    /// project, and default every existing project to off).
    ///
    /// **Not an analysis input.** Diagnostics are byte-identical either
    /// way; only what codegen emits changes. `debug_info_policy_query`'s
    /// narrow projection means an already-analyzed project re-runs
    /// `story_data_query` and nothing else — so the recompile a host pays
    /// for at debug-start is a codegen pass, not a reanalysis. Hence
    /// `sync_db_options` here and deliberately no `reanalyze()`: calling
    /// it would throw away memoized diagnostics to no effect.
    ///
    /// No-ops when the value is unchanged, so a host may call it
    /// unconditionally on every debug-session start without invalidating
    /// the compiled artifact it already has.
    pub fn set_emit_debug_info(&mut self, enabled: bool) {
        if self.emit_debug_info == enabled {
            return;
        }
        self.emit_debug_info = enabled;
        self.sync_db_options();
    }

    /// Whether this session's compiles emit the `DebugInfo` section
    /// (#3229). See [`Self::set_emit_debug_info`].
    #[must_use]
    pub fn emit_debug_info(&self) -> bool {
        self.emit_debug_info
    }

    /// Set the severity policy for manifest-driven external checks, then
    /// re-analyze.
    pub fn set_external_check(&mut self, severity: ExternalCheckSeverity) {
        self.external_check = severity;
        self.reanalyze();
    }

    /// Set the severity policy for unknown-semantic-type diagnostics
    /// (`E040`), then re-analyze. Parallel to [`Self::set_external_check`]
    /// (#532) — raise to `Error` to re-enable strict checking (catching
    /// typo'd host semantic-type tags) even with no manifest registered.
    pub fn set_semantic_type_check(&mut self, severity: SemanticTypeDiagnosticSeverity) {
        self.semantic_type_check = severity;
        self.reanalyze();
    }

    /// Re-establish analysis freshness after an option change: push the
    /// session's options into the db (see
    /// [`sync_db_options`](Self::sync_db_options) — every option setter
    /// funnels through here) and eagerly pull the memoized analysis so the
    /// cost lands here rather than on the next query. The projection cache
    /// clear rides the options write inside `sync_db_options`.
    fn reanalyze(&mut self) {
        self.sync_db_options();
        self.db.analysis();
    }

    /// Write the session's current [`analysis_options`](Self::analysis_options)
    /// into its own [`ProjectDb`] as a salsa input (issue #1553). Since
    /// option A (2026-08-24) EVERY analysis read is a db query gated on this
    /// input — `analysis_query` included, not just the direct readers
    /// (`per_file_diagnostics`, `symbol_index`, `diagnostics`, `effects`,
    /// `infer_body`) #1553 originally closed the gap for.
    ///
    /// Guarded against unchanged values: salsa's `set_analysis_options`
    /// stamps the current revision unconditionally on every write, so an
    /// unguarded call would invalidate every reader even when the value
    /// didn't actually change. An actual write also clears the projection
    /// cache — the identity join is derived from analysis, which the write
    /// just invalidated. [`compile`](Self::compile) applies the same guard
    /// to its caller-supplied options (#2885's resolution — one db, one
    /// options input, every writer guarded).
    fn sync_db_options(&mut self) {
        let options = self.analysis_options();
        if self.db.analysis_options() != &options {
            self.db.set_analysis_options(options);
        }
    }

    /// Add or update a source file in the database. Projection
    /// invalidation is salsa's job since #3064 B2 — the
    /// per-segment projection memos backdate on their own, no session
    /// cache to wipe.
    pub fn update_source(&mut self, path: &str, source: String) -> FileId {
        self.db.update_file(path, source)
    }

    /// Remove a file from the project.
    pub fn remove_file(&mut self, path: &str) {
        self.db.remove_file(path);
    }

    /// The file's HIR projection — computed once per source/analysis
    /// generation and shared by every structural view (#480). Always carries
    /// the analyzer identity join (a superset the structural views simply
    /// ignore) — analysis is always available from the db since option A.
    /// The file's assembled per-line contexts (#3064 B3) — per-segment
    /// memoized on the db road; dialect-classified when a dialect config
    /// is registered via [`set_dialect_config`](Self::set_dialect_config).
    pub fn line_contexts(
        &self,
        file: FileId,
    ) -> Option<Arc<Vec<brink_ir::hir::line_context::LineContext>>> {
        self.db.line_contexts(file)
    }

    /// The file's assembled semantic tokens (#3064 B4) — per-segment
    /// memoized on the db road with a range-free resolution-kind seam.
    pub fn semantic_tokens(
        &self,
        file: FileId,
    ) -> Option<Arc<Vec<brink_ir::semantic_tokens::RawToken>>> {
        self.db.semantic_tokens(file)
    }

    /// Outbound-delta manifest passthrough (#3064 option A).
    pub fn segment_manifest(&self, file: FileId) -> Option<(Vec<(String, u32)>, u32)> {
        self.db.segment_manifest(file)
    }

    /// Outbound-delta context slice passthrough (#3064 option A).
    pub fn segment_line_contexts_slice(
        &self,
        file: FileId,
        key: &str,
    ) -> Option<Vec<brink_ir::hir::line_context::LineContext>> {
        self.db.segment_line_contexts_slice(file, key)
    }

    /// Classifier-only token slice passthrough (#3064 micro) — never
    /// pulls index/resolve; the deferred refresh swaps in the refined
    /// slice.
    pub fn segment_semantic_tokens_slice_fast(
        &self,
        file: FileId,
        key: &str,
    ) -> Option<Vec<brink_ir::semantic_tokens::RawToken>> {
        self.db.segment_semantic_tokens_slice_fast(file, key)
    }

    /// Outbound-delta token slice passthrough (#3064 option A).
    pub fn segment_semantic_tokens_slice(
        &self,
        file: FileId,
        key: &str,
    ) -> Option<Vec<brink_ir::semantic_tokens::RawToken>> {
        self.db.segment_semantic_tokens_slice(file, key)
    }

    pub fn projection(&self, file: FileId) -> Option<Arc<Projection>> {
        // #3064 B2: the db's assembled per-segment projection replaces the
        // session-level wipe-on-every-edit cache — a keystroke inside one
        // knot re-walks that knot's fragment only, and salsa owns the
        // invalidation (the identity join reads `resolutions_index`, the
        // cheap FG-3 half). Byte-identity with the whole-file walk is
        // corpus-gated in `brink-db`'s `segments` tests.
        self.db.projection(file)
    }

    /// Update source and re-establish fresh analysis: write the file input,
    /// sync the session's options into the db (the #2885 gap, CLOSED here by
    /// option A — a caller whose only interaction is `update_and_analyze`
    /// now analyzes under the session's real options, no setter required),
    /// and eagerly pull the memoized analysis so the incremental cost lands
    /// at the edit rather than on the first query after it.
    pub fn update_and_analyze(&mut self, path: &str, source: String) -> FileId {
        let file_id = self.update_source(path, source);
        self.refresh_analysis();
        file_id
    }

    /// The analysis half of [`update_and_analyze`](Self::update_and_analyze):
    /// sync the session's options into the db (guarded) and eagerly pull the
    /// memoized analysis. Public so phase-timing callers (`brink-web`'s perf
    /// counters, `ide_bench`) can split "write the file input" from
    /// "re-establish analysis" without duplicating this composition.
    pub fn refresh_analysis(&mut self) {
        self.sync_db_options();
        self.db.analysis();
    }

    /// Get the underlying project database (for queries that need it).
    pub fn db(&self) -> &ProjectDb {
        &self.db
    }

    /// The project's analysis — the db's memoized `analysis_query` (option
    /// A). Always `Some`: an empty project analyzes to an empty result.
    /// The `Option` shape is kept for the ~40 existing consumers whose
    /// `None` arms became vacuous (fresh sessions no longer silently skip
    /// gates that treated `None` as "nothing to check").
    ///
    /// Borrow invariant: the returned reference is tied to the db's salsa
    /// storage — hold it across `&self` queries freely (every current
    /// consumer does exactly that), never across a `&mut self` call.
    pub fn analysis(&self) -> Option<&AnalysisResult> {
        Some(self.db.analysis())
    }

    /// Re-analyze the project with `overlay` (project-relative path → source)
    /// replacing the on-disk content of matching files, **without mutating this
    /// session**. Files absent from the overlay keep their current source.
    ///
    /// Returns the fresh analysis paired with the throwaway `ProjectDb` it was
    /// run against — the db reassigns `FileId`s, so callers must resolve any
    /// `FileId` in the result (paths, sources) through the returned db, not
    /// through this session.
    ///
    /// Used by safe-rename (and other hypothetical refactors) to gate on the
    /// diagnostics an edit *would* introduce before applying it. This re-lowers
    /// every file, so it is for one-shot author actions — never a hot path.
    #[must_use]
    pub fn analyze_overlay(
        &self,
        overlay: &std::collections::BTreeMap<String, String>,
    ) -> (AnalysisResult, ProjectDb) {
        let mut db = ProjectDb::new();
        for id in self.db.file_ids() {
            let Some(path) = self.db.file_path(id) else {
                continue;
            };
            let source = overlay
                .get(path)
                .cloned()
                .or_else(|| self.db.source(id).map(str::to_owned))
                .unwrap_or_default();
            db.update_file(path, source);
        }
        // The gate db's own options input (#1553), so any query a caller
        // reads back off the returned db is judged under the same policy as
        // the session's — not `AnalysisOptions::default()`.
        db.set_analysis_options(self.analysis_options());
        // The throwaway db's OWN `analysis_query` (option A, 2026-08-24 —
        // one engine): the gate's baseline is the session db's analysis, so
        // the overlay must be judged by the exact same road, or the
        // road-difference reads as "introduced" diagnostics. That was a real
        // bug in the monolith-road version this replaced: the db road
        // classifies nativeness per FILE while `analyze_with_modules` took
        // one project-wide flag, so a mixed set (mounted `.brink` stdlib in
        // an ink project) got the ink arm's `E051` over the stdlib in the
        // overlay result only — every structural op read as unsafe. Module
        // identity (#1526), module-map diagnostics (#1553), and per-file
        // nativeness (#1358) are all inside `analysis_query` already.
        let result = db.analysis().clone();
        (result, db)
    }

    /// Analyze a *complete* project projection — an explicit `path → source`
    /// map that stands in for the entire file set, **without mutating this
    /// session**. Unlike [`analyze_overlay`](Self::analyze_overlay) (which keeps
    /// the current paths and only substitutes sources), this replaces the whole
    /// project, so files may appear at *different* paths than they do now. Used
    /// by directory rename/move (#314), where the gate must model files that
    /// have relocated to new keys.
    ///
    /// Returns the fresh analysis paired with the throwaway `ProjectDb` it ran
    /// against — the db owns the new `FileId`s, so callers must resolve any
    /// `FileId` in the result (paths, sources) through the returned db.
    #[must_use]
    pub fn analyze_projection(
        &self,
        projection: &std::collections::BTreeMap<String, String>,
    ) -> (AnalysisResult, ProjectDb) {
        let mut db = ProjectDb::new();
        for (path, source) in projection {
            db.update_file(path, source.clone());
        }
        // Same as `analyze_overlay` (#1553): the gate db is judged under the
        // session's options, not the defaults.
        db.set_analysis_options(self.analysis_options());
        // The projected db's OWN `analysis_query` (option A, 2026-08-24 —
        // one engine, same reasoning as `analyze_overlay`). Everything the
        // monolith-road version threaded by hand is inside the query:
        // path-derived module identity after the move (#1526), the map's
        // stem-collision diagnostics a move can introduce (#1553), and
        // per-file extension-derived nativeness at the NEW keys (#1358).
        let result = db.analysis().clone();
        (result, db)
    }

    /// The current analysis options (registered host manifest +
    /// external-check / semantic-type-check severities + T1b compiler
    /// dialect + TM-3 typed-mode policy + resolved `[lints]` policy), for
    /// callers that run their own analysis/compile pass.
    /// `analyze_overlay`/`analyze_projection` use this, so the declared
    /// dialect and types policy carry through to their
    /// gate-check passes too (#611, #660).
    pub fn analysis_options(&self) -> AnalysisOptions {
        AnalysisOptions {
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
            semantic_type_check: self.semantic_type_check,
            dialect: self.language_dialect,
            types: self.type_policy,
            // See the matching note on `IdeSnapshot::analyze` (#1160/#1366):
            // the policy `set_lint_policy` resolved. Spelled out explicitly,
            // not `..Default::default()` — see that note for why.
            lints: self.lints.clone(),
            // D6/#3229: the session's own per-session debug-compile flag
            // (`set_emit_debug_info`). ON by default since the 2026-08-29
            // ruling (see the field's doc) — off only when a host's
            // App-settings opt-out turned it off. This method's result is
            // what `sync_db_options` writes into `ProjectDb`, so this
            // field is the single thing standing between a studio session
            // and a debuggable artifact.
            emit_debug_info: self.emit_debug_info,
            // `set_conventions` (issue #1880) — see the matching note on
            // `IdeSnapshot::analyze` above. This method's result is also
            // what `sync_db_options` writes into `ProjectDb`, so an
            // `IdeSession`-mounted project now reaches the `E169`
            // confinement check identically through both the off-db
            // snapshot path and the db-direct query surface, matching a
            // project compiled via `brink compile`/`brink check`.
            conventions: self.conventions.clone(),
        }
    }

    /// Compile the project rooted at `entry` on **this session's own
    /// `ProjectDb`** — the same db the analysis path reads (#1032). Sets the
    /// db's compile entry and analysis options as salsa inputs, then pulls the
    /// memoized `story_data` artifact query. Because compile and analysis now
    /// share one db (one file set, one lowering, one options input), a compile
    /// can never diverge from analysis on manifest/dialect/policy/file-state:
    /// the divergence that produced #1004 (manifest missing from compile) and
    /// its siblings is structurally unrepresentable rather than closed by
    /// wiring each input into a second, throwaway driver.
    ///
    /// `options` are the analysis options to compile under — pass
    /// [`analysis_options`](Self::analysis_options) for the session default, or
    /// an override for a per-call variation (e.g. compile-for-export). They are
    /// written to the db as an input; an unchanged value is a salsa no-op (no
    /// memo invalidation), so repeated compiles under the same options reuse the
    /// warm db's incremental results.
    ///
    /// **Editor diagnostic state and compiles share one truth** (option A,
    /// 2026-08-24 — this paragraph FORMERLY claimed compiles could not
    /// perturb the editor's off-db cached analysis; that cache no longer
    /// exists). A compile under the session's own options (the only
    /// production case — `brink-web` passes `analysis_options()`) is a
    /// guarded no-op on the options input, so it perturbs nothing. A compile
    /// under OVERRIDE options legitimately changes `db.analysis()` for every
    /// subsequent editor query: one db, one options input — an override
    /// caller owns restoring the session's options afterwards
    /// (`sync_db_options` runs on the next edit or setter regardless).
    ///
    /// The returned [`CompileProduct`]'s diagnostics are keyed by [`FileId`]
    /// into **this** db, so a caller resolves each to a path/source through the
    /// same session (`file_path`/`source`) — no throwaway-driver id remapping.
    ///
    /// ## Ruling: this stays on the imperative `set_file`/`set_entry` path (#1385)
    ///
    /// #1361 migrated `brink-web`'s one-shot `compile()`/`compile_fragment()`
    /// onto the #1306 producer (`brink_environment::Project::load` →
    /// `brink_environment::compile(&env)`), but deliberately scope-fenced
    /// `EditorSession::compile_project` — which delegates to this method —
    /// because it drives a different, incremental, live-editing db shared with
    /// `brink-lsp`. #1385 is the owner issue for resolving that fence, and the
    /// deliberate decision is: **do not migrate.** `IdeSession::compile` keeps
    /// pushing salsa inputs directly onto its own long-lived [`ProjectDb`].
    ///
    /// This ruling is scoped to **today's `compile(&Environment)` free
    /// function** (`crates/internal/brink-environment/src/lib.rs`), not to
    /// `Environment` as a value type in the abstract — see the last bullet
    /// below for the alternative that scoping deliberately excludes.
    /// Reasoning:
    ///
    /// - **`compile(&Environment)` is intentionally non-incremental.** Per its
    ///   own doc, it "seeds a **fresh** salsa `ProjectDb`" from a frozen,
    ///   point-in-time value on every call — "no ambient reads, no walk-up, no
    ///   I/O." That is exactly right for a one-shot mount handed a full
    ///   document snapshot per call (the CLI, `brink-web`'s stateless
    ///   `compile()`/`compile_fragment()`). It is exactly wrong for
    ///   `IdeSession`, whose entire reason to exist (see the module doc) is to
    ///   hold **one** persistent `ProjectDb` across many edits and queries so
    ///   an unrelated file's parse/HIR/analysis memos survive a single-file
    ///   keystroke edit. Routing every `compile_project` call through
    ///   *today's* `Project::load`/`compile(&env)` would re-walk the tree,
    ///   re-hash every source, and reseed a brand-new `Driver::new()` db on
    ///   each call (that function's own body does exactly that:
    ///   `set_analysis_options` + `set_file` per key + `set_entry` onto a
    ///   fresh driver) — discarding the incremental state this session exists
    ///   to keep warm, with no compensating benefit.
    /// - **As it stands, it would resurrect the #1004 divergence class.**
    ///   #1032 made compile and analysis share this one db specifically so
    ///   they can never diverge on manifest/dialect/policy/file-state (see
    ///   this method's opening paragraph). *Today's* `Project::load` +
    ///   `compile(&env)` builds an `Environment` from a fresh `SourceTree`
    ///   walk and compiles it on its own freshly-minted `ProjectDb` (via
    ///   `Driver::new()`). Wiring `compile_project` through that entry point
    ///   unmodified would put a *second* db back in the picture alongside
    ///   `IdeSession`'s own. This is a property of `compile`'s current body
    ///   (it always mints a throwaway driver), not an inherent property of
    ///   the `Environment` value type — see below.
    /// - **The live alternative, named and deferred, not dismissed.** A
    ///   `compile_into(&mut Driver, &Environment)` variant — same
    ///   `set_analysis_options`/`set_file`/`set_entry` push, applied to the
    ///   *session's own* `ProjectDb` instead of a fresh one — would preserve
    ///   incrementality outright (salsa's `set_file` is a no-op for unchanged
    ///   content, so unrelated files' memos survive) while keeping exactly
    ///   one db, sidestepping the #1004 divergence concern above by
    ///   construction. That is a real, live alternative, not a hypothetical
    ///   future-salsa escape hatch — it is deliberately **out of scope for
    ///   this ruling** because it requires designing and landing a new
    ///   `brink-environment` entry point, which #1385 did not ask this PR to
    ///   do. It is the natural next step if/when `IdeSession` is revisited to
    ///   compile against `Environment`-shaped input; bring it back as its own
    ///   proposal rather than treating this doc comment as having settled it.
    /// - **The adjacent design question is RESOLVED** (option A, ruled
    ///   2026-08-24): #1347's preserved call — whether live-typing analysis
    ///   routes through `ProjectDb`'s own `analysis_query` — was answered
    ///   yes, and landed; the off-db snapshot road no longer exists. This
    ///   ruling's compile-path conclusion (stay on the session's own
    ///   incremental db) is thereby REINFORCED, not disturbed: compile and
    ///   live analysis now literally share every query, not merely a db.
    ///
    /// **Relationship to the #1306 decision-log entry**
    /// (`docs/decision-log.md`, "Compilation environment as a deterministic,
    /// serializable input" — "producer vs. pure input"): that entry names
    /// `set_file`/`set_entry`/`set_analysis_options` as exactly the imperative
    /// push `Environment`/`compile(&env)` exists to reify, and folds "the LSP
    /// mount (#1131)" into the producer's scope — i.e. it anticipated
    /// `IdeSession`-shaped mounts eventually feeding `Environment` too. This
    /// ruling is a **narrower, present-tense carve-out**: it does not
    /// contradict #1306's long-run direction, it says today's
    /// `compile(&Environment)` shape (one throwaway db per call) is not yet
    /// the right fit for `IdeSession`'s incremental db, pending the
    /// `compile_into`-style seam above.
    ///
    /// # Errors
    /// Returns [`CompileEntryError::EntryNotFound`] if `entry` is not a file
    /// loaded in this session.
    pub fn compile(
        &mut self,
        entry: &str,
        options: &AnalysisOptions,
    ) -> Result<CompileProduct, CompileEntryError> {
        if self.db.set_entry(entry).is_none() {
            return Err(CompileEntryError::EntryNotFound(entry.to_owned()));
        }
        // Change-guarded like `sync_db_options` (#2885's resolution): an
        // equal-options compile must not stamp the revision — under option A
        // that would cold-invalidate the very analysis the editor reads.
        if self.db.analysis_options() != options {
            self.db.set_analysis_options(options.clone());
        }
        // `story_data()` is `Some` whenever an entry is set (just done above);
        // clone the memoized product out so the borrow on the db ends here.
        Ok(self.db.story_data().cloned().unwrap_or_default())
    }

    /// Look up a file's ID by path.
    pub fn file_id(&self, path: &str) -> Option<FileId> {
        self.db.file_id(path)
    }

    /// Project-relative paths of the current compile closure (#3017) —
    /// keyed by the entry the most recent [`compile`](Self::compile) set
    /// (`ProjectDb::compilation_closure`). Empty before any compile. Files
    /// the session holds but this list omits are on disk, not in the
    /// story — the "not analyzed" banner/marks read exactly that
    /// difference.
    pub fn compilation_closure_paths(&self) -> Vec<String> {
        self.db
            .compilation_closure()
            .into_iter()
            .filter_map(|id| self.db.file_path(id).map(str::to_owned))
            .collect()
    }

    /// Get the HIR for a file.
    pub fn hir(&self, id: FileId) -> Option<&HirFile> {
        self.db.hir(id)
    }

    /// Get the symbol manifest for a file.
    pub fn manifest(&self, id: FileId) -> Option<&SymbolManifest> {
        self.db.manifest(id)
    }

    /// Get the source text for a file.
    pub fn source(&self, id: FileId) -> Option<&str> {
        self.db.source(id)
    }

    /// Get the path for a file.
    pub fn file_path(&self, id: FileId) -> Option<&str> {
        self.db.file_path(id)
    }

    /// Get the parse tree root for a file.
    ///
    /// This always runs the **ink** frontend, regardless of the file's
    /// extension (`ProjectDb::parse`'s doc comment: `parse()` "stays
    /// ink-typed and untouched for the LSP/IDE ink path"). A native
    /// (`.brink`) file's *real* CST — the one its own HIR/resolutions were
    /// built from — is [`syntax_root_native`](Self::syntax_root_native); a
    /// caller presenting syntax to a user (semantic tokens, folding, ...)
    /// must check [`is_native`](Self::is_native) and use that instead, or it
    /// ends up classifying the ink-parsed garbage tree ink's grammar
    /// produces from native source text (issue #2280).
    pub fn syntax_root(&self, id: FileId) -> Option<brink_syntax::SyntaxNode> {
        self.db.parse(id).map(brink_syntax::Parse::syntax)
    }

    /// Get the native (`.brink`) parse tree root for a file (issue #2280) —
    /// the native sibling of [`syntax_root`](Self::syntax_root), backed by
    /// `ProjectDb::parse_native` (the same native CST `lowered_query` builds
    /// this file's HIR/resolutions from, so ranges from this root line up
    /// with [`crate::semantic_tokens`]'s resolution index). `None` for a
    /// file this session hasn't loaded, same as `syntax_root`.
    pub fn syntax_root_native(&self, id: FileId) -> Option<brink_syntax_native::SyntaxNode> {
        self.db
            .parse_native(id)
            .map(brink_syntax_native::Parse::syntax)
    }

    /// Whether `id` is a native (`.brink`) module rather than an ink file
    /// (issue #2280) — a thin pass-through to `ProjectDb::is_native`, so a
    /// query-layer caller (`brink-web`'s `semantic_tokens_impl`, ...) can
    /// pick [`syntax_root`](Self::syntax_root) vs
    /// [`syntax_root_native`](Self::syntax_root_native) without reaching
    /// past this session into the db directly.
    pub fn is_native(&self, id: FileId) -> bool {
        self.db.is_native(id)
    }
}

impl Default for IdeSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use brink_ir::{
        BaseType, Constraint, DiagnosticCode, ExternalKind, HostManifest, ManifestExternal,
        ManifestParam, SemanticTypeDef, TypeRef,
    };

    use super::{
        AnalysisOptions, Dialect, ExternalCheckSeverity, IdeSession,
        SemanticTypeDiagnosticSeverity, TypePolicy,
    };

    fn color_manifest() -> HostManifest {
        HostManifest {
            markup: Vec::new(),
            externals: vec![ManifestExternal {
                name: "tint".into(),
                params: vec![ManifestParam {
                    name: "c".into(),
                    ty: TypeRef("color".into()),
                }],
                returns: TypeRef::default(),
                kind: ExternalKind::Presentation,
                doc: None,

                widgets: vec![],
                path: Vec::new(),
            }],
            types: vec![SemanticTypeDef {
                name: "color".into(),
                base: BaseType::String,
                constraint: Some(Constraint::Enum {
                    values: vec!["#FF0000".into()],
                }),
                values: None,
                widget: None,
            }],
        }
    }

    const SRC: &str = "EXTERNAL tint(c)\n~ tint(\"nope\")\n-> END\n";

    #[test]
    fn registered_manifest_drives_checks_and_enrichment() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", SRC.to_string());
        session.set_host_manifest(color_manifest());
        let analysis = session.analysis().expect("analysis");

        // Enrichment is surfaced.
        assert!(
            analysis
                .symbol_meta
                .values()
                .any(|m| m.kind == ExternalKind::Presentation),
            "symbol_meta should carry the registered kind"
        );
        // The closed-domain (enum) violation on the literal "nope" is flagged.
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E042),
            "expected E042, got {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn external_check_off_suppresses_diagnostics() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", SRC.to_string());
        session.set_external_check(ExternalCheckSeverity::Off);
        session.set_host_manifest(color_manifest());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E042),
            "Off should suppress manifest diagnostics"
        );
        // ...but enrichment is still built.
        assert!(!analysis.symbol_meta.is_empty(), "meta built even when Off");
    }

    // ── Markup vocabulary (#1733, docs/prose-dialect-spec.md §4.2) ──────
    //
    // End-to-end through the surface a host actually drives: register a
    // manifest on a live session (the same call `@brink-lang/web`'s
    // `EditorHandle::set_host_manifest` makes) and read the diagnostics the
    // editor renders. Nothing below reaches into the analyzer pass directly.

    /// Native prose with one declared tag and one undeclared one.
    const MARKUP_SRC: &str =
        "flow a() {\n  <wave amount=\"3\">shimmer</wave> <glitch>zap</glitch>\n}\n";

    fn markup_manifest() -> HostManifest {
        HostManifest {
            markup: vec![brink_ir::ManifestSpanKind {
                name: "wave".into(),
                attrs: vec![brink_ir::ManifestSpanAttr {
                    name: "amount".into(),
                    required: false,
                    ty: None,
                }],
            }],
            ..HostManifest::default()
        }
    }

    #[test]
    fn markup_is_freeform_with_no_manifest_registered() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.brink", MARKUP_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| matches!(d.code, DiagnosticCode::E164 | DiagnosticCode::E165)),
            "freeform-by-default: undeclared markup must not be diagnosed: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn markup_is_still_freeform_under_an_externals_only_manifest() {
        // A host that registers a manifest for its *externals* has not opted
        // into markup checking — `markup` is the only key that tightens.
        let mut session = IdeSession::new();
        session.update_and_analyze("t.brink", MARKUP_SRC.to_string());
        session.set_host_manifest(color_manifest());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| matches!(d.code, DiagnosticCode::E164 | DiagnosticCode::E165)),
            "an externals-only manifest must not enable markup checking: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn a_declared_markup_vocabulary_diagnoses_the_undeclared_tag_only() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.brink", MARKUP_SRC.to_string());
        session.set_host_manifest(markup_manifest());
        let analysis = session.analysis().expect("analysis");

        let markup: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| matches!(d.code, DiagnosticCode::E164 | DiagnosticCode::E165))
            .collect();
        assert_eq!(
            markup.len(),
            1,
            "exactly the undeclared `<glitch>` should be flagged: {:?}",
            analysis.diagnostics
        );
        assert_eq!(markup[0].code, DiagnosticCode::E164);
        assert!(
            markup[0].message.contains("<glitch>"),
            "message must name the tag: {}",
            markup[0].message
        );
    }

    #[test]
    fn clearing_the_manifest_returns_markup_to_freeform() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.brink", MARKUP_SRC.to_string());
        session.set_host_manifest(markup_manifest());
        session.clear_host_manifest();
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| matches!(d.code, DiagnosticCode::E164 | DiagnosticCode::E165)),
            "clearing the manifest must restore the freeform default: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn the_salsa_compile_path_also_reports_markup_diagnostics_as_warnings() {
        // The three tests above read the off-db background analysis (what the
        // editor squiggles). This one drives `compile`, the *other* consumer
        // — `brink-web`'s `EditorSession::compile` returns `product.warnings`
        // to JS as its `warnings` array — so the db's
        // `per_file_diagnostics_query` reaches the check too, and a project
        // compiling with a declared vocabulary really does see it.
        let mut session = IdeSession::new();
        session.update_and_analyze(
            "main.brink",
            "flow main() {\n  <glitch>zap</glitch>\n  -> END\n}\n".to_string(),
        );
        let options = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            host_manifest: Some(markup_manifest()),
            ..Default::default()
        };
        let product = session
            .compile("main.brink", &options)
            .expect("entry file is loaded");

        assert!(
            product
                .warnings
                .iter()
                .any(|d| d.code == DiagnosticCode::E164),
            "expected E164 in compile warnings: errors={:?} warnings={:?}",
            product.errors,
            product.warnings
        );
        // A `Warning` never blocks the artifact — freeform-by-default's
        // sibling guarantee: tightening the vocabulary reports, it does not
        // stop a project from compiling until `[lints]` says so.
        assert!(
            product.story.is_some(),
            "a markup warning must not block the story: {:?}",
            product.errors
        );
    }

    /// #532: ink with a host semantic type param and no registered manifest.
    const HOST_TYPE_SRC: &str = "\
/// @param who {actor_id}
EXTERNAL add_state(who)
";

    #[test]
    fn semantic_type_check_defaults_to_tolerant_with_no_manifest() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", HOST_TYPE_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E040),
            "default (Tolerant) with no manifest: no E040: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn semantic_type_check_error_diagnoses_with_no_manifest() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", HOST_TYPE_SRC.to_string());
        session.set_semantic_type_check(SemanticTypeDiagnosticSeverity::Error);
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E040),
            "Error with no manifest: E040 fires: {:?}",
            analysis.diagnostics
        );
    }

    /// #611: a `~ { … }` multi-line logic block is brink-extension syntax
    /// (docs/t1b-surface-spec.md §1) — flagged `E051` under the default
    /// `StrictInk` dialect, silent under `Brink`.
    const BRINK_EXT_SRC: &str = "~ {\n    temp x = 0\n}\n-> END\n";

    #[test]
    fn strict_ink_default_flags_e051_on_extension_syntax() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", BRINK_EXT_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "StrictInk default: E051 stands: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_brink_suppresses_e051_on_analyze() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", BRINK_EXT_SRC.to_string());
        session.set_language_dialect(Dialect::Brink);
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "brink dialect: no E051 on valid extension syntax: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_reanalyzes_files_added_afterward() {
        // The dialect is set before the file is even loaded — `reanalyze`
        // must be picked up by the subsequent `update_and_analyze` (which
        // re-snapshots and re-reads `language_dialect`), not just by files
        // present at `set_language_dialect` time.
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.update_and_analyze("t.ink", BRINK_EXT_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "brink dialect set before load: no E051: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn analyze_overlay_and_analyze_projection_respect_declared_dialect() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", "-> END\n".to_string());
        session.set_language_dialect(Dialect::Brink);

        let mut overlay = std::collections::BTreeMap::new();
        overlay.insert("t.ink".to_string(), BRINK_EXT_SRC.to_string());
        let (overlay_result, _db) = session.analyze_overlay(&overlay);
        assert!(
            !overlay_result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "analyze_overlay: brink dialect carries through: {:?}",
            overlay_result.diagnostics
        );

        let mut projection = std::collections::BTreeMap::new();
        projection.insert("t.ink".to_string(), BRINK_EXT_SRC.to_string());
        let (projection_result, _db) = session.analyze_projection(&projection);
        assert!(
            !projection_result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "analyze_projection: brink dialect carries through: {:?}",
            projection_result.diagnostics
        );
    }

    /// #660/#1127: with no explicit `set_type_policy`, the effective policy
    /// is the dialect-keyed default (2026-07-19 ruling) — `Gradual` under
    /// the default `StrictInk` dialect (byte-identical to pre-#619
    /// behavior), `Strict` once the dialect is `Brink`.
    #[test]
    fn type_policy_default_is_dialect_keyed() {
        let mut session = IdeSession::new();
        assert_eq!(session.type_policy(), TypePolicy::Gradual);
        assert_eq!(
            session.analysis_options().type_policy(),
            TypePolicy::Gradual,
            "analysis_options must mirror the resolved default"
        );

        session.set_language_dialect(Dialect::Brink);
        assert_eq!(session.type_policy(), TypePolicy::Strict);
        assert_eq!(
            session.analysis_options().type_policy(),
            TypePolicy::Strict,
            "brink dialect with no explicit types defaults strict (#1127)"
        );
    }

    /// #660 (TM-3 basic reachability): `types = strict` under the default
    /// `StrictInk` dialect is a project-level config error (`E064`) — the
    /// IDE surface must reach this the same way the compiler CLI's
    /// `--types strict --dialect strict-ink` does.
    #[test]
    fn set_type_policy_strict_with_strict_ink_dialect_is_config_error() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", "-> END\n".to_string());
        session.set_type_policy(TypePolicy::Strict);
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E064),
            "types=strict + dialect=strict-ink (default): expected E064: {:?}",
            analysis.diagnostics
        );
    }

    /// #660: with `dialect = brink`, `types = strict` turns on the
    /// Unknown-escape check (`E065`) on `analyze` — proving the setter
    /// reaches the real strict-mode checks, not just the config-error path.
    #[test]
    fn set_type_policy_strict_with_brink_dialect_flags_unknown_escape() {
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.set_type_policy(TypePolicy::Strict);
        session.update_and_analyze("t.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "types=strict + dialect=brink: expected E065 on unused param `x`: {:?}",
            analysis.diagnostics
        );
    }

    /// B0.9 native strict-only enforcement (issue #1342): the IDE/editor
    /// surface (`brink-web`'s `EditorSession::compile_project`, which calls
    /// `IdeSession::compile` exactly as here — see that method's doc)
    /// reaches `E137` the same way `brink_compiler::compile_path_with_options`
    /// does — proving the salsa `story_data()` compile path (not just the
    /// pure `analyze_with_options` diagnostics path `set_type_policy`'s
    /// sibling tests above exercise) also hits the gate.
    #[test]
    fn compile_native_file_with_explicit_gradual_types_reports_e137() {
        let mut session = IdeSession::new();
        session.update_and_analyze(
            "main.brink",
            "flow main() {\n  Hello. -> END\n}\n".to_string(),
        );
        let options = AnalysisOptions {
            types: Some(TypePolicy::Gradual),
            ..Default::default()
        };
        let product = session
            .compile("main.brink", &options)
            .expect("entry file is loaded");

        assert!(
            product
                .errors
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E137),
            "types=gradual native compile: expected E137: {:?}",
            product.errors
        );
        assert!(
            product.story.is_none(),
            "a hard error must not emit a story"
        );
    }

    /// The paired positive case: `types = strict` on the same native entry
    /// compiles cleanly (no `E137`).
    #[test]
    fn compile_native_file_with_explicit_strict_types_has_no_e137() {
        let mut session = IdeSession::new();
        session.update_and_analyze(
            "main.brink",
            "flow main() {\n  Hello. -> END\n}\n".to_string(),
        );
        let options = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..Default::default()
        };
        let product = session
            .compile("main.brink", &options)
            .expect("entry file is loaded");

        assert!(
            !product
                .errors
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E137),
            "types=strict native compile: expected no E137: {:?}",
            product.errors
        );
        assert!(product.story.is_some(), "expected a compiled story");
    }

    /// #1127 (default flip): under `dialect = brink` with NO explicit
    /// `set_type_policy`, the effective policy is `Strict`, so the
    /// Unknown-escape check fires — the strict default reaches the analysis
    /// path without any opt-in call.
    #[test]
    fn brink_dialect_default_flags_unknown_escape() {
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.update_and_analyze("t.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "brink-dialect default is strict (#1127): expected E065: {:?}",
            analysis.diagnostics
        );
    }

    /// #1127: `set_type_policy(Gradual)` remains the explicit opt-out knob
    /// under `dialect = brink` — the same construct is silent again.
    #[test]
    fn explicit_gradual_opt_out_suppresses_unknown_escape() {
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.set_type_policy(TypePolicy::Gradual);
        session.update_and_analyze("t.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "explicit gradual opt-out must not flag E065: {:?}",
            analysis.diagnostics
        );
    }

    /// #660: `analyze_overlay`/`analyze_projection` must carry the declared
    /// types policy through to their gate-check passes too, mirroring the
    /// existing dialect coverage.
    #[test]
    fn analyze_overlay_and_analyze_projection_respect_declared_type_policy() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", "-> END\n".to_string());
        session.set_language_dialect(Dialect::Brink);
        session.set_type_policy(TypePolicy::Strict);

        let mut overlay = std::collections::BTreeMap::new();
        overlay.insert(
            "t.ink".to_string(),
            "=== noop(x) ===\nHello.\n-> DONE\n".to_string(),
        );
        let (overlay_result, _db) = session.analyze_overlay(&overlay);
        assert!(
            overlay_result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "analyze_overlay: strict types policy carries through: {:?}",
            overlay_result.diagnostics
        );

        let mut projection = std::collections::BTreeMap::new();
        projection.insert(
            "t.ink".to_string(),
            "=== noop(x) ===\nHello.\n-> DONE\n".to_string(),
        );
        let (projection_result, _db) = session.analyze_projection(&projection);
        assert!(
            projection_result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "analyze_projection: strict types policy carries through: {:?}",
            projection_result.diagnostics
        );
    }

    #[test]
    fn projection_cache_shares_and_invalidates() {
        let mut s = IdeSession::new();
        let file = s.update_and_analyze("main.ink", "=== a ===\n-> DONE\n".to_owned());

        let p1 = s.projection(file).expect("projection");
        let p2 = s.projection(file).expect("projection");
        assert!(Arc::ptr_eq(&p1, &p2), "same generation → shared Arc");

        // A source update invalidates: new Arc, new content.
        let file = s.update_and_analyze("main.ink", "=== a ===\n=== b ===\n-> DONE\n".to_owned());
        let p3 = s.projection(file).expect("projection");
        assert!(!Arc::ptr_eq(&p1, &p3), "update → fresh projection");

        // With analysis present the cached flavor carries identity.
        assert!(
            p3.spans.iter().any(|sp| sp.def_id.is_some()),
            "identity-joined flavor cached when analysis exists"
        );
    }

    // ── apply_analysis_options (issue #2334's shared seam) ──────────

    /// The one seam every `IdeSession` producer forwards a resolved
    /// `[project]`/`[lints]` policy through must actually forward all four
    /// of the fields it claims to — the completeness the issue's own
    /// "spelled-out-not-Default" fix asks for. Every value here is
    /// deliberately non-default (a session default could never produce any
    /// of them), so this only stays green if `apply_analysis_options`
    /// really writes each field, not merely if some already sat at the
    /// asserted value.
    #[test]
    fn apply_analysis_options_forwards_dialect_types_lints_and_conventions_in_one_call() {
        let mut session = IdeSession::new();
        let lints = brink_analyzer::LintPolicy {
            deny_warnings: true,
            ..brink_analyzer::LintPolicy::default()
        };
        let options = AnalysisOptions {
            host_manifest: None,
            external_check: ExternalCheckSeverity::default(),
            semantic_type_check: SemanticTypeDiagnosticSeverity::default(),
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Gradual),
            lints,
            emit_debug_info: false,
            conventions: Some("conventions.brink".to_owned()),
        };

        session.apply_analysis_options(&options);

        assert_eq!(
            session.language_dialect(),
            Dialect::Brink,
            "apply_analysis_options must forward dialect"
        );
        assert_eq!(
            session.type_policy_override(),
            Some(TypePolicy::Gradual),
            "apply_analysis_options must forward types"
        );
        assert!(
            session.lint_policy().deny_warnings,
            "apply_analysis_options must forward lints"
        );
        assert_eq!(
            session.conventions(),
            Some("conventions.brink"),
            "apply_analysis_options must forward conventions"
        );
    }

    /// `apply_analysis_options` must leave `host_manifest`/`external_check`/
    /// `semantic_type_check` alone — those three are deliberately out of
    /// scope for this seam (see its own doc comment): a producer manages
    /// them through their own dedicated setters, so a call built from an
    /// options value with defaults in those three slots must not clobber a
    /// manifest/severity a caller registered separately through
    /// `set_host_manifest`/`set_external_check`/`set_semantic_type_check`.
    #[test]
    fn apply_analysis_options_does_not_touch_host_manifest_or_check_severities() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", SRC.to_string());
        session.set_host_manifest(color_manifest());
        session.set_semantic_type_check(SemanticTypeDiagnosticSeverity::Error);

        // Must differ from the session's already-registered
        // `dialect`/`types`/`lints`/`conventions` (all still at their
        // `IdeSession::new()` defaults here) so `apply_analysis_options`'s
        // `changed` guard actually trips and the seam runs its body,
        // instead of the whole call short-circuiting before touching
        // anything — which is exactly how this test stayed green with
        // `apply_analysis_options` doing nothing at all (#2334 review).
        session.apply_analysis_options(&AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        });

        // Confirms the seam actually ran (not merely that the `changed`
        // guard happened to short-circuit before doing any damage).
        assert_eq!(
            session.language_dialect(),
            Dialect::Brink,
            "sanity: apply_analysis_options must have forwarded the changed dialect"
        );

        let analysis = session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E042),
            "apply_analysis_options must not clear the registered host manifest: {:?}",
            analysis.diagnostics
        );
        assert_eq!(
            session.analysis_options().semantic_type_check,
            SemanticTypeDiagnosticSeverity::Error,
            "apply_analysis_options must not reset the registered semantic-type-check severity"
        );
    }

    /// Regression for the actual bug pattern (#1880/#2317): a producer that
    /// resolves `AnalysisOptions` from `brink.toml` and forwards it through
    /// this one seam reaches the `E169` confinement check exactly like the
    /// db-direct path does — proving the seam's `conventions` forwarding is
    /// live end to end, not merely a field assignment nobody reads.
    #[test]
    fn apply_analysis_options_conventions_reaches_the_confinement_check() {
        // Mirrors `brink-web`'s
        // `compile_project_does_not_misfire_e169_once_conventions_reaches_the_editor_session`
        // fixture exactly (a claim handler and the flow it claims into, in
        // one file) — proven end to end there through `EditorSession`; here
        // through `IdeSession::apply_analysis_options` directly.
        const CLAIMING_HANDLER: &str = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", \
             order = 10)]\nfn interior(place: content) {\n  return place;\n}\n";
        let mut session = IdeSession::new();
        session.update_and_analyze(
            "conventions.brink",
            format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
        );

        // No `conventions` pointer registered yet: the handler above is
        // unconfined, so `E169` fires — sanity that the fixture actually
        // reaches the check before asserting the fix clears it.
        let unconfined = session.analysis().expect("analysis");
        assert!(
            unconfined
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E169),
            "sanity: an unconfigured conventions pointer must misfire E169: {:?}",
            unconfined.diagnostics
        );

        // Registering the pointer through the seam must clear it.
        session.apply_analysis_options(&AnalysisOptions {
            conventions: Some("conventions.brink".to_owned()),
            ..AnalysisOptions::default()
        });
        let confined = session.analysis().expect("analysis");
        assert!(
            !confined
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E169),
            "apply_analysis_options's conventions forwarding must reach the confinement check: {:?}",
            confined.diagnostics
        );
    }
}
