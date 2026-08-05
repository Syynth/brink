use std::collections::BTreeMap;

use brink_ide::session::IdeSession;
use wasm_bindgen::prelude::*;

use crate::compile::{CompileResult, DiagnosticJs};

mod code_actions;
mod completion;
mod doc_handles;
mod folding;
mod hints;
mod hover;
mod navigation;
mod outline;
mod refactor;
mod spans;
mod story_graph;
mod transform;

// ── EditorSession ───────────────────────────────────────────────────

/// Stateful IDE session for the web editor. Wraps `IdeSession` and exposes
/// all IDE queries as methods that return JSON strings.
/// A view context scopes the editor to a sub-region of a file.
/// When active, `update_source` splices the fragment into the full file
/// at `[start, end)`, and IDE responses adjust offsets relative to the view.
#[derive(Clone, Copy)]
struct ViewContext {
    /// Byte offset where the view begins in the full file.
    start: u32,
    /// Byte offset where the view ends (exclusive) in the full file.
    end: u32,
    /// 0-based line number of the view start (for line-based IDE responses).
    start_line: u32,
    /// Whether `full[original_end..]` started with `\n` when the context was set.
    /// When true, `update_source` ensures a `\n` separator is maintained after
    /// the fragment, so edits at the end don't merge with the next section.
    trailing_newline: bool,
}

/// A document handle's state: the file it addresses plus an optional
/// sub-file view context (fragment handles).
struct DocState {
    path: String,
    view: Option<ViewContext>,
}

#[wasm_bindgen]
pub struct EditorSession {
    session: IdeSession,
    /// The active file path for IDE queries (legacy singleton API).
    active_path: String,
    /// Optional sub-file view context for focused editing (legacy singleton API).
    view: Option<ViewContext>,
    /// Open document handles, keyed by id. `BTreeMap` for deterministic
    /// iteration order (project rule: never iterate a `HashMap` where order
    /// can affect output).
    docs: BTreeMap<u32, DocState>,
    /// Next document-handle id. Starts at 1 — 0 is the "invalid handle"
    /// sentinel returned by `open_document`/`open_fragment` on failure.
    next_doc_id: u32,
    /// Whether `folding_ranges`/`folding_ranges_doc` compute the
    /// machinery/narrative fold runs (#479). Off by default: the runs only
    /// matter to hosts that implement prose/logic view modes, and computing
    /// them costs a full per-line classification on every folding query.
    fold_runs_enabled: bool,
    /// The T1b compiler dialect (docs/t1b-surface-spec.md §1), set via
    /// `set_language_dialect`. Defaults to `StrictInk`, matching
    /// `AnalysisOptions::default()`. Gates whether stdlib slice 1
    /// completion/signature help are offered (#589, #600), mirroring
    /// `brink-lsp`'s `Backend::dialect` — kept as its own field (rather than
    /// reading back through `session`) because those two call sites need a
    /// plain value, not an `Option<&_>`/reference. Since #611 it is also
    /// forwarded to `IdeSession::set_language_dialect`, so it gates the
    /// background analysis pass's `E051` diagnostic too.
    dialect: brink_analyzer::Dialect,
    /// Whether `set_language_dialect` has been called explicitly on this
    /// session (#1005). `apply_project_config` skips `dialect` when true —
    /// explicit API calls always override a discovered `brink.toml`'s
    /// `[project] dialect`, mirroring the CLI's `--dialect` flag precedence.
    dialect_explicit: bool,
    /// Same as `dialect_explicit`, for `set_type_policy` (#1005).
    types_explicit: bool,
    /// The `[lints]` policy last resolved from an applied `brink.toml`
    /// (issue #1417) — the baseline `set_lint_overrides`/
    /// `set_deny_warnings_override` layer their explicit overrides on top
    /// of, via `reapply_lint_overrides`. Tracked separately from
    /// `self.session.lint_policy()` (the *combined*, already-overridden
    /// result actually in effect) so an override can be **cleared** and
    /// correctly revert to the file's policy — recomputing on top of the
    /// already-combined policy would make a cleared override "stick"
    /// (the same accumulation bug #1397 fixed for the file tier itself).
    /// Defaults to `LintPolicy::default()` (no file ever applied).
    file_lint_policy: brink_analyzer::LintPolicy,
    /// Explicit CLI/API-tier per-code lint-level overrides (issue #1417),
    /// set via `set_lint_overrides`. The wasm counterpart of the
    /// compiler CLI's repeatable `--deny`/`--warn`/`--allow` flags (#1373)
    /// and `brink-lsp`'s `initializationOptions.lints`. Always wins over
    /// the same code in `file_lint_policy` — applied via
    /// `reapply_lint_overrides` under the #1005 explicit-over-file-over-default
    /// precedence rule.
    lint_overrides: BTreeMap<String, brink_analyzer::LintLevel>,
    /// Explicit `deny-warnings` override (issue #1417), parallel to
    /// `lint_overrides`. `None` means unset — `file_lint_policy.deny_warnings`
    /// (or `false`, absent a file) applies.
    deny_warnings_override: Option<bool>,
}

impl Default for EditorSession {
    fn default() -> Self {
        Self::new()
    }
}
#[wasm_bindgen]
impl EditorSession {
    /// Create a new empty editor session.
    #[wasm_bindgen(constructor)]
    pub fn new() -> EditorSession {
        EditorSession {
            session: IdeSession::new(),
            active_path: "main.ink".to_owned(),
            view: None,
            docs: BTreeMap::new(),
            next_doc_id: 1,
            fold_runs_enabled: false,
            dialect: brink_analyzer::Dialect::StrictInk,
            dialect_explicit: false,
            types_explicit: false,
            file_lint_policy: brink_analyzer::LintPolicy::default(),
            lint_overrides: BTreeMap::new(),
            deny_warnings_override: None,
        }
    }

    /// Update the active file's source text. Reparses, lowers, and analyzes.
    ///
    /// When a view context is active, `source` is treated as a fragment that
    /// gets spliced into the full file at `[view.start, view.end)`.
    pub fn update_source(&mut self, source: &str) {
        if let Some(view) = self.view {
            let full = self
                .session
                .file_id(&self.active_path)
                .and_then(|id| self.session.source(id).map(str::to_owned))
                .unwrap_or_default();
            let outcome = splice_fragment(&full, &view, source);
            if let Some(v) = &mut self.view {
                v.end = outcome.new_view_end;
            }
            self.session
                .update_and_analyze(&self.active_path, outcome.spliced);
        } else {
            self.session
                .update_and_analyze(&self.active_path, source.to_owned());
        }
    }

    /// Add or update a file by path. Re-analyzes the project.
    pub fn update_file(&mut self, path: &str, source: &str) {
        self.session.update_and_analyze(path, source.to_owned());
    }

    /// Remove a file from the project.
    pub fn remove_file(&mut self, path: &str) {
        self.session.remove_file(path);
    }

    /// Register (or replace) the host-capability manifest from a JSON string,
    /// then re-analyze. The manifest describes the host's external-function
    /// vocabulary (types, semantic types) for author-time validation and
    /// richer hover/completion. Tooling-only — never affects the runtime.
    pub fn set_host_manifest(&mut self, json: &str) -> Result<(), JsError> {
        let manifest: brink_ir::HostManifest = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid host manifest: {e}")))?;
        self.session.set_host_manifest(manifest);
        Ok(())
    }

    /// Clear any registered host manifest, then re-analyze.
    pub fn clear_host_manifest(&mut self) {
        self.session.clear_host_manifest();
    }

    /// Register (or replace) the dialogue dialect (#368) from a JSON string.
    /// The dialect describes the project's dialogue-line conventions (cues,
    /// parentheticals, dialogue chains) so `line_contexts` can classify
    /// lines without hardcoding any one convention. Tooling-only — never
    /// affects the runtime or analysis; consumed at query time by
    /// `line_contexts`/`line_contexts_doc`. Mirrors `set_host_manifest`.
    pub fn set_dialect(&mut self, json: &str) -> Result<(), JsError> {
        let dialect: brink_ir::DialogueDialect = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid dialect: {e}")))?;
        brink_ir::dialect::validate(&dialect)
            .map_err(|errs| JsError::new(&format!("invalid dialect: {errs:?}")))?;
        let resolved = brink_ir::ResolvedDialect::compile(&dialect)
            .map_err(|e| JsError::new(&format!("invalid dialect: {e}")))?;
        self.session.set_dialect(resolved);
        Ok(())
    }

    /// Clear the registered dialect. `line_contexts` reverts to plain
    /// structural classification.
    pub fn clear_dialect(&mut self) {
        self.session.clear_dialect();
    }

    /// Enable or disable the machinery/narrative fold runs (#479 — off by
    /// default). Hosts that implement prose/logic view modes turn this on
    /// (typically once at mount, alongside activating the fold kinds in the
    /// editor); everyone else skips the per-query run computation entirely.
    /// Session-wide, like `set_dialect`.
    pub fn set_fold_runs_enabled(&mut self, enabled: bool) {
        self.fold_runs_enabled = enabled;
    }

    /// Set the T1b compiler dialect (docs/t1b-surface-spec.md §1, #589,
    /// #600, #611): `"brink"` or `"strict-ink"`; any other value (or never
    /// calling this at all) keeps the `StrictInk` default. Mirrors
    /// `brink-lsp`'s `initializationOptions.dialect` handling. Gates stdlib
    /// slice 1 completion (`completions`/`completions_doc`), dialect-aware
    /// signature help (`signature_help`/`signature_help_doc`), and — since
    /// #611 — the background analysis pass's `E051` "brink extension"
    /// diagnostic: a `brink`-dialect project no longer shows permanent
    /// spurious `E051` on valid extension syntax. Re-analyzes immediately
    /// (like `set_external_check`/`set_semantic_type_check`).
    pub fn set_language_dialect(&mut self, value: &str) {
        self.dialect = match value {
            "brink" => brink_analyzer::Dialect::Brink,
            _ => brink_analyzer::Dialect::StrictInk,
        };
        self.dialect_explicit = true;
        self.session.set_language_dialect(self.dialect);
    }

    /// Set the TM-3 typed-mode policy (docs/typed-mode-spec.md §1, #660):
    /// `"strict"` or `"gradual"`. An unrecognized value is ignored — it
    /// behaves exactly like never calling this at all (the pre-NS-A9
    /// contract "any other value keeps the default", carried forward), so
    /// garbage input can never silently opt a brink session out of strict.
    /// Never calling this (and having no `brink.toml` `types` key) leaves
    /// the dialect-keyed default in effect (issue #1127, ruled 2026-07-19):
    /// `"brink"` → strict, `"strict-ink"` → gradual. An explicit call
    /// always wins. Mirrors `set_language_dialect`
    /// exactly — this is the compile-facing counterpart of the compiler
    /// CLI's `--types strict`, previously reachable only there (PR #656 left
    /// `IdeSession` hardcoded to `Gradual`). `TypePolicy::Strict` requires
    /// `language_dialect() == "brink"`, or `compile_project`/the background
    /// analysis surface a single project-level `E064` config-error
    /// diagnostic instead of running the normal passes (the caller's
    /// responsibility, same as the CLI). Re-analyzes immediately (like
    /// `set_language_dialect`).
    ///
    /// **wasm-observable**: `compile_project` reads this through
    /// `IdeSession::analysis_options`, so a project that opts into
    /// `types = strict` now gets the `E065`/`E066`/`E067` strict-mode
    /// diagnostics (or the `E064` config error, if `dialect` isn't also
    /// `"brink"`) surfaced through `@brink-lang/web`'s compile/editor entry
    /// points — behavior no wasm consumer could previously reach at all.
    pub fn set_type_policy(&mut self, value: &str) {
        let types = match value {
            "strict" => brink_analyzer::TypePolicy::Strict,
            "gradual" => brink_analyzer::TypePolicy::Gradual,
            // Unrecognized: behave like unset — keep the dialect-keyed
            // default (and any earlier explicit choice) in effect.
            _ => return,
        };
        self.types_explicit = true;
        self.session.set_type_policy(types);
    }

    /// Set explicit CLI/API-tier per-code lint-level overrides from a JSON
    /// object `{ "<CODE>": "deny" | "warn" | "allow" | "info" | "hint" }`
    /// (issue #1417; `"info"`/`"hint"` added by issue #1162) — the
    /// wasm/editor counterpart of `brink compile`'s repeatable
    /// `--deny`/`--warn`/`--allow <CODE>` flags (#1373) and `brink-lsp`'s
    /// `initializationOptions.lints`, extending the same
    /// `AnalysisOptions::apply_lint_overrides` seam those two established
    /// to the embedded editor surface. Wholesale **replaces** this
    /// session's explicit override map (mirrors `apply_parsed_config`'s
    /// own `[lints]`-replace-not-merge semantics, #1397) — call with
    /// `"{}"` to clear every override.
    ///
    /// Always wins over the same code in an applied `brink.toml`'s
    /// `[lints]` table, in either call order: this reapplies on top of
    /// whatever `apply_project_config`/`discover_project_config` last
    /// resolved from the file, and the file tier itself reapplies these
    /// overrides on its own next call (`Self::reapply_lint_overrides` is
    /// the shared tail both funnel through) — so a `brink.toml` reload can
    /// never silently drop a previously-set explicit override.
    ///
    /// Errors only on malformed JSON. An unrecognized per-code level
    /// string (anything but `"deny"`/`"warn"`/`"allow"`/`"info"`/`"hint"`)
    /// and an unrecognized/non-overridable diagnostic code are never hard
    /// errors — both are reported as warning strings in the returned JSON
    /// array (a `string[]`), the same "warn, never silently drop" channel
    /// `apply_project_config` already uses. Re-analyzes immediately.
    pub fn set_lint_overrides(&mut self, json: &str) -> Result<String, JsError> {
        let raw: BTreeMap<String, String> = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid lint overrides: {e}")))?;
        let mut overrides = BTreeMap::new();
        let mut warnings = Vec::new();
        for (code, level) in raw {
            match level.as_str() {
                "deny" => {
                    overrides.insert(code, brink_analyzer::LintLevel::Deny);
                }
                "warn" => {
                    overrides.insert(code, brink_analyzer::LintLevel::Warn);
                }
                "allow" => {
                    overrides.insert(code, brink_analyzer::LintLevel::Allow);
                }
                "info" => {
                    overrides.insert(code, brink_analyzer::LintLevel::Info);
                }
                "hint" => {
                    overrides.insert(code, brink_analyzer::LintLevel::Hint);
                }
                other => warnings.push(format!(
                    "[lints] `{code}` has unrecognized level `{other}` (expected \"allow\" | \"warn\" | \"deny\" | \"info\" | \"hint\"); ignored"
                )),
            }
        }
        self.lint_overrides = overrides;
        warnings.extend(self.reapply_lint_overrides());
        Ok(serde_json::to_string(&warnings).unwrap_or_default())
    }

    /// Set an explicit `deny-warnings` override (issue #1417), parallel to
    /// [`Self::set_lint_overrides`] — the wasm/editor counterpart of
    /// `brink compile`'s `-D warnings` and `brink-lsp`'s
    /// `initializationOptions.denyWarnings`. Always wins over an applied
    /// `brink.toml`'s `deny-warnings` key. Re-analyzes immediately.
    pub fn set_deny_warnings_override(&mut self, deny: bool) {
        self.deny_warnings_override = Some(deny);
        self.reapply_lint_overrides();
    }

    /// Clear the explicit `deny-warnings` override set by
    /// [`Self::set_deny_warnings_override`] — reverts to the applied
    /// `brink.toml`'s `deny-warnings` value (or `false`, absent a file).
    /// Re-analyzes immediately.
    pub fn clear_deny_warnings_override(&mut self) {
        self.deny_warnings_override = None;
        self.reapply_lint_overrides();
    }

    /// Parse a `brink.toml` project-settings file (#1005) and apply its
    /// `[project] dialect`/`types` to this session — the wasm/editor-mount
    /// wiring for the config file every compiler mount reads. Prefer
    /// [`Self::discover_project_config`] (#1414) when `brink.toml` is (or
    /// can be) loaded into this session as an ordinary document — it
    /// resolves the file automatically through the same `SourceTree` seam
    /// `brink compile`/`brink ide`/`bevy-brink` use, instead of requiring
    /// the embedder to locate and read it with host-specific filesystem
    /// code. This method stays for embedders that read `brink.toml`
    /// straight from a host API (Node `fs`, the File System Access API, …)
    /// without ever loading it into the session, or that want the file text
    /// applied without it being a queryable document.
    ///
    /// Call this once, at session construction, before any explicit
    /// `set_language_dialect`/`set_type_policy` call — a field already set
    /// explicitly on this session is left untouched (explicit calls always
    /// win over the file, matching the CLI's `--dialect`/`--types`
    /// precedence). Re-analyzes immediately for whichever field the file
    /// actually sets (like `set_language_dialect`/`set_type_policy`
    /// themselves).
    ///
    /// Returns the list of warning strings for unrecognized keys — as JSON
    /// (a `string[]`) — never an error (forward compat). Errors only on
    /// malformed TOML or a recognized key with an invalid value.
    ///
    /// Also **replaces** the session's resolved lint policy from the file's
    /// `[lints]` table / `deny-warnings` flag (issue #1160; fixed to
    /// replace rather than merge in #1397) — see [`Self::apply_parsed_config`],
    /// the merge point this and [`Self::discover_project_config`] both
    /// funnel through.
    pub fn apply_project_config(&mut self, toml: &str) -> Result<String, JsError> {
        let (config, warnings) = brink_project_config::parse_str(toml)
            .map_err(|e| JsError::new(&format!("invalid brink.toml: {e}")))?;
        let mut all_warnings: Vec<String> = warnings.into_iter().map(|w| w.0).collect();
        all_warnings.extend(self.apply_parsed_config(&config));
        Ok(serde_json::to_string(&all_warnings).unwrap_or_default())
    }

