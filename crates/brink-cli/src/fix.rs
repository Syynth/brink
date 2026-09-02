//! `brink fix` — auto-fix M6 (`docs/autofix-spec.md` §8, issue #3421).
//!
//! A thin CLI driver over `brink_ide::fix`'s batching core (§5): every round
//! and every collision rule live there and are NOT reimplemented here. This
//! module only resolves the project's `[fix]` policy (§6.1) into the
//! `brink_ide::fix::policy::FixPolicy` the core reads, builds the `Select`
//! from `--code`, runs `fix_all` to a fixpoint, and renders the result as a
//! report, a `git apply`-able diff, or a set of file writes.
//!
//! `path` is the same entry-file addressing `brink compile`/`brink ide` use:
//! `crate::ide::project::Project::load` discovers `brink.toml` from the
//! entry's directory (§6.1's project-owned policy) and follows `INCLUDE`s (or
//! the native module graph) exactly like a real compile — "the project
//! (`brink.toml`) or a single file" is one code path, not two: a bare file
//! with no discovered `brink.toml` is just the same load with an empty
//! `ProjectConfig`.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use brink_ide::LineIndex;
use brink_ide::fix::policy::{FixMode, FixPolicy};
use brink_ide::fix::{Applicability, FIXERS, FixCx, Select, fix_all, fixes_for};
use brink_ir::suppressions::apply_suppressions;
use brink_ir::{DiagnosticCode, FileId};
use rowan::TextSize;

use crate::ide::project::{LintOverrides, Project, discover_fix_project_config, resolve_fs_path};

/// `brink fix`'s resolved CLI arguments (see `Commands::Fix` in `main.rs` for
/// the `clap` surface each field comes from).
pub struct FixOpts {
    /// Entry-point file — an `.ink`/`.brink` source, or a project's real
    /// entry if `brink.toml` names one relative to it.
    pub path: PathBuf,
    /// Print the report; write nothing to disk.
    pub dry_run: bool,
    /// Emit a unified diff instead of writing — `"-"` for stdout, a path to
    /// write it to a file. Implies no disk write, same as `dry_run`.
    pub diff: Option<String>,
    /// `None`: don't promote the Suggested tier. `Some("*")`: promote every
    /// Suggested-max fixer for this run. `Some(codes)`: promote only the
    /// comma-separated codes named. Wins over the project's `[fix]` table for
    /// the same code (CLI > file, `docs/autofix-spec.md` §6.2's "explicit
    /// actions may widen per run").
    pub suggested: Option<String>,
    /// Also report every Placeholder-tier fix available in the selection.
    /// Never applied — `FixPolicy::admits` refuses `Placeholder`
    /// unconditionally (§3, "Batchable: never") — this only makes the CLI
    /// name where an author has a hole to fill by hand.
    pub placeholder: bool,
    /// Restrict the run to these diagnostic codes (comma-separated,
    /// case-sensitive, e.g. `E025,E080`). Empty means every code.
    pub code: Vec<String>,
    /// The §5 round cap.
    pub max_rounds: u8,
}

