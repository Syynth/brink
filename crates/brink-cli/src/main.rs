mod batch;
mod debug;
mod ide;
mod lint_overrides;
mod tui;

use std::io::{BufRead, IsTerminal, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// brink — an Ink compiler and runtime
#[derive(Parser)]
#[command(name = "brink", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// CLI-facing mirror of `brink_compiler::Dialect` (docs/t1b-surface-spec.md
/// §1) — a separate type so `brink-analyzer` doesn't need a `clap` dependency
/// just for argument parsing.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum DialectArg {
    /// Reject brink-extension syntax with a targeted diagnostic (default).
    StrictInk,
    /// Accept brink-extension syntax: logic blocks (`~ { … }`), collection
    /// literals (`#[…]`/`#{…}`), and indexing (T1b-1 through T1b-3).
    Brink,
}

impl From<DialectArg> for brink_compiler::Dialect {
    fn from(arg: DialectArg) -> Self {
        match arg {
            DialectArg::StrictInk => brink_compiler::Dialect::StrictInk,
            DialectArg::Brink => brink_compiler::Dialect::Brink,
        }
    }
}

/// CLI-facing mirror of `brink_compiler::TypePolicy` (docs/typed-mode-spec.md
/// §1/§9-step-3, TM-3) — same rationale as [`DialectArg`], a plain `clap`
/// surface distinct from the analyzer's own enum.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum TypesArg {
    /// `Unknown` unifies with anything; annotations are optional (default).
    Gradual,
    /// `Unknown`/`Conflicted` escapes are compile errors; requires
    /// `dialect = brink`.
    Strict,
}

impl From<TypesArg> for brink_compiler::TypePolicy {
    fn from(arg: TypesArg) -> Self {
        match arg {
            TypesArg::Gradual => brink_compiler::TypePolicy::Gradual,
            TypesArg::Strict => brink_compiler::TypePolicy::Strict,
        }
    }
}

