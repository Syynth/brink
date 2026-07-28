//! The `Project` loader and its support types: discovery (`.ink` `INCLUDE`
//! BFS or `.brink` native), symbol/cursor resolution, diagnostics, and
//! in-memory edit application — plus `Loc` and the small output-entry types
//! (`SymEntry`, `DiagEntry`, `EditEntry`) every handler formats through, and
//! the git-baseline loader `effects-diff --rev` uses.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use brink_analyzer::AnalysisResult;
use brink_driver::{Driver, GitRev, RealFs, SourceTree};
use brink_ide::LineIndex;
use brink_ide::document::DocumentSymbol;
use brink_ide::effects::EffectRowView;
use brink_ide::navigation::find_def_at_offset;
use brink_ide::rename::FileEdit;
use brink_ide::session::IdeSession;
use brink_ide::structural_result::StructuralResult;
use brink_ir::symbols::{SymbolInfo, SymbolKind};
use brink_ir::{Diagnostic, FileId};
use rowan::TextRange;

use super::commands::{Address, Format, KindFilter, MutOpts, kind_name};
use super::handlers::{Mutation, emit_mutation};

/// Resolved `--deny`/`--warn`/`--allow`/`-D warnings` CLI overrides (issue
/// #1417) — `brink ide`'s counterpart of
/// [`brink_environment::OptionOverrides`]'s `.lints`/`.deny_warnings`
/// fields (#1373), scoped to just those two: `brink ide` has no
/// `--dialect`/`--types` flags of its own (see [`Project::load`]'s doc
/// comment), so unlike the CLI's full `OptionOverrides` there is nothing
/// else to carry. Built once per invocation by
/// [`super::commands::LintOverrideArgs::resolve`] and threaded through
/// every `brink ide` entry point that builds its own `Driver`
/// (`Project::load`, `load_git_baseline`) — then stored on [`Project`]
/// itself so the safety-gate re-analysis in
/// [`Project::introduced_diagnostics`] applies the *same* resolved policy
/// the original load did, without needing its own copy of the raw CLI
/// flags (reusing the seam #1373/#1394/#1553 established, not inventing a
/// fourth path).
#[derive(Clone, Default)]
pub(super) struct LintOverrides {
    pub(super) lints: BTreeMap<String, brink_analyzer::LintLevel>,
    pub(super) deny_warnings: Option<bool>,
}

/// Render a [`brink_ir::Severity`] as the lowercase string `brink ide`'s
/// `DiagEntry` JSON/text output uses (issue #1616: extends the CLI
/// renderer's `"error"`/`"warning"` two-tier rendering to the full
/// four-tier `Severity` — `Info`/`Hint` included — that #1162/#1615 added
/// to `DiagnosticCode`/the `[lints]` control plane, so a down-leveled code
/// is no longer misreported as `"warning"`).
fn severity_str(severity: brink_ir::Severity) -> &'static str {
    match severity {
        brink_ir::Severity::Error => "error",
        brink_ir::Severity::Warning => "warning",
        brink_ir::Severity::Info => "info",
        brink_ir::Severity::Hint => "hint",
    }
}

/// Discover + apply `brink.toml` (#1005) to a fresh `AnalysisOptions`,
/// honoring the "explicit flag always wins over the file" precedence rule.
/// This is the single source every `brink ide` code path that builds its own
/// `Driver` from scratch must call — `Project::load` (the baseline), the
/// re-analysis driver in `introduced_diagnostics`, and the git-baseline
/// driver in `load_git_baseline` — so none of them can silently disagree
/// about which dialect/type-policy governs the same project. Unknown keys in
/// the file are reported as warnings on stderr, never treated as errors.
///
/// Issue #1403: discovers over `tree` (a [`SourceTree`]) via
/// [`brink_project_config::discover_from_entry_in_tree`] — the identical
/// probe [`brink_environment::Project::load`]'s producer uses to resolve a
/// mount's `brink.toml` (#1312/#1370) — instead of the path-based
/// `brink_project_config::load_from_entry`. Previously `brink ide` was the
/// one caller left resolving config straight off `std::fs`, so it could
/// silently disagree with `brink compile`/brink-web/bevy-brink (all of which
/// go through the `SourceTree` seam) and could never honor a non-`RealFs`
/// mount. `entry_key` is `tree`-relative (as returned by
/// [`brink_driver::relative_key`]), matching every other `SourceTree`
/// caller's convention.
///
/// `root` is only used to render the discovered config's path in the
/// stderr warnings below — `discover_from_entry_in_tree` returns a
/// `tree`-relative key, so without `root` a warning could only ever print
/// the bare `brink.toml`, leaving the user unable to tell *which*
/// `brink.toml` (of possibly several on disk across invocations) warned
/// (review finding on #1403/PR #1412). The same `root`-joined path is now
/// also threaded into `parse_str_at` (#1384), so a parse failure's own
/// `Display` names the full path too, rather than relying solely on the
/// `format!` wrapper this function used to hand-roll for that purpose.
///
/// `overrides` (issue #1417) is applied last, via
/// `AnalysisOptions::apply_lint_overrides` — the top of the #1005/#1373
/// `CLI/API > file > default` precedence stack, so an explicit
/// `--deny`/`--warn`/`--allow`/`-D warnings` always wins over the same code
/// in a discovered `brink.toml`'s `[lints]` table.
fn resolve_analysis_options(
    tree: &dyn SourceTree,
    root: &Path,
    entry_key: &str,
    overrides: &LintOverrides,
) -> Result<brink_analyzer::AnalysisOptions, String> {
    let mut options = brink_analyzer::AnalysisOptions::default();
    if let Some(config_key) = brink_project_config::discover_from_entry_in_tree(tree, entry_key)
        .map_err(|e| format!("{e}"))?
    {
        let text = tree
            .read(&config_key)
            .map_err(|e| format!("failed to read project config {config_key}: {e}"))?;
        let config_path = root.join(&config_key).display().to_string();
        let (config, warnings) = brink_project_config::parse_str_at(config_path.clone(), &text)
            .map_err(|e| e.to_string())?;
        for warning in &warnings {
            let _ = writeln!(io::stderr(), "warning: [{config_path}] {warning}");
        }
        let lint_warnings = options.apply_project_config(&config, false, false);
        for warning in &lint_warnings {
            let _ = writeln!(io::stderr(), "warning: [{config_path}] {warning}");
        }
    }
    let override_warnings = options.apply_lint_overrides(&overrides.lints, overrides.deny_warnings);
    for warning in &override_warnings {
        // Same "warn, never silently drop" channel as the file-sourced
        // warnings above (house rule) — no `config_path` prefix since these
        // came from the CLI, not a file.
        let _ = writeln!(io::stderr(), "warning: {warning}");
    }
    Ok(options)
}

/// Resolve a project file key back to a real filesystem path, for the
/// `--write` mutation sites (issue #1295). Native (`.brink`) discovery keys
/// files root-relative to [`brink_driver::native_source_root`] (#1288), not
/// cwd-relative — so a key must be rejoined with that root before it names
/// a real path. `.ink` discovery still keys files by the same cwd-relative
/// path a caller would pass straight to `std::fs`, so it resolves as
/// identity. Without this, `--write` on a nested native entry (a
/// `brink.toml` above `entry`'s own directory, so `native_source_root(entry)
/// != cwd`) would write the bare key literally, landing on a phantom path
/// under cwd instead of the real file.
pub(super) fn resolve_fs_path(entry: &Path, key: &str) -> PathBuf {
    if brink_driver::is_native(entry) {
        brink_driver::native_source_root(entry).join(key)
    } else {
        PathBuf::from(key)
    }
}