    /// Discover and apply this session's `brink.toml`, if one exists among
    /// the currently loaded documents (issue #1414) — the web-mount
    /// counterpart of `brink compile`/`brink_environment::Project::load`'s
    /// producer discovery and `brink ide`'s #1403 fix: both resolve
    /// `brink.toml` by walking a [`brink_source_tree::SourceTree`] via
    /// [`brink_project_config::discover_from_entry_in_tree`], never a
    /// path-based filesystem walk. `EditorSession` previously had no
    /// equivalent — `apply_project_config` only *applies* text the embedder
    /// already found and read through its own host APIs, so brink-web
    /// (which is inherently virtual: documents live only in this session's
    /// memory, there is no real filesystem to walk) was the one mount left
    /// unable to discover `brink.toml` itself.
    ///
    /// Builds a [`brink_source_tree::InMemory`] view of every file currently
    /// held by this session (whatever `update_file`/`update_source` have
    /// loaded, keyed exactly as those calls were made) and walks up from
    /// `entry`'s directory for a `brink.toml`, exactly like every other
    /// mount. An embedder that serves `brink.toml` as an ordinary document
    /// — `update_file("brink.toml", text)`, or nested at any ancestor of
    /// `entry` — needs no host-specific directory-walk code of its own;
    /// call this once (e.g. right after loading the project's files) in
    /// place of `apply_project_config`.
    ///
    /// `entry` is a session document path (this session's own path
    /// convention — whatever was passed to `update_file`), not a
    /// filesystem path. The document tree built here is keyed by exactly
    /// the strings passed to `update_file`/`update_source`, and the
    /// ancestor walk-up matches candidate keys by exact string equality —
    /// so `entry` and every `brink.toml`/document path in this session must
    /// share the same root-relative spelling (no leading `/`). Mixing a
    /// `/`-prefixed `entry` (or `/`-prefixed document paths) with
    /// unprefixed ones is a silent no-op: the walk-up finds nothing and
    /// this returns `Ok("[]")` exactly as if no `brink.toml` existed, with
    /// no warning.
    ///
    /// Returns `"[]"` when no `brink.toml` is found anywhere from `entry`'s
    /// directory up to the tree root — missing config is unchanged
    /// behavior, never an error. Otherwise applies and re-analyzes exactly
    /// like `apply_project_config`: explicit `set_language_dialect`/
    /// `set_type_policy` calls still win over the file, `[lints]` is still
    /// fully replaced from the file on every call (see
    /// [`Self::apply_parsed_config`]; #1397), and the returned JSON carries
    /// the same unrecognized-key/lint-code warnings.
    pub fn discover_project_config(&mut self, entry: &str) -> Result<String, JsError> {
        let db = self.session.db();
        let files: BTreeMap<String, String> = db
            .file_ids()
            .filter_map(|id| {
                let path = db.file_path(id)?;
                let source = db.source(id)?;
                Some((path.to_owned(), source.to_owned()))
            })
            .collect();
        let tree = brink_source_tree::InMemory::new(files);

        let Some(config_key) = brink_project_config::discover_from_entry_in_tree(&tree, entry)
            .map_err(|e| JsError::new(&format!("failed to discover brink.toml: {e}")))?
        else {
            return Ok("[]".to_owned());
        };
        let text = brink_source_tree::SourceTree::read(&tree, &config_key).map_err(|e| {
            JsError::new(&format!("failed to read project config {config_key}: {e}"))
        })?;
        let (config, warnings) = brink_project_config::parse_str_at(config_key.clone(), &text)
            .map_err(|e| JsError::new(&e.to_string()))?;

        let mut all_warnings: Vec<String> = warnings.into_iter().map(|w| w.0).collect();
        all_warnings.extend(self.apply_parsed_config(&config));
        Ok(serde_json::to_string(&all_warnings).unwrap_or_default())
    }

    /// Push the host's current values for `host`-source semantic types (Tier 3,
    /// #174) from a JSON object `{ "<type>": [{ "value", "label", "detail"? }] }`
    /// — a full snapshot that **replaces** the cache. The attached host (e.g.
    /// RPG Maker MZ) calls this with its named switches / items / … so the
    /// argument picker + value-label inlay hints stay current. Tooling-only;
    /// no re-analyze (values are consumed at query time, not in analysis).
    pub fn set_host_values(&mut self, json: &str) -> Result<(), JsError> {
        let values: brink_ide::HostValues = serde_json::from_str(json)
            .map_err(|e| JsError::new(&format!("invalid host values: {e}")))?;
        self.session.set_host_values(values);
        Ok(())
    }

    /// Clear the host-pushed value cache (e.g. on host disconnect). The picker
    /// degrades to plain literal entry for `host`-source params.
    pub fn clear_host_values(&mut self) {
        self.session.clear_host_values();
    }

    /// Set the severity policy for manifest-driven external diagnostics:
    /// `"error"` (default — a registered manifest is binding) or `"off"`.
    pub fn set_external_check(&mut self, level: &str) -> Result<(), JsError> {
        let severity = match level {
            "error" => brink_analyzer::ExternalCheckSeverity::Error,
            "off" => brink_analyzer::ExternalCheckSeverity::Off,
            other => {
                return Err(JsError::new(&format!(
                    "unknown external-check level `{other}` (expected \"error\" or \"off\")"
                )));
            }
        };
        self.session.set_external_check(severity);
        Ok(())
    }

    /// Set the severity policy for unknown-semantic-type diagnostics
    /// (`E040`), parallel to [`Self::set_external_check`] (#532): `"tolerant"`
    /// (default — unresolved types are only diagnosed once a manifest is
    /// registered, #339/#527) or `"error"` (always diagnose, even with no
    /// manifest registered — catches typo'd host semantic-type tags).
    pub fn set_semantic_type_check(&mut self, level: &str) -> Result<(), JsError> {
        let severity = match level {
            "tolerant" => brink_analyzer::SemanticTypeDiagnosticSeverity::Tolerant,
            "error" => brink_analyzer::SemanticTypeDiagnosticSeverity::Error,
            other => {
                return Err(JsError::new(&format!(
                    "unknown semantic-type-check level `{other}` (expected \"tolerant\" or \"error\")"
                )));
            }
        };
        self.session.set_semantic_type_check(severity);
        Ok(())
    }

    /// Switch the active file for IDE queries. Returns false if the file is not loaded.
    /// Clears any active view context (view is file-specific).
    pub fn set_active_file(&mut self, path: &str) -> bool {
        if self.session.file_id(path).is_some() {
            path.clone_into(&mut self.active_path);
            self.view = None;
            true
        } else {
            false
        }
    }

    /// Set a view context scoping the editor to `[start, end)` of the active file.
    /// IDE queries will adjust offsets relative to this range.
    pub fn set_view_context(&mut self, start: u32, end: u32) {
        self.view = Some(self.compute_view_context(&self.active_path, start, end));
    }

    /// Clear the view context, returning to full-file mode.
    pub fn clear_view_context(&mut self) {
        self.view = None;
    }

    /// Get the source text for the current view. Returns the fragment if a view
    /// context is active, or the full file otherwise. Returns a JSON string.
    pub fn get_view_source(&self) -> String {
        self.get_view_source_impl(&self.active_path, self.view.as_ref())
    }

    // ── Document handles ────────────────────────────────────────────
    //
    // Multi-document addressing: each handle pairs a file path with an
    // optional fragment view, so N live editor views can issue IDE queries
    // independently. The legacy active-file/view-context singleton above is
    // untouched by everything below. See the `*_doc` query variants.

    /// Compile the project using all loaded files. Returns JSON `CompileResult`.
    ///
    /// The artifact is assembled by querying **the session's own `ProjectDb`**
    /// — the same db the analysis path reads (#1032) — rather than spinning up
    /// a fresh compiler driver per call. One db means one file set, one
    /// lowering, and one analysis-options input shared by both compile and
    /// analysis, so the two can never diverge on manifest/dialect/policy: the
    /// class of bug that produced #1004 (manifest missing from compile) is
    /// structurally unrepresentable. The registered host manifest, T1b dialect,
    /// and TM-3 type policy are carried in through `analysis_options()`, exactly
    /// as the background analysis pass reads them.
    ///
    /// Takes `&mut self` because pointing the shared db at this entry and
    /// syncing its options input are salsa input writes; they do not touch the
    /// editor's cached diagnostic state (see `IdeSession::compile`).
    ///
    /// Unlike [`crate::compile::compile`]/[`crate::compile::compile_fragment`]
    /// (migrated onto the #1306 `Project::load` → `compile(&env)` producer by
    /// #1361), this method is **deliberately not migrated** — see the ruling
    /// and its reasoning on `IdeSession::compile`'s doc comment (issue #1385).
    /// In short: `compile(&Environment)` reseeds a fresh, throwaway `ProjectDb`
    /// on every call, which is correct for a stateless one-shot compile but
    /// would defeat the incremental, live-editing `ProjectDb` this session
    /// exists to keep warm, and would reopen the #1004 two-driver divergence
    /// #1032 closed by unifying compile and analysis onto one db.
    pub fn compile_project(&mut self, entry: &str) -> String {
        let options = self.session.analysis_options();
        let product = match self.session.compile(entry, &options) {
            Ok(product) => product,
            Err(e) => {
                let resp = CompileResult {
                    ok: false,
                    story_bytes: None,
                    warnings: Vec::new(),
                    error: Some(format!("{e}")),
                };
                return serde_json::to_string(&resp).unwrap_or_default();
            }
        };

        // Diagnostics are keyed by `FileId` into this session's own db, so
        // resolve each against its OWN file's source and path (offsets are
        // file-relative) — an INCLUDEd file's error lands on the right tab
        // instead of collapsing onto the entry. No throwaway-driver id
        // remapping: the ids are already this db's.
        let to_js = |d: &brink_ir::Diagnostic| {
            let src = self.session.source(d.file).unwrap_or("");
            DiagnosticJs {
                message: d.message.clone(),
                start: byte_to_utf16(src, d.range.start().into()),
                end: byte_to_utf16(src, d.range.end().into()),
                // Effective severity (issue #1367), not the raw
                // `DiagnosticCode::severity()` default — `options` is the
                // same `AnalysisOptions` `compile` above ran under.
                severity: format!(
                    "{:?}",
                    brink_analyzer::effective_severity(
                        d.code,
                        options.type_policy(),
                        &options.lints
                    )
                ),
                code: d.code.as_str().to_owned(),
                file: self.session.file_path(d.file).unwrap_or("").to_owned(),
            }
        };

        if let Some(story) = product.story {
            let warnings: Vec<DiagnosticJs> = product.warnings.iter().map(to_js).collect();

            let mut bytes = Vec::new();
            brink_format::write_inkb(&story, &mut bytes);

            let resp = CompileResult {
                ok: true,
                story_bytes: Some(bytes),
                warnings,
                error: None,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        } else {
            // Match the prior driver's failure shape: the diagnostics
            // channel carries the errors that prevented compilation,
            // followed by any warnings gathered alongside them.
            let diagnostics: Vec<DiagnosticJs> = product
                .errors
                .iter()
                .chain(product.warnings.iter())
                .map(to_js)
                .collect();

            let resp = CompileResult {
                ok: false,
                story_bytes: None,
                warnings: diagnostics,
                error: None,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
    }
}

impl EditorSession {
    /// Apply an already-parsed `[project]`/`[lints]` table to this session —
    /// the one merge point [`Self::apply_project_config`] (caller-supplied
    /// text) and [`Self::discover_project_config`] (#1414 — text located by
    /// walking this session's own in-memory document tree) both funnel
    /// through, so the two entry points can never disagree on how a parsed
    /// config is applied.
    ///
    /// `dialect`/`types` are skipped when the corresponding
    /// `set_language_dialect`/`set_type_policy` API was already called
    /// explicitly on this session (explicit calls always win over the
    /// file). `[lints]`/`deny-warnings` (issue #1160) has no explicit-call
    /// precedence to honor yet, so the file's table always **replaces**
    /// (not merges onto, issue #1397) the session's resolved lint policy —
    /// via a throwaway `AnalysisOptions` (`IdeSession` has no lint-specific
    /// fields of its own to hand `apply_project_config`), pushed back only
    /// when it actually changed (`LintPolicy` is `Eq`; `set_lint_policy`
    /// always re-analyzes, so a `brink.toml` with no `[lints]` table — or
    /// one resolving to the policy already in effect — must not trigger a
    /// redundant full re-analysis). Replace semantics matter specifically
    /// here: this is the one call site among `apply_project_config`'s
    /// callers that's a long-lived, repeatedly-re-applied session rather
    /// than a fresh one-shot compile, so a code (or `deny-warnings`)
    /// deleted from `brink.toml` between two calls must actually revert
    /// instead of staying stuck at whatever an earlier call resolved.
    ///
    /// Returns the `[lints]`/non-overridable-code warning strings (unknown
    /// top-level/`[project]` key warnings are the caller's own — parsed
    /// alongside `config`, not part of it).
    fn apply_parsed_config(&mut self, config: &brink_project_config::ProjectConfig) -> Vec<String> {
        if !self.dialect_explicit
            && let Some(dialect) = config.dialect
        {
            self.dialect = dialect;
            self.session.set_language_dialect(dialect);
        }
        if !self.types_explicit
            && let Some(types) = config.types
        {
            self.session.set_type_policy(types);
        }
        // No need to seed `lints` from the session's current policy:
        // `apply_project_config` replaces `.lints` wholesale from `config`
        // (issue #1397), so a throwaway `AnalysisOptions::default()` is
        // enough — `dialect_overridden`/`types_overridden` are passed
        // `true` (irrelevant to lint resolution; `dialect`/`types` are
        // already applied above), so this call touches nothing but `.lints`.
        let mut lint_options = brink_analyzer::AnalysisOptions::default();
        let lint_warnings = lint_options.apply_project_config(config, true, true);
        self.file_lint_policy = lint_options.lints;
        // #1417: the CLI/API tier (`set_lint_overrides`/
        // `set_deny_warnings_override`) always wins over what the file
        // above just resolved — reapplied here so a `brink.toml` reload
        // can never silently drop a previously-set explicit override
        // (`reapply_lint_overrides` is the one place that actually pushes
        // into `self.session`).
        let override_warnings = self.reapply_lint_overrides();
        lint_warnings
            .into_iter()
            .map(|w| w.0)
            .chain(override_warnings)
            .collect()
    }

    /// Resolve this session's effective `[lints]` policy by layering the
    /// explicit CLI/API-tier overrides (`self.lint_overrides`/
    /// `.deny_warnings_override`, issue #1417) on top of
    /// `self.file_lint_policy` — via the same
    /// `AnalysisOptions::apply_lint_overrides` seam `brink compile`'s
    /// `--deny`/`--warn`/`--allow` (#1373) and `brink-lsp`'s
    /// `initializationOptions.lints` (#1417) already use, reused rather
    /// than reimplemented. Recomputes from `self.file_lint_policy` (not
    /// `self.session.lint_policy()`, the already-combined result) every
    /// call, so a cleared override actually reverts instead of "sticking"
    /// on top of its own prior application (the same accumulation bug
    /// #1397 fixed for the file tier). Pushes into the session only if the
    /// resolved policy actually changed (mirrors `apply_parsed_config`'s
    /// own no-redundant-reanalyze guard). Returns the override warnings.
    fn reapply_lint_overrides(&mut self) -> Vec<String> {
        let mut options = brink_analyzer::AnalysisOptions {
            lints: self.file_lint_policy.clone(),
            ..brink_analyzer::AnalysisOptions::default()
        };
        let warnings =
            options.apply_lint_overrides(&self.lint_overrides, self.deny_warnings_override);
        if options.lints != *self.session.lint_policy() {
            self.session.set_lint_policy(options.lints);
        }
        warnings.into_iter().map(|w| w.0).collect()
    }

    /// Allocate the next handle id and insert `state`. Ids are monotonically
    /// increasing and never reused within a session.
    fn insert_doc(&mut self, state: DocState) -> u32 {
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        self.docs.insert(id, state);
        id
    }

    /// Source text of a file, if loaded.
    fn source_of(&self, path: &str) -> Option<&str> {
        self.session
            .file_id(path)
            .and_then(|id| self.session.source(id))
    }

    /// Convert a UTF-16 view-relative offset (the boundary convention) to a
    /// file-absolute **byte** offset for `brink-ide`/rowan.
    ///
    /// When a view context is given the offset is relative to the displayed
    /// fragment (`source[view.start..view.end]`); otherwise it's relative to
    /// the whole file.
    fn to_absolute(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> u32 {
        let Some(source) = self.source_of(path) else {
            return offset;
        };
        match view {
            Some(v) => {
                let start = floor_char_boundary(source, v.start as usize);
                let end = floor_char_boundary(source, (v.end as usize).max(start));
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink files are always < 4GB"
                )]
                let abs = start as u32 + utf16_to_byte(&source[start..end], offset);
                abs
            }
            None => utf16_to_byte(source, offset),
        }
    }

    /// Convert a file-absolute **byte** offset (from `brink-ide`) to a
    /// UTF-16 view-relative offset for the editor.
    /// Returns `None` if the offset is outside the view range.
    fn to_relative(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> Option<u32> {
        let source = self.source_of(path)?;
        match view {
            Some(v) => {
                if offset < v.start || offset > v.end {
                    return None;
                }
                let start = floor_char_boundary(source, v.start as usize);
                let end = floor_char_boundary(source, (v.end as usize).max(start));
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "ink files are always < 4GB"
                )]
                let byte_in_fragment = (offset as usize).saturating_sub(start) as u32;
                Some(byte_to_utf16(&source[start..end], byte_in_fragment))
            }
            None => Some(byte_to_utf16(source, offset)),
        }
    }

    /// Convert a file-absolute line number (0-based) to a view-relative line.
    /// Returns `None` if the line is before the view start.
    fn to_relative_line(view: Option<&ViewContext>, line: u32) -> Option<u32> {
        view.map_or(Some(line), |v| {
            (line >= v.start_line).then(|| line - v.start_line)
        })
    }

    /// Compute the end line of the view in the current source.
    fn view_end_line(&self, path: &str, view: &ViewContext) -> Option<u32> {
        let source = self.source_of(path)?;
        let byte_end = (view.end as usize).min(source.len());
        Some(count_newlines(&source[..byte_end]))
    }

    /// Compute a `ViewContext` scoping `path` to `[start, end)` (UTF-16
    /// offsets): converts the boundary offsets to bytes, trims trailing blank
    /// lines (keeping at most one newline), detects the newline separator at
    /// the boundary, and records the 0-based start line.
    fn compute_view_context(&self, path: &str, start: u32, end: u32) -> ViewContext {
        // Boundary offsets are UTF-16 code units; convert to bytes for the
        // internal byte-indexed logic below (and stored ViewContext range).
        let (start, end) = match self.source_of(path) {
            Some(s) => (utf16_to_byte(s, start), utf16_to_byte(s, end)),
            None => (start, end),
        };
        // Check if there's a newline right at `end` (the separator between this
        // section and the next). If so, we'll ensure it's preserved after splices.
        // Trim trailing blank lines from the view range and check if there's a
        // newline separator at the boundary that should be preserved across splices.
        let (end, trailing_newline) = self.source_of(path).map_or((end, false), |s| {
            let e = (end as usize).min(s.len());
            let start_usize = (start as usize).min(e);
            let view = &s[start_usize..e];
            // Trim trailing newlines (keep at most one)
            let trimmed = view.trim_end_matches('\n');
            let keep = if trimmed.len() < view.len() {
                trimmed.len() + 1
            } else {
                view.len()
            };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink files are always < 4GB"
            )]
            let trimmed_end = (start_usize + keep).min(e) as u32;
            // Check if there's a newline right after the trimmed end
            let has_nl = s.as_bytes().get(trimmed_end as usize) == Some(&b'\n')
                || (trimmed_end > 0
                    && s.as_bytes().get((trimmed_end as usize).wrapping_sub(1)) == Some(&b'\n'));
            (trimmed_end, has_nl)
        });