// ⚠ `packages/brink-desktop/src-tauri`'s `run_cli` sidecar command hardcodes
// a subset of these subcommand names in its own `ALLOWED_CLI_SUBCOMMANDS`
// allowlist (`src-tauri` cannot depend on this crate — it is deliberately
// excluded from the root cargo workspace, docs/desktop-shell-spec.md
// "Workspace placement"). Renaming or removing a variant here is checked by
// `packages/brink-desktop/src-tauri/src/lib.rs`'s
// `cli_allowlist_subcommands_exist_in_brink_cli_surface` test, which parses
// this enum as text and applies clap's default kebab-case renaming — keep
// every variant on that default (no `#[command(name = ...)]`/`rename_all`
// override) or update that test's parsing to match (#2507).
#[derive(Subcommand)]
enum Commands {
    /// Compile an .ink story (native pipeline)
    Compile {
        /// Entry-point .ink file
        input: PathBuf,
        /// Output file (format inferred from extension, defaults to stdout as .inkt)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Compiler dialect (docs/t1b-surface-spec.md §1). `strict-ink`
        /// (default) rejects brink-extension syntax (`~ { … }` blocks,
        /// `#[…]`/`#{…}` sigil literals, indexing) with a targeted
        /// diagnostic; `brink` accepts and compiles the syntax (T1b-2 and
        /// T1b-3 are live). Mount-time only:
        /// never embedded in `.inkb`, never delivered to the runtime.
        /// Overrides `[project] dialect` in a discovered `brink.toml`
        /// (#1005) — omit this flag to use the file's value, if any.
        #[arg(long, value_enum)]
        dialect: Option<DialectArg>,
        /// Typed-mode policy (docs/typed-mode-spec.md §1, TM-3). `gradual`
        /// (default) is today's behavior, byte-identical forever. `strict`
        /// makes `Unknown`/`Conflicted`-escaping inference a compile error
        /// and requires `--dialect brink` (a config error otherwise).
        /// Mount-time only: never embedded in `.inkb`. Overrides
        /// `[project] types` in a discovered `brink.toml` (#1005) — omit
        /// this flag to use the file's value, if any.
        #[arg(long, value_enum)]
        types: Option<TypesArg>,
        /// Deny a diagnostic code, promoting it to a hard compile error
        /// (issue #1373). Repeatable. Only codes whose *default* severity is
        /// `Warning` are overridable (#1160) — an unrecognized or
        /// non-overridable code is ignored with a warning through the usual
        /// channel, never silently. The special code `warnings` (`-D
        /// warnings`, mirroring rustc) is `deny-warnings`: promote every
        /// diagnostic that would otherwise resolve to `Warning` up to
        /// `Error`, the CLI equivalent of `[lints] deny-warnings = true`.
        /// Always wins over the same code in a discovered `brink.toml`'s
        /// `[lints]` table (#1005 `CLI/API > file > default` precedence).
        #[arg(short = 'D', long = "deny", value_name = "CODE")]
        deny: Vec<String>,
        /// Force a diagnostic code to `Warning`, promotable back to `Error`
        /// by `-D warnings`/`deny-warnings` like any unconfigured warning
        /// (issue #1373). Repeatable; same overridability rules and
        /// precedence as `--deny`.
        #[arg(long = "warn", value_name = "CODE")]
        warn: Vec<String>,
        /// Never escalate a diagnostic code past `Warning`, even under `-D
        /// warnings`/`deny-warnings` (issue #1373). Repeatable; same
        /// overridability rules and precedence as `--deny`.
        #[arg(long = "allow", value_name = "CODE")]
        allow: Vec<String>,
        /// D6 (`docs/debugger-spec.md` §1.2/§2, issue #3184): emit the
        /// `SectionKind::DebugInfo` bytecode-offset → source-range section
        /// (tag `0x11`) — the dev/studio-compile debug flag the ship-policy
        /// ruling names. Off by default: a release compile omits the
        /// section entirely and the `.inkb` stays byte-identical to a
        /// pre-D6 compile. Mount-time only, no `brink.toml` spelling
        /// (`docs/debugger-spec.md` §1.2).
        #[arg(long = "debug-info")]
        debug_info: bool,
    },
    /// Convert between ink formats (.inkb, .inkt)
    Convert {
        /// Input file (.ink, .brink, .inkb, or .inkt)
        input: PathBuf,
        /// Output file (format inferred from extension, defaults to stdout as .inkt)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export line tables from a compiled story as XLIFF 2.0
    ExportXliff {
        /// Input story file (.inkb, .ink, .brink, or .inkt)
        input: PathBuf,
        /// BCP 47 source language tag (e.g. "en")
        #[arg(long, default_value = "en")]
        src_lang: String,
        /// BCP 47 target language tag (e.g. "es")
        #[arg(long)]
        trg_lang: Option<String>,
        /// Output .xlf file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compile a translated XLIFF file into a .inkl locale overlay
    CompileLocale {
        /// Base .inkb file
        #[arg(long)]
        base: PathBuf,
        /// Translated .xlf file
        #[arg(long)]
        xliff: PathBuf,
        /// BCP 47 locale tag (e.g. "es", "ja")
        #[arg(long)]
        locale: String,
        /// Output .inkl file
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Regenerate XLIFF preserving existing translations after recompilation
    RegenerateXliff {
        /// Recompiled .inkb file
        #[arg(long)]
        base: PathBuf,
        /// Existing translated .xlf file
        #[arg(long)]
        existing: PathBuf,
        /// BCP 47 source language tag (e.g. "en")
        #[arg(long, default_value = "en")]
        src_lang: String,
        /// Output updated .xlf file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Migrate an existing XLIFF file's unit ids to the canonical
    /// scope-id-based scheme, preserving every translation, state, and hash
    /// in place. Safe to run unconditionally — units already on the new
    /// scheme are left untouched. Not needed after a `#@was` rename:
    /// `compile-locale` and `regenerate-xliff` rebind moved scope ids
    /// themselves.
    MigrateXliff {
        /// Existing .xlf file (any unit-id scheme)
        input: PathBuf,
        /// Output migrated .xlf file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Format .ink source files
    Fmt {
        /// .ink files to format
        files: Vec<PathBuf>,
        /// Check formatting without writing (exit 1 if unformatted)
        #[arg(long)]
        check: bool,
        /// Read from stdin, write formatted output to stdout
        #[arg(long)]
        stdin: bool,
    },
    /// Play an ink story interactively
    Play {
        /// Story file (.ink or .brink source, .inkb, or .inkt)
        file: PathBuf,
        /// Read choice inputs from a file (one 1-indexed choice per line)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Typewriter speed in characters per second (0 = instant)
        #[arg(short, long, default_value_t = 30)]
        speed: u64,
        /// Locale overlay files (.inkl) — switchable at runtime via [l] key
        #[arg(long)]
        locale: Vec<PathBuf>,
        /// Save the execution transcript to a .brkt file after playing
        #[arg(long)]
        save_transcript: Option<PathBuf>,
    },
    /// Step through a story in a debugger (breakpoints, stepping, locals)
    #[command(long_about = "\
Step through a story from the terminal.

Compiles the story WITH debug info (`--debug-info`'s equivalent) — without it
there is nothing to map a bytecode position back to a line, so breakpoints
could not bind and `step` could not know when it had crossed a line.

Verbs are the same ones the scripted test harness and the studio use:
  break <file>:<line>   arm a breakpoint (1-based lines)
  run, continue         advance to the next breakpoint/choice/terminal
  step into|over|out    advance one SOURCE line   (`next` = `step over`)
  stepi into|over|out   advance one VM INSTRUCTION
  locals, stack, list   inspect

Pass --script to run a .dbg file non-interactively and print the transcript;
that is the same format the harness fixtures use, so a session can move
between the two without translation.")]
    Debug {
        /// Story file (.ink or .brink source, .inkb, or .inkt)
        file: PathBuf,
        /// Run a .dbg script instead of starting a REPL
        #[arg(long)]
        script: Option<PathBuf>,
    },
    /// Re-render a saved transcript against a story (optionally with a locale)
    Replay {
        /// Transcript file (.brkt)
        transcript: PathBuf,
        /// Story file (.ink or .brink source, .inkb, or .inkt)
        #[arg(short, long)]
        story: PathBuf,
        /// Locale overlay file (.inkl) to apply before rendering
        #[arg(long)]
        locale: Option<PathBuf>,
    },
    /// Scriptable IDE queries (definitions, references, …) over an ink project
    #[command(long_about = "\
Scriptable IDE queries over an ink project.

Address a symbol by its qualified name — the same dotted paths ink uses:
  intro            a knot
  intro.evidence   a stitch
  Colors.Red       a list item
Build the project from an entry file with --entry/-e (INCLUDEs are followed).
Use --format json for machine-readable output. Exit codes: 0 ok, 1 query-false,
2 usage error.")]
    Ide {
        #[command(subcommand)]
        command: ide::IdeCommand,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    if let Some(command) = cli.command {
        return run_command(command);
    }

    ExitCode::SUCCESS
}

/// Log an `Err` via `tracing::error!` and collapse a fallible command's
/// result down to the process exit code `main` returns. Pulled out of
/// `run_command` so each match arm is one line — the match was tripping
/// `clippy::too_many_lines` on its own repeated `if let Err(e) = … { … }`
/// boilerplate well before it was doing anything complex per-command.
fn report_result(result: Result<(), Box<dyn std::error::Error>>) -> ExitCode {
    if let Err(e) = result {
        tracing::error!("{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_command(command: Commands) -> ExitCode {
    match command {
        Commands::Compile {
            input,
            output,
            dialect,
            types,
            deny,
            warn,
            allow,
            debug_info,
        } => run_compile_command(
            &input,
            output.as_deref(),
            dialect.map(Into::into),
            types.map(Into::into),
            &deny,
            &warn,
            &allow,
            debug_info,
        ),
        Commands::Convert { input, output } => run_convert_command(&input, output.as_deref()),
        Commands::ExportXliff {
            input,
            src_lang,
            trg_lang,
            output,
        } => report_result(run_export_xliff(
            &input,
            &src_lang,
            trg_lang.as_deref(),
            output.as_deref(),
        )),
        Commands::CompileLocale {
            base,
            xliff,
            locale,
            output,
        } => report_result(run_compile_locale(&base, &xliff, &locale, &output)),
        Commands::RegenerateXliff {
            base,
            existing,
            src_lang,
            output,
        } => report_result(run_regenerate_xliff(
            &base,
            &existing,
            &src_lang,
            output.as_deref(),
        )),
        Commands::MigrateXliff { input, output } => {
            report_result(run_migrate_xliff(&input, output.as_deref()))
        }
        Commands::Fmt {
            files,
            check,
            stdin,
        } => report_result(run_fmt(&files, check, stdin)),
        Commands::Play {
            file,
            input,
            speed,
            locale,
            save_transcript,
        } => {
            let locale_refs: Vec<&std::path::Path> = locale.iter().map(PathBuf::as_path).collect();
            report_result(run_play(
                &file,
                input.as_deref(),
                speed,
                &locale_refs,
                save_transcript.as_deref(),
            ))
        }
        Commands::Replay {
            transcript,
            story,
            locale,
        } => report_result(run_replay(&transcript, &story, locale.as_deref())),
        Commands::Debug { file, script } => {
            report_result(debug::run_debug(&file, script.as_deref()))
        }
        Commands::Ide { command } => ide::run(&command),
    }
}

/// Log one compile diagnostic at the `tracing` level matching its actual
/// resolved severity (`ResolvedDiagnostic::severity`, issue #1162) — a
/// `[lints]` code down-leveled to `info`/`hint` must render at the matching
/// tier here rather than every `CompileOutput::warnings` entry printing as
/// `warn!` regardless of what it actually resolved to. Used for both the
/// non-fatal `CompileOutput::warnings` set and — since #1957 — the fatal
/// `CompileError::Diagnostics` payload (see [`render_fatal_compile_error`]),
/// so `Severity::Error` is a real, common case here now, not the dead arm
/// the match used to carry defensively.
///
/// Renders `path:start..end [CODE] message`. `ResolvedDiagnostic` carries a
/// byte-offset `range` and the file's `path`, but not a resolved line/column
/// or the quoted source line — `range` is deliberately left as raw byte
/// offsets (see its doc comment) because column units are consumer-specific
/// and the consumer already holds the source text to resolve them in the
/// unit it needs. A terminal-friendly line:column + source-quoting
/// presentation is possible (the CLI does still hold the source text at the
/// `compile_entry` call site) but is a follow-up, not this fix — this is the
/// "at least tell me the code, message, and where" bar, not a rustc-style
/// renderer.
fn log_diagnostic(d: &brink_compiler::ResolvedDiagnostic) {
    let start = u32::from(d.range.start());
    let end = u32::from(d.range.end());
    match d.severity {
        brink_ir::Severity::Error => {
            tracing::error!(
                "{}:{start}..{end} [{}] {}",
                d.path,
                d.code.as_str(),
                d.message
            );
        }
        brink_ir::Severity::Warning => {
            tracing::warn!(
                "{}:{start}..{end} [{}] {}",
                d.path,
                d.code.as_str(),
                d.message
            );
        }
        brink_ir::Severity::Info => {
            tracing::info!(
                "{}:{start}..{end} [{}] {}",
                d.path,
                d.code.as_str(),
                d.message
            );
        }
        brink_ir::Severity::Hint => {
            tracing::debug!(
                "{}:{start}..{end} [{}] {}",
                d.path,
                d.code.as_str(),
                d.message
            );
        }
    }
}

/// Render a fatal [`brink_compiler::CompileError::Diagnostics`] payload
/// through [`log_diagnostic`] before it is boxed and bubbles up through
/// [`compile_entry`] to `report_result`/`run_compile_command`'s generic
/// `tracing::error!("{e}")` (issue #1957).
///
/// Without this, every caller only ever sees `CompileError`'s `Display` —
/// `"N diagnostic(s) prevented compilation"`, the count and nothing else —
/// even though the fully-resolved diagnostic set (code, message, severity,
/// path, byte range) already exists on the error value. `compile_entry` is
/// the one seam every `brink compile`/`convert`/`play`/`replay`/
/// `export-xliff` invocation flows through (see its own doc comment), so
/// wiring the render in here — rather than in each subcommand — covers all
/// of them at once. The count line still prints afterward, now as a
/// trailing summary under the individual `[CODE] message` lines instead of
/// the only thing printed.
fn render_fatal_compile_error(err: brink_compiler::CompileError) -> Box<dyn std::error::Error> {
    if let brink_compiler::CompileError::Diagnostics(diags) = &err {
        for d in diags {
            log_diagnostic(d);
        }
    }
    Box::new(err)
}

/// Build the #1306 [`Environment`](brink_environment::Environment) for `entry`
/// and run the pure compile over it — `Project::load` → `compile(&env)`, the
/// one path every `brink compile`/`convert`/`play`/`replay`/`export-xliff`
/// invocation flows through now. A fatal `CompileError::Diagnostics` is
/// routed through [`render_fatal_compile_error`] here — the shared seam —
/// so every one of those subcommands renders the resolved diagnostic set
/// instead of only the bare count (issue #1957).
///
/// The CLI mounts a [`RealFs::new`](brink_driver::RealFs::new) tree
/// rooted at [`native_source_root`] — a lazy real-filesystem `SourceTree`, not
/// a whole-tree eager drain (issue #1357): `list` enumerates `.brink`
/// keys by stat alone (never descending into `target/`, `.git/`, or
/// `node_modules/` — issue #1381), and `read` serves any one of them off
/// disk only when `Project::load` actually needs it (an ink entry's
/// `INCLUDE`-reachable set, a native entry's whole `.brink` universe, and
/// whichever single `brink.toml` config discovery resolves). An unrelated
/// malformed/non-UTF8 file elsewhere under the root is therefore never read
/// and can no longer fail an otherwise-valid compile — config discovery and
/// source enumeration both still run over the one seam inside the producer
/// (rather than the CLI resolving `AnalysisOptions` itself). Policy layers, in
/// increasing priority:
/// 1. `AnalysisOptions::default()` (`strict-ink`; `types` dialect-keyed per
///    #1127);
/// 2. a discovered `brink.toml`'s `[project] dialect`/`types`/`[lints]`
///    (#1005), walked up from `entry`; a missing file changes nothing;
/// 3. `--dialect`/`--types`/`--deny`/`--warn`/`--allow`/`-D warnings`, as
///    [`OptionOverrides`](brink_environment::OptionOverrides) that always
///    win over the file (#1373).
///
/// Unknown keys in the file — and unrecognized or non-overridable
/// `--deny`/`--warn`/`--allow` codes — are logged as warnings by the
/// producer, never treated as errors (forward compat / #1160).
///
/// [`native_source_root`]: brink_driver::native_source_root
fn compile_entry(
    entry: &std::path::Path,
    dialect: Option<brink_compiler::Dialect>,
    types: Option<brink_compiler::TypePolicy>,
    lints: std::collections::BTreeMap<String, brink_driver::LintLevel>,
    deny_warnings: Option<bool>,
    debug_info: bool,
) -> Result<brink_compiler::CompileOutput, Box<dyn std::error::Error>> {
    let (root, warnings) = brink_driver::native_source_root_with_warnings(entry);
    for warning in &warnings {
        let _ = writeln!(std::io::stderr(), "warning: {warning}");
    }
    let tree = brink_driver::RealFs::new(&root);
    let entry_key = brink_driver::relative_key(&root, entry);
    let overrides = brink_environment::OptionOverrides {
        dialect,
        types,
        lints,
        deny_warnings,
        debug_info,
    };
    let env = brink_environment::Project::load(&tree, &entry_key, &overrides)?;
    brink_environment::compile(&env).map_err(render_fatal_compile_error)
}

/// `Commands::Compile`'s dispatch, factored out of [`run_command`] (matching
/// the `Commands::Ide => return ide::run(&command)` shape already used
/// there) — [`run_command`]'s `match` arms stay one-liners, keeping the
/// function within `clippy::too_many_lines`.
#[expect(clippy::too_many_arguments, reason = "one param per CLI flag")]
fn run_compile_command(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
    dialect: Option<brink_compiler::Dialect>,
    types: Option<brink_compiler::TypePolicy>,
    deny: &[String],
    warn: &[String],
    allow: &[String],
    debug_info: bool,
) -> ExitCode {
    if let Err(e) = run_compile(input, output, dialect, types, deny, warn, allow, debug_info) {
        tracing::error!("{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[expect(clippy::too_many_arguments, reason = "one param per CLI flag")]
fn run_compile(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
    dialect: Option<brink_compiler::Dialect>,
    types: Option<brink_compiler::TypePolicy>,
    deny: &[String],
    warn: &[String],
    allow: &[String],
    debug_info: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (lints, deny_warnings) = lint_overrides::resolve_lint_overrides(deny, warn, allow);
    let output_result = compile_entry(input, dialect, types, lints, deny_warnings, debug_info)?;
    for w in &output_result.warnings {
        log_diagnostic(w);
    }
    let data = output_result.data;

    let out_ext = output
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or("inkt");

    if out_ext == "inkb" {
        let mut buf = Vec::new();
        brink_format::write_inkb(&data, &mut buf);
        if let Some(path) = output {
            std::fs::write(path, &buf)?;
            // The RESOLVED dialogue dialect beside the story (#3393, RULED
            // 2026-08-30): a derived product like the `.inkb` — never the
            // `brink.toml` source declaration — that an engine reads with
            // `@brink-lang/dialect`. Only when the project declares one.
            if let Some(json) = resolved_dialect_json(input)? {
                let sidecar = path.with_extension("dialect.json");
                std::fs::write(&sidecar, json)?;
                tracing::info!("wrote {}", sidecar.display());
            }
        } else {
            std::io::stdout().lock().write_all(&buf)?;
        }
    } else {
        let mut buf = String::new();
        brink_format::write_inkt(&data, &mut buf)?;
        if let Some(path) = output {
            std::fs::write(path, &buf)?;
        } else {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(buf.as_bytes())?;
            handle.write_all(b"\n")?;
        }
    }

    Ok(())
}

/// The project's resolved dialogue dialect as JSON (#3393): discover the
/// entry's `brink.toml` through the same source-tree road `compile_entry`
/// uses, resolve `[dialogue]` (preset merged, affix sugar expanded, the
/// file form read relative to the config), and serialize. `None` when no
/// `brink.toml` is found or it declares no `[dialogue]`.
///
/// # Errors
/// A `brink.toml` that parses but whose `[dialogue]` cannot resolve — the
/// resolver's own readable message, so a broken declaration fails the
/// compile loudly rather than shipping a story without its conventions.
fn resolved_dialect_json(
    entry: &std::path::Path,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let (root, _warnings) = brink_driver::native_source_root_with_warnings(entry);
    let tree = brink_driver::RealFs::new(&root);
    let entry_key = brink_driver::relative_key(&root, entry);
    resolved_dialect_json_in_tree(&tree, &entry_key)
}

/// [`resolved_dialect_json`] over any source tree — the testable half.
fn resolved_dialect_json_in_tree(
    tree: &impl brink_source_tree::SourceTree,
    entry_key: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Some(config_key) = brink_project_config::discover_from_entry_in_tree(tree, entry_key)?
    else {
        return Ok(None);
    };
    let text = brink_source_tree::SourceTree::read(tree, &config_key)?;
    let (config, _warnings) = brink_project_config::parse_str_at(config_key.clone(), &text)?;
    let Some(dialogue) = config.dialogue.as_ref() else {
        return Ok(None);
    };
    let config_dir = config_key.rfind('/').map(|i| config_key[..i].to_owned());
    let read_file = |path: &str| -> Option<String> {
        let candidates = match config_dir.as_deref() {
            Some(d) => vec![format!("{d}/{path}"), path.to_owned()],
            None => vec![path.to_owned()],
        };
        candidates
            .iter()
            .find_map(|key| brink_source_tree::SourceTree::read(tree, key).ok())
    };
    let dialect = brink_ide::dialect_config::resolve_dialogue_config(dialogue, &read_file)
        .map_err(|message| format!("brink.toml [dialogue]: {message}"))?;
    Ok(Some(serde_json::to_string_pretty(&dialect)?))
}

/// A linked program plus the per-file line tables `link` returns beside it
/// — what the debugger needs in hand to start a session.
type DebuggableProgram = (brink_runtime::Program, Vec<Vec<brink_format::LineEntry>>);

/// Load `input` and link it **with debug info**, for the debugger
/// (#3248). Source entries are recompiled with `debug_info: true`; a
/// prebuilt `.inkb`/`.inkt` is taken as-is, since whether it carries the
/// section was decided when it was built.
///
/// Returns the linked program rather than `StoryData` because every
/// debugger operation needs the `Program`'s resolvers — `resolve_source_line`
/// to bind a breakpoint, `resolve_debug_position`/`line_at` to say where it
/// stopped.
fn load_program_with_debug_info(
    input: &std::path::Path,
) -> Result<DebuggableProgram, Box<dyn std::error::Error>> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
    let data = if ext == "ink" || ext == "brink" {
        let out = compile_entry(
            input,
            None,
            None,
            std::collections::BTreeMap::new(),
            None,
            true,
        )?;
        for w in &out.warnings {
            log_diagnostic(w);
        }
        out.data
    } else {
        load_story_data(input)?
    };
    let linked = brink_runtime::link(&data)?;
    Ok(linked)
}

fn load_story_data(
    input: &std::path::Path,
) -> Result<brink_format::StoryData, Box<dyn std::error::Error>> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "ink" || ext == "brink" {
        // Raw .ink or .brink source — compile in-memory via the native
        // pipeline (no temp artifact written to disk; the `StoryData` goes
        // straight from `compile_entry` to the runtime, exactly like `brink
        // compile -o -` piped into `brink play` would, minus the round
        // trip), discovering + applying a `brink.toml` (#1005) just like
        // `brink compile` does. Every mount that compiles from source
        // (`brink convert`, `brink play`, `brink replay`, `brink
        // export-xliff`) reads the same file `brink compile` does, rather
        // than silently falling back to `AnalysisOptions::default()` and
        // rejecting extension syntax on a `dialect = "brink"` project.
        //
        // A `.brink` entry is routed through the exact same `compile_entry`
        // call as `.ink` (issue #1949) — `compile_entry` is already
        // extension-agnostic: `native_source_root_with_warnings` discovers
        // the project root + `brink.toml` from `entry`'s directory
        // regardless of `entry`'s own extension, and
        // `brink_environment::Project::load` dispatches on `entry`'s
        // extension internally (`collect_sources`) to compile the *whole*
        // native source tree under that root as one project (tree-is-
        // universe, unlike `.ink`'s `INCLUDE`-reachable set) — the same
        // discovery `brink compile scene.brink` already performs and that
        // the respell fixtures oracle-verify. `play`/`replay`/`convert`/
        // `export-xliff` on a `.brink` entry therefore get identical
        // project discovery to `compile`, not a standalone-file compile.
        let output_result = compile_entry(
            input,
            None,
            None,
            std::collections::BTreeMap::new(),
            None,
            false,
        )?;
        for w in &output_result.warnings {
            log_diagnostic(w);
        }
        Ok(output_result.data)
    } else if ext == "inkb" {
        let bytes = std::fs::read(input)?;
        Ok(brink_format::read_inkb(&bytes)?)
    } else if ext == "inkt" {
        let text = std::fs::read_to_string(input)?;
        Ok(brink_format::read_inkt(&text)?)
    } else {
        Err(format!(
            "unsupported story format: {} (expected .ink, .brink, .inkb, or .inkt; \
             .ink.json ingestion was retired — compile the .ink source instead)",
            input.display()
        )
        .into())
    }
}

/// `Commands::Convert`'s dispatch — same one-line-arm rationale as
/// [`run_compile_command`].
fn run_convert_command(input: &std::path::Path, output: Option<&std::path::Path>) -> ExitCode {
    if let Err(e) = run_convert(input, output) {
        tracing::error!("{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_convert(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = load_story_data(input)?;

    let out_ext = output
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or("inkt");

    if out_ext == "inkb" {
        let mut buf = Vec::new();
        brink_format::write_inkb(&data, &mut buf);
        if let Some(path) = output {
            std::fs::write(path, &buf)?;
        } else {
            std::io::stdout().lock().write_all(&buf)?;
        }
    } else {
        let mut buf = String::new();
        brink_format::write_inkt(&data, &mut buf)?;
        if let Some(path) = output {
            std::fs::write(path, &buf)?;
        } else {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(buf.as_bytes())?;
            handle.write_all(b"\n")?;
        }
    }

    Ok(())
}

fn run_export_xliff(
    input: &std::path::Path,
    src_lang: &str,
    trg_lang: Option<&str>,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    // For .inkb files, extract the checksum from the header.
    let (data, checksum) = if input.extension().and_then(|e| e.to_str()) == Some("inkb") {
        let bytes = std::fs::read(input)?;
        let index = brink_format::read_inkb_index(&bytes)?;
        let story = brink_format::read_inkb(&bytes)?;
        (story, index.checksum)
    } else {
        (load_story_data(input)?, 0)
    };

    let doc = brink_intl::generate_locale(&data, checksum, src_lang, trg_lang);
    let xml = xliff2::write::to_string(&doc)?;

    if let Some(path) = output {
        std::fs::write(path, &xml)?;
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(xml.as_bytes())?;
        handle.write_all(b"\n")?;
    }

    Ok(())
}

fn run_compile_locale(
    base: &std::path::Path,
    xliff: &std::path::Path,
    locale: &str,
    output: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_bytes = std::fs::read(base)?;
    let xliff_text = std::fs::read_to_string(xliff)?;
    let doc = xliff2::read::read_xliff(&xliff_text)?;
    let inkl_bytes = brink_intl::compile_locale_xliff(&base_bytes, &doc, locale)?;
    std::fs::write(output, &inkl_bytes)?;
    Ok(())
}

fn run_regenerate_xliff(
    base: &std::path::Path,
    existing: &std::path::Path,
    src_lang: &str,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_bytes = std::fs::read(base)?;
    let index = brink_format::read_inkb_index(&base_bytes)?;
    let data = brink_format::read_inkb(&base_bytes)?;

    let existing_text = std::fs::read_to_string(existing)?;
    let existing_doc = xliff2::read::read_xliff(&existing_text)?;

    let merged_doc = brink_intl::regenerate_locale(&data, index.checksum, src_lang, &existing_doc)?;
    let xml = xliff2::write::to_string(&merged_doc)?;

    if let Some(path) = output {
        std::fs::write(path, &xml)?;
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(xml.as_bytes())?;
        handle.write_all(b"\n")?;
    }

    Ok(())
}

fn run_migrate_xliff(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(input)?;
    let mut doc = xliff2::read::read_xliff(&text)?;

    for file in &doc.files {
        let has_scope_id = file
            .extensions
            .attributes
            .iter()
            .any(|a| a.namespace == "brink" && a.local_name == "scope-id");
        if !has_scope_id {
            tracing::warn!(
                "file {:?} has no brink:scope-id extension; migration falls back to \
                 file.id ({:?}) as the scope id, which will not match a freshly \
                 exported .xlf for the same scope",
                file.id,
                file.id
            );
        }
    }

    let changed = brink_intl::migrate_unit_ids(&mut doc)?;
    tracing::info!("migrated {changed} unit id(s)");
    let xml = xliff2::write::to_string(&doc)?;

    if let Some(path) = output {
        std::fs::write(path, &xml)?;
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(xml.as_bytes())?;
        handle.write_all(b"\n")?;
    }

    Ok(())
}

/// The formatter settings that apply to `path`, from the `brink.toml`
/// discovered by walking up from its directory (#3149).
///
/// Per file rather than once for the whole run: `brink fmt a/x.ink b/y.ink`
/// can legitimately span two projects, and formatting one of them with the
/// other's indent width would be a silent, whole-file diff.
///
/// Every failure here — no config found, unreadable, malformed — falls back
/// to [`brink_fmt::FormatConfig::default`], which is the shared
/// `DEFAULT_INDENT` rather than a default of the CLI's own. A malformed
/// `brink.toml` is reported by `brink check`, and refusing to format over
/// it would make the formatter the messenger for an unrelated problem.
fn format_config_for(path: &std::path::Path) -> brink_fmt::FormatConfig {
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let Some(config_path) = brink_project_config::find_config(dir) else {
        return brink_fmt::FormatConfig::default();
    };
    let Ok(text) = std::fs::read_to_string(&config_path) else {
        return brink_fmt::FormatConfig::default();
    };
    match brink_project_config::parse_str(&text) {
        Ok((config, _)) => brink_fmt::FormatConfig::from_project_config(&config),
        Err(_) => brink_fmt::FormatConfig::default(),
    }
}

fn run_fmt(files: &[PathBuf], check: bool, stdin: bool) -> Result<(), Box<dyn std::error::Error>> {
    if stdin {
        // Nothing to discover from: stdin has no path, so no project. The
        // shared default applies.
        let config = brink_fmt::FormatConfig::default();
        let mut source = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut source)?;
        let formatted = brink_fmt::format(&source, &config);
        std::io::Write::write_all(&mut std::io::stdout().lock(), formatted.as_bytes())?;
        return Ok(());
    }

    if files.is_empty() {
        return Err("no files specified; use --stdin to read from stdin".into());
    }

    let mut any_unformatted = false;

    for path in files {
        let config = format_config_for(path);
        let source = std::fs::read_to_string(path)?;
        let formatted = brink_fmt::format(&source, &config);

        if check {
            if formatted != source {
                tracing::error!("{}: not formatted", path.display());
                any_unformatted = true;
            }
        } else if formatted != source {
            std::fs::write(path, &formatted)?;
        }
    }

    if check && any_unformatted {
        return Err("some files are not formatted".into());
    }

    Ok(())
}

fn run_play(
    file: &std::path::Path,
    input: Option<&std::path::Path>,
    speed: u64,
    locale_paths: &[&std::path::Path],
    save_transcript: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = load_story_data(file)?;
    let (program, line_tables) = brink_runtime::link(&data)?;
    let program = std::sync::Arc::new(program);

    if let Some(input_path) = input {
        // Batch mode: read choices from a file
        let mut story = brink_runtime::Story::new(std::sync::Arc::clone(&program), line_tables);
        let file = std::fs::File::open(input_path)?;
        let reader = std::io::BufReader::new(file);
        batch::play_loop(&mut story, reader.lines(), false)?;
        if let Some(path) = save_transcript {
            save_transcript_file(&story, &program, path)?;
        }
    } else if std::io::stdin().is_terminal() {
        // Interactive TUI mode
        let char_delay_ms = 1000_u64.checked_div(speed).unwrap_or(0);

        // Auto-discover .inkl files next to the story if none were specified.
        let discovered: Vec<PathBuf>;
        let effective_locale_paths: Vec<&std::path::Path> = if locale_paths.is_empty() {
            discovered = discover_inkl_files(file);
            discovered.iter().map(PathBuf::as_path).collect()
        } else {
            locale_paths.to_vec()
        };

        let locales = tui::load_locales(&effective_locale_paths)?;
        let base_tables = line_tables;
        tui::run(
            &program,
            &base_tables,
            &locales,
            &tui::TuiConfig { char_delay_ms },
        )?;
    } else {
        // Batch mode: stdin is piped
        let mut story = brink_runtime::Story::new(std::sync::Arc::clone(&program), line_tables);
        let stdin = std::io::stdin();
        batch::play_loop(&mut story, stdin.lock().lines(), false)?;
        if let Some(path) = save_transcript {
            save_transcript_file(&story, &program, path)?;
        }
    }

    Ok(())
}

fn save_transcript_file(
    story: &brink_runtime::Story,
    program: &brink_runtime::Program,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = brink_runtime::transcript::write_transcript(
        story.transcript(),
        program.source_checksum(),
        story.fragments(),
    );
    std::fs::write(path, bytes)?;
    Ok(())
}

fn run_replay(
    transcript_path: &std::path::Path,
    story_path: &std::path::Path,
    locale_path: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = load_story_data(story_path)?;
    let (program, base_tables) = brink_runtime::link(&data)?;

    // Load and validate transcript
    let transcript_bytes = std::fs::read(transcript_path)?;
    let transcript_data = brink_runtime::transcript::read_transcript(&transcript_bytes)?;

    if transcript_data.source_checksum != program.source_checksum() {
        return Err(
            brink_runtime::transcript::TranscriptError::ChecksumMismatch {
                transcript: transcript_data.source_checksum,
                program: program.source_checksum(),
            }
            .into(),
        );
    }

    // Optionally apply locale
    let line_tables = if let Some(locale_file) = locale_path {
        let locale_bytes = std::fs::read(locale_file)?;
        let locale_data = brink_format::read_inkl(&locale_bytes)?;
        brink_runtime::apply_locale(
            &program,
            &locale_data,
            &base_tables,
            brink_runtime::LocaleMode::Overlay,
        )?
    } else {
        base_tables
    };

    // Re-render transcript
    let lines = brink_runtime::transcript::render_transcript(
        &transcript_data.parts,
        &program,
        &line_tables,
        None,
        &transcript_data.fragments,
    );

    let mut stdout = std::io::stdout().lock();
    for (i, (text, _tags)) in lines.iter().enumerate() {
        if i > 0 {
            writeln!(stdout)?;
        }
        write!(stdout, "{text}")?;
    }
    writeln!(stdout)?;
    stdout.flush()?;

    Ok(())
}

/// Find all `.inkl` files in the same directory as the story file.
fn discover_inkl_files(story_path: &std::path::Path) -> Vec<PathBuf> {
    let Some(dir) = story_path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("inkl"))
        .collect();
    paths.sort();
    paths
}

#[cfg(test)]
mod dialect_output_tests {
    use super::resolved_dialect_json_in_tree;
    use std::collections::BTreeMap;

    fn tree(files: &[(&str, &str)]) -> brink_source_tree::InMemory {
        brink_source_tree::InMemory::new(
            files
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn no_config_or_no_dialogue_means_no_sidecar() {
        let t = tree(&[("main.ink", "Hello.\n")]);
        assert!(
            resolved_dialect_json_in_tree(&t, "main.ink")
                .expect("ok")
                .is_none()
        );
        let t = tree(&[
            ("main.ink", "Hello.\n"),
            ("brink.toml", "[project]\nentry = \"main.ink\"\n"),
        ]);
        assert!(
            resolved_dialect_json_in_tree(&t, "main.ink")
                .expect("ok")
                .is_none()
        );
    }

    #[test]
    fn a_declared_dialogue_resolves_to_the_merged_artifact() {
        let t = tree(&[
            ("game/main.ink", "Hello.\n"),
            (
                "game/brink.toml",
                "[dialogue]\npreset = \"at-cue\"\n\n[[dialogue.elements]]\nkind = \"action\"\nprefix = \">\"\n",
            ),
        ]);
        let json = resolved_dialect_json_in_tree(&t, "game/main.ink")
            .expect("ok")
            .expect("declared");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        let kinds: Vec<&str> = v["elements"]
            .as_array()
            .expect("elements")
            .iter()
            .filter_map(|e| e["kind"].as_str())
            .collect();
        assert_eq!(kinds, ["character", "parenthetical", "dialogue", "action"]);
        // Sugar is EXPANDED in the derived product — an engine sees patterns.
        assert!(v["elements"][3]["emitted"]["pattern"].is_string(), "{json}");
    }

    #[test]
    fn a_broken_declaration_fails_the_compile_loudly() {
        let t = tree(&[
            ("main.ink", "Hello.\n"),
            ("brink.toml", "[dialogue]\npreset = \"fountain\"\n"),
        ]);
        let err = resolved_dialect_json_in_tree(&t, "main.ink").expect_err("refused");
        assert!(err.to_string().contains("unknown dialogue preset"), "{err}");
    }
}