/// A [`SourceTree`] that overlays in-memory edits (and a moved-away key) on
/// top of the real filesystem — the seam [`Project::introduced_diagnostics`]
/// re-discovers a native project through so a pending rename/move's edited
/// content is visible to the safety-gate re-analysis without touching disk.
/// `list` unions in any edited key not already on disk (a move's new key)
/// and drops `removed`; `read` prefers an edited value, then reports
/// `removed` as not-found (mirroring the `.ink` closure's "file moved"
/// synthetic error below), then falls back to disk.
struct EditOverlay<'a> {
    inner: RealFs,
    edited: &'a BTreeMap<String, String>,
    removed: Option<&'a str>,
}

impl SourceTree for EditOverlay<'_> {
    fn list(&self) -> io::Result<Vec<String>> {
        let mut keys: BTreeSet<String> = self.inner.list()?.into_iter().collect();
        if let Some(r) = self.removed {
            keys.remove(r);
        }
        keys.extend(self.edited.keys().cloned());
        Ok(keys.into_iter().collect())
    }

    fn read(&self, key: &str) -> io::Result<String> {
        if Some(key) == self.removed {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{key}: file moved"),
            ));
        }
        if let Some(s) = self.edited.get(key) {
            return Ok(s.clone());
        }
        self.inner.read(key)
    }
}

pub(super) struct Project {
    pub(super) driver: Driver,
    pub(super) analysis: AnalysisResult,
    pub(super) entry_id: FileId,
    /// The resolved `--deny`/`--warn`/`--allow`/`-D warnings` overrides this
    /// project loaded under (issue #1417), stashed so
    /// [`Self::introduced_diagnostics`]'s safety-gate re-analysis applies
    /// the identical policy without needing its own copy of the raw CLI
    /// flags.
    lint_overrides: LintOverrides,
}

impl Project {
    /// Discover + analyze the project rooted at `entry` (follows `INCLUDE`s
    /// for `.ink`, or [`discover_native`](Driver::discover_native) over a
    /// [`RealFs`] tree for `.brink` — B0.10b, issue #1295: the same dispatch
    /// `load_git_baseline` uses, so every `brink ide` subcommand (not just
    /// `effects-diff --rev`) sees a multi-file native project's whole file
    /// set, not just the entry), exactly like `brink compile`. Also
    /// discovers a `brink.toml` (#1005) starting from `entry`'s directory
    /// and applies its `[project] dialect`/`types`/`[lints]` to analysis —
    /// `brink ide` has no `--dialect`/`--types` flags of its own, so the
    /// file (or, absent one, `AnalysisOptions::default()`, byte-identical
    /// to pre-#1005 behavior) is the only source for those two. `[lints]`
    /// does have a CLI override tier (`lints`, issue #1417's
    /// `--deny`/`--warn`/`--allow`/`-D warnings`), applied on top of the
    /// file by [`resolve_analysis_options`]. Unknown keys in the file (and
    /// unrecognized/non-overridable override codes) are reported as
    /// warnings on stderr, never treated as errors.
    pub(super) fn load(entry: &Path, lints: &LintOverrides) -> Result<Self, String> {
        let (root, warnings) = brink_driver::native_source_root_with_warnings(entry);
        for warning in &warnings {
            let _ = writeln!(io::stderr(), "warning: {warning}");
        }
        let tree = RealFs::new(&root);
        let entry_key = brink_driver::relative_key(&root, entry);

        let mut driver = Driver::new();
        driver.set_analysis_options(resolve_analysis_options(&tree, &root, &entry_key, lints)?);

        let entry_key = if brink_driver::is_native(entry) {
            // Reuse the same `SourceTree` config resolution just probed —
            // the "tree it already builds" issue #1403 asks for.
            driver.discover_native(&tree).map_err(|e| format!("{e}"))?;
            entry_key
        } else {
            let entry_s = entry.to_string_lossy().into_owned();
            driver
                .discover(&entry_s, |p| {
                    std::fs::read_to_string(p)
                        .map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
                })
                .map_err(|e| format!("{e}"))?;
            entry_s
        };

        let analysis = driver.analyze().clone();
        let entry_id = driver
            .db()
            .file_id(&entry_key)
            .ok_or_else(|| format!("entry file not found after discovery: {entry_key}"))?;
        Ok(Self {
            driver,
            analysis,
            entry_id,
            lint_overrides: lints.clone(),
        })
    }

    /// Resolve a query's target to a single symbol — by `--at FILE:LINE:COL`
    /// (cursor → the symbol there, resolving a reference to its definition) or
    /// by qualified name.
    ///
    /// B3a UFCS resolution (issue #1539): `--at` routes through the same
    /// verdict table `navigation::goto_definition`/`ufcs_hover` use before
    /// falling back to `find_def_at_offset` — without this, a cursor on a
    /// UFCS call's method segment (`recv.verb`) resolved to the *receiver*
    /// instead of the free function `.verb(...)` dispatches to, exactly the
    /// bug #1534 already fixed for hover/go-to-def. Owned, not borrowed: the
    /// UFCS path's symbol comes from `db.resolutions_index()`, a freshly
    /// computed `Arc` local to this call, not `self.analysis`.
    ///
    /// Review finding on #1539/PR #1543: this resolver is shared by `def`,
    /// `hover`, `references`, and `rename --at` (all four route through
    /// `Project::resolve`), so the UFCS override only short-circuits on a
    /// verdict that actually names a `DefinitionId`
    /// (`FreeFnDesugar`/`FreeFnAutoRef` — the case this fix targets). A
    /// field-call/prelude-intrinsic verdict (resolved, but with no
    /// `DefinitionId` to report) falls through to the generic
    /// `find_def_at_offset` lookup below exactly as if `offset` weren't on a
    /// UFCS call at all — its pre-#1539 behavior (reporting the *receiver*'s
    /// own declaration) — rather than hard-failing with `"no symbol at
    /// {at}"` for all four commands. Turning that case into a hard error
    /// would have been a new, undocumented behavior change to
    /// `hover`/`references`/`rename --at` that issue #1539 never asked for
    /// (it named only `def --at`, `find_references`, and `rename` as the
    /// three UFCS-*receiver*-target bugs to fix).
    pub(super) fn resolve(
        &self,
        addr: &Address,
        kind: Option<KindFilter>,
    ) -> Result<SymbolInfo, String> {
        if let Some(at) = &addr.at {
            let (file, line, col) = parse_at(at)?;
            let db = self.driver.db();
            let file_id = db
                .file_id(&file)
                .ok_or_else(|| format!("file not in project: {file}"))?;
            let src = db.source(file_id).unwrap_or_default();
            // `--at` is 1-based; LineIndex (like line_col) is 0-based.
            let offset = LineIndex::new(src).offset(line.saturating_sub(1), col.saturating_sub(1));

            if let Some(hir) = db.hir(file_id)
                && let Some(Some(target)) =
                    brink_ide::ufcs_hover::ufcs_goto_definition_target(db, hir, file_id, offset)
            {
                return db
                    .resolutions_index()
                    .index
                    .symbols
                    .get(&target)
                    .cloned()
                    .ok_or_else(|| format!("no symbol at {at}"));
            }

            find_def_at_offset(&self.analysis, file_id, offset)
                .cloned()
                .ok_or_else(|| format!("no symbol at {at}"))
        } else if let Some(name) = &addr.symbol {
            self.resolve_unique(name, kind).cloned()
        } else {
            Err("provide a symbol name or --at FILE:LINE:COL".to_string())
        }
    }