        let start_line = self.source_of(path).map_or(0, |s| {
            let byte_start = (start as usize).min(s.len());
            count_newlines(&s[..byte_start])
        });
        ViewContext {
            start,
            end,
            start_line,
            trailing_newline,
        }
    }

    fn get_view_source_impl(&self, path: &str, view: Option<&ViewContext>) -> String {
        let source = self.source_of(path);
        match (source, view) {
            (Some(s), Some(v)) => {
                let start = (v.start as usize).min(s.len());
                let end = (v.end as usize).min(s.len());
                serde_json::to_string(&s[start..end]).unwrap_or_default()
            }
            (Some(s), None) => serde_json::to_string(s).unwrap_or_default(),
            _ => "null".to_owned(),
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn count_newlines(s: &str) -> u32 {
    s.matches('\n').count() as u32
}

// ── UTF-16 ↔ byte offset conversion ─────────────────────────────────
//
// The wasm `EditorSession` boundary speaks UTF-16 code-unit offsets, to
// match CodeMirror / JS string indexing on the TypeScript side. Internally
// (rowan, `TextSize`, `&str`) everything is byte offsets. These helpers
// translate at the boundary. Both clamp to the end of `s` when the input
// falls past the string, and round a position that lands inside a multi-unit
// boundary up to the next char start (CodeMirror never produces such inputs,
// but the clamp keeps us panic-free).

/// Convert a byte offset within `s` to a UTF-16 code-unit offset.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
pub(crate) fn byte_to_utf16(s: &str, byte: u32) -> u32 {
    let byte = byte as usize;
    let mut units = 0u32;
    for (i, c) in s.char_indices() {
        if i >= byte {
            return units;
        }
        units += c.len_utf16() as u32;
    }
    units
}

/// Convert a UTF-16 code-unit offset within `s` to a byte offset.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink files are always < 4GB"
)]
fn utf16_to_byte(s: &str, utf16: u32) -> u32 {
    let mut units = 0u32;
    for (i, c) in s.char_indices() {
        if units >= utf16 {
            return i as u32;
        }
        units += c.len_utf16() as u32;
    }
    s.len() as u32
}

/// Largest byte index `<= i` that is a char boundary in `s` (clamped to `len`).
/// Keeps fragment slicing panic-free if a stored byte offset ever lands inside
/// a multibyte char.
fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

// ── Fragment view rebasing ──────────────────────────────────────────

/// Shift a fragment `ViewContext` in place to account for an out-of-band
/// whole-file edit that inserted/removed content at byte `insert_at`.
///
/// `byte_delta` is the net byte change of the edit (inserted − removed) and
/// `line_delta` the net newline change. Only views that begin at or after
/// `insert_at` move — a view before the edit is unaffected. This keeps the
/// stored byte range (and start line) of every open fragment handle consistent
/// with the mutated file, so a subsequent fragment splice targets the correct
/// window instead of clobbering the shifted content.
fn rebase_view(view: &mut ViewContext, insert_at: u32, byte_delta: i64, line_delta: i64) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "offsets stay within a <4GB file and never go negative for a valid edit"
    )]
    let shift = |offset: u32| -> u32 {
        if offset < insert_at {
            offset
        } else {
            (i64::from(offset) + byte_delta).max(0) as u32
        }
    };
    let start_moves = view.start >= insert_at;
    // The insertion point sits at (or before) the view start for the auto-import
    // case (INCLUDE block above the fragment), so both boundaries move together.
    view.start = shift(view.start);
    view.end = shift(view.end);
    if start_moves {
        // The view's start byte shifted, so its first line shifts by the net
        // newline delta of the edit.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "line counts stay within a <4GB file"
        )]
        if line_delta >= 0 {
            view.start_line = view.start_line.saturating_add(line_delta as u32);
        } else {
            view.start_line = view.start_line.saturating_sub((-line_delta) as u32);
        }
    }
}

// ── Fragment splicing ───────────────────────────────────────────────

/// Result of splicing a fragment into its full file.
struct SpliceOutcome {
    /// The new full file content.
    spliced: String,
    /// New byte end of the fragment within the file (excludes any separator).
    new_view_end: u32,
    /// Byte start of the replaced range in the old file content.
    replaced_start: u32,
    /// Byte end (exclusive) of the replaced range in the old file content.
    replaced_end: u32,
    /// Whether a `\n` separator was appended after the fragment.
    inserted_separator: bool,
}

