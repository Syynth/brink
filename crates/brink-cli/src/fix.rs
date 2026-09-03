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
    /// write it to a file. Implies no disk write, same as `dry_run`. Composes
    /// with `dry_run` rather than silently overriding it: the diff goes to
    /// its destination, nothing is written, and the report goes to stderr
    /// (never stdout, which must stay a clean `git apply`-able patch) —
    /// likewise whenever `cap_hit` is set, so a capped run always explains
    /// its exit-1, `--dry-run` or not (issue #3463).
    pub diff: Option<String>,
    /// `None`: don't promote the Suggested tier. `Some("*")`: promote every
    /// Suggested-max fixer for this run — except one the project's `[fix]`
    /// table explicitly sets to `"off"`, which stays off (`off` means never
    /// offer or batch a fixer for this code in this project,
    /// `docs/book/src/toolchain/project-config.md` §Fix policy — a bare,
    /// codeless flag is not the "explicit action" that section's widening
    /// applies to). `Some(codes)`: promote only the comma-separated codes
    /// named — naming a code IS the explicit action, so it wins over the
    /// project's `[fix]` table for that code even when the table says
    /// `"off"` (CLI > file, `docs/autofix-spec.md` §6.2's "explicit actions
    /// may widen per run"; its own sanctioned example, `brink fix --suggested E033`,
    /// is this code-explicit form).
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
        // `--diff` implies no disk write, same as `--dry-run` — but it must
        // not silently win over `--dry-run` and swallow the report, and a
        // capped run must never exit 1 with nothing explaining why (issue
        // #3463). The diff itself may already be on stdout (`dest == "-"`),
        // and that stream must stay a clean `git apply`-able patch (the same
        // contract `--placeholder` already keeps, above) — so the report
        // goes to stderr here, unconditionally of the destination, whenever
        // there is something to say: `--dry-run` asked for it, or `cap_hit`
        // means the `1` exit code needs an explanation.
        if opts.dry_run || report.cap_hit {
            let mut err = io::stderr().lock();
            print_report(&mut err, &session, &report)?;
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
        // Stderr, deliberately never `out` (stdout): the `--diff` branch
        // above may already have written a unified diff to stdout, and a
        // pipeline like `brink fix story.ink --diff --placeholder | git
        // apply` (advertised in `docs/book/src/toolchain/cli/fix.md`) must
        // see nothing but that patch there. See `print_placeholders`' own
        // doc comment for why this listing has no positive-path CLI test
        // today.
        let mut err = io::stderr().lock();
        print_placeholders(&mut err, &session, &select)?;
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
/// `--suggested` — a CLI action, so an *explicit* code wins over the file for
/// that code (`docs/autofix-spec.md` §6.2, "explicit actions may widen per
/// run"; #1005 CLI/API > file > default is the same precedence every other
/// `brink` override follows). The bare, codeless form is not that explicit
/// action: it must not silently re-enable a code the project withdrew with
/// `"off"` (`docs/book/src/toolchain/project-config.md` §Fix policy — "never
/// offer or batch a fixer for this code in this project" — and this crate's
/// own `docs/book/src/toolchain/cli/fix.md` example comment on
/// `E014 = "off"`, both state `off` unconditionally).
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
        // `FixMode::from_config` is the one place `brink_project_config`'s
        // `Off`/`Auto`/`Ask` maps onto this crate's own `FixMode` — see its
        // doc comment for why `Ask` elides to "no override" rather than
        // `FixMode::Ask` (issue #3464: this bridge used to be hand-rolled
        // here and independently in `brink-web`'s `fix_batch.rs`).
        if let Some(mode) = FixMode::from_config(config.effective_fix_policy(code.as_str(), None)) {
            policy.set(code, mode);
        }
    }
    match suggested {
        None => {}
        Some("*") => {
            for fixer in FIXERS {
                let code = fixer.code();
                // The bare form promotes every Suggested-max fixer EXCEPT
                // one the project explicitly turned off — see this
                // function's own doc comment. A named code (the `Some(codes)`
                // arm below) is the one place `off` is meant to be
                // overridable.
                if fixer.max_applicability() == Applicability::Suggested
                    && config.effective_fix_policy(code.as_str(), None)
                        != brink_project_config::FixPolicy::Off
                {
                    policy.set(code, FixMode::Auto);
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
///
/// Split into collection ([`collect_placeholders`]) and rendering
/// ([`render_placeholders`]) so the rendering half is unit-testable on its
/// own: every fixer in [`FIXERS`] today declares `Applicability::Suggested`
/// (no `Placeholder`-max fixer is registered yet), so `collect_placeholders`
/// can never actually return anything through a real `brink fix` invocation
/// — there is no positive-path CLI test for this listing's content, only for
/// the fact that it changes nothing else (`fix_cli.rs`'s
/// `placeholder_flag_does_not_change_the_write_outcome`). Tracked as a named
/// follow-up (issue #3456) for once a `Placeholder`-tier fixer exists to
/// drive it.
fn print_placeholders(
    out: &mut impl Write,
    session: &brink_ide::session::IdeSession,
    select: &Select,
) -> Result<(), String> {
    let found = collect_placeholders(session, select);
    render_placeholders(out, session, &found)
}

/// Collect every `Applicability::Placeholder` fix in `select`'s diagnostics,
/// as `(code, file, start offset, title)` — see [`print_placeholders`]'s doc
/// comment for why this is always empty against the registry as it stands
/// today.
fn collect_placeholders(
    session: &brink_ide::session::IdeSession,
    select: &Select,
) -> Vec<(DiagnosticCode, FileId, TextSize, String)> {
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
    found
}

/// Render `found` (from [`collect_placeholders`]) as `--placeholder`'s
/// listing. A pure formatting step over already-collected entries, so it can
/// be unit-tested with a hand-built `found` even though nothing in the
/// current fixer registry can produce one for real (see
/// [`print_placeholders`]'s doc comment).
fn render_placeholders(
    out: &mut impl Write,
    session: &brink_ide::session::IdeSession,
    found: &[(DiagnosticCode, FileId, TextSize, String)],
) -> Result<(), String> {
    if found.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "{} placeholder fix(es) available (not applied):",
        found.len()
    )
    .map_err(|e| e.to_string())?;
    for (code, file, offset, title) in found {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A minimal loaded [`Project`] + its [`brink_ide::session::IdeSession`],
    /// just so [`render_placeholders`]'s `loc_str` calls (which need a real
    /// `FileId`/session pair) have something to resolve against — the
    /// content is irrelevant, this never runs a real fix.
    fn loaded_session(tag: &str) -> brink_ide::session::IdeSession {
        let dir = std::env::temp_dir().join(format!("brink-fix-unit-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("story.ink"), "Hello.\n-> END\n").unwrap();
        let project = Project::load(&dir.join("story.ink"), &LintOverrides::default()).unwrap();
        project.ide_session()
    }

    // ── render_placeholders: pure formatting, no live Placeholder fixer
    // needed (see `print_placeholders`'s doc comment for why the CLI itself
    // can't drive this positively yet — issue #3456) ──────────────────────

    #[test]
    fn render_placeholders_lists_every_entry_with_its_location_and_title() {
        let session = loaded_session("render-nonempty");
        let file = session.db().file_ids().next().expect("at least one file");
        let code = DiagnosticCode::from_str_code("E025").unwrap();
        let found = vec![(
            code,
            file,
            TextSize::from(0),
            "fill this in by hand".to_string(),
        )];

        let mut buf: Vec<u8> = Vec::new();
        render_placeholders(&mut buf, &session, &found).unwrap();
        let text = String::from_utf8(buf).unwrap();

        assert!(
            text.starts_with("1 placeholder fix(es) available (not applied):"),
            "got: {text}"
        );
        assert!(text.contains("[E025]"), "got: {text}");
        assert!(text.contains("fill this in by hand"), "got: {text}");
    }

    #[test]
    fn render_placeholders_prints_nothing_for_an_empty_list() {
        let session = loaded_session("render-empty");
        let mut buf: Vec<u8> = Vec::new();
        render_placeholders(&mut buf, &session, &[]).unwrap();
        assert!(buf.is_empty(), "an empty list must print nothing at all");
    }

    // ── build_policy: bare --suggested must not override an explicit
    // `[fix] = "off"` entry; a named code still may (finding on PR #3453)
    // ──────────────────────────────────────────────────────────────────

    #[test]
    fn bare_suggested_does_not_promote_a_code_the_project_turned_off() {
        let mut config = brink_project_config::ProjectConfig::default();
        config
            .fix
            .insert("E025".to_string(), brink_project_config::FixPolicy::Off);

        let policy = build_policy(&config, Some("*")).unwrap();
        assert_eq!(
            policy.override_for(DiagnosticCode::from_str_code("E025").unwrap()),
            Some(FixMode::Off),
            "bare --suggested must leave an explicit [fix] E025 = \"off\" alone"
        );
    }

    #[test]
    fn explicit_suggested_code_still_overrides_an_off_entry() {
        let mut config = brink_project_config::ProjectConfig::default();
        config
            .fix
            .insert("E025".to_string(), brink_project_config::FixPolicy::Off);

        let policy = build_policy(&config, Some("E025")).unwrap();
        assert_eq!(
            policy.override_for(DiagnosticCode::from_str_code("E025").unwrap()),
            Some(FixMode::Auto),
            "naming the code explicitly is the sanctioned widening — it must still win"
        );
    }
}