    /// Resolve a qualified name to exactly one symbol, honoring `--kind`.
    pub(super) fn resolve_unique(
        &self,
        name: &str,
        kind: Option<KindFilter>,
    ) -> Result<&SymbolInfo, String> {
        let ids = self.analysis.index.by_name.get(name);
        let mut hits: Vec<&SymbolInfo> = ids
            .into_iter()
            .flatten()
            .filter_map(|id| self.analysis.index.symbols.get(id))
            .filter(|s| kind.is_none_or(|k| k.matches(s.kind)))
            .collect();
        let db = self.driver.db();
        hits.sort_by_key(|s| {
            (
                db.file_path(s.file).unwrap_or_default().to_string(),
                s.range.start(),
            )
        });

        match hits.as_slice() {
            [] => Err(format!("no symbol named '{name}' in the project")),
            [one] => Ok(one),
            many => {
                let kinds: Vec<&str> = many.iter().map(|s| kind_name(s.kind)).collect();
                Err(format!(
                    "'{name}' is ambiguous (matches: {}); disambiguate with --kind",
                    kinds.join(", ")
                ))
            }
        }
    }

    /// Every knot/stitch's inferred effect row, keyed by `"<kind> <name>"`
    /// (e.g. `"knot spend"`, `"stitch hub.market"`) — the stable identity the
    /// `effects-diff` compares across two builds. `db.effects` is `None` for
    /// any non-callable def, so only real container rows appear. Deterministic
    /// (`BTreeMap`, and `EffectRowView` sorts its members by name).
    pub(super) fn collect_effect_rows(&self) -> BTreeMap<String, EffectRowView> {
        let db = self.driver.db();
        let mut rows = BTreeMap::new();
        for info in self.analysis.index.symbols.values() {
            if !matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch) {
                continue;
            }
            let Some(row) = db.effects(info.id) else {
                continue;
            };
            let key = format!("{} {}", kind_name(info.kind), info.name);
            rows.insert(key, EffectRowView::from_row(&row, &self.analysis.index));
        }
        rows
    }

    pub(super) fn location_of(&self, file: FileId, range: TextRange) -> Loc {
        let db = self.driver.db();
        let path = db.file_path(file).unwrap_or_default().to_string();
        let src = db.source(file).unwrap_or_default();
        let idx = LineIndex::new(src);
        let (line, col) = idx.line_col(range.start());
        Loc {
            path,
            line: line + 1,
            col: col + 1,
            byte_start: u32::from(range.start()),
            byte_end: u32::from(range.end()),
        }
    }

    /// Build a [`DiagEntry`], resolving `d`'s actual severity through
    /// [`brink_driver::effective_severity`] rather than trusting which of
    /// `DiagnosticReport`'s two buckets (`errors`/`warnings`) the caller
    /// pulled `d` from (issue #1616: that partition is binary —
    /// `effective_severity(...) == Error` or not — so a `[lints]` code
    /// down-leveled to `Info`/`Hint` still lands in `warnings`, and a
    /// caller-supplied `"warning"` literal would misreport it here exactly
    /// as `brink compile`'s CLI renderer did before `ResolvedDiagnostic::
    /// severity` landed in #1615).
    pub(super) fn diag_entry(&self, d: &Diagnostic) -> DiagEntry {
        let opts = self.driver.db().analysis_options();
        let severity = brink_driver::effective_severity(d.code, opts.type_policy(), &opts.lints);
        DiagEntry {
            severity: severity_str(severity).to_string(),
            code: d.code.as_str().to_string(),
            message: d.message.clone(),
            location: self.location_of(d.file, d.range),
        }
    }

    /// Apply rename edits in-memory, returning the new source per touched file.
    pub(super) fn apply_edits(
        &self,
        edits: &[FileEdit],
    ) -> Result<BTreeMap<String, String>, String> {
        let db = self.driver.db();
        let mut by_file: HashMap<FileId, Vec<&FileEdit>> = HashMap::new();
        for e in edits {
            by_file.entry(e.file).or_default().push(e);
        }
        let mut out = BTreeMap::new();
        for (file, mut es) in by_file {
            let path = db
                .file_path(file)
                .ok_or("edit targets an unknown file")?
                .to_string();
            let mut src = db.source(file).unwrap_or_default().to_string();
            // Splice from the end so earlier offsets stay valid.
            es.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
            for e in es {
                src.replace_range(
                    usize::from(e.range.start())..usize::from(e.range.end()),
                    &e.new_text,
                );
            }
            out.insert(path, src);
        }
        Ok(out)
    }

    /// Re-analyze the project with the edited sources and return the diagnostics
    /// the edit *introduced* — any error or warning present now but not in the
    /// baseline (matched by code + message). A rename that creates a collision or
    /// shadow surfaces as a warning, so warnings count.
    pub(super) fn introduced_diagnostics(
        &self,
        entry: &Path,
        edited: &BTreeMap<String, String>,
        removed: Option<&str>,
    ) -> Result<Vec<DiagEntry>, String> {
        let (root, warnings) = brink_driver::native_source_root_with_warnings(entry);
        for warning in &warnings {
            let _ = writeln!(io::stderr(), "warning: {warning}");
        }
        let entry_key = brink_driver::relative_key(&root, entry);

        let mut driver = Driver::new();
        driver.set_analysis_options(resolve_analysis_options(
            &RealFs::new(&root),
            &root,
            &entry_key,
            &self.lint_overrides,
        )?);

        let entry_key = if brink_driver::is_native(entry) {
            let tree = EditOverlay {
                inner: RealFs::new(&root),
                edited,
                removed,
            };
            driver.discover_native(&tree).map_err(|e| format!("{e}"))?;
            entry_key
        } else {
            let entry_s = entry.to_string_lossy().into_owned();
            driver
                .discover(&entry_s, |p| {
                    // A moved file no longer exists at its old path: surface any
                    // stale reference as a diagnostic instead of reading the disk copy.
                    if Some(p) == removed {
                        return Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            format!("{p}: file moved"),
                        ));
                    }
                    if let Some(s) = edited.get(p) {
                        Ok(s.clone())
                    } else {
                        std::fs::read_to_string(p)
                            .map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
                    }
                })
                .map_err(|e| format!("{e}"))?;
            entry_s
        };

        let new_analysis = driver.analyze().clone();
        let new_entry = driver
            .db()
            .file_id(&entry_key)
            .ok_or("entry file vanished during re-analysis")?;
        let new_report = driver.collect_diagnostics(&new_analysis, Some(new_entry));
        let base_report = self
            .driver
            .collect_diagnostics(&self.analysis, Some(self.entry_id));

        // Baseline diagnostic multiset (errors + warnings) keyed by (code, message).
        let mut baseline: HashMap<(String, String), i32> = HashMap::new();
        for d in base_report.errors.iter().chain(base_report.warnings.iter()) {
            *baseline
                .entry((d.code.as_str().to_string(), d.message.clone()))
                .or_default() += 1;
        }

        let new_diags = new_report.errors.iter().chain(new_report.warnings.iter());
        // Resolved the same way `diag_entry` does (issue #1616): the
        // partition above is binary, so a `[lints]` code down-leveled to
        // `Info`/`Hint` still lands in `new_report.warnings` and must not
        // be rendered as a literal `"warning"`.
        let new_opts = driver.db().analysis_options();
        let new_types = new_opts.type_policy();

        let mut introduced = Vec::new();
        for d in new_diags {
            let key = (d.code.as_str().to_string(), d.message.clone());
            let count = baseline.entry(key).or_default();
            if *count > 0 {
                *count -= 1;
            } else {
                // Location lives in the *new* driver's db.
                let path = driver
                    .db()
                    .file_path(d.file)
                    .unwrap_or_default()
                    .to_string();
                let src = driver.db().source(d.file).unwrap_or_default();
                let (line, col) = LineIndex::new(src).line_col(d.range.start());
                let severity = brink_driver::effective_severity(d.code, new_types, &new_opts.lints);
                introduced.push(DiagEntry {
                    severity: severity_str(severity).to_string(),
                    code: d.code.as_str().into(),
                    message: d.message.clone(),
                    location: Loc {
                        path,
                        line: line + 1,
                        col: col + 1,
                        byte_start: u32::from(d.range.start()),
                        byte_end: u32::from(d.range.end()),
                    },
                });
            }
        }
        Ok(introduced)
    }

    /// One preview entry per edit: where, and the old → new text.
    pub(super) fn edit_entries(&self, edits: &[FileEdit]) -> Vec<EditEntry> {
        let db = self.driver.db();
        let mut v: Vec<EditEntry> = edits
            .iter()
            .map(|e| {
                let src = db.source(e.file).unwrap_or_default();
                let old = src
                    .get(usize::from(e.range.start())..usize::from(e.range.end()))
                    .unwrap_or_default()
                    .to_string();
                EditEntry {
                    location: self.location_of(e.file, e.range),
                    old,
                    new: e.new_text.clone(),
                }
            })
            .collect();
        v.sort_by(|a, b| {
            (&a.location.path, a.location.byte_start)
                .cmp(&(&b.location.path, b.location.byte_start))
        });
        v
    }

    /// A `git apply`-able patch for the edited files (whole-file hunks).
    pub(super) fn unified_diff(&self, edited: &BTreeMap<String, String>) -> Result<String, String> {
        let db = self.driver.db();
        let mut out = String::new();
        for (path, new_src) in edited {
            let file = db.file_id(path).ok_or("diff targets an unknown file")?;
            let old = db.source(file).unwrap_or_default();
            file_diff(&mut out, path, old, new_src);
        }
        Ok(out)
    }

    /// Build an `IdeSession` seeded with every project file, for the
    /// `brink-ide` ops (`file_rename`) that take a session.
    ///
    /// Issue #1393: forwards the already-resolved project policy —
    /// `Project::load` has already merged `brink.toml`'s `[project]
    /// dialect`/`types` and `[lints]` into `driver`'s `AnalysisOptions` via
    /// `resolve_analysis_options` — onto the session via
    /// `set_language_dialect`/`set_type_policy`/`set_lint_policy`. Previously
    /// this built a bare `IdeSession::new()` and never called any of those
    /// setters (issue #1382 audit), so `structural_result::gate`/
    /// `gate_with_source` (which `rename_file` calls internally) always saw
    /// `LintPolicy::default()`/`Dialect::default()` and — because
    /// `session.analysis()` also stayed `None` with no setter ever
    /// triggering a `reanalyze()` — short-circuited to an empty breakage
    /// report regardless. Today's one caller (`run_move_file`) discards that
    /// `StructuralResult`'s `safe`/`introduced` fields entirely, re-deriving
    /// the real safety-gate diagnostics through `introduced_diagnostics`
    /// (this struct's own method, which *does* resolve `[lints]`), so this
    /// was not a live behavioral drop for `move-file` specifically — but the
    /// CLI IDE surface was still silently ignoring project config that
    /// `brink compile` (and `brink ide check`, which reads `project.driver`
    /// directly rather than going through this session) already honored, and
    /// any future caller reading `rename_file`'s own gate output, or a new
    /// `ide_session()` consumer, would have inherited the drop. Each setter
    /// re-analyzes, so they're called after every source is loaded — calling
    /// them first would reanalyze against an empty file set, then leave that
    /// stale (empty) result in place once sources are added via
    /// `update_source` (which does not itself trigger re-analysis).
    pub(super) fn ide_session(&self) -> IdeSession {
        let db = self.driver.db();
        let mut session = IdeSession::new();
        let ids: Vec<FileId> = db.file_ids().collect();
        for id in ids {
            if let (Some(path), Some(src)) = (db.file_path(id), db.source(id)) {
                session.update_source(path, src.to_string());
            }
        }
        let options = db.analysis_options();
        session.set_language_dialect(options.dialect);
        if let Some(types) = options.types {
            session.set_type_policy(types);
        }
        session.set_lint_policy(options.lints.clone());
        session
    }

    /// The `(id, source)` for `file` (project-relative) or, if `None`, the entry.
    pub(super) fn file_or_entry(&self, file: Option<&str>) -> Result<(FileId, String), String> {
        let db = self.driver.db();
        let id = match file {
            Some(f) => db
                .file_id(f)
                .ok_or_else(|| format!("file not in project: {f}"))?,
            None => self.entry_id,
        };
        let src = db.source(id).unwrap_or_default().to_string();
        Ok((id, src))
    }

    /// Resolve a knot name to the file that declares it (and that file's source).
    pub(super) fn knot_file(&self, knot: &str) -> Result<(FileId, String), String> {
        let sym = self.resolve_unique(knot, Some(KindFilter::Knot))?;
        let file = sym.file;
        let src = self
            .driver
            .db()
            .source(file)
            .unwrap_or_default()
            .to_string();
        Ok((file, src))
    }

    /// Emit a single-file refactor result (`old_source` → `new_source`) through
    /// the requested mode. Reports "no change" if the refactor is a no-op.
    pub(super) fn emit_single(
        &self,
        id: FileId,
        old_source: &str,
        new_source: String,
        mode: &MutOpts,
    ) -> Result<ExitCode, String> {
        if new_source == old_source {
            let mut out = io::stdout().lock();
            match mode.format {
                Format::Text => writeln!(out, "no change").map_err(|e| e.to_string())?,
                Format::Json => writeln!(
                    out,
                    "{}",
                    to_json(&serde_json::json!({
                        "changed": false,
                        "diff": "",
                        "files": Vec::<String>::new(),
                        "introducedDiagnostics": Vec::<DiagEntry>::new(),
                        "safe": true,
                    }))?
                )
                .map_err(|e| e.to_string())?,
            }
            return Ok(ExitCode::SUCCESS);
        }
        let path = self
            .driver
            .db()
            .file_path(id)
            .ok_or("refactor targets an unknown file")?
            .to_string();
        let mut edited = BTreeMap::new();
        edited.insert(path, new_source);
        let mutation = Mutation {
            edited,
            edits: None,
        };
        let m = mode.flags.mode();
        emit_mutation(
            self,
            &mode.entry,
            &mutation,
            &m,
            mode.format,
            mode.flags.unsafe_mode,
        )
    }

    /// Emit a cross-file [`StructuralResult`] (primary `new_source` + reference
    /// edits in other files) through the requested mode. The primary file is
    /// covered by `new_source`, so any cross-file edit landing on it is overridden.
    pub(super) fn emit_move_result(
        &self,
        primary: FileId,
        result: StructuralResult,
        mode: &MutOpts,
    ) -> Result<ExitCode, String> {
        let primary_path = self
            .driver
            .db()
            .file_path(primary)
            .ok_or("move targets an unknown file")?
            .to_string();
        let new_source = result
            .new_source
            .ok_or("structural move produced no primary source")?;
        let mut edited = self.apply_edits(&result.cross_file_edits)?;
        edited.insert(primary_path, new_source);
        let mutation = Mutation {
            edited,
            edits: None,
        };
        let m = mode.flags.mode();
        emit_mutation(
            self,
            &mode.entry,
            &mutation,
            &m,
            mode.format,
            mode.flags.unsafe_mode,
        )
    }
}