/// Splice `source` into `full` over the view's `[start, end)` byte range.
///
/// If the original boundary had a newline separator and the fragment doesn't
/// end with one, a `\n` separator is inserted after the fragment to prevent
/// merging with the next section. `new_view_end` tracks only the fragment,
/// NOT the separator — the separator lives at `spliced[new_view_end]` and is
/// preserved across splices.
fn splice_fragment(full: &str, view: &ViewContext, source: &str) -> SpliceOutcome {
    let start = (view.start as usize).min(full.len());
    let end = (view.end as usize).clamp(start, full.len());

    let after = &full[end..];
    // If the original boundary had a newline separator and the fragment
    // doesn't end with one, insert a newline to prevent merging.
    let needs_sep = view.trailing_newline
        && !source.ends_with('\n')
        && !after.starts_with('\n')
        && !after.is_empty();
    let sep = if needs_sep { "\n" } else { "" };
    let mut spliced = String::with_capacity(start + source.len() + sep.len() + after.len());
    spliced.push_str(&full[..start]);
    spliced.push_str(source);
    spliced.push_str(sep);
    spliced.push_str(after);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ink files are always < 4GB"
    )]
    SpliceOutcome {
        spliced,
        new_view_end: view.start + source.len() as u32,
        replaced_start: start as u32,
        replaced_end: end as u32,
        inserted_separator: needs_sep,
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        reason = "tiny literal test strings; offsets cannot overflow u32"
    )]
    use super::{byte_to_utf16, utf16_to_byte};

    #[test]
    fn ascii_is_identity() {
        let s = "hello world";
        for i in 0..=s.len() as u32 {
            assert_eq!(byte_to_utf16(s, i), i, "byte_to_utf16 at {i}");
            assert_eq!(utf16_to_byte(s, i), i, "utf16_to_byte at {i}");
        }
    }

    #[test]
    fn two_byte_char_caf_e() {
        // "café": c=0, a=1, f=2, é=3..5 (2 bytes, 1 UTF-16 unit), len=5 bytes / 4 units
        let s = "café";
        assert_eq!(byte_to_utf16(s, 3), 3); // start of é
        assert_eq!(byte_to_utf16(s, 5), 4); // end of string
        assert_eq!(utf16_to_byte(s, 3), 3);
        assert_eq!(utf16_to_byte(s, 4), 5);
    }

    #[test]
    fn three_byte_em_dash() {
        // "a—b": a=0, —=1..4 (U+2014, 3 bytes, 1 unit), b=4..5
        let s = "a—b";
        assert_eq!(byte_to_utf16(s, 4), 2); // start of b
        assert_eq!(byte_to_utf16(s, 5), 3); // end
        assert_eq!(utf16_to_byte(s, 2), 4);
        assert_eq!(utf16_to_byte(s, 3), 5);
    }

    #[test]
    fn four_byte_astral_emoji() {
        // "a😀b": a=0, 😀=1..5 (U+1F600, 4 bytes, 2 UTF-16 units), b=5..6
        let s = "a😀b";
        assert_eq!(byte_to_utf16(s, 1), 1); // start of emoji
        assert_eq!(byte_to_utf16(s, 5), 3); // after emoji (1 + 2 units)
        assert_eq!(byte_to_utf16(s, 6), 4); // end
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(utf16_to_byte(s, 4), 6);
    }

    #[test]
    fn round_trip_on_char_boundaries() {
        let s = "x—y😀zé!";
        for (i, _) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
            let units = byte_to_utf16(s, i as u32);
            assert_eq!(utf16_to_byte(s, units), i as u32, "round-trip at byte {i}");
        }
    }

    #[test]
    fn clamps_past_end() {
        let s = "café";
        assert_eq!(byte_to_utf16(s, 999), 4); // total UTF-16 length
        assert_eq!(utf16_to_byte(s, 999), 5); // total byte length
    }

    // ── End-to-end boundary tests ───────────────────────────────────
    // These prove the EditorSession surfaces UTF-16 offsets even when a
    // non-ASCII char shifts the byte/UTF-16 mapping. The é before the knot
    // makes every byte offset past it 1 larger than its UTF-16 offset.

    use super::EditorSession;

    #[test]
    fn hir_spans_doc_projects_spans_and_line_stacks() {
        // é in the content line shifts bytes vs UTF-16 by 1 for anything after it.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "VAR name = \"x\"\n=== start ===\né {name} here.\n* [Go] -> hub\n=== hub ===\n-> DONE\n",
        );
        assert!(s.set_active_file("main.ink"));
        let doc = s.open_document("main.ink");

        let json = s.hir_spans_doc(doc);
        let p: serde_json::Value = serde_json::from_str(&json).unwrap();
        let spans = p["spans"].as_array().unwrap();
        assert!(!spans.is_empty(), "projection has spans");

        // A knot container span with a handle and a def_id string.
        let knot = spans
            .iter()
            .find(|s| s["kind"] == "knot" && s["container"] == true)
            .expect("knot container span");
        assert!(knot["handle"].is_u64());
        // The knot's decl span carries a $-prefixed DefinitionId string.
        assert!(
            spans
                .iter()
                .any(|s| s["kind"] == "knot"
                    && s["def_id"].as_str().is_some_and(|d| d.starts_with('$'))),
            "knot decl carries a string def_id"
        );
        // The `-> hub` divert resolves: a divert span with a target_id string.
        assert!(
            spans.iter().any(|s| s["kind"] == "divert"
                && s["target_id"].as_str().is_some_and(|d| d.starts_with('$'))),
            "resolved divert carries target_id"
        );

        // The {name} var ref sits after the 2-byte é on line 2: its UTF-16
        // start_char must be 3 (é=1 unit + space + '{'), not the byte offset 4.
        let var_ref = spans
            .iter()
            .find(|s| s["kind"] == "var_ref" && s["start_line"] == 2)
            .expect("var ref span on the é line");
        assert_eq!(var_ref["start_char"].as_u64().unwrap(), 3, "UTF-16 column");

        // Per-line stacks: line 3 (the choice) is inside knot + choice.
        let lines = p["lines"].as_array().unwrap();
        let choice_line = lines[3].as_array().unwrap();
        assert!(
            choice_line.len() >= 2,
            "choice line inside knot + choice: {choice_line:?}"
        );
        // Depth-ordered outermost→innermost.
        let depths: Vec<u64> = choice_line
            .iter()
            .map(|c| c["depth"].as_u64().unwrap())
            .collect();
        assert!(depths.windows(2).all(|w| w[0] <= w[1]), "{depths:?}");
    }

    #[test]
    fn hir_spans_view_drops_straddling_non_containers() {
        // A multi-line construct span (`conditional`) starting above the
        // view must be dropped, not clamped to (0, 0) — a clamped inline
        // mark would paint from the view's top-left over unrelated text.
        // Containers straddling the start keep the clamp (partial rails).
        let src =
            "=== start ===\n{ ready:\nGo now.\nSecond line.\n- else:\nWait here.\n}\n-> DONE\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("Second line.").unwrap() as u32;
        let doc = s.open_fragment("main.ink", start, src.len() as u32);

        let json = s.hir_spans_doc(doc);
        let p: serde_json::Value = serde_json::from_str(&json).unwrap();
        let spans = p["spans"].as_array().unwrap();
        assert!(
            !spans.iter().any(|s| s["kind"] == "conditional"),
            "straddling non-container construct span must be dropped: {spans:?}"
        );
        assert!(
            spans.iter().any(|s| s["kind"] == "knot"
                && s["container"] == true
                && s["start_line"] == 0
                && s["start_char"] == 0),
            "straddling knot container is clamped to the view start: {spans:?}"
        );
    }

    #[test]
    fn document_symbols_returns_utf16_offsets() {
        // "é\n=== k ===\n": é = 2 bytes / 1 UTF-16 unit, so the knot header
        // starts at byte 3 but UTF-16 offset 2; the name `k` at byte 7 / unit 6.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é\n=== k ===\nhi\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.document_symbols();
        let syms: serde_json::Value = serde_json::from_str(&json).unwrap();
        let knot = &syms[0];
        assert_eq!(knot["name"], "k");
        // UTF-16 offsets, not bytes (byte would be 7 / full_start 3).
        assert_eq!(
            knot["start"].as_u64().unwrap(),
            6,
            "name start must be UTF-16"
        );
        assert_eq!(
            knot["full_start"].as_u64().unwrap(),
            2,
            "knot full_start must be UTF-16"
        );
    }

    #[test]
    fn goto_definition_round_trips_utf16() {
        // Divert target on line 1, knot on line 3, with é shifting offsets.
        // "é -> k\n\n=== k ===\nhi\n"
        //  byte:  é(0..2) space(2) -(3) >(4) space(5) k(6) \n(7) ...
        //  utf16: é(0..1) space(1) -(2) >(3) space(4) k(5) \n(6) ...
        // Cursor on the `k` of `-> k` is UTF-16 offset 5.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é -> k\n\n=== k ===\nhi\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.goto_definition(5);
        assert_ne!(json, "null", "should resolve the divert target");
        let loc: serde_json::Value = serde_json::from_str(&json).unwrap();
        // goto resolves to the knot's name `k` inside `=== k ===`. That `k` is
        // at byte 13 but UTF-16 offset 12 (one é before it). A byte-based
        // result would be 13 — so 12 proves both the input offset (UTF-16 5 →
        // byte 6, the divert's `k`) and the output (byte 13 → UTF-16 12).
        assert_eq!(
            loc["start"].as_u64().unwrap(),
            12,
            "definition start must be UTF-16, not bytes"
        );
    }

    #[test]
    fn story_graph_edges_carry_utf16_occurrences() {
        // "é\n=== a ===\n-> b\n\n=== b ===\n-> DONE\n"
        //  The é (2 bytes / 1 UTF-16 unit) shifts every later byte offset
        //  1 past its UTF-16 offset. The divert target `b` on line 3 is at
        //  byte 16 → UTF-16 15.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é\n=== a ===\n-> b\n\n=== b ===\n-> DONE\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.story_graph();
        assert_ne!(json, "null");
        let graph: serde_json::Value = serde_json::from_str(&json).unwrap();
        let edges = graph["edges"].as_array().unwrap();

        let edge = |from: &str, to: &str| {
            edges
                .iter()
                .find(|e| e["from"] == from && e["to"] == to)
                .expect("missing edge")
        };

        // a -> b: one occurrence anchored on the target path `b`.
        let occ = &edge("a", "b")["occurrences"][0];
        assert_eq!(occ["file"], "main.ink");
        assert_eq!(occ["start"].as_u64().unwrap(), 15, "must be UTF-16");
        assert_eq!(occ["end"].as_u64().unwrap(), 16);

        // b -> DONE: occurrence spans the whole `-> DONE` statement.
        // Bytes: `-> DONE` starts at byte 29 → UTF-16 28, 7 chars long.
        let occ = &edge("b", "DONE")["occurrences"][0];
        assert_eq!(occ["file"], "main.ink");
        assert_eq!(occ["start"].as_u64().unwrap(), 28);
        assert_eq!(occ["end"].as_u64().unwrap(), 35);
    }

    // ── Cross-file structural-move edits (#12) ──────────────────────

    use crate::editor_refactor::apply_edits;

    #[test]
    fn apply_edits_applies_descending_and_preserves_offsets() {
        let src = "alpha beta gamma";
        // Two edits given out of order; both must land correctly.
        let out = apply_edits(
            src,
            vec![
                (11, 16, "GAMMA".to_owned()), // gamma -> GAMMA
                (0, 5, "ALPHA".to_owned()),   // alpha -> ALPHA
            ],
        );
        assert_eq!(out, "ALPHA beta GAMMA");
    }

    #[test]
    fn apply_edits_skips_out_of_bounds() {
        let src = "short";
        let out = apply_edits(src, vec![(0, 999, "x".to_owned())]);
        assert_eq!(out, "short", "out-of-bounds edit is skipped, not panicking");
    }

    #[test]
    fn cross_file_move_resolves_reference_edit_to_new_source() {
        // `other.ink` diverts into a stitch of `main.ink`; moving that stitch to
        // another knot must produce a cross-file edit updating the divert, now
        // delivered as the full new source of `other.ink`.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== a ===\n= s\nstuff\n-> END\n\n=== b ===\nbee\n-> END\n",
        );
        s.update_file("other.ink", "-> a.s\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.move_stitch("main.ink", "a", "s", "b");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], true, "move should succeed: {json}");

        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other_edit = cfe.iter().find(|e| e["path"] == "other.ink");
        assert!(
            other_edit.is_some(),
            "expected a cross-file edit for other.ink, got {cfe:?}"
        );
        let other = other_edit.unwrap();
        // The divert `-> a.s` must now point at the moved stitch's new path.
        assert!(
            other["new_source"].as_str().unwrap().contains("b.s"),
            "cross-file new_source should reference b.s: {other:?}"
        );
        // It carries the full file source (path-keyed), not byte ranges.
        assert!(other.get("new_source").is_some());
        assert!(other.get("start").is_none());
    }

    // ── Directory rename/move (#314) ────────────────────────────────

    #[test]
    fn rename_dir_returns_moved_files_and_inbound_edits() {
        let mut s = EditorSession::new();
        // main.ink (outside) includes into the folder; a folder file includes an
        // outside lib; two folder siblings include each other.
        s.update_file("main.ink", "INCLUDE chapters/intro.ink\n-> END\n");
        s.update_file("lib.ink", "=== helper ===\n-> END\n");
        s.update_file(
            "chapters/intro.ink",
            "INCLUDE ../lib.ink\nINCLUDE scene.ink\n-> END\n",
        );
        s.update_file("chapters/scene.ink", "=== scene ===\n-> END\n");

        let json = s.rename_dir("chapters", "book/chapters");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], true, "dir move should succeed: {json}");
        assert_eq!(v["safe"], true, "content-preserving move is safe: {json}");

        // Two moved files at their new paths.
        let moved = v["moved_files"].as_array().unwrap();
        assert_eq!(moved.len(), 2);
        let intro = moved
            .iter()
            .find(|m| m["new_path"] == "book/chapters/intro.ink")
            .unwrap();
        assert_eq!(intro["old_path"], "chapters/intro.ink");
        // Outbound: ../lib.ink now two levels deep; sibling stays bare.
        let intro_src = intro["new_source"].as_str().unwrap();
        assert!(
            intro_src.contains("INCLUDE ../../lib.ink"),
            "outbound not recomputed: {intro_src}"
        );
        assert!(
            intro_src.contains("INCLUDE scene.ink"),
            "sibling include should stay bare: {intro_src}"
        );

        // Inbound: main re-points into the new folder, delivered as full source.
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let main_edit = cfe.iter().find(|e| e["path"] == "main.ink").unwrap();
        assert!(
            main_edit["new_source"]
                .as_str()
                .unwrap()
                .contains("INCLUDE book/chapters/intro.ink"),
            "inbound not rewritten: {main_edit:?}"
        );
    }

    #[test]
    fn rename_dir_error_is_dir_move_shaped_json() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "-> END\n");
        let json = s.rename_dir("ghost", "x");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().is_some());
        assert!(v["moved_files"].as_array().unwrap().is_empty());
    }

    // ── Safe symbol rename (#305) ───────────────────────────────────

    #[test]
    fn rename_symbol_safe_rewrites_refs_with_empty_breakage() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "hello", "", "greeting")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "consistent rename is safe: {v}");
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
        let new_source = v["new_source"].as_str().unwrap();
        assert!(new_source.contains("=== greeting ==="));
        assert!(
            new_source.contains("-> greeting"),
            "divert rewritten: {new_source}"
        );
    }

    #[test]
    fn rename_symbol_collision_reports_breakage_and_cross_file_edits() {
        // `other.ink` diverts `-> a`; renaming knot `a` to `b` both collides
        // with the existing `b` (breakage) and rewrites the cross-file divert.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n\n=== b ===\n-> END\n");
        s.update_file("other.ink", "-> a\n");
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "a", "", "b")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], false, "collision is unsafe: {v}");

        let diags = v["introduced_diagnostics"].as_array().unwrap();
        assert!(
            diags.iter().any(|d| d["code"] == "E022"),
            "expected E022 duplicate-knot in breakage report: {diags:?}"
        );
        // Every diag carries the fields the report renders.
        let first = &diags[0];
        for key in ["severity", "code", "message", "path", "line", "col"] {
            assert!(first.get(key).is_some(), "diag missing {key}: {first:?}");
        }

        // The cross-file divert is still rewritten (edits computed regardless).
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other = cfe.iter().find(|e| e["path"] == "other.ink").unwrap();
        assert!(other["new_source"].as_str().unwrap().contains("-> b"));
    }

    #[test]
    fn rename_symbol_at_renames_by_offset_cross_file() {
        // F2's offset-based path: rename the knot whose declaration the cursor
        // sits in, rewriting a divert in another file.
        let mut s = EditorSession::new();
        let main = "=== hello ===\nHi.\n-> END\n";
        s.update_file("main.ink", main);
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset of the `hello` name in `=== hello ===` (ASCII ⇒ UTF-16 == byte).
        let offset = u32::try_from(main.find("hello").unwrap()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol_at("main.ink", offset, "greeting")).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["safe"], true);
        assert!(
            v["new_source"]
                .as_str()
                .unwrap()
                .contains("=== greeting ===")
        );
        let cfe = v["cross_file_edits"].as_array().unwrap();
        let other = cfe.iter().find(|e| e["path"] == "other.ink").unwrap();
        assert!(
            other["new_source"]
                .as_str()
                .unwrap()
                .contains("-> greeting")
        );
    }

    #[test]
    fn rename_symbol_unknown_returns_error() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.rename_symbol("main.ink", "nope", "", "x")).unwrap();
        assert_eq!(v["ok"], false);
    }

    // ── Unified StructuralResult + deleteSymbol (#316) ──────────────

    #[test]
    fn delete_symbol_referenced_knot_reports_breakage() {
        // `start` diverts to `target`; deleting `target` dangles that divert.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n-> target\n=== target ===\n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "target", "")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(
            v["safe"], false,
            "deleting a referenced knot is unsafe: {v}"
        );
        let diags = v["introduced_diagnostics"].as_array().unwrap();
        assert!(
            !diags.is_empty(),
            "the dangling divert is reported: {diags:?}"
        );
        // Every diag carries the breakage-report fields.
        for key in ["severity", "code", "message", "path", "line", "col"] {
            assert!(diags[0].get(key).is_some(), "diag missing {key}: {diags:?}");
        }
        let new_source = v["new_source"].as_str().unwrap();
        assert!(
            !new_source.contains("=== target ==="),
            "target removed: {new_source}"
        );
    }

    #[test]
    fn delete_symbol_unreferenced_is_safe() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n=== b ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "b", "")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "no references, so safe: {v}");
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
    }

    #[test]
    fn delete_symbol_stitch_keeps_siblings() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== k ===\n= a\nA.\n= b\nB.\n= c\nC.\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "k", "b")).unwrap();
        let new_source = v["new_source"].as_str().unwrap();
        assert!(!new_source.contains("= b"), "b removed: {new_source}");
        assert!(
            new_source.contains("= a") && new_source.contains("= c"),
            "siblings kept: {new_source}"
        );
    }

    #[test]
    fn delete_symbol_unknown_returns_error() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.delete_symbol("main.ink", "ghost", "")).unwrap();
        assert_eq!(v["ok"], false);
    }

    // ── extract-selection ops (#315 H) ─────────────────────────────

    #[test]
    fn extract_to_knot_returns_structural_result_with_tunnel_call() {
        let mut s = EditorSession::new();
        let src = "=== start ===\nHello.\nWorld.\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        // UTF-16 offsets == byte offsets here (ASCII source).
        let start = src.find("Hello.").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "greeting")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true, "self-contained extraction is safe: {v}");
        let new_source = v["new_source"].as_str().unwrap();
        assert!(new_source.contains("=== greeting ==="), "{new_source}");
        assert!(new_source.contains("-> greeting ->"), "{new_source}");
        assert!(new_source.contains("->->"), "tunnel return: {new_source}");
    }

    #[test]
    fn extract_to_function_returns_structural_result_with_call() {
        let mut s = EditorSession::new();
        let src = "=== start ===\n{2 + 3}\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("{2 + 3}").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_function("main.ink", start, end, "calc")).unwrap();
        assert_eq!(v["ok"], true);
        let new_source = v["new_source"].as_str().unwrap();
        assert!(
            new_source.contains("=== function calc() ==="),
            "{new_source}"
        );
        assert!(new_source.contains("{calc()}"), "inline call: {new_source}");
    }

    #[test]
    fn extract_that_breaks_scope_reports_breakage() {
        let mut s = EditorSession::new();
        let src = "=== start ===\n~ temp count = 3\n{count}\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("{count}").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "shower")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], false, "out-of-scope temp makes it unsafe: {v}");
        assert!(
            !v["introduced_diagnostics"].as_array().unwrap().is_empty(),
            "breakage reported: {v}"
        );
    }

    #[test]
    fn extract_header_crossing_returns_error() {
        let mut s = EditorSession::new();
        let src = "=== a ===\nContent.\n=== b ===\n-> END\n";
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));
        let start = src.find("Content.").unwrap() as u32;
        let end = src.find("-> END").unwrap() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&s.extract_to_knot("main.ink", start, end, "x")).unwrap();
        assert_eq!(v["ok"], false, "crossing a header is rejected: {v}");
    }

    #[test]
    fn reorder_returns_safe_with_empty_breakage() {
        // Reorders change no qualification — the unified result is trivially safe.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\n-> END\n=== b ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.reorder_knot("main.ink", "a", 1)).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["safe"], true);
        assert!(v["introduced_diagnostics"].as_array().unwrap().is_empty());
        // The unified result still round-trips through the StructuralResult JSON:
        // every field the studio reads is present.
        assert!(v.get("new_source").is_some());
        assert!(v.get("cross_file_edits").is_some());
    }

    #[test]
    fn breaking_move_reports_introduced_diagnostics() {
        // Moving a stitch whose bare same-knot reference can't be requalified
        // into the destination is gated. Here a divert in `other` targets the
        // qualified stitch; the move rewrites it, staying safe — but a move that
        // collides surfaces breakage. We assert the unified result always carries
        // the gate fields regardless of outcome.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== src ===\n= movable\n-> END\n=== dst ===\nDest.\n",
        );
        assert!(s.set_active_file("main.ink"));
        let v: serde_json::Value =
            serde_json::from_str(&s.move_stitch("main.ink", "src", "movable", "dst")).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v.get("safe").is_some(), "move carries the gate flag: {v}");
        assert!(
            v.get("introduced_diagnostics").is_some(),
            "move carries the breakage list: {v}"
        );
    }

    // ── Document handles (#122) ─────────────────────────────────────

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn two_handles_on_different_files_query_independently() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "=== alpha ===\nA line\n-> END\n");
        s.update_file("b.ink", "hello b\n=== beta ===\nB line\n-> END\n");
        // No set_active_file: the singleton still points at the unloaded
        // default, proving handles don't depend on it.
        let da = s.open_document("a.ink");
        let db = s.open_document("b.ink");
        assert_ne!(da, 0);
        assert_ne!(db, 0);
        assert_ne!(da, db);
        assert_eq!(s.active_file(), "main.ink");

        // hover over each file's knot name resolves per-handle.
        // a.ink: `alpha` at offsets 4..9; b.ink: `beta` at 12..16.
        let ha = s.hover_doc(da, 5);
        let hb = s.hover_doc(db, 13);
        assert!(ha.contains("alpha"), "hover via handle a: {ha}");
        assert!(hb.contains("beta"), "hover via handle b: {hb}");

        // line_contexts are file-specific per handle.
        let la = json(&s.line_contexts_doc(da));
        let lb = json(&s.line_contexts_doc(db));
        assert_eq!(la[0]["element"], "knot_header", "a.ink starts with a knot");
        assert_eq!(lb[0]["element"], "narrative", "b.ink starts with narrative");

        // completions work through a handle with no active file set.
        let ca = json(&s.completions_doc(da, 0));
        let names: Vec<&str> = ca
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|i| i["name"].as_str())
            .collect();
        assert!(names.contains(&"alpha"), "completions: {names:?}");
        assert!(names.contains(&"beta"), "completions: {names:?}");

        // The singleton queries still target the (unloaded) active file.
        assert_eq!(s.line_contexts(), "[]");
    }

    #[test]
    fn fragment_update_splices_and_reports_change_spec() {
        // é = 2 bytes / 1 UTF-16 unit: every offset past it differs between
        // byte and UTF-16 coordinates, proving the spec is UTF-16.
        //   bytes: "é intro\n"(0..9) "=== a ===\n"(9..19) "A line\n"(19..26)
        //          "=== b ===\n"(26..36) "B line\n"(36..43)
        //   utf16: 0..8 / 8..18 / 18..25 / 25..35 / 35..42
        let full = "é intro\n=== a ===\nA line\n=== b ===\nB line\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", full);

        let file_doc = s.open_document("main.ink");
        // Fragment over knot `a`: UTF-16 [8, 25).
        let frag = s.open_fragment("main.ink", 8, 25);
        assert_ne!(file_doc, 0);
        assert_ne!(frag, 0);
        assert_eq!(
            s.get_view_source_doc(frag),
            serde_json::to_string("=== a ===\nA line\n").unwrap()
        );

        // Update the fragment WITHOUT a trailing newline: the splice inserts
        // a `\n` separator, and the spec must carry the actually-inserted
        // text (source + "\n").
        let spec = json(&s.update_document(frag, "=== a ===\nA new"));
        assert_eq!(spec["path"], "main.ink");
        assert_eq!(spec["start"], 8, "UTF-16 start of replaced range");
        assert_eq!(spec["end"], 25, "UTF-16 end of replaced range");
        assert_eq!(
            spec["text"], "=== a ===\nA new\n",
            "separator nuance: actually-inserted text differs from source"
        );

        let expected = "é intro\n=== a ===\nA new\n=== b ===\nB line\n";
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string(expected).unwrap()
        );
        // The full-file handle on the same file sees the spliced content.
        assert_eq!(
            s.get_view_source_doc(file_doc),
            serde_json::to_string(expected).unwrap()
        );
        // The fragment handle's own view tracked the new fragment extent
        // (excluding the separator).
        assert_eq!(
            s.get_view_source_doc(frag),
            serde_json::to_string("=== a ===\nA new").unwrap()
        );

        // Update again WITH a trailing newline: no separator inserted (one
        // already follows the fragment), so no `text` in the spec. New file
        // coords: fragment is bytes [9, 24) = UTF-16 [8, 23).
        let spec = json(&s.update_document(frag, "=== a ===\nA two"));
        assert_eq!(spec["start"], 8);
        assert_eq!(spec["end"], 23);
        assert!(
            spec.get("text").is_none(),
            "no separator inserted -> no text field: {spec}"
        );
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("é intro\n=== a ===\nA two\n=== b ===\nB line\n").unwrap()
        );
    }

    #[test]
    fn full_file_handle_update_reports_whole_file_spec() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "é one\n"); // 7 bytes, 6 UTF-16 units
        let d = s.open_document("main.ink");

        let spec = json(&s.update_document(d, "two\n"));
        assert_eq!(spec["path"], "main.ink");
        assert_eq!(spec["start"], 0);
        assert_eq!(spec["end"], 6, "whole previous file range, in UTF-16");
        assert!(spec.get("text").is_none());
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("two\n").unwrap()
        );
    }

    #[test]
    fn close_reopen_handle_lifecycle() {
        let mut s = EditorSession::new();
        s.update_file("a.ink", "=== alpha ===\nA\n-> END\n");

        // Unknown files don't get handles.
        assert_eq!(s.open_document("nope.ink"), 0);
        assert_eq!(s.open_fragment("nope.ink", 0, 1), 0);

        let d1 = s.open_document("a.ink");
        assert_eq!(d1, 1, "ids start at 1");
        assert!(s.close_document(d1));
        assert!(!s.close_document(d1), "double close reports unknown");

        // Closed handles answer with the same sentinels as a missing file.
        assert_eq!(s.hover_doc(d1, 5), "null");
        assert_eq!(s.line_contexts_doc(d1), "[]");
        assert_eq!(s.get_view_source_doc(d1), "null");
        assert_eq!(s.update_document(d1, "x"), "null");

        // Reopen: a fresh id, never a reused one.
        let d2 = s.open_document("a.ink");
        assert_ne!(d2, 0);
        assert_ne!(d2, d1, "handle ids are not reused");
        assert_ne!(s.get_view_source_doc(d2), "null");
    }

    #[test]
    fn singleton_api_unaffected_by_handle_operations() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== a ===\nA line\n=== b ===\nB line\n");
        s.update_file("other.ink", "hello\n");
        assert!(s.set_active_file("main.ink"));
        // Singleton view over knot `a`: UTF-16 [0, 17).
        s.set_view_context(0, 17);
        let before = s.get_view_source();
        assert_eq!(
            before,
            serde_json::to_string("=== a ===\nA line\n").unwrap()
        );

        // Handle operations on another file leave the singleton alone.
        let d = s.open_document("other.ink");
        let _ = s.update_document(d, "world\n");
        assert!(s.close_document(d));
        assert_eq!(s.active_file(), "main.ink");
        assert_eq!(s.get_view_source(), before);

        // The singleton splice path still works after handle traffic.
        s.update_source("=== a ===\nA edit");
        assert_eq!(
            s.get_file_source("main.ink"),
            serde_json::to_string("=== a ===\nA edit\n=== b ===\nB line\n").unwrap()
        );
        assert_eq!(
            s.get_view_source(),
            serde_json::to_string("=== a ===\nA edit").unwrap()
        );
    }

    // ── Story graph (#96) ───────────────────────────────────────────

    const GRAPH_MAIN: &str = "é\n=== start ===\n* [Go] -> east.gate\n- -> END\n";
    const GRAPH_EAST: &str = "=== east ===\n= gate\nGate.\n-> start\n";

    #[test]
    fn story_graph_null_without_analysis() {
        let s = EditorSession::new();
        assert_eq!(s.story_graph(), "null");
    }

    #[test]
    fn story_graph_shape_and_utf16_offsets() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", GRAPH_MAIN);
        s.update_file("east.ink", GRAPH_EAST);

        let json = s.story_graph();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Nodes sorted by id; pseudo-node END present (referenced), DONE not.
        let ids: Vec<&str> = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["END", "east", "east.gate", "start"]);

        let nodes = v["nodes"].as_array().unwrap();
        let node = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();

        // `start` is declared after the 2-byte/1-unit `é`: its name sits at
        // byte 7 but UTF-16 offset 6 — the span must be UTF-16.
        let start = node("start");
        assert_eq!(start["kind"], "knot");
        assert_eq!(start["file"], "main.ink");
        assert_eq!(start["start"].as_u64().unwrap(), 6, "must be UTF-16");
        assert_eq!(start["end"].as_u64().unwrap(), 11);
        assert!(start.get("parent").is_none(), "knots carry no parent");

        let gate = node("east.gate");
        assert_eq!(gate["kind"], "stitch");
        assert_eq!(gate["file"], "east.ink");
        assert_eq!(gate["parent"], "east");

        let end = node("END");
        assert_eq!(end["kind"], "end");
        assert!(end.get("file").is_none(), "pseudo-nodes have no file");
        assert!(end.get("start").is_none(), "pseudo-nodes have no span");

        // Edges sorted by (from, to, kind); choice aggregation + cross-file
        // resolution + the auto-enter divert east -> east.gate.
        let edges: Vec<(String, String, String)> = v["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["from"].as_str().unwrap().to_owned(),
                    e["to"].as_str().unwrap().to_owned(),
                    e["kind"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let owned: Vec<(&str, &str, &str)> = edges
            .iter()
            .map(|(f, t, k)| (f.as_str(), t.as_str(), k.as_str()))
            .collect();
        assert_eq!(
            owned,
            vec![
                ("east", "east.gate", "divert"),
                ("east.gate", "start", "divert"),
                ("start", "END", "divert"),
                ("start", "east.gate", "choice"),
            ]
        );
    }

    #[test]
    fn story_graph_deterministic_across_file_insertion_order() {
        let mut a = EditorSession::new();
        a.update_file("main.ink", GRAPH_MAIN);
        a.update_file("east.ink", GRAPH_EAST);

        let mut b = EditorSession::new();
        b.update_file("east.ink", GRAPH_EAST);
        b.update_file("main.ink", GRAPH_MAIN);

        assert_eq!(
            a.story_graph(),
            b.story_graph(),
            "story graph JSON must be identical regardless of insertion order"
        );
    }

    // ── Document-agnostic / symbol-keyed references (#317) ──────────

    fn refs_count(json: &str) -> usize {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v.as_array().map_or(0, Vec::len)
    }

    #[test]
    fn find_references_at_same_file() {
        // Two diverts into `hello` plus its declaration, all in one file.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset on the first `-> hello` reference (the `h` of the target).
        let json = s.find_references_at("main.ink", 3, true);
        // Declaration + two divert references = 3.
        assert_eq!(refs_count(&json), 3, "same-file refs incl. decl: {json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        for loc in v.as_array().unwrap() {
            assert_eq!(loc["file"], "main.ink");
        }
    }

    #[test]
    fn find_references_at_cross_file() {
        // `other.ink` diverts into `hello` declared in `main.ink`.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        // Offset on the `hello` of the declaration `=== hello ===` (utf16 = byte).
        let json = s.find_references_at("main.ink", 4, true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files: std::collections::BTreeSet<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["file"].as_str().unwrap())
            .collect();
        assert!(
            files.contains("main.ink") && files.contains("other.ink"),
            "cross-file refs must span both files: {json}"
        );
    }

    #[test]
    fn find_references_at_honors_include_declaration() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> hello\n=== hello ===\nHi.\n-> hello\n");
        assert!(s.set_active_file("main.ink"));

        let with_decl = refs_count(&s.find_references_at("main.ink", 3, true));
        let without_decl = refs_count(&s.find_references_at("main.ink", 3, false));
        assert_eq!(
            with_decl,
            without_decl + 1,
            "excluding the declaration drops exactly one result"
        );
    }

    #[test]
    fn references_to_symbol_name_keyed_lookup() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        s.update_file("other.ink", "-> hello\n");
        assert!(s.set_active_file("main.ink"));

        let json = s.references_to_symbol("hello", true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files: std::collections::BTreeSet<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["file"].as_str().unwrap())
            .collect();
        assert!(
            files.contains("main.ink") && files.contains("other.ink"),
            "symbol-keyed lookup resolves the declaration + cross-file ref: {json}"
        );

        // include_declaration is honored through the symbol-keyed path too.
        let with_decl = refs_count(&json);
        let without_decl = refs_count(&s.references_to_symbol("hello", false));
        assert_eq!(with_decl, without_decl + 1);
    }

    #[test]
    fn references_to_symbol_nonexistent_is_empty() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        assert_eq!(
            s.references_to_symbol("does_not_exist", true),
            "[]",
            "unknown symbol fails safe to []"
        );
    }

    // ── resolve_code_action (#321 Track N) ──────────────────────────
    // The code_actions JSON carries a self-describing `data` discriminator;
    // feeding that payload back to resolve_code_action applies the action and
    // returns StructuralResult-shaped JSON with the rewritten source.

    /// The byte offset (cursor) inside the first knot's body — enough to scope
    /// cursor-anchored actions to that knot.
    const UNSORTED_KNOTS: &str = "=== beta ===\nhi\n-> END\n=== alpha ===\nyo\n-> END\n";

    fn find_action<'a>(
        actions: &'a serde_json::Value,
        title_contains: &str,
    ) -> &'a serde_json::Value {
        actions
            .as_array()
            .expect("code_actions returns a JSON array")
            .iter()
            .find(|a| {
                a["title"]
                    .as_str()
                    .is_some_and(|t| t.contains(title_contains))
            })
            .expect("a code action whose title matches the expected substring")
    }

    #[test]
    fn code_action_data_is_self_describing() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(0)).expect("valid JSON array");
        let sort = find_action(&actions, "Sort knots");
        // The tagged discriminator must be present and round-trippable.
        assert_eq!(
            sort["data"]["action"], "SortKnots",
            "data carries the tagged discriminator: {actions}"
        );
    }

    #[test]
    fn resolve_sort_action_yields_sorted_source() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(0)).expect("valid JSON array");
        let data = find_action(&actions, "Sort knots")["data"].to_string();

        let result: serde_json::Value = serde_json::from_str(&s.resolve_code_action(&data, 0))
            .expect("valid StructuralResult JSON");
        assert_eq!(result["ok"], true, "resolve succeeds: {result}");
        let new_source = result["new_source"]
            .as_str()
            .expect("new_source is present and a string");
        assert!(
            !new_source.is_empty(),
            "sort action produces non-empty edits"
        );
        // alpha now precedes beta.
        let alpha = new_source.find("alpha").expect("alpha knot present");
        let beta = new_source.find("beta").expect("beta knot present");
        assert!(
            alpha < beta,
            "knots are sorted alphabetically: {new_source:?}"
        );
    }

    #[test]
    fn references_to_symbol_ambiguous_is_empty() {
        // A knot and a variable share the name `dup`. They are different kinds,
        // so both land under one `by_name` key → ambiguous (two ids).
        let mut s = EditorSession::new();
        s.update_file("main.ink", "VAR dup = 0\n=== dup ===\nhi.\n-> END\n");
        assert!(s.set_active_file("main.ink"));

        assert_eq!(
            s.references_to_symbol("dup", true),
            "[]",
            "ambiguous symbol name fails safe to []"
        );
    }

    #[test]
    fn completions_tag_out_of_scope_symbols_with_source_file() {
        // main.ink INCLUDEs included.ink but NOT economy.ink. A knot from the
        // reachable file is in scope; one from the unreachable file is tagged
        // out-of-scope with its source path (#312 F).
        let mut s = EditorSession::new();
        s.update_file("included.ink", "=== reachable_knot ===\nhi.\n-> END\n");
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        let main = "INCLUDE included.ink\n=== start ===\n-> re\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        // Cursor after `-> re` (a divert context, which surfaces knots).
        let offset = u32::try_from(main.find("-> re").expect("divert present") + 5)
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let arr = items.as_array().expect("completions is an array");

        let reachable = arr
            .iter()
            .find(|i| i["name"] == "reachable_knot")
            .expect("reachable knot offered");
        assert!(
            reachable.get("out_of_scope").is_none(),
            "in-scope knot is not flagged out_of_scope: {reachable}"
        );

        let trade = arr
            .iter()
            .find(|i| i["name"] == "trade")
            .expect("out-of-scope knot offered");
        assert_eq!(
            trade["out_of_scope"], true,
            "unreachable knot flagged out_of_scope: {trade}"
        );
        assert_eq!(
            trade["source_file"], "economy.ink",
            "out-of-scope knot carries its source file: {trade}"
        );
    }

    #[test]
    fn completions_after_ref_keyword_offers_only_durable_variables() {
        // T1e (docs/t1e-spec.md §2, issue #850): right after `ref `, only a
        // `VAR` is a legal `ref lvalue-path` root (E080) — a CONST, param,
        // or temp is not, so it must not be offered there even though the
        // ordinary `FunctionArgs` completion set includes all of them.
        let mut s = EditorSession::new();
        let main = "VAR npc = 0\n\
                     CONST MAX_HP = 100\n\
                     === main(amount) ===\n\
                     ~ temp scratch = 0\n\
                     ~ heal(ref \n\
                     -> END\n\n\
                     === function heal(ref hp, k) ===\n\
                     ~ hp = hp + k\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        let offset =
            u32::try_from(main.find("heal(ref \n").expect("call present") + "heal(ref ".len())
                .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let arr = items.as_array().expect("completions is an array");
        let names: Vec<&str> = arr
            .iter()
            .map(|i| i["name"].as_str().expect("name is a string"))
            .collect();

        assert!(
            names.contains(&"npc"),
            "the durable VAR is offered: {names:?}"
        );
        assert!(
            !names.contains(&"MAX_HP"),
            "a CONST is not a legal ref root, must not be offered: {names:?}"
        );
        assert!(
            !names.contains(&"scratch"),
            "a temp is not a legal ref root, must not be offered: {names:?}"
        );
        assert!(
            !names.contains(&"amount"),
            "a param is not a legal ref root, must not be offered: {names:?}"
        );
    }

    #[test]
    fn completions_in_ordinary_arg_position_still_offers_everything() {
        // Sanity check for the test above: without a preceding `ref `, the
        // full FunctionArgs set (including CONST/param/temp) is unaffected.
        let mut s = EditorSession::new();
        let main = "VAR npc = 0\n\
                     CONST MAX_HP = 100\n\
                     === main(amount) ===\n\
                     ~ temp scratch = 0\n\
                     ~ heal(\n\
                     -> END\n\n\
                     === function heal(hp, k) ===\n\
                     ~ hp = hp + k\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        let offset = u32::try_from(main.find("heal(\n").expect("call present") + "heal(".len())
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let arr = items.as_array().expect("completions is an array");
        let names: Vec<&str> = arr
            .iter()
            .map(|i| i["name"].as_str().expect("name is a string"))
            .collect();

        assert!(names.contains(&"npc"), "VAR offered: {names:?}");
        assert!(names.contains(&"MAX_HP"), "CONST offered: {names:?}");
        assert!(names.contains(&"scratch"), "temp offered: {names:?}");
    }

    #[test]
    fn completions_dedupe_out_of_scope_keeps_nearest() {
        // Two unreachable files both define `dup`. The nearer one (same dir as
        // the current file) wins deterministically; only one row survives.
        let mut s = EditorSession::new();
        s.update_file("near.ink", "=== dup ===\nn.\n-> END\n");
        s.update_file("deep/far.ink", "=== dup ===\nf.\n-> END\n");
        let main = "=== start ===\n-> du\n";
        s.update_file("main.ink", main);
        assert!(s.set_active_file("main.ink"));

        let offset = u32::try_from(main.find("-> du").expect("divert present") + 5)
            .expect("offset fits u32");
        let items: serde_json::Value =
            serde_json::from_str(&s.completions(offset)).expect("valid completions JSON");
        let dups: Vec<&serde_json::Value> = items
            .as_array()
            .expect("array")
            .iter()
            .filter(|i| i["name"] == "dup")
            .collect();

        assert_eq!(
            dups.len(),
            1,
            "duplicate out-of-scope name collapses to one: {dups:?}"
        );
        assert_eq!(
            dups[0]["source_file"], "near.ink",
            "nearest source file wins: {:?}",
            dups[0]
        );
    }

    #[test]
    fn auto_import_apply_include_doc_rebases_open_fragment_view() {
        // Regression (#312 F): the fragment-view auto-import path. A raw
        // whole-file INCLUDE write shifts the fragment content right but leaves
        // the open fragment handle's view range at pre-shift offsets, so the
        // NEXT fragment splice clobbers the INCLUDE line and the knot header.
        // `auto_import_apply_include_doc` must apply the INCLUDE *and* rebase
        // the open fragment view so the subsequent splice lands correctly.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        let main_src = "=== start ===\nThe cursor is here.\n";
        s.update_file("main.ink", main_src);

        // Open a fragment over the knot BODY ("The cursor is here.\n"). The body
        // begins right after the "=== start ===\n" header (byte 14) and runs to
        // end of file.
        let body_start = "=== start ===\n".len() as u32;
        let body_end = main_src.len() as u32;
        let doc = s.open_fragment("main.ink", body_start, body_end);
        assert_ne!(doc, 0, "fragment handle opened");

        // Accept an out-of-scope completion: auto-import economy.ink into the
        // fragment's file, applying + rebasing out-of-band.
        let applied: serde_json::Value =
            serde_json::from_str(&s.auto_import_apply_include_doc(doc, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(applied["ok"], true, "op ok: {applied}");
        assert_eq!(
            applied["already_reachable"], false,
            "not yet reachable: {applied}"
        );
        // The returned edit DESCRIBES the applied shift (for the caller to
        // rebase its own TS-side range) — it is NOT to be re-applied.
        assert_eq!(
            applied["edit"]["insert"].as_str(),
            Some("INCLUDE economy.ink\n"),
            "returned edit describes the applied INCLUDE shift: {applied}"
        );
        assert_eq!(
            applied["edit"]["from"], 0,
            "INCLUDE inserted at file top: {applied}"
        );

        // The whole file now carries the INCLUDE above the untouched knot.
        let full_after_import = s.source_of("main.ink").expect("source").to_owned();
        assert_eq!(
            full_after_import, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.\n",
            "INCLUDE prepended, knot intact"
        );

        // Now the completion dispatches the accepted symbol into the FRAGMENT
        // view (the edited body). This routes through update_document, which
        // splices at the (now rebased) view range.
        let edited_body = "The cursor is here.trade\n";
        let spec = s.update_document(doc, edited_body);
        assert_ne!(spec, "null", "fragment push produced a change spec");

        // The INCLUDE line and knot header must survive; only the body changed.
        let full_after_push = s.source_of("main.ink").expect("source");
        assert_eq!(
            full_after_push, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.trade\n",
            "INCLUDE + header intact; only the fragment body was replaced"
        );
    }

    #[test]
    fn raw_update_file_then_fragment_push_corrupts_without_rebase() {
        // Documents the pre-fix bug (#312 F): applying the INCLUDE via the raw
        // whole-file `update_file` (which does NOT rebase open fragment views)
        // and then pushing the fragment splices at the STALE view range,
        // clobbering the INCLUDE line and the knot header. This is exactly the
        // corruption `auto_import_apply_include_doc` avoids. If this assertion
        // ever flips to producing clean output, `update_file` grew rebase
        // semantics and the fragment auto-import path can be simplified.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\n-> END\n");
        let main_src = "=== start ===\nThe cursor is here.\n";
        s.update_file("main.ink", main_src);

        let body_start = "=== start ===\n".len() as u32;
        let doc = s.open_fragment("main.ink", body_start, main_src.len() as u32);

        // OLD path: prepend INCLUDE via a raw whole-file replace (no rebase).
        s.update_file("main.ink", &format!("INCLUDE economy.ink\n{main_src}"));
        // Then push the edited fragment — splices at the stale [14, 34) range.
        s.update_document(doc, "The cursor is here.trade\n");

        let corrupted = s.source_of("main.ink").expect("source");
        assert_ne!(
            corrupted, "INCLUDE economy.ink\n=== start ===\nThe cursor is here.trade\n",
            "raw path corrupts — this is the bug the apply-and-rebase op fixes"
        );
    }

    #[test]
    fn auto_import_apply_include_doc_idempotent_when_reachable() {
        // When the target is already reachable, the apply-and-rebase op is a
        // no-op: no INCLUDE added, view range untouched.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\n-> END\n");
        let main_src = "INCLUDE economy.ink\n=== start ===\nbody.\n";
        s.update_file("main.ink", main_src);
        // Re-analyze so the INCLUDE edge binds to the now-loaded target.
        s.update_file("main.ink", main_src);

        let body_start = "INCLUDE economy.ink\n=== start ===\n".len() as u32;
        let doc = s.open_fragment("main.ink", body_start, main_src.len() as u32);
        assert_ne!(doc, 0);

        let applied: serde_json::Value =
            serde_json::from_str(&s.auto_import_apply_include_doc(doc, "economy.ink"))
                .expect("valid JSON");
        assert_eq!(applied["already_reachable"], true, "already reachable");
        assert!(applied.get("edit").is_none(), "no edit");

        // Pushing the fragment still lands correctly (view range never moved).
        s.update_document(doc, "body.\n-> trade\n");
        assert_eq!(
            s.source_of("main.ink").expect("source"),
            "INCLUDE economy.ink\n=== start ===\nbody.\n-> trade\n"
        );
    }

    #[test]
    fn auto_import_doc_edit_is_utf16_and_idempotent() {
        // The doc-based auto-import returns a whole-file edit for an unreachable
        // target, and no edit once the file already reaches it.
        let mut s = EditorSession::new();
        s.update_file("economy.ink", "=== trade ===\nbuy.\n-> END\n");
        s.update_file("main.ink", "=== start ===\n-> END\n");
        assert!(s.set_active_file("main.ink"));
        let doc = s.open_document("main.ink");

        let first: serde_json::Value =
            serde_json::from_str(&s.auto_import_include_doc(doc, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(first["ok"], true, "op ok: {first}");
        assert_eq!(
            first["already_reachable"], false,
            "not yet reachable: {first}"
        );
        let insert = first["edit"]["insert"]
            .as_str()
            .expect("edit carries an insert string");
        assert!(
            insert.contains("INCLUDE economy.ink"),
            "insert adds the INCLUDE: {first}"
        );

        // Apply the edit, re-analyze, then a second call is a no-op.
        s.update_file("main.ink", &format!("{insert}=== start ===\n-> END\n"));
        assert!(s.set_active_file("main.ink"));
        let doc2 = s.open_document("main.ink");
        let second: serde_json::Value =
            serde_json::from_str(&s.auto_import_include_doc(doc2, "economy.ink"))
                .expect("valid auto-import JSON");
        assert_eq!(second["already_reachable"], true, "now reachable: {second}");
        assert!(
            second.get("edit").is_none(),
            "idempotent — no second INCLUDE: {second}"
        );
    }

    #[test]
    fn resolve_format_action_yields_formatted_source() {
        // A knot whose body has a formatting deviation the formatter fixes.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== k ===\n* [opt]   trailing spaces   \n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        // Cursor inside the knot body.
        let offset = 12;
        let actions: serde_json::Value =
            serde_json::from_str(&s.code_actions(offset)).expect("valid JSON array");
        let format = find_action(&actions, "Format knot");
        assert_eq!(
            format["data"]["action"], "FormatKnot",
            "format action carries its discriminator: {actions}"
        );
        let data = format["data"].to_string();

        let result: serde_json::Value = serde_json::from_str(&s.resolve_code_action(&data, offset))
            .expect("valid StructuralResult JSON");
        assert_eq!(result["ok"], true, "resolve succeeds: {result}");
        let new_source = result["new_source"]
            .as_str()
            .expect("new_source is present and a string");
        assert!(
            !new_source.is_empty(),
            "format action produces non-empty edits"
        );
    }

    #[test]
    fn resolve_rejects_malformed_data() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", UNSORTED_KNOTS);
        assert!(s.set_active_file("main.ink"));

        let result: serde_json::Value =
            serde_json::from_str(&s.resolve_code_action("{ not valid }", 0))
                .expect("error is still StructuralResult-shaped JSON");
        assert_eq!(result["ok"], false, "malformed data -> ok:false: {result}");
        assert!(
            result["error"].as_str().is_some(),
            "an error message is present"
        );
    }

    // ── Dialogue dialect (#368) ──────────────────────────────────────

    #[test]
    fn set_dialect_then_line_contexts_reports_character_kind() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\nHello there.\n");
        assert!(s.set_active_file("main.ink"));

        // No dialect registered yet: no `dialect` facet on any line.
        let before = json(&s.line_contexts());
        assert!(before[1].get("dialect").is_none(), "{before}");

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");

        let after = json(&s.line_contexts());
        assert_eq!(after[1]["dialect"]["kind"], "character", "{after}");
        assert_eq!(after[2]["dialect"]["kind"], "dialogue", "{after}");
    }

    #[test]
    fn folding_ranges_include_machinery_and_narrative_runs() {
        // #365: `folding_ranges()` returns structural folds (from the
        // pre-existing pass) plus machinery/narrative fold runs computed
        // from the same `line_contexts` classification the editor already
        // consumes — a real user path (folding gutter), not a separate
        // code path only a unit test reaches. Runs are opt-in since #479.
        let mut s = EditorSession::new();
        s.set_fold_runs_enabled(true);
        s.update_file(
            "main.ink",
            "=== start ===\n~ temp x = 1\n~ temp y = 2\nHello there.\nHow are you?\n",
        );
        assert!(s.set_active_file("main.ink"));

        let ranges = json(&s.folding_ranges());
        let kinds: Vec<&str> = ranges
            .as_array()
            .expect("array")
            .iter()
            .map(|r| r["kind"].as_str().expect("kind is a string"))
            .collect();
        assert!(kinds.contains(&"machinery"), "{ranges}");
        assert!(kinds.contains(&"narrative"), "{ranges}");

        let machinery = ranges
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["kind"] == "machinery")
            .expect("machinery fold present");
        assert_eq!(machinery["start_line"], 1);
        assert_eq!(machinery["end_line"], 2);
    }

    #[test]
    fn fold_runs_are_gated_off_by_default() {
        // #479: machinery/narrative runs are computed only when the host
        // opts in via set_fold_runs_enabled; the default folding output is
        // structural-only.
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n~ temp x = 1\n~ temp y = 2\nHello\n",
        );
        assert!(s.set_active_file("main.ink"));

        let ranges = json(&s.folding_ranges());
        assert!(
            ranges
                .as_array()
                .expect("array")
                .iter()
                .all(|r| r["kind"] == "structural"),
            "no run kinds without opt-in: {ranges}"
        );

        s.set_fold_runs_enabled(true);
        let ranges = json(&s.folding_ranges());
        assert!(
            ranges
                .as_array()
                .expect("array")
                .iter()
                .any(|r| r["kind"] == "machinery"),
            "machinery runs appear once enabled: {ranges}"
        );
    }

    #[test]
    fn folding_ranges_respect_registered_dialect_nature() {
        // A dialect-classified cue+dialogue pair is Narrative-natured (the
        // at-cue preset) — the fold run must follow the registered dialect,
        // not a hardcoded kind list.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\nHello there.\n");
        assert!(s.set_active_file("main.ink"));

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");
        s.set_fold_runs_enabled(true);

        let ranges = json(&s.folding_ranges());
        let has_narrative_run = ranges
            .as_array()
            .expect("array")
            .iter()
            .any(|r| r["kind"] == "narrative" && r["start_line"] == 1 && r["end_line"] == 2);
        assert!(has_narrative_run, "{ranges}");
    }

    #[test]
    fn clear_dialect_reverts_to_plain_classification() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n@Alice:<>\n");
        assert!(s.set_active_file("main.ink"));

        let preset = serde_json::to_string(&brink_ir::DialogueDialect::default())
            .expect("preset serializes");
        s.set_dialect(&preset).expect("preset validates");
        assert_eq!(json(&s.line_contexts())[1]["dialect"]["kind"], "character");

        s.clear_dialect();
        let after = json(&s.line_contexts());
        assert!(after[1].get("dialect").is_none(), "{after}");
    }

    // `set_dialect`'s error path constructs a `JsError`, which panics when
    // called on a non-wasm target ("cannot call wasm-bindgen imported
    // functions on non-wasm targets") — same constraint as every other
    // `Result<_, JsError>`-returning method in this file (see
    // `StoryRunner::new`, `continue_story`, etc., whose error paths are only
    // exercised under `binding_wasm_tests` below). The rejection tests live
    // there; validated JSON acceptance is what's tested natively above.
    //
    // `WebSession` test coverage lives in `websession_wasm_tests` below —
    // its methods take `&JsValue`/`Vec<JsValue>` args, and constructing a
    // `JsValue` (e.g. `JsValue::from_f64`) itself panics off wasm32 (not just
    // the error paths), unlike `StoryRunner`'s native-testable subset.

    // ── Lines table (#366) ────────────────────────────────────────────
    // `StoryRunner::lines_table`'s happy path never constructs a `JsError`
    // (only the `serde_json` error branch would, and that can't fail for a
    // `LinesJson` value), so — like `set_dialect`'s acceptance path above —
    // it's exercised natively here rather than under `binding_wasm_tests`.

    fn compiled(src: &str) -> crate::story_runner::StoryRunner {
        let out = brink_compiler::compile("main.ink", |_path| Ok(src.to_owned()))
            .expect("test source compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        crate::story_runner::StoryRunner::new(&bytes).expect("runner constructs")
    }

    #[test]
    fn lines_table_reports_text_and_source_span() {
        let runner = compiled("=== start ===\nHello, world!\n-> END\n");
        let table = runner.lines_table().expect("lines_table succeeds");
        let v: serde_json::Value = serde_json::from_str(&table).expect("valid json");

        assert_eq!(v["version"], 1);
        let scopes = v["scopes"].as_array().expect("scopes array");
        let scope = scopes
            .iter()
            .find(|s| s["name"] == "start")
            .expect("start scope present");
        let line = scope["lines"]
            .as_array()
            .expect("lines array")
            .iter()
            .find(|l| l["content"] == "Hello, world!")
            .expect("the line's plain text content is present");
        let source = &line["source"];
        assert_eq!(source["file"], "main.ink", "{table}");
        assert!(
            source["range_start"].as_u64().is_some() && source["range_end"].as_u64().is_some(),
            "source span present: {table}"
        );
    }

    #[test]
    fn lines_table_resolves_includes_project_wide() {
        let out = brink_compiler::compile("main.ink", |path| match path {
            "main.ink" => Ok(
                "INCLUDE included.ink\n=== start ===\n-> other.other_stitch\n-> END\n".to_owned(),
            ),
            "included.ink" => {
                Ok("=== other ===\n= other_stitch\nFrom the included file.\n-> END\n".to_owned())
            }
            _ => Err(std::io::Error::other(format!("unknown file {path}"))),
        })
        .expect("multi-file project compiles");
        let mut bytes = Vec::new();
        brink_format::write_inkb(&out.data, &mut bytes);
        let runner = crate::story_runner::StoryRunner::new(&bytes).expect("runner constructs");

        let table = runner.lines_table().expect("lines_table succeeds");
        let v: serde_json::Value = serde_json::from_str(&table).expect("valid json");
        let scopes = v["scopes"].as_array().expect("scopes array");
        let has_included_line = scopes.iter().any(|scope| {
            scope["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|l| l["content"] == "From the included file.")
        });
        assert!(
            has_included_line,
            "the lines table covers the whole project, INCLUDEs resolved: {table}"
        );
        let included_source_file = scopes
            .iter()
            .flat_map(|scope| scope["lines"].as_array().into_iter().flatten())
            .find(|l| l["content"] == "From the included file.")
            .and_then(|l| l["source"]["file"].as_str().map(str::to_owned));
        assert_eq!(
            included_source_file.as_deref(),
            Some("included.ink"),
            "source span attributes the line to its own included file: {table}"
        );
    }

    // ── #600: wire the #589 IDE features into the wasm bridge ─────────

    #[test]
    fn stdlib_completions_offered_only_after_opting_into_brink_dialect() {
        // Before `set_language_dialect("brink")` the bridge defaults to
        // `StrictInk` (matching `AnalysisOptions::default()`) — stdlib slice
        // 1 names (docs/t1b-surface-spec.md §5) must not be offered.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n~ temp x = \nEND\n");
        assert!(s.set_active_file("main.ink"));

        let offset = u32::try_from("=== start ===\n~ temp x = ".len()).expect("fits u32");
        let before = json(&s.completions(offset));
        assert!(
            before
                .as_array()
                .expect("array")
                .iter()
                .all(|i| i["name"] != "len"),
            "stdlib names withheld under the StrictInk default: {before}"
        );

        s.set_language_dialect("brink");
        let after = json(&s.completions(offset));
        let len_item = after
            .as_array()
            .expect("array")
            .iter()
            .find(|i| i["name"] == "len")
            .expect("stdlib `len` offered once brink dialect is set");
        assert_eq!(len_item["kind"], "stdlib", "{len_item}");
        assert_eq!(len_item["detail"], "len(x) -> int", "{len_item}");
    }

    #[test]
    fn stdlib_completions_never_offered_for_strict_ink_value() {
        // Any value other than the exact string "brink" (including a typo
        // or an explicit "strict-ink") keeps the StrictInk default, mirroring
        // brink-lsp's `initializationOptions.dialect` handling.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\n~ temp x = \nEND\n");
        assert!(s.set_active_file("main.ink"));
        s.set_language_dialect("strict-ink");

        let offset = u32::try_from("=== start ===\n~ temp x = ".len()).expect("fits u32");
        let items = json(&s.completions(offset));
        assert!(
            items
                .as_array()
                .expect("array")
                .iter()
                .all(|i| i["name"] != "len"),
            "{items}"
        );
    }

    #[test]
    fn signature_help_is_dialect_aware_for_stdlib_mutators() {
        // #600: `signature_help` must call `signature_help_with_dialect` —
        // the lvalue-mutator rendering (`push(a: lvalue, v)`,
        // docs/t1b-surface-spec.md §5) only surfaces once brink dialect is
        // set, and never under the StrictInk default.
        let src = "=== start ===\n~ push(inventory, \"sword\")\n-> END\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", src);
        assert!(s.set_active_file("main.ink"));

        let offset =
            u32::try_from(src.find("push(").expect("call present") + "push(".len()).expect("fits");

        let before = s.signature_help(offset);
        assert_eq!(before, "null", "no stdlib signature help under StrictInk");

        s.set_language_dialect("brink");
        let sig = json(&s.signature_help(offset));
        assert_eq!(sig["label"], "push(a: lvalue, v)", "{sig}");
        assert_eq!(sig["active_parameter"], 0, "{sig}");
    }

    #[test]
    fn folding_ranges_include_logic_block_folds() {
        // #600: `folding_ranges` must call `block_folds` — a `~ { … }`
        // logic block folds as its own structural region, with no dialect
        // gate (it folds identically in a strict-ink file, where it is
        // flagged E051, as in a brink one).
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "=== start ===\n~ {\n    temp x = 1\n    temp y = 2\n}\nHello.\n-> END\n",
        );
        assert!(s.set_active_file("main.ink"));

        let ranges = json(&s.folding_ranges());
        let block = ranges
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["kind"] == "structural" && r["start_line"] == 1)
            .expect("logic block folds as a structural region");
        assert_eq!(block["end_line"], 4, "{block}");
    }

    // ── #611: thread the declared dialect into background analysis ────

    /// A `~ { … }` multi-line logic block is brink-extension syntax
    /// (docs/t1b-surface-spec.md §1) — flagged `E051` under the default
    /// `StrictInk` dialect, silent under `Brink`.
    const BRINK_EXT_SRC: &str = "=== start ===\n~ {\n    temp x = 1\n}\n-> END\n";

    #[test]
    fn strict_ink_default_flags_e051_in_background_analysis() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "StrictInk default: E051 stands: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_brink_suppresses_e051_in_background_analysis() {
        // #611: before this fix, `set_language_dialect` only updated the
        // bridge's local `dialect` field (consumed by completions/signature
        // help) — the `IdeSession`'s background analysis pass never saw it,
        // so a brink-dialect project kept showing spurious `E051`.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.set_language_dialect("brink");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "brink dialect: no E051 on valid extension syntax: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_strict_ink_value_keeps_e051() {
        // Any value other than the exact string "brink" (including an
        // explicit "strict-ink") keeps the StrictInk default.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.set_language_dialect("strict-ink");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "explicit strict-ink: E051 stands: {:?}",
            analysis.diagnostics
        );
    }

    // ── TM-3 typed-mode policy (#660) ──────────────────────────────────

    /// #660 / NS-A9: with `set_type_policy` never called, the dialect-keyed
    /// default applies (issue #1127, ruled 2026-07-19) — a strict-ink
    /// (default-dialect) session resolves gradual (no E064, no E065), and a
    /// brink session resolves strict (E065 fires on an unannotatable param).
    #[test]
    fn type_policy_defaults_are_dialect_keyed() {
        // strict-ink cell: gradual — never any strict diagnostic.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis.diagnostics.iter().any(|d| matches!(
                d.code,
                brink_ir::DiagnosticCode::E064 | brink_ir::DiagnosticCode::E065
            )),
            "strict-ink + unset types resolves gradual: {:?}",
            analysis.diagnostics
        );

        // brink cell: strict — the Unknown-escape fires with no explicit
        // `set_type_policy` call at all.
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "brink + unset types resolves strict (E065 fires): {:?}",
            analysis.diagnostics
        );
    }

    /// #660: `types = strict` under the default `StrictInk` dialect is a
    /// project-level config error (`E064`) — reached through
    /// `set_type_policy`, mirroring the compiler CLI's
    /// `--types strict --dialect strict-ink`.
    #[test]
    fn set_type_policy_strict_with_strict_ink_dialect_flags_e064() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> END\n");
        s.set_type_policy("strict");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E064),
            "types=strict + dialect=strict-ink (default): expected E064: {:?}",
            analysis.diagnostics
        );
    }

    /// #660 counterpart: `types = strict` + `dialect = brink` turns on the
    /// Unknown-escape check (`E065`) — proving `set_type_policy` reaches the
    /// real strict-mode checks, not just the config-error path, and that
    /// `compile_project` (which reads `IdeSession::analysis_options`) would
    /// see the same policy.
    #[test]
    fn set_type_policy_strict_with_brink_dialect_flags_e065() {
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "types=strict + dialect=brink: expected E065 on unused param `x`: {:?}",
            analysis.diagnostics
        );

        // The compile entry point (`compile_project`) reads the same
        // session's `analysis_options`, so the wasm-observable compile path
        // sees the strict-mode diagnostic too, not just background analysis.
        // E065 is error-severity, so compilation fails (`ok: false`).
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(v["ok"], false, "{result}");
        let has_unknown_escape = v["warnings"].as_array().is_some_and(|ws| {
            ws.iter()
                .any(|w| w["message"].as_str().unwrap_or("").contains("Unknown"))
        });
        assert!(
            has_unknown_escape,
            "compile_project should surface the strict-mode E065 diagnostic too: {result}"
        );
    }

    /// #660 / NS-A9: an explicit "gradual" opts a brink session out of the
    /// dialect-keyed strict default.
    #[test]
    fn set_type_policy_gradual_value_keeps_default() {
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.set_type_policy("gradual");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "explicit gradual: no E065: {:?}",
            analysis.diagnostics
        );
    }

    /// NS-A9: an unrecognized `set_type_policy` value behaves like never
    /// calling — the dialect-keyed default stays in effect (it must NOT be
    /// treated as an explicit gradual opt-out, the pre-NS-A9 "any other
    /// value keeps the default" contract carried forward).
    #[test]
    fn set_type_policy_unrecognized_value_keeps_dialect_keyed_default() {
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.set_type_policy("bogus");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "unrecognized value must keep the brink dialect's strict default (E065 fires): {:?}",
            analysis.diagnostics
        );
    }

    // ── Project config file (#1005) ────────────────────────────────────

    #[test]
    fn apply_project_config_applies_dialect_from_file() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        let warnings = s
            .apply_project_config("[project]\ndialect = \"brink\"\n")
            .expect("valid brink.toml");
        assert_eq!(warnings, "[]");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "brink.toml's dialect = brink: no E051 on valid extension syntax: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_applies_types_from_file() {
        let mut s = EditorSession::new();
        s.apply_project_config("[project]\ndialect = \"brink\"\ntypes = \"strict\"\n")
            .expect("valid brink.toml");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "brink.toml's types = strict: expected E065 on unused param `x`: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_no_file_values_leaves_defaults() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        let warnings = s.apply_project_config("").expect("empty document is valid");
        assert_eq!(warnings, "[]");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "no [project] table: StrictInk default stands: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_after_explicit_set_language_dialect_is_a_no_op_for_dialect() {
        // #1005 precedence: an explicit `set_language_dialect` call always
        // wins over a later `apply_project_config` — the file only ever
        // supplies a default.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.set_language_dialect("strict-ink");
        s.apply_project_config("[project]\ndialect = \"brink\"\n")
            .expect("valid brink.toml");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "explicit set_language_dialect(\"strict-ink\") must survive a later \
             apply_project_config(dialect = brink): {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_before_explicit_set_language_dialect_lets_explicit_win() {
        // The opposite ordering: config first (typical embedder flow — load
        // at mount time), then an explicit call after — last-write-wins,
        // same as the CLI's flag always overriding the file.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.apply_project_config("[project]\ndialect = \"brink\"\n")
            .expect("valid brink.toml");
        s.set_language_dialect("strict-ink");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "a later explicit set_language_dialect must win: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn apply_project_config_reports_unknown_keys_as_warnings() {
        let mut s = EditorSession::new();
        let warnings = s
            .apply_project_config("[project]\ndialect = \"brink\"\nfuture_key = \"x\"\n")
            .expect("unknown keys are warnings, not errors");
        let parsed: Vec<String> = serde_json::from_str(&warnings).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].contains("project.future_key"));
    }

    /// Issue #1874: an unrecognized `[project] conventions` preset name
    /// reaches the wasm-exported `apply_project_config` — proving the
    /// closed-set check added to `AnalysisOptions::apply_project_config`
    /// (`brink-analyzer`) is actually wired into `@brink-lang/web`'s editor
    /// session, not merely covered by an analyzer-crate unit test.
    ///
    /// Key renamed from `elements` by issue #2180 — see
    /// `apply_project_config_reports_the_deprecated_elements_alias_as_a_warning`
    /// below for the back-compat alias path this same wasm entry point
    /// still accepts.
    #[test]
    fn apply_project_config_reports_unrecognized_conventions_preset_as_a_warning() {
        let mut s = EditorSession::new();
        let warnings = s
            .apply_project_config("[project]\nconventions = \"screnplay\"\n")
            .expect("an unrecognized preset name is a warning, not a parse error");
        let parsed: Vec<String> = serde_json::from_str(&warnings).expect("valid json");
        assert_eq!(parsed.len(), 1, "{parsed:?}");
        assert!(parsed[0].contains("screnplay"));
    }

    /// The path-shaped sibling of the above: a project-relative `.brink`
    /// pointer must never be rejected by the preset-name closed set — that
    /// would break the custom-conventions-module case #1844's confinement
    /// rule is built around.
    #[test]
    fn apply_project_config_accepts_path_shaped_conventions_pointer_with_no_warning() {
        let mut s = EditorSession::new();
        let warnings = s
            .apply_project_config("[project]\nconventions = \"conventions.brink\"\n")
            .expect("a path-shaped conventions value is valid");
        let parsed: Vec<String> = serde_json::from_str(&warnings).expect("valid json");
        assert!(parsed.is_empty(), "{parsed:?}");
    }

    /// Issue #2180: the deprecated `[project] elements` alias must still
    /// reach the wasm-exported `apply_project_config` — an embedder running
    /// an older `brink.toml` gets a deprecation warning, not a silently
    /// unconfigured conventions module.
    #[test]
    fn apply_project_config_reports_the_deprecated_elements_alias_as_a_warning() {
        let mut s = EditorSession::new();
        let warnings = s
            .apply_project_config("[project]\nelements = \"conventions.brink\"\n")
            .expect("the deprecated `elements` key must still parse, not hard-error");
        let parsed: Vec<String> = serde_json::from_str(&warnings).expect("valid json");
        assert_eq!(parsed.len(), 1, "{parsed:?}");
        assert!(parsed[0].contains("project.elements"));
        assert!(parsed[0].contains("deprecated"));
    }

    // ── Issue #1414: `discover_project_config` (SourceTree seam, no ────────
    // external host filesystem read) ────────────────────────────────────

    #[test]
    fn discover_project_config_finds_and_applies_a_brink_toml_loaded_as_a_document() {
        // `brink.toml` is served exactly like any other file — via
        // `update_file` — never read off a real filesystem. Proves
        // `discover_project_config` resolves it purely from the session's
        // own in-memory document tree.
        let mut s = EditorSession::new();
        s.update_file("brink.toml", "[project]\ndialect = \"brink\"\n");
        s.update_file("main.ink", BRINK_EXT_SRC);
        let warnings = s
            .discover_project_config("main.ink")
            .expect("discovers and applies the in-memory brink.toml");
        assert_eq!(warnings, "[]");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "discovered brink.toml's dialect = brink: no E051 on valid extension syntax: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn discover_project_config_walks_up_from_a_nested_entry_document() {
        // A `brink.toml` at the tree root governs an entry document nested
        // several directories below it — the same ancestor walk-up
        // `brink_project_config::discover_from_entry_in_tree` gives every
        // other mount.
        let mut s = EditorSession::new();
        s.update_file("brink.toml", "[project]\ndialect = \"brink\"\n");
        s.update_file("book/chapters/main.ink", BRINK_EXT_SRC);
        let warnings = s
            .discover_project_config("book/chapters/main.ink")
            .expect("walks up from the nested entry to find brink.toml");
        assert_eq!(warnings, "[]");
        assert_eq!(s.dialect, brink_analyzer::Dialect::Brink);
    }

    #[test]
    fn discover_project_config_no_config_in_the_document_set_yields_no_warnings_and_default_dialect()
     {
        // No `brink.toml` loaded anywhere in the session: `Ok("[]")`, never
        // an error, and the `StrictInk` default stands — mirrors
        // `Project::load`/`resolve_options`'s "missing config = unchanged
        // defaults" contract.
        let mut s = EditorSession::new();
        s.update_file("main.ink", BRINK_EXT_SRC);
        let warnings = s
            .discover_project_config("main.ink")
            .expect("no brink.toml anywhere is not an error");
        assert_eq!(warnings, "[]");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "no brink.toml in the tree: StrictInk default stands: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn discover_project_config_after_explicit_set_language_dialect_is_a_no_op_for_dialect() {
        // #1005 precedence, proven against the discovery path too: an
        // explicit `set_language_dialect` call always wins over a later
        // `discover_project_config`.
        let mut s = EditorSession::new();
        s.update_file("brink.toml", "[project]\ndialect = \"brink\"\n");
        s.update_file("main.ink", BRINK_EXT_SRC);
        s.set_language_dialect("strict-ink");
        s.discover_project_config("main.ink")
            .expect("discovers and applies the in-memory brink.toml");
        let analysis = s.session.analysis().expect("analysis");
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E051),
            "explicit set_language_dialect(\"strict-ink\") must survive a later \
             discover_project_config finding dialect = brink: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn discover_project_config_reports_unknown_keys_as_warnings() {
        let mut s = EditorSession::new();
        s.update_file(
            "brink.toml",
            "[project]\ndialect = \"brink\"\nfuture_key = \"x\"\n",
        );
        s.update_file("main.ink", BRINK_EXT_SRC);
        let warnings = s
            .discover_project_config("main.ink")
            .expect("unknown keys are warnings, not errors");
        let parsed: Vec<String> = serde_json::from_str(&warnings).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].contains("project.future_key"));
    }

    /// The `apply_project_config_applies_lints_from_file` companion for the
    /// discovery path: `[lints]` reaches the editor session — and
    /// `compile_project`'s rendered severity — when `brink.toml` is
    /// discovered from the document tree, not just when its text is handed
    /// in directly.
    #[test]
    fn discover_project_config_applies_lints_from_a_discovered_file() {
        let mut s = EditorSession::new();
        s.update_file("brink.toml", "[lints]\nE014 = \"deny\"\n");
        s.update_file("main.ink", "~\nHello.\n-> DONE\n");
        let warnings = s
            .discover_project_config("main.ink")
            .expect("valid brink.toml");
        assert_eq!(warnings, "[]", "a valid overridable code earns no warning");
        let result = s.compile_project("main.ink");
        let parsed = json(&result);
        assert_eq!(
            parsed["ok"],
            serde_json::json!(false),
            "discovered brink.toml's [lints] E014 = \"deny\" promotes E014 to \
             Error, so compilation must now fail: {result}"
        );
    }

    /// Regression pin for the root-relative-key constraint documented on
    /// `discover_project_config`: a session keyed with a leading `/` on
    /// every document path (a spelling `update_file`/`update_source` never
    /// reject) walks up from a `/`-prefixed `entry`, trims the leading `/`
    /// down to an unprefixed candidate key (`"brink.toml"`), and misses the
    /// `/`-prefixed document the session actually holds (`"/brink.toml"`).
    /// The result is a silent no-op — `Ok("[]")`, indistinguishable from
    /// "no `brink.toml` anywhere" — not an error and not a warning.
    #[test]
    fn discover_project_config_with_leading_slash_document_keys_silently_finds_nothing() {
        let mut s = EditorSession::new();
        s.update_file("/brink.toml", "[project]\ndialect = \"brink\"\n");
        s.update_file("/main.ink", BRINK_EXT_SRC);
        let warnings = s
            .discover_project_config("/main.ink")
            .expect("a key-spelling mismatch is a silent miss, never an error");
        assert_eq!(
            warnings, "[]",
            "leading-slash document keys must not be discoverable by this walk-up"
        );
        assert_eq!(
            s.dialect,
            brink_analyzer::Dialect::StrictInk,
            "the discovered-but-missed brink.toml's dialect = brink must NOT apply"
        );
    }

    // ── Issue #1160/#1366: `[lints]` reaches the editor session ────────────

    /// A `[lints]` re-leveled code shows its overridden severity through the
    /// *editor session* path (`apply_project_config` → `compile_project`),
    /// not just through `AnalysisOptions::apply_project_config` in isolation
    /// (which #1160 already covered) or `Driver`/CLI compile (#1160/#1367).
    /// `E014` ("logic line has no effect", a bare `~` with nothing after it)
    /// is `Warning` by default; `[lints] E014 = "deny"` must promote it to
    /// `Error` on the diagnostic `compile_project` actually returns.
    #[test]
    fn apply_project_config_applies_lints_from_file() {
        let source = "~\nHello.\n-> DONE\n";

        let mut default_severity = EditorSession::new();
        default_severity.update_file("main.ink", source);
        let before = default_severity.compile_project("main.ink");
        let before_severity = json(&before)["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E014"))
            .map(|d| d["severity"].clone());
        assert_eq!(
            before_severity,
            Some(serde_json::json!("Warning")),
            "precondition: E014's raw default is Warning: {before}"
        );

        let mut s = EditorSession::new();
        let warnings = s
            .apply_project_config("[lints]\nE014 = \"deny\"\n")
            .expect("valid brink.toml");
        assert_eq!(warnings, "[]", "a valid overridable code earns no warning");
        s.update_file("main.ink", source);
        let result = s.compile_project("main.ink");
        let parsed = json(&result);
        // Promoting E014 to `Error` doesn't just re-render its `severity`
        // string — it makes the diagnostic count as an error for
        // `has_errors_in_closure_query`'s partitioning, so `compile_project`
        // fails to produce a story at all: `ok: false`, `story_bytes: null`.
        // `warnings` is `errors ⧺ warnings` on the failure branch, so
        // checking only that field can't distinguish "still compiled, just
        // re-leveled" from "compile now fails" — assert on `ok`/`story_bytes`
        // directly.
        assert_eq!(
            parsed["ok"],
            serde_json::json!(false),
            "brink.toml's [lints] E014 = \"deny\" promotes E014 to Error, so \
             compilation must now fail: {result}"
        );
        assert_eq!(
            parsed["story_bytes"],
            serde_json::json!(null),
            "a failed compile must not carry story bytes: {result}"
        );
        let severity = parsed["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E014"))
            .map(|d| d["severity"].clone());
        assert_eq!(
            severity,
            Some(serde_json::json!("Error")),
            "brink.toml's [lints] E014 = \"deny\": expected Error, not the raw \
             Warning default: {result}"
        );
    }

    /// `deny-warnings = true` promotes every `Warning`-severity diagnostic to
    /// `Error`, reachable the same way — through the editor session's
    /// `compile_project`, not just `AnalysisOptions` in isolation.
    #[test]
    fn apply_project_config_applies_deny_warnings_from_file() {
        let mut s = EditorSession::new();
        s.apply_project_config("[lints]\ndeny-warnings = true\n")
            .expect("valid brink.toml");
        s.update_file("main.ink", "~\nHello.\n-> DONE\n");
        let result = s.compile_project("main.ink");
        let parsed = json(&result);
        // Same consequence as the per-code override above: `deny-warnings`
        // makes every `Warning`-severity diagnostic count as an error for
        // partitioning, so this now fails to compile — `ok: false`,
        // `story_bytes: null` — not merely a re-rendered `severity` string.
        assert_eq!(
            parsed["ok"],
            serde_json::json!(false),
            "brink.toml's [lints] deny-warnings = true promotes E014 to \
             Error, so compilation must now fail: {result}"
        );
        assert_eq!(
            parsed["story_bytes"],
            serde_json::json!(null),
            "a failed compile must not carry story bytes: {result}"
        );
        let severity = parsed["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E014"))
            .map(|d| d["severity"].clone());
        assert_eq!(
            severity,
            Some(serde_json::json!("Error")),
            "brink.toml's [lints] deny-warnings = true: expected Error: {result}"
        );
    }

    /// An unrecognized `[lints]` code is reported through the same
    /// unknown-key warnings channel as `[project]`'s unrecognized keys
    /// (`AnalysisOptions::apply_project_config`'s `ConfigWarning`s), not
    /// silently dropped.
    #[test]
    fn apply_project_config_reports_unknown_lint_code_as_warning() {
        let mut s = EditorSession::new();
        let warnings = s
            .apply_project_config("[lints]\nE9999 = \"deny\"\n")
            .expect("unrecognized lint codes are warnings, not errors");
        let parsed: Vec<String> = serde_json::from_str(&warnings).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].contains("E9999"));
    }

    /// Issue #1397: a live editor session re-applying `brink.toml` after a
    /// `[lints]` entry is deleted from the file must actually un-set that
    /// override, not leave it stuck — the exact scenario this issue names
    /// (`apply_project_config` previously only ever merged into the
    /// session's resolved policy, so a removed entry never went away). The
    /// first apply sets both `deny-warnings` and a per-code override —
    /// mirroring `apply_project_config_applies_lints_from_file` and
    /// `apply_project_config_applies_deny_warnings_from_file` above — so the
    /// empty re-apply proves *both* revert end-to-end; `deny_warnings`
    /// reverts through a different line
    /// (`config.deny_warnings.unwrap_or(false)`) than per-code overrides, so
    /// exercising only one would leave the other unproven.
    #[test]
    fn apply_project_config_reapply_without_a_previously_set_code_restores_base_severity() {
        let source = "~\nHello.\n-> DONE\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", source);

        // First apply: deny-warnings is set AND E014 is promoted to `deny`,
        // so compilation fails.
        s.apply_project_config("[lints]\ndeny-warnings = true\nE014 = \"deny\"\n")
            .expect("valid brink.toml");
        let promoted = json(&s.compile_project("main.ink"));
        assert_eq!(
            promoted["ok"],
            serde_json::json!(false),
            "precondition: deny-warnings = true / E014 = deny must fail compilation: {promoted}"
        );

        // Second apply: the user deleted the `[lints]` table from
        // brink.toml (an editor session re-applies on every config
        // change) — both deny-warnings and E014 must revert to their base
        // state, so compilation now succeeds again.
        s.apply_project_config("").expect("empty document is valid");
        let reverted = json(&s.compile_project("main.ink"));
        assert_eq!(
            reverted["ok"],
            serde_json::json!(true),
            "removing [lints] E014 = \"deny\" from brink.toml must restore \
             E014's base Warning severity, letting compilation succeed \
             again: {reverted}"
        );
        let severity = reverted["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E014"))
            .map(|d| d["severity"].clone());
        assert_eq!(
            severity,
            Some(serde_json::json!("Warning")),
            "E014 must show its base Warning severity once the override is \
             gone, not the stale Error from the first apply: {reverted}"
        );
    }

    // ── Issue #1417: set_lint_overrides/set_deny_warnings_override ────────
    //
    // Extends #1160/#1397's file-only `[lints]` resolution (above) with an
    // explicit CLI/API-tier override — the wasm/editor counterpart of
    // `brink compile`'s `--deny`/`--warn`/`--allow`/`-D warnings` (#1373)
    // and `brink-lsp`'s `initializationOptions.lints`/`.denyWarnings`
    // (#1417's other two surfaces). All three now go through the same
    // `AnalysisOptions::apply_lint_overrides` seam.

    #[test]
    fn set_lint_overrides_deny_promotes_e014_to_error_and_fails_compile() {
        let source = "~\nHello.\n-> DONE\n";

        let mut s = EditorSession::new();
        s.update_file("main.ink", source);
        let before = json(&s.compile_project("main.ink"));
        assert_eq!(
            before["ok"],
            serde_json::json!(true),
            "precondition: E014's raw default is Warning, so compilation \
             succeeds with no override: {before}"
        );

        let warnings = s
            .set_lint_overrides(r#"{"E014":"deny"}"#)
            .expect("valid lint overrides");
        assert_eq!(warnings, "[]", "a valid overridable code earns no warning");

        let after = json(&s.compile_project("main.ink"));
        assert_eq!(
            after["ok"],
            serde_json::json!(false),
            "set_lint_overrides({{\"E014\":\"deny\"}}) promotes E014 to \
             Error, so compilation must now fail: {after}"
        );
        assert_eq!(after["story_bytes"], serde_json::json!(null));
        let severity = after["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E014"))
            .map(|d| d["severity"].clone());
        assert_eq!(severity, Some(serde_json::json!("Error")));
    }

    /// #1162: `set_lint_overrides({"E014":"hint"})` must down-level E014 to
    /// `Hint` — unlike `deny`, this must NOT fail the compile (`Info`/`Hint`
    /// stay non-blocking exactly like the `Warning` they're demoted from).
    #[test]
    fn set_lint_overrides_hint_relevels_e014_and_still_compiles() {
        let source = "~\nHello.\n-> DONE\n";

        let mut s = EditorSession::new();
        s.update_file("main.ink", source);

        let warnings = s
            .set_lint_overrides(r#"{"E014":"hint"}"#)
            .expect("valid lint overrides");
        assert_eq!(warnings, "[]", "a valid overridable code earns no warning");

        let after = json(&s.compile_project("main.ink"));
        assert_eq!(
            after["ok"],
            serde_json::json!(true),
            "set_lint_overrides({{\"E014\":\"hint\"}}) must stay non-blocking: {after}"
        );
        let severity = after["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E014"))
            .map(|d| d["severity"].clone());
        assert_eq!(severity, Some(serde_json::json!("Hint")));
    }

    /// #1162: an unrecognized per-code level string must still be reported
    /// through the same warning channel now that there are five valid
    /// strings instead of three.
    #[test]
    fn set_lint_overrides_unrecognized_level_is_reported_and_ignored() {
        let mut s = EditorSession::new();
        let warnings = s
            .set_lint_overrides(r#"{"E014":"sideways"}"#)
            .expect("malformed-level JSON is still well-formed JSON");
        assert!(
            warnings.contains("sideways") && warnings.contains("E014"),
            "unrecognized level must be named in the returned warning: {warnings}"
        );
    }

    #[test]
    fn set_deny_warnings_override_promotes_e014_to_error_and_fails_compile() {
        let source = "~\nHello.\n-> DONE\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", source);

        s.set_deny_warnings_override(true);
        let after = json(&s.compile_project("main.ink"));
        assert_eq!(
            after["ok"],
            serde_json::json!(false),
            "set_deny_warnings_override(true) promotes every Warning \
             (including E014) to Error, so compilation must now fail: {after}"
        );
        let severity = after["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E014"))
            .map(|d| d["severity"].clone());
        assert_eq!(severity, Some(serde_json::json!("Error")));
    }

    /// `set_lint_overrides` must win over a conflicting applied
    /// `brink.toml`'s `[lints] E014 = "allow"` for the same code (#1005
    /// `CLI/API > file > default` precedence) — proves the override is
    /// applied *after* the file's own resolution, regardless of call
    /// order.
    #[test]
    fn set_lint_overrides_wins_over_a_conflicting_applied_brink_toml_allow() {
        let source = "~\nHello.\n-> DONE\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", source);
        s.apply_project_config("[lints]\nE014 = \"allow\"\n")
            .expect("valid brink.toml");
        let allowed = json(&s.compile_project("main.ink"));
        assert_eq!(
            allowed["ok"],
            serde_json::json!(true),
            "precondition: the file allows E014, so compilation succeeds: {allowed}"
        );

        s.set_lint_overrides(r#"{"E014":"deny"}"#)
            .expect("valid lint overrides");
        let after = json(&s.compile_project("main.ink"));
        assert_eq!(
            after["ok"],
            serde_json::json!(false),
            "set_lint_overrides({{\"E014\":\"deny\"}}) must win over the \
             file's [lints] E014 = \"allow\": {after}"
        );
    }

    /// A `brink.toml` re-applied after `set_lint_overrides` must not
    /// silently drop the explicit override — the CLI/API tier is
    /// re-layered on top of every fresh file resolution, not just the one
    /// in effect when it was first set (mirrors #1397's own "a file
    /// re-apply must not lose state" concern, one tier up).
    #[test]
    fn brink_toml_reapply_after_set_lint_overrides_keeps_the_override_in_effect() {
        let source = "~\nHello.\n-> DONE\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", source);
        s.set_lint_overrides(r#"{"E014":"deny"}"#)
            .expect("valid lint overrides");

        // A brink.toml with no opinion on E014 at all — the file must not
        // clear the still-active explicit override.
        s.apply_project_config("[project]\ndialect = \"strict-ink\"\n")
            .expect("valid brink.toml");
        let after = json(&s.compile_project("main.ink"));
        assert_eq!(
            after["ok"],
            serde_json::json!(false),
            "a brink.toml re-apply with no [lints] table must not drop the \
             explicit set_lint_overrides({{\"E014\":\"deny\"}}) override: {after}"
        );
    }

    /// `clear_deny_warnings_override` must actually revert to the file's
    /// (or default) `deny-warnings`, not leave the explicit `true` stuck —
    /// the same accumulation hazard #1397 guards against for the file
    /// tier.
    #[test]
    fn clear_deny_warnings_override_reverts_to_the_unset_default() {
        let source = "~\nHello.\n-> DONE\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", source);

        s.set_deny_warnings_override(true);
        let denied = json(&s.compile_project("main.ink"));
        assert_eq!(
            denied["ok"],
            serde_json::json!(false),
            "precondition: set_deny_warnings_override(true) fails compilation: {denied}"
        );

        s.clear_deny_warnings_override();
        let reverted = json(&s.compile_project("main.ink"));
        assert_eq!(
            reverted["ok"],
            serde_json::json!(true),
            "clear_deny_warnings_override must restore E014's base Warning \
             severity, letting compilation succeed again: {reverted}"
        );
    }

    // ── Issue #1004: manifest-typed external params under strict ──────────
    //
    // The maintainer's exact repro (issue #1004 final comment): a host
    // manifest typing an `EXTERNAL`'s param must make `compile_project`'s
    // warnings channel clean under `dialect = brink, types = strict`. Before
    // the fix the compile-path strict pass never escape-checked external
    // declarations at all, so the guard below (a genuinely-unresolvable
    // manifest `ty` still reports) pins that the clean result is real
    // consumption of the manifest signature, not blanket suppression.

    /// A manifest whose `TypeRef`s type an external's params resolves them in
    /// strict inference — `compile_project().warnings` is EMPTY, and the story
    /// still compiles (`ok: true`).
    #[test]
    fn issue_1004_manifest_typed_external_param_is_clean_under_strict() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1) == 2:\n  yes\n}\n-> DONE\n",
        );
        s.set_host_manifest(
            r#"{ "types": [{ "name": "thing_id", "base": "int", "values": { "source": "host" } }],
                "externals": [{ "name": "get_thing", "params": [{ "name": "id", "ty": "thing_id" }], "returns": "float", "kind": "query" }] }"#,
        )
        .expect("valid manifest");
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(
            v["ok"], true,
            "manifest-typed external must compile: {result}"
        );
        let warnings = v["warnings"].as_array().expect("warnings array");
        assert!(
            warnings.is_empty(),
            "a manifest-typed external param must not escape under strict: {result}"
        );
    }

    /// The don't-over-suppress guard: a *registered* external whose
    /// `ManifestParam.ty` fails to resolve (empty `ty`) still escapes — the
    /// warnings channel is NON-empty, the wire object carries the `E065`
    /// `code`, and its span anchors at the external's own declaration (the
    /// `get_thing` name in `EXTERNAL get_thing(id)`, bytes 9..18), not an
    /// arbitrary fixed line.
    #[test]
    fn issue_1004_unresolvable_external_param_escapes_with_code_and_range() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1) == 2:\n  yes\n}\n-> DONE\n",
        );
        s.set_host_manifest(
            r#"{ "externals": [{ "name": "get_thing", "params": [{ "name": "id", "ty": "" }], "returns": "float", "kind": "query" }] }"#,
        )
        .expect("valid manifest");
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        let warnings = v["warnings"].as_array().expect("warnings array");
        let escape = warnings
            .iter()
            .find(|w| w["code"] == "E065")
            .expect("expected an E065 escape from the unresolvable external param");
        assert!(
            escape["message"]
                .as_str()
                .unwrap_or_default()
                .contains("get_thing"),
            "escape must name the offending external: {result}"
        );
        // Anchored at the `get_thing` name in `EXTERNAL get_thing(id)`, not
        // line 1 / a shared fixed span.
        assert_eq!(
            escape["start"], 9,
            "escape anchors at the decl span: {result}"
        );
        assert_eq!(
            escape["end"], 18,
            "escape anchors at the decl span: {result}"
        );
    }

    /// An `EXTERNAL` with no manifest entry at all stays entirely unchecked
    /// under strict (the deliberate "unregistered external = unchecked"
    /// posture): there is no in-language way to type its bare-identifier
    /// params, so this never emits an unactionable escape.
    #[test]
    fn issue_1004_unregistered_external_stays_unchecked_under_strict() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1) == 2:\n  yes\n}\n-> DONE\n",
        );
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        let result = s.compile_project("main.ink");
        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(
            v["ok"], true,
            "unregistered external must compile: {result}"
        );
        assert!(
            !v["warnings"]
                .as_array()
                .expect("warnings")
                .iter()
                .any(|w| w["code"] == "E065"),
            "an unregistered external's params must stay unchecked: {result}"
        );
    }

    // ── #1032: compile ⇄ analysis agree because they share one ProjectDb ──
    //
    // The collapse (#1032): `compile_project` assembles its artifact by
    // querying the session's OWN `ProjectDb` — the same db the background
    // analysis pass reads — instead of building a throwaway compiler driver
    // per call. With one db, one file set, and one analysis-options input
    // feeding both, a compile can never diverge from analysis on
    // manifest/dialect/policy: the class of bug that produced #1004 (manifest
    // missing from compile) and its siblings becomes structurally
    // unrepresentable. This suite pins that invariant — for every session
    // input (host manifest, T1b dialect, TM-3 policy, and the `brink.toml`
    // applied via `apply_project_config`), the option-driven diagnostic must
    // appear on BOTH the analysis channel and `compile_project`'s.

    /// Does the session's cached background analysis carry `code`?
    fn analysis_has(s: &EditorSession, code: brink_ir::DiagnosticCode) -> bool {
        s.session
            .analysis()
            .is_some_and(|a| a.diagnostics.iter().any(|d| d.code == code))
    }

    /// Does a `compile_project` result JSON carry a diagnostic with `code`?
    /// (`warnings` carries success-path warnings and, on a failed compile, the
    /// blocking errors too — see `compile_project`.)
    fn compile_has(result_json: &str, code: &str) -> bool {
        json(result_json)["warnings"]
            .as_array()
            .is_some_and(|w| w.iter().any(|d| d["code"] == code))
    }

    #[test]
    fn manifest_domain_violation_agrees_across_analysis_and_compile() {
        // A host manifest closing `tint`'s param to the `color` enum, and a
        // literal violating it (`"nope"`). Analysis flags the closed-domain
        // violation (E042); so must compile — the manifest is one input on the
        // shared db, not a thing wired separately into a second driver.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "EXTERNAL tint(c)\n~ tint(\"nope\")\n-> END\n");
        s.set_host_manifest(
            r##"{ "types": [{ "name": "color", "base": "string", "constraint": { "kind": "enum", "values": ["#FF0000"] } }],
                "externals": [{ "name": "tint", "params": [{ "name": "c", "ty": "color" }], "returns": "", "kind": "presentation" }] }"##,
        )
        .expect("valid manifest");

        assert!(
            analysis_has(&s, brink_ir::DiagnosticCode::E042),
            "analysis flags the closed-domain violation"
        );
        let result = s.compile_project("main.ink");
        assert!(
            compile_has(&result, "E042"),
            "compile must agree with analysis on the manifest-driven E042: {result}"
        );
    }

    #[test]
    fn dialect_e051_gate_agrees_across_analysis_and_compile() {
        // A `~ { … }` multi-line logic block is brink-extension syntax:
        // flagged E051 under the StrictInk default, silent under Brink. The
        // gate must move identically on both channels.
        let src = "~ {\n    temp x = 0\n}\n-> END\n";
        let mut s = EditorSession::new();
        s.update_file("main.ink", src);
        assert!(
            analysis_has(&s, brink_ir::DiagnosticCode::E051),
            "strict-ink default: analysis flags E051"
        );
        let strict = s.compile_project("main.ink");
        assert!(
            compile_has(&strict, "E051"),
            "strict-ink default: compile flags E051 too: {strict}"
        );

        s.set_language_dialect("brink");
        assert!(
            !analysis_has(&s, brink_ir::DiagnosticCode::E051),
            "brink dialect: analysis drops E051"
        );
        let brink = s.compile_project("main.ink");
        assert!(
            !compile_has(&brink, "E051"),
            "brink dialect: compile drops E051 too: {brink}"
        );
    }

    #[test]
    fn type_policy_e065_agrees_across_analysis_and_compile() {
        // `types = strict` under `dialect = brink` turns on the unused-param
        // escape check (E065) on `=== noop(x) ===`. Both channels must fire.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        assert!(
            analysis_has(&s, brink_ir::DiagnosticCode::E065),
            "strict policy: analysis flags E065"
        );
        let result = s.compile_project("main.ink");
        assert!(
            compile_has(&result, "E065"),
            "strict policy: compile flags E065 too: {result}"
        );
    }

    /// #1367: `compile_project`'s diagnostic JSON must render the *effective*
    /// severity, not the raw `DiagnosticCode::severity()` default. `E063`
    /// (annotation-vs-inference mismatch, `Warning` by default per
    /// `DiagnosticCode::severity()`) is only ever wired into production
    /// under `types = strict` (`annotations::mismatches` is called solely
    /// from `strict::check`), where the #640-round ruling promotes it to
    /// `Error`. Before this fix, the `to_js` closure called the raw default
    /// and would have shown `"Warning"` even here.
    #[test]
    fn compile_project_severity_reflects_strict_types_e063_promotion() {
        let source = "=== heal(hp: string) ===\n{hp > 1:\n  ok\n}\n-> DONE\n";

        let mut strict = EditorSession::new();
        strict.set_language_dialect("brink");
        strict.set_type_policy("strict");
        strict.update_file("main.ink", source);
        let strict_result = strict.compile_project("main.ink");
        let strict_severity = json(&strict_result)["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|d| d["code"] == "E063"))
            .map(|d| d["severity"].clone());
        assert_eq!(
            strict_severity,
            Some(serde_json::json!("Error")),
            "types = strict: E063 must promote to Error, not stay at the raw \
             Warning default: {strict_result}"
        );
    }

    #[test]
    fn apply_project_config_strict_agrees_across_analysis_and_compile() {
        // The `brink.toml` path (#1005) sets dialect + policy on the session;
        // both compile and analysis read them off the shared db, so a
        // config-driven E065 must appear on both.
        let mut s = EditorSession::new();
        s.apply_project_config("[project]\ndialect = \"brink\"\ntypes = \"strict\"\n")
            .expect("valid config");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        assert!(
            analysis_has(&s, brink_ir::DiagnosticCode::E065),
            "config strict: analysis flags E065"
        );
        let result = s.compile_project("main.ink");
        assert!(
            compile_has(&result, "E065"),
            "config strict: compile flags E065 too: {result}"
        );
    }

    #[test]
    fn compile_project_does_not_perturb_editor_diagnostic_state() {
        // Care point (#1032): assembling the compile artifact points the
        // shared db at this entry and syncs its options input — but must NOT
        // mutate the editor's cached analysis (computed off-db), nor perturb
        // the db-derived per-file diagnostics query on a *repeat* compile
        // under unchanged options (`compile`'s own doc: "an unchanged value
        // is a salsa no-op — repeated compiles under the same options reuse
        // the warm db's incremental results").
        //
        // `s.session.analysis()` alone is a tautology (PR #1048 review
        // finding): it reads the off-db cached `self.analysis` field, which
        // `compile()` provably never writes, so comparing it before/after
        // passes regardless of whether `compile()`'s db-input writes
        // (`set_entry`/`set_analysis_options`) are actually correct. Add
        // `db().diagnostics(file)` — a real db-input-derived salsa query —
        // to the comparison. The *first* compile call legitimately changes
        // it (it's what first syncs the db's `AnalysisOptions` input to the
        // editor's already-active dialect/policy, which until then had only
        // lived in the off-db cached fields `analysis()` reads), so settle
        // that with a first compile before snapshotting; the *second*
        // compile (same entry, same options) is the one that must be a
        // true no-op on both surfaces.
        let mut s = EditorSession::new();
        s.set_language_dialect("brink");
        s.set_type_policy("strict");
        s.update_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n");
        let file = s.session.file_id("main.ink").expect("main.ink loaded");

        let _ = s.compile_project("main.ink");

        let before: Vec<_> = s
            .session
            .analysis()
            .expect("analysis")
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect();
        let before_db: Vec<_> = s
            .session
            .db()
            .diagnostics(file)
            .expect("main.ink loaded")
            .iter()
            .map(|d| d.code)
            .collect();
        assert!(
            before_db.contains(&brink_ir::DiagnosticCode::E065),
            "sanity: the db-derived query must see the strict+brink policy \
             the first compile settled: {before_db:?}"
        );

        let _ = s.compile_project("main.ink");

        let after: Vec<_> = s
            .session
            .analysis()
            .expect("analysis")
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect();
        let after_db: Vec<_> = s
            .session
            .db()
            .diagnostics(file)
            .expect("main.ink loaded")
            .iter()
            .map(|d| d.code)
            .collect();
        assert_eq!(
            before, after,
            "compile_project must not perturb the editor's cached analysis"
        );
        assert_eq!(
            before_db, after_db,
            "a repeat compile_project under unchanged options must not \
             perturb the db-derived per-file diagnostics query either"
        );
    }

    #[test]
    fn compile_project_unknown_entry_reports_an_error_not_a_panic() {
        // Entry not loaded in the session → a clean `ok:false` error, mirroring
        // the prior driver's file-not-found path.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> END\n");
        let result = s.compile_project("nope.ink");
        let v = json(&result);
        assert_eq!(v["ok"], false, "{result}");
        assert!(
            v["error"].as_str().unwrap_or_default().contains("nope.ink"),
            "error names the missing entry: {result}"
        );
    }

    // ── #1032 collapse follow-up: closure-scoped compile gate ───────────
    //
    // PR #1048's adversarial review (finding 1) caught that collapsing
    // compile onto the shared analysis `ProjectDb` silently widened the
    // compile error-gate from entry-reachable to whole-project: any error
    // anywhere in the session's loaded files — a WIP scratch file, a second
    // unrelated story — now flipped `compileProject(entry)` from `ok:true`
    // to `ok:false`, diverging from both the prior throwaway-driver
    // behavior and the CLI's still-`discover`-scoped compile path. Ruled
    // (issuecomment-5009848672): `compileProject` gates on `entry`'s
    // `INCLUDE` closure only (`has_errors_in_closure_query`,
    // `brink-db`'s `queries/analysis.rs`); errors outside that closure keep
    // surfacing as diagnostics but no longer block the build. The two tests
    // below pin exactly that: an unrelated broken file must not block a
    // clean entry (closure scoping, not suppression), but a broken file the
    // entry *does* `INCLUDE` must still block it.

    #[test]
    fn compile_project_ignores_an_unrelated_broken_file_but_still_diagnoses_it() {
        // scratch.ink is loaded into the same session db as main.ink but
        // main.ink never INCLUDEs it — exactly the "WIP scratch file" /
        // "second unrelated story coexisting in one editor session" shape
        // the finding named. Its unresolved divert (E024, unconditionally
        // Error-severity, not gated by dialect/type-policy) must not block
        // `main.ink`'s build, but must still show up on a whole-project
        // diagnostics read.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "-> END\n");
        s.update_file("scratch.ink", "== broken ==\n-> nowhere\n");

        assert!(
            analysis_has(&s, brink_ir::DiagnosticCode::E024),
            "sanity: whole-project analysis flags the unrelated file's unresolved divert"
        );

        let result = s.compile_project("main.ink");
        let v = json(&result);
        assert_eq!(
            v["ok"], true,
            "an error in a file main.ink never INCLUDEs must not block its build: {result}"
        );
        assert!(
            v["story_bytes"].is_array() || v["story_bytes"].is_string(),
            "a successful compile must still hand back story bytes: {result}"
        );
        assert!(
            !compile_has(&result, "E024"),
            "the unrelated file's error must not leak into compileProject's own \
             diagnostics either: {result}"
        );

        // The broken file's diagnostic must still be live on a whole-project
        // read after the compile — closure scoping narrows the *build gate*,
        // it is not suppression of the diagnostic itself.
        let scratch = s
            .session
            .file_id("scratch.ink")
            .expect("scratch.ink loaded");
        let scratch_diags = s
            .session
            .db()
            .diagnostics(scratch)
            .expect("scratch.ink loaded");
        assert!(
            scratch_diags
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E024),
            "the unrelated broken file must still report its own diagnostics \
             after compileProject succeeds: {scratch_diags:?}"
        );
    }

    #[test]
    fn compile_project_fails_when_an_included_file_is_broken() {
        // Same broken content as above, but this time main.ink actually
        // INCLUDEs it — inside entry's closure, so the build must still
        // fail. Proves the closure scoping is a *narrowing*, not a
        // blanket "compile never fails on multi-file errors" regression.
        let mut s = EditorSession::new();
        s.update_file("main.ink", "INCLUDE broken.ink\n-> END\n");
        s.update_file("broken.ink", "== broken ==\n-> nowhere\n");

        let result = s.compile_project("main.ink");
        let v = json(&result);
        assert_eq!(
            v["ok"], false,
            "an error in an INCLUDEd file must still block the entry's build: {result}"
        );
        assert!(
            compile_has(&result, "E024"),
            "the included file's unresolved divert must surface as a compile error: {result}"
        );
    }
}