/// Run `brink fix`. Returns the process exit code: `0` when `fix_all` reached
/// a fixpoint, `1` when the round cap was hit or a fixer failed to discharge
/// its own diagnostic (both surface as [`brink_ide::fix::Report::cap_hit`]),
/// `2` on a usage/IO error.
pub fn run(opts: &FixOpts) -> ExitCode {
    match run_inner(opts) {
        Ok(code) => code,
        Err(msg) => {
            let _ = writeln!(io::stderr(), "error: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run_inner(opts: &FixOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.path, &LintOverrides::default())?;
    let config = discover_fix_project_config(&opts.path)?.unwrap_or_default();
    let policy = build_policy(&config, opts.suggested.as_deref())?;
    let select = build_select(&opts.code)?;

    let mut session = project.ide_session();

    // Snapshot every file's pre-fix source, so the diff/write path only
    // touches what `fix_all` actually changed — the same "only the edited
    // files" contract `Project::unified_diff`'s caller (`ide`'s own
    // `Mode::Patch`) already keeps.
    let tracked: Vec<(FileId, String)> = session
        .db()
        .file_ids()
        .filter_map(|id| session.file_path(id).map(|p| (id, p.to_owned())))
        .collect();
    let before: BTreeMap<String, String> = tracked
        .iter()
        .filter_map(|(id, path)| session.source(*id).map(|s| (path.clone(), s.to_owned())))
        .collect();

    let report = fix_all(&mut session, &select, &policy, opts.max_rounds);

    let mut edited = BTreeMap::new();
    for (id, path) in &tracked {
        if let Some(src) = session.source(*id)
            && before.get(path).map(String::as_str) != Some(src)
        {
            edited.insert(path.clone(), src.to_owned());
        }
    }

    let mut out = io::stdout().lock();
    if let Some(dest) = &opts.diff {
        let diff = project.unified_diff(&edited)?;
        if dest == "-" {
            write!(out, "{diff}").map_err(|e| e.to_string())?;
        } else {
            std::fs::write(dest, diff).map_err(|e| format!("{dest}: {e}"))?;
        }
    } else if opts.dry_run {
        print_report(&mut out, &session, &report)?;
    } else {
        for (path, src) in &edited {
            let fs_path = resolve_fs_path(&opts.path, path);
            std::fs::write(&fs_path, src).map_err(|e| format!("{}: {e}", fs_path.display()))?;
        }
        writeln!(out, "wrote {} file(s)", edited.len()).map_err(|e| e.to_string())?;
        print_report(&mut out, &session, &report)?;
    }

    if opts.placeholder {
        print_placeholders(&mut out, &session, &select)?;
    }

    Ok(if report.cap_hit {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// The project's `[fix]` policy (`docs/autofix-spec.md` §6.1), resolved with
/// no app-scope ceiling (`brink fix` is not an app — §6.2's ceiling is a
/// per-editor personal setting with nothing to plug in here), then widened by
/// `--suggested` — a CLI action, so it wins over the file for the same code
/// (`docs/autofix-spec.md` §6.2, "explicit actions may widen per run"; #1005
/// CLI/API > file > default is the same precedence every other `brink`
/// override follows).
///
/// Only [`FIXERS`]' own registered codes are considered: a code with no
/// fixer can never produce a `Fix` for [`fixes_for`] to filter, so recording
/// an override for it would be dead weight.
fn build_policy(
    config: &brink_project_config::ProjectConfig,
    suggested: Option<&str>,
) -> Result<FixPolicy, String> {
    let mut policy = FixPolicy::new();
    for fixer in FIXERS {
        let code = fixer.code();
        match config.effective_fix_policy(code.as_str(), None) {
            brink_project_config::FixPolicy::Off => policy.set(code, FixMode::Off),
            brink_project_config::FixPolicy::Auto => policy.set(code, FixMode::Auto),
            // `Ask` is the neutral value — both "the file doesn't mention
            // this code" and an explicit `= "ask"` resolve to it, and both
            // mean "the tier default governs" (a Safe fixer still batches).
            // Recording an `Ask` override here would instead force a
            // Safe-max fixer down to non-batchable, which is exactly the
            // regression `docs/autofix-spec.md` §6.1's TOML comment rules
            // out ("absent ⇒ ask: … batchable (Safe)").
            brink_project_config::FixPolicy::Ask => {}
        }
    }
    match suggested {
        None => {}
        Some("*") => {
            for fixer in FIXERS {
                if fixer.max_applicability() == Applicability::Suggested {
                    policy.set(fixer.code(), FixMode::Auto);
                }
            }
        }
        Some(codes) => {
            for code in codes.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                policy.set(parse_code(code)?, FixMode::Auto);
            }
        }
    }
    Ok(policy)
}

/// Parse a `--code`/`--suggested` code string, or a clear error naming the
/// bad token — never a silent no-op on a typo (house rule).
fn parse_code(code: &str) -> Result<DiagnosticCode, String> {
    DiagnosticCode::from_str_code(code).ok_or_else(|| format!("unknown diagnostic code: {code}"))
}

/// `--code`'s selection: every code when empty, else exactly the named ones.
fn build_select(code: &[String]) -> Result<Select, String> {
    if code.is_empty() {
        return Ok(Select::all());
    }
    let codes = code
        .iter()
        .map(|c| parse_code(c))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Select::all().with_codes(codes))
}

/// Render [`brink_ide::fix::Report`] as text. `applied`/`skipped_overlap`
/// sites are named by file path only — their `range` was captured against
/// whichever round's source that was, and a later round's edits can shift
/// every offset after it, so printing a line:col from a stale round would be
/// silently wrong. `remaining` is recomputed once, after the loop, against
/// the session's *current* source (`fix_all`'s own doc comment), so its
/// offsets are safe to resolve to a line:col.
fn print_report(
    out: &mut impl Write,
    session: &brink_ide::session::IdeSession,
    report: &brink_ide::fix::Report,
) -> Result<(), String> {
    writeln!(
        out,
        "{} fix(es) applied over {} round(s)",
        report.applied.len(),
        report.rounds
    )
    .map_err(|e| e.to_string())?;
    for site in &report.applied {
        let path = session.file_path(site.file).unwrap_or("?");
        writeln!(out, "  [{}] {path}", site.code.as_str()).map_err(|e| e.to_string())?;
    }
    if report.skipped_overlap > 0 {
        writeln!(
            out,
            "{} fix(es) deferred to a later round (edit overlap)",
            report.skipped_overlap
        )
        .map_err(|e| e.to_string())?;
    }
    if report.cap_hit {
        writeln!(
            out,
            "cap hit after {} round(s); {} diagnostic(s) still admit a fix:",
            report.rounds,
            report.remaining.len()
        )
        .map_err(|e| e.to_string())?;
        for site in &report.remaining {
            writeln!(
                out,
                "  [{}] {}",
                site.code.as_str(),
                loc_str(session, site.file, site.range.start())
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// `path:line:col`, 1-based, against `session`'s *current* source for `file`.
fn loc_str(session: &brink_ide::session::IdeSession, file: FileId, offset: TextSize) -> String {
    let path = session.file_path(file).unwrap_or("?");
    let src = session.source(file).unwrap_or_default();
    let (line, col) = LineIndex::new(src).line_col(offset);
    format!("{path}:{}:{}", line + 1, col + 1)
}

/// `--placeholder`'s own listing: every `Applicability::Placeholder` fix in
/// the selection, never batchable (§3), so this is informational only —
/// never folded into `report`. Mirrors [`brink_ide::fix::batch::collect`]'s
/// own walk (diagnostics → suppressions → `fixes_for`) minus the
/// `policy.admits` filter, since a placeholder fix would fail that filter by
/// construction.
fn print_placeholders(
    out: &mut impl Write,
    session: &brink_ide::session::IdeSession,
    select: &Select,
) -> Result<(), String> {
    let db = session.db();
    let cx = FixCx::new(db);
    let mut found = Vec::new();
    for file in select.files(db) {
        let (Some(raw), Some(source)) = (db.diagnostics(file), db.source(file)) else {
            continue;
        };
        let diagnostics = match db.suppressions(file) {
            Some(sup) => apply_suppressions(file, source, raw.to_vec(), sup),
            None => raw.to_vec(),
        };
        for d in &diagnostics {
            if !select.matches(db, d) {
                continue;
            }
            for fix in fixes_for(&cx, d) {
                if fix.applicability == Applicability::Placeholder {
                    found.push((d.code, d.file, d.range.start(), fix.title));
                }
            }
        }
    }
    if found.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "{} placeholder fix(es) available (not applied):",
        found.len()
    )
    .map_err(|e| e.to_string())?;
    for (code, file, offset, title) in &found {
        writeln!(
            out,
            "  [{}] {} — {title}",
            code.as_str(),
            loc_str(session, *file, *offset)
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