/// Build a baseline [`Project`] from the *same* entry path, but reading every
/// file's content from git revision `rev` instead of the working tree — the
/// working-tree-vs-HEAD story. A file absent from `rev` reads as not-found,
/// so its defs surface as `added` in the diff.
///
/// Dispatches on `entry`'s extension (B0.10b, issue #1288, closing #1224): a
/// `.brink` entry discovers via [`brink_driver::Driver::discover_native`]
/// with a [`GitRev`] tree — native has no `INCLUDE` graph, so the old
/// `read_file(path) -> String` closure (which can only answer for a path it's
/// given, never enumerate "what exists under this root") could never have
/// found any file but the entry itself. `GitRev` enumerates the whole
/// project at `rev` via `git ls-tree`, so every `.brink` file the revision
/// contains is discovered and read from git, not the working tree. A `.ink`
/// entry is unchanged: `git_show`-driven `INCLUDE` BFS.
pub(super) fn load_git_baseline(
    entry: &Path,
    rev: &str,
    lints: &LintOverrides,
) -> Result<Project, String> {
    let entry_s = entry.to_string_lossy().into_owned();
    let (root, warnings) = brink_driver::native_source_root_with_warnings(entry);
    for warning in &warnings {
        let _ = writeln!(io::stderr(), "warning: {warning}");
    }
    let entry_key = brink_driver::relative_key(&root, entry);

    let mut driver = Driver::new();
    // Config resolution still reads `brink.toml` off the working tree, not
    // `rev` — the baseline and head sides must agree on the *same* resolved
    // policy (see `load_git_baseline_matches_project_load_analysis_options`
    // below); only the source content itself is read from `rev`. The CLI
    // override tier (issue #1417) is `lints` here — the caller's own
    // resolved flags, same as the head side's `Project::load`, so a
    // `--deny`/`--warn`/`--allow` on the `effects-diff --rev` invocation
    // governs both sides identically.
    driver.set_analysis_options(resolve_analysis_options(
        &RealFs::new(&root),
        &root,
        &entry_key,
        lints,
    )?);

    let entry_key = if brink_driver::is_native(entry) {
        let repo_dir = Path::new(".");
        ensure_repo_dir_is_toplevel(repo_dir, &root)?;
        let tree = GitRev::new(repo_dir, rev, &root);
        driver
            .discover_native(&tree)
            .map_err(|e| format!("baseline {rev}: {e}"))?;
        entry_key
    } else {
        driver
            .discover(&entry_s, |p| git_show(rev, p))
            .map_err(|e| format!("baseline {rev}: {e}"))?;
        entry_s.clone()
    };

    let analysis = driver.analyze().clone();
    let entry_id = driver
        .db()
        .file_id(&entry_key)
        .ok_or_else(|| format!("entry file not found in {rev}: {entry_key}"))?;
    Ok(Project {
        driver,
        analysis,
        entry_id,
        lint_overrides: lints.clone(),
    })
}