// ── Dialect (#368) wasm-only error-path tests ─────────────────────────
//
// `set_dialect`'s rejection path constructs a `JsError`, which panics on a
// non-wasm target (`wasm-bindgen`'s "cannot call wasm-bindgen imported
// functions on non-wasm targets") — the same constraint every other
// `Result<_, JsError>` method in this file has. Acceptance-path coverage
// lives in the native `mod tests` above; rejection coverage lives here.
#[cfg(all(test, target_arch = "wasm32"))]
mod dialect_wasm_tests {
    use super::EditorSession;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn set_dialect_rejects_invalid_json() {
        let mut s = EditorSession::new();
        assert!(s.set_dialect("{ not valid }").is_err());
    }

    #[wasm_bindgen_test]
    fn set_dialect_rejects_undeclared_chain_kind() {
        let mut s = EditorSession::new();
        // Corrupt the preset: reference a kind nothing declares.
        let json_str =
            serde_json::to_string(&brink_ir::DialogueDialect::default()).expect("serializes");
        let mut value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        value["chain"][0]["after"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::String("nonexistent".to_owned()));
        assert!(s.set_dialect(&value.to_string()).is_err());
    }

    /// Issue #2115: `templates` is now checked the same way `transitions`
    /// already was (`dialect::validate_succession`, shared by both) — a
    /// `templates` entry naming a kind nothing declares is rejected here,
    /// through the real wasm-facing `set_dialect` entry point, not merely
    /// at the native `brink_ir::dialect::validate` unit-test layer.
    #[wasm_bindgen_test]
    fn set_dialect_rejects_undeclared_template_kind() {
        let mut s = EditorSession::new();
        let json_str =
            serde_json::to_string(&brink_ir::DialogueDialect::default()).expect("serializes");
        let mut value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        value["templates"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "kind": "nonexistent",
                "label": "Nonexistent",
                "blank_tab": false,
            }));
        assert!(s.set_dialect(&value.to_string()).is_err());
    }

    #[wasm_bindgen_test]
    fn apply_project_config_rejects_malformed_toml() {
        let mut s = EditorSession::new();
        assert!(s.apply_project_config("this is not [ valid toml").is_err());
    }

    #[wasm_bindgen_test]
    fn apply_project_config_rejects_invalid_dialect_value() {
        let mut s = EditorSession::new();
        assert!(
            s.apply_project_config("[project]\ndialect = \"sideways\"\n")
                .is_err()
        );
    }

    /// The discovery-path (#1414) companion to
    /// `apply_project_config_rejects_invalid_dialect_value`: a `brink.toml`
    /// with an invalid `dialect` value, discovered from the session's own
    /// in-memory document tree, is still a rejected `Result`, never a panic.
    #[wasm_bindgen_test]
    fn discover_project_config_rejects_invalid_dialect_value() {
        let mut s = EditorSession::new();
        s.update_file("brink.toml", "[project]\ndialect = \"sideways\"\n");
        s.update_file("main.ink", "-> END\n");
        assert!(s.discover_project_config("main.ink").is_err());
    }

    /// The discovery-path (#1414) companion to
    /// `apply_project_config_rejects_malformed_toml`: malformed TOML
    /// discovered from the session's own in-memory document tree hits
    /// `discover_project_config`'s own `parse_str_at` call (#1384; the
    /// discovered `config_key` is threaded straight into the `ConfigError`,
    /// so its own `Display` names the file) — still a rejected `Result`,
    /// never a panic.
    #[wasm_bindgen_test]
    fn discover_project_config_rejects_malformed_toml() {
        let mut s = EditorSession::new();
        s.update_file("brink.toml", "this is not [ valid toml");
        s.update_file("main.ink", "-> END\n");
        assert!(s.discover_project_config("main.ink").is_err());
    }
}