/// Guard the native branch of [`load_git_baseline`] against its `repo_dir =
/// Path::new(".")` assumption: [`GitRev`]'s `read` joins `root` directly onto
/// a key with no `./` prefix — so the resulting `git show <rev>:<path>`
/// pathspec resolves against the repository's *top-level* directory, not cwd
/// (unlike the `./`-prefixed pathspec [`git_show`] below uses for the `.ink`
/// branch). Two ways that assumption can fail, both checked here so the
/// caller gets a clear error instead of a wrong-or-missing read or a garbled
/// `git` error:
///
/// - cwd is not the repo root — `effects-diff --rev` invoked from a
///   subdirectory of a multi-file native project (issue #1295 fold-in: "add
///   a guard/assertion here").
/// - `root` (as resolved by [`brink_driver::native_source_root`]) is not
///   inside the repo at all. Since issue #1413's absolutized retry,
///   `native_source_root` can return an absolute path when it walks up past
///   cwd to find a `brink.toml`. Before #1425 that walk was unbounded, so a
///   config living in an *ancestor of the repo root* (with none found inside
///   the repo first) could actually be discovered, and `GitRev`'s pathspec
///   would then be an out-of-repo absolute path, causing `git ls-tree`/`git
///   show` to fail with an opaque `fatal: ... is outside repository` instead
///   of this guard's fail-fast message. (Note this check is independent of
///   the cwd check above: passing it does not by itself guarantee `root`
///   stays inside the repo, which is why both are verified.)
///
///   **Since #1425**, `brink_project_config::find_config`'s walk itself stops
///   at a workspace/git boundary (a directory containing `.git`), so an
///   *ancestor* `brink.toml` (one that would have been discovered by
///   climbing past the repo root) is now invisible to the walk rather than
///   discovered-then-rejected here. But `native_source_root` falls back to
///   `entry_dir` whenever no config is found at all, and `entry_dir` itself
///   can still be outside the repo the cwd belongs to — e.g. `effects-diff
///   --rev` invoked with an entry in a sibling tree
///   (`-e ../sibling/story.brink` from inside this repo). That still reaches
///   this branch end-to-end, which is why it stays covered both directly
///   (`ensure_repo_dir_is_toplevel_rejects_a_root_outside_the_repo`, in
///   isolation) and via `load_git_baseline`
///   (`git_baseline_for_an_out_of_repo_entry_errors_via_this_guard`) in the
///   tests below.
fn ensure_repo_dir_is_toplevel(repo_dir: &Path, root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .current_dir(repo_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("git rev-parse --show-toplevel: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse --show-toplevel failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let toplevel = String::from_utf8(output.stdout)
        .map_err(|e| format!("git rev-parse --show-toplevel: non-utf8 output: {e}"))?;
    let toplevel = toplevel.trim();
    let cwd = std::env::current_dir().map_err(|e| format!("current dir: {e}"))?;
    let toplevel_abs =
        std::path::absolute(Path::new(toplevel)).unwrap_or_else(|_| PathBuf::from(toplevel));
    let cwd_abs = std::path::absolute(&cwd).unwrap_or(cwd);
    if toplevel_abs != cwd_abs {
        return Err(format!(
            "effects-diff --rev must be run from the git repository root ({}), not {} — \
             native baseline discovery keys files relative to cwd and would misalign \
             otherwise (issue #1295)",
            toplevel_abs.display(),
            cwd_abs.display()
        ));
    }

    let root_abs = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    if root_abs.starts_with(&toplevel_abs) {
        Ok(())
    } else {
        Err(format!(
            "effects-diff --rev found the native project root at {} via brink.toml, which is \
             outside the git repository ({}) — native baseline discovery can only read files \
             from inside the repo at a git revision (issue #1413)",
            root_abs.display(),
            toplevel_abs.display()
        ))
    }
}

/// Read `path` (project-relative, as discovered) at git revision `rev`. The
/// `./` prefix makes git resolve the pathspec relative to the current working
/// directory, matching how the entry/`INCLUDE` paths were given on the command
/// line. A non-zero git exit (file absent in `rev`, not a repo, …) maps to a
/// `NotFound` error the discovery walk reports.
fn git_show(rev: &str, path: &str) -> Result<String, io::Error> {
    let spec = format!("{rev}:./{path}");
    let output = Command::new("git").args(["show", &spec]).output()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{path}: {e}")))
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{path} not in {rev}"),
        ))
    }
}

/// Append a whole-file unified-diff hunk for `path` (old → new) to `out`.
pub(super) fn file_diff(out: &mut String, path: &str, old: &str, new: &str) {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let _ = write!(
        out,
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for l in &old_lines {
        push_diff_line(out, '-', l);
    }
    for l in &new_lines {
        push_diff_line(out, '+', l);
    }
}

pub(super) fn push_diff_line(out: &mut String, sign: char, line: &str) {
    out.push(sign);
    out.push_str(line.strip_suffix('\n').unwrap_or(line));
    out.push('\n');
    if !line.ends_with('\n') {
        out.push_str("\\ No newline at end of file\n");
    }
}

/// Recursively convert a `brink-ide` outline node into an output entry.
pub(super) fn doc_to_entry(project: &Project, file: FileId, d: &DocumentSymbol) -> SymEntry {
    SymEntry {
        name: d.name.clone(),
        kind: kind_name(d.kind).into(),
        detail: d.detail.clone(),
        location: project.location_of(file, d.range),
        children: d
            .children
            .iter()
            .map(|c| doc_to_entry(project, file, c))
            .collect(),
    }
}

pub(super) fn print_tree(
    out: &mut impl Write,
    entries: &[SymEntry],
    depth: usize,
) -> Result<(), String> {
    for e in entries {
        let indent = "  ".repeat(depth);
        let detail = e
            .detail
            .as_deref()
            .map(|d| format!(" [{d}]"))
            .unwrap_or_default();
        writeln!(
            out,
            "{indent}{} {}{}  {}",
            e.kind,
            e.name,
            detail,
            e.location.display()
        )
        .map_err(|x| x.to_string())?;
        print_tree(out, &e.children, depth + 1)?;
    }
    Ok(())
}

pub(super) fn to_json<T: serde::Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string(v).map_err(|e| e.to_string())
}

/// Parse `FILE:LINE:COL` (line/col 1-based). The file may itself contain `:`,
/// so split the two numeric fields off the right.
pub(super) fn parse_at(s: &str) -> Result<(String, u32, u32), String> {
    let mut parts = s.rsplitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(col), Some(line), Some(file)) if !file.is_empty() => {
            let line = line
                .parse::<u32>()
                .map_err(|_| format!("bad line in --at '{s}'"))?;
            let col = col
                .parse::<u32>()
                .map_err(|_| format!("bad column in --at '{s}'"))?;
            Ok((file.to_string(), line, col))
        }
        _ => Err(format!("--at must be FILE:LINE:COL, got '{s}'")),
    }
}

// ── Output location ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub(super) struct Loc {
    pub(super) path: String,
    pub(super) line: u32,
    pub(super) col: u32,
    pub(super) byte_start: u32,
    pub(super) byte_end: u32,
}

impl Loc {
    pub(super) fn display(&self) -> String {
        format!("{}:{}:{}", self.path, self.line, self.col)
    }
}

/// An outline / search / unused entry (a symbol with an optional child list).
#[derive(serde::Serialize)]
pub(super) struct SymEntry {
    pub(super) name: String,
    pub(super) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) detail: Option<String>,
    pub(super) location: Loc,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) children: Vec<SymEntry>,
}

/// A diagnostic entry for `brink ide check`.
#[derive(serde::Serialize)]
pub(super) struct DiagEntry {
    pub(super) severity: String,
    pub(super) code: String,
    pub(super) message: String,
    pub(super) location: Loc,
}

/// One edit in a rename preview: where, and the old → new text.
#[derive(serde::Serialize)]
pub(super) struct EditEntry {
    pub(super) location: Loc,
    pub(super) old: String,
    pub(super) new: String,
}

#[cfg(test)]
mod git_baseline_config_tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests in this module that change the process cwd —
    /// `git_show` spawns `git show <rev>:./<path>` with no explicit
    /// `Command::current_dir`, so it inherits whatever the process's cwd is
    /// at call time. There is only one such test today; the lock is cheap
    /// insurance against a future one racing it.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    struct CwdGuard(std::path::PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Regression for the review finding on `load_git_baseline` (the
    /// baseline driver behind `brink ide effects-diff --rev`): it used to
    /// build its `Driver` with a bare `AnalysisOptions::default()`, ignoring
    /// any discovered `brink.toml`, while `Project::load` (the head side of
    /// the very same diff) discovers + applies it. This is not observable
    /// through `effects-diff`'s CLI output today — dialect/types only gate
    /// diagnostic severity, never effect-row content, since the dialect
    /// grammar is a superset that always parses (see
    /// `brink_analyzer::dialect_gate`) — so it must be caught by comparing
    /// the resolved `AnalysisOptions` directly instead.
    #[test]
    fn load_git_baseline_matches_project_load_analysis_options() {
        let _lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_cwd = std::env::current_dir().unwrap();

        let dir = std::env::temp_dir().join(format!(
            "brink-ide-unit-git-baseline-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("brink.toml"), "[project]\ndialect = \"brink\"\n").unwrap();
        std::fs::write(dir.join("story.ink"), "Hello.\n-> END\n").unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "test"]);
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "initial"]);

        // Match the process cwd to `dir` for the duration of the two loads
        // below, so `git_show`'s `./`-relative pathspec resolves — restored
        // by `CwdGuard` on drop (including on assertion panic).
        std::env::set_current_dir(&dir).unwrap();
        let cwd_guard = CwdGuard(original_cwd);

        let entry = Path::new("story.ink");
        let head = Project::load(entry, &LintOverrides::default()).expect("head loads");
        let baseline = load_git_baseline(entry, "HEAD", &LintOverrides::default())
            .expect("git baseline loads");

        assert_eq!(
            head.driver.db().analysis_options().dialect,
            baseline.driver.db().analysis_options().dialect,
            "load_git_baseline must apply the same brink.toml dialect as Project::load"
        );
        assert_eq!(
            head.driver.db().analysis_options().dialect,
            brink_analyzer::Dialect::Brink,
            "sanity: brink.toml's dialect = \"brink\" must actually be in effect"
        );

        // Restore cwd (still holding `_lock`) before removing `dir`.
        drop(cwd_guard);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression for #1224 ("git-revision baselines silently read the
    /// working tree"): a `.brink` git-baseline must discover *every*
    /// `.brink` file that exists at the revision — not just the entry,
    /// which is all the old closure-only `discover` could ever reach for
    /// native (no `INCLUDE` graph to BFS through) — and must read each
    /// file's *committed* content, never the uncommitted working-tree copy.
    #[test]
    fn git_baseline_for_brink_entry_discovers_all_files_from_the_revision_not_the_working_tree() {
        let _lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_cwd = std::env::current_dir().unwrap();

        let dir = std::env::temp_dir().join(format!(
            "brink-ide-unit-git-baseline-native-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.brink"), "flow main() {\n  Hi. -> END\n}\n").unwrap();
        std::fs::write(
            dir.join("other.brink"),
            "flow other() {\n  Committed. -> END\n}\n",
        )
        .unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "test"]);
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "initial"]);

        // Uncommitted working-tree edit: a real diff-of-nothing bug would
        // read this content for the baseline too.
        std::fs::write(
            dir.join("other.brink"),
            "flow other() {\n  Working tree only. -> END\n}\n",
        )
        .unwrap();

        std::env::set_current_dir(&dir).unwrap();
        let cwd_guard = CwdGuard(original_cwd);

        let entry = Path::new("main.brink");
        let baseline = load_git_baseline(entry, "HEAD", &LintOverrides::default())
            .expect("git baseline loads");

        let db = baseline.driver.db();
        let mut paths: Vec<_> = db.file_ids().filter_map(|id| db.file_path(id)).collect();
        paths.sort_unstable();
        assert_eq!(
            paths,
            vec!["main.brink", "other.brink"],
            "git baseline must discover every .brink file at the revision, not just the entry"
        );

        let other_id = db.file_id("other.brink").expect("other.brink discovered");
        let other_source = db.source(other_id).unwrap_or_default();
        assert!(
            other_source.contains("Committed."),
            "must read other.brink's content from the git revision, got: {other_source:?}"
        );
        assert!(
            !other_source.contains("Working tree only."),
            "must NOT read the uncommitted working-tree copy, got: {other_source:?}"
        );

        drop(cwd_guard);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression for the #1295 fold-in: `load_git_baseline`'s native branch
    /// assumes cwd *is* the git repository root (`repo_dir = Path::new(".")`
    /// in the source). Running `effects-diff --rev` from a subdirectory of a
    /// multi-file native project must fail fast with a clear error instead
    /// of silently misaligning `GitRev`'s pathspecs and reading the wrong
    /// (or no) content.
    #[test]
    fn git_baseline_for_brink_entry_from_a_subdirectory_of_the_repo_errors_instead_of_misaligning()
    {
        let _lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_cwd = std::env::current_dir().unwrap();

        let dir = std::env::temp_dir().join(format!(
            "brink-ide-unit-git-baseline-subdir-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("sub").join("main.brink"),
            "flow main() {\n  Hi. -> END\n}\n",
        )
        .unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "test"]);
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "initial"]);

        // cwd is `dir/sub` — inside the repo, but not its top-level
        // directory — exactly the misalignment risk the fold-in flags.
        std::env::set_current_dir(dir.join("sub")).unwrap();
        let cwd_guard = CwdGuard(original_cwd);

        let entry = Path::new("main.brink");
        let err = load_git_baseline(entry, "HEAD", &LintOverrides::default())
            .err()
            .expect("baseline load from a repo subdirectory must fail fast, not misalign");
        assert!(
            err.contains("git repository root"),
            "error must name the actual problem, got: {err}"
        );

        drop(cwd_guard);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression for #1425 (superseding the PR #1420 review-finding
    /// regression this test used to pin): before the walk was bounded at a
    /// workspace/git boundary, a `brink.toml` living in an *ancestor of the
    /// repo root* (with none found inside the repo) could be discovered by
    /// #1413's absolutized retry, and `load_git_baseline` would fail fast
    /// via `ensure_repo_dir_is_toplevel`'s "outside the git repository"
    /// branch (what this test used to assert). Now that
    /// `brink_project_config::find_config` itself stops climbing the moment
    /// it passes the `.git`-marked repository root, that ancestor
    /// `brink.toml` is never discovered in the first place —
    /// `native_source_root` falls back to `entry_dir` exactly as if no
    /// config existed at all, and the baseline loads successfully with
    /// default `AnalysisOptions`, never reaching
    /// `ensure_repo_dir_is_toplevel`'s outside-repo branch *for this
    /// specific trigger* (an ancestor `brink.toml`). That guard branch is
    /// not dead overall, though — see
    /// `git_baseline_for_an_out_of_repo_entry_errors_via_this_guard` below
    /// for a different trigger (an out-of-repo *entry*) that still reaches
    /// it end-to-end, and
    /// `ensure_repo_dir_is_toplevel_rejects_a_root_outside_the_repo` for
    /// direct, isolated coverage of the guard function itself.
    #[test]
    fn git_baseline_ignores_a_brink_toml_outside_the_repo_instead_of_erroring() {
        let _lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_cwd = std::env::current_dir().unwrap();

        let ancestor = std::env::temp_dir().join(format!(
            "brink-ide-unit-git-baseline-outside-repo-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&ancestor);
        let repo = ancestor.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        // `brink.toml` lives one directory *above* the repo root — never
        // inside the repo itself, and (post-#1425) outside the walk's
        // workspace/git boundary too.
        std::fs::write(
            ancestor.join("brink.toml"),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();
        std::fs::write(repo.join("story.brink"), "flow main() {\n  Hi. -> END\n}\n").unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "test"]);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "initial"]);

        // cwd is the repo's own toplevel — satisfies the pre-existing
        // cwd-equals-toplevel guard on its own.
        std::env::set_current_dir(&repo).unwrap();
        let cwd_guard = CwdGuard(original_cwd);

        let entry = Path::new("story.brink");
        let baseline = load_git_baseline(entry, "HEAD", &LintOverrides::default()).expect(
            "an out-of-repo brink.toml must be invisible to the bounded walk, not surfaced as \
             an error",
        );
        assert_eq!(
            baseline.driver.db().analysis_options().dialect,
            brink_analyzer::Dialect::StrictInk,
            "the out-of-repo brink.toml (dialect = \"brink\") must never be discovered, so \
             default AnalysisOptions apply"
        );

        drop(cwd_guard);
        std::fs::remove_dir_all(&ancestor).ok();
    }

    /// Direct, isolated coverage for `ensure_repo_dir_is_toplevel`'s
    /// outside-repo branch: exercises the guard function directly with a
    /// fabricated out-of-repo `root`, independent of how a caller might
    /// arrive at one. The branch is *not* unreachable via
    /// `load_git_baseline`'s real call path — #1425 only closed the
    /// ancestor-`brink.toml` trigger (see
    /// `git_baseline_ignores_a_brink_toml_outside_the_repo_instead_of_erroring`
    /// above); an out-of-repo *entry* still reaches this branch end-to-end
    /// (`native_source_root` falls back to `entry_dir` when no config is
    /// found at all, and `entry_dir` can itself be outside the repo), which
    /// `git_baseline_for_an_out_of_repo_entry_errors_via_this_guard` below
    /// proves directly through `load_git_baseline`.
    #[test]
    fn ensure_repo_dir_is_toplevel_rejects_a_root_outside_the_repo() {
        let _lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_cwd = std::env::current_dir().unwrap();

        let ancestor = std::env::temp_dir().join(format!(
            "brink-ide-unit-ensure-toplevel-outside-repo-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&ancestor);
        let repo = ancestor.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".keep"), "").unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "test"]);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "initial"]);

        std::env::set_current_dir(&repo).unwrap();
        let cwd_guard = CwdGuard(original_cwd);

        let err = ensure_repo_dir_is_toplevel(Path::new("."), &ancestor)
            .expect_err("a root outside the repository must be rejected");
        assert!(
            err.contains("outside the git repository"),
            "error must name the actual problem, got: {err}"
        );

        drop(cwd_guard);
        std::fs::remove_dir_all(&ancestor).ok();
    }

    /// End-to-end regression (w52 review of #1432): `ensure_repo_dir_is_toplevel`'s
    /// outside-repo branch is *not* dead via `load_git_baseline`'s real call
    /// path, despite #1425 bounding `find_config`'s walk. `native_source_root`
    /// falls back to `entry_dir` whenever no `brink.toml` is found at all — and
    /// `entry_dir` can itself sit outside the repo the cwd belongs to, e.g. an
    /// entry in a sibling tree passed to `effects-diff --rev` from inside this
    /// repo. cwd is the repo's own toplevel (satisfying the first guard check
    /// on its own), so this drives the *second* check — `root` resolving
    /// outside the repo — through `load_git_baseline` itself, not by calling
    /// `ensure_repo_dir_is_toplevel` directly.
    #[test]
    fn git_baseline_for_an_out_of_repo_entry_errors_via_this_guard() {
        let _lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_cwd = std::env::current_dir().unwrap();

        let ancestor = std::env::temp_dir().join(format!(
            "brink-ide-unit-git-baseline-out-of-repo-entry-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&ancestor);
        let repo = ancestor.join("repo");
        let sibling = ancestor.join("sibling");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        // The entry lives entirely outside `repo`, in a sibling tree with no
        // `brink.toml` anywhere above it either — so `native_source_root`
        // finds no config and falls back to `entry_dir` (the sibling dir
        // itself), which is outside the repo.
        std::fs::write(
            sibling.join("story.brink"),
            "flow main() {\n  Hi. -> END\n}\n",
        )
        .unwrap();
        std::fs::write(repo.join(".keep"), "").unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "test"]);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "initial"]);

        // cwd is the repo's own toplevel, exactly like `effects-diff --rev`
        // requires — the entry, not cwd, is what puts `root` outside the repo.
        std::env::set_current_dir(&repo).unwrap();
        let cwd_guard = CwdGuard(original_cwd);

        // An absolute entry path, not a `../`-relative one: `root` ends up
        // exactly `sibling` (an unresolved-`.."` relative root would
        // lexically "start with" `repo` — a separate, narrower quirk of
        // this guard's `Path::starts_with`, not what this test is proving).
        let entry = sibling.join("story.brink");
        let err = load_git_baseline(&entry, "HEAD", &LintOverrides::default())
            .err()
            .expect("an out-of-repo entry must fail fast via ensure_repo_dir_is_toplevel");
        assert!(
            err.contains("outside the git repository"),
            "error must name the actual problem, got: {err}"
        );

        drop(cwd_guard);
        std::fs::remove_dir_all(&ancestor).ok();
    }

    /// Coverage for the review-named gap (issue #1425): `GitRev` with `root
    /// != cwd` — a native project rooted in a *subdirectory* of the git
    /// repository, not at its top level. Every prior `load_git_baseline`
    /// test either resolved `root == "."` (`GitRev::repo_relative`'s
    /// dot-shortcut, the #1403/PR #1412 trap this issue explicitly flags) or
    /// hit an error path before `GitRev` was ever constructed. This drives
    /// the *non*-dot `repo_relative` branch
    /// (`format!("{}/{key}", to_key(&self.root))`) for both `GitRev::list`
    /// and `GitRev::read`, end to end.
    #[test]
    fn git_baseline_discovers_a_native_project_rooted_in_a_repo_subdirectory() {
        let _lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_cwd = std::env::current_dir().unwrap();

        let dir = std::env::temp_dir().join(format!(
            "brink-ide-unit-git-baseline-root-ne-cwd-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("sub").join("brink.toml"),
            "[project]\ndialect = \"brink\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("sub").join("main.brink"),
            "flow main() {\n  Hi. -> END\n}\n",
        )
        .unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "test"]);
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "initial"]);

        // cwd is the repo's own toplevel (satisfies `ensure_repo_dir_is_toplevel`'s
        // cwd check), but the native project root — discovered via
        // `sub/brink.toml` — is `sub`, not `.`: `root != cwd` (and `root != "."`).
        std::env::set_current_dir(&dir).unwrap();
        let cwd_guard = CwdGuard(original_cwd);

        let entry = Path::new("sub/main.brink");
        let baseline = load_git_baseline(entry, "HEAD", &LintOverrides::default())
            .expect("git baseline must succeed for a project rooted in a repo subdirectory");

        assert_eq!(
            baseline.driver.db().analysis_options().dialect,
            brink_analyzer::Dialect::Brink,
            "sub/brink.toml's dialect must be discovered and applied"
        );
        let db = baseline.driver.db();
        let entry_id = db
            .file_id("main.brink")
            .expect("main.brink discovered, keyed root-relative to sub/");
        let source = db.source(entry_id).unwrap_or_default();
        assert!(
            source.contains("Hi."),
            "must read main.brink's committed content via GitRev's non-dot repo_relative \
             branch, got: {source:?}"
        );

        drop(cwd_guard);
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// Regression for issue #1393: `Project::ide_session()` used to build a bare
/// `IdeSession::new()` with no `set_lint_policy`/`set_language_dialect`/
/// `set_type_policy` call, so a project's `brink.toml` never reached any
/// `brink ide` subcommand that goes through a session — the CLI IDE surface
/// silently ignored config that `brink compile` (and `brink ide check`,
/// which reads `Project::driver`'s own resolved `AnalysisOptions` directly)
/// already honored.
#[cfg(test)]
mod ide_session_project_config_tests {
    use super::*;

    #[test]
    fn ide_session_applies_the_resolved_lints_dialect_and_types() {
        let dir = std::env::temp_dir().join(format!(
            "brink-ide-unit-ide-session-config-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("brink.toml"),
            "[project]\ndialect = \"brink\"\ntypes = \"gradual\"\n\n[lints]\nE014 = \"deny\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("story.ink"), "Hello.\n-> END\n").unwrap();

        let entry = dir.join("story.ink");
        let project = Project::load(&entry, &LintOverrides::default()).expect("project loads");
        let session = project.ide_session();

        assert_eq!(
            session.language_dialect(),
            brink_analyzer::Dialect::Brink,
            "ide_session() must forward the resolved [project] dialect"
        );
        // `types = "gradual"` is the non-default posture for the `Brink`
        // dialect (which otherwise resolves `None` to `Strict` — see
        // `resolve_type_policy`), so this only stays green if `ide_session()`
        // actually forwards the explicit `types` value instead of falling
        // through to the dialect-keyed default.
        assert_eq!(
            session.type_policy(),
            brink_analyzer::TypePolicy::Gradual,
            "ide_session() must forward the resolved [project] types"
        );
        assert_eq!(
            session.lint_policy().overrides.get("E014"),
            Some(&brink_analyzer::LintLevel::Deny),
            "ide_session() must forward the resolved [lints] re-level"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Sanity companion: with no `brink.toml` at all, `ide_session()` must
    /// still resolve to the same byte-identical defaults `IdeSession::new()`
    /// starts with — the fix must not invent policy out of nothing.
    #[test]
    fn ide_session_matches_session_defaults_when_no_brink_toml_is_present() {
        let dir = std::env::temp_dir().join(format!(
            "brink-ide-unit-ide-session-no-config-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("story.ink"), "Hello.\n-> END\n").unwrap();

        let entry = dir.join("story.ink");
        let project = Project::load(&entry, &LintOverrides::default()).expect("project loads");
        let session = project.ide_session();

        assert_eq!(
            session.language_dialect(),
            brink_analyzer::Dialect::default()
        );
        assert_eq!(
            session.type_policy(),
            brink_analyzer::TypePolicy::Gradual,
            "no brink.toml must resolve through the StrictInk-keyed default, not invent a policy"
        );
        assert_eq!(
            *session.lint_policy(),
            brink_analyzer::LintPolicy::default()
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}

/// Regression for issue #1403: `resolve_analysis_options` used to call the
/// path-based `brink_project_config::load_from_entry`, which reads straight
/// off `std::fs` and can only ever see the real filesystem — the same
/// hardcoding `brink_environment::Project::load` (the `brink
/// compile`/brink-web/bevy-brink producer) had already moved off of via the
/// `SourceTree` seam (#1312/#1370). These tests resolve config from a
/// `brink_db::InMemory` tree — a `brink.toml` that is never written to disk
/// at all — so they only pass if `resolve_analysis_options` is generic over
/// `&dyn SourceTree` rather than secretly still bound to `RealFs`/`std::fs`.
#[cfg(test)]
mod resolve_analysis_options_source_tree_seam_tests {
    use super::*;

    #[test]
    fn resolves_brink_toml_from_a_non_filesystem_source_tree() {
        let tree = brink_db::InMemory::new(BTreeMap::from([
            (
                "brink.toml".to_string(),
                "[project]\ndialect = \"brink\"\n".to_string(),
            ),
            (
                "chapters/main.ink".to_string(),
                "Hello.\n-> END\n".to_string(),
            ),
        ]));

        let options = resolve_analysis_options(
            &tree,
            Path::new("."),
            "chapters/main.ink",
            &LintOverrides::default(),
        )
        .expect("resolves options over an in-memory tree");

        assert_eq!(
            options.dialect,
            brink_analyzer::Dialect::Brink,
            "must discover + apply a brink.toml that exists only in the SourceTree, never on disk"
        );
    }

    /// The "missing file changes nothing" guarantee, proven against the
    /// tree-based probe: no `brink.toml` anywhere in the tree resolves to
    /// byte-identical defaults.
    #[test]
    fn no_brink_toml_in_tree_yields_default_options() {
        let tree = brink_db::InMemory::new(BTreeMap::from([(
            "main.ink".to_string(),
            "Hello.\n-> END\n".to_string(),
        )]));

        let options =
            resolve_analysis_options(&tree, Path::new("."), "main.ink", &LintOverrides::default())
                .expect("resolves options with no config");

        assert_eq!(options, brink_analyzer::AnalysisOptions::default());
    }

    /// The tree-based probe walks up from a nested entry key exactly like
    /// the filesystem-based one did — a `brink.toml` two levels above the
    /// entry is still discovered.
    #[test]
    fn walks_up_from_a_nested_entry_key_in_the_tree() {
        let tree = brink_db::InMemory::new(BTreeMap::from([
            (
                "brink.toml".to_string(),
                "[project]\ndialect = \"brink\"\n".to_string(),
            ),
            (
                "book/chapters/main.ink".to_string(),
                "Hello.\n-> END\n".to_string(),
            ),
        ]));

        let options = resolve_analysis_options(
            &tree,
            Path::new("."),
            "book/chapters/main.ink",
            &LintOverrides::default(),
        )
        .expect("resolves options by walking up the tree");

        assert_eq!(options.dialect, brink_analyzer::Dialect::Brink);
    }
}
