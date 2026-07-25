mod batch;
mod ide;
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
    },
    /// Convert between ink formats (.inkb, .inkt)
    Convert {
        /// Input file (.ink, .inkb, or .inkt)
        input: PathBuf,
        /// Output file (format inferred from extension, defaults to stdout as .inkt)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export line tables from a compiled story as XLIFF 2.0
    ExportXliff {
        /// Input story file (.inkb, .ink, or .inkt)
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
        /// Story file (.ink source, .inkb, or .inkt)
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
    /// Re-render a saved transcript against a story (optionally with a locale)
    Replay {
        /// Transcript file (.brkt)
        transcript: PathBuf,
        /// Story file (.ink source, .inkb, or .inkt)
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
        } => {
            return run_compile_command(
                &input,
                output.as_deref(),
                dialect.map(Into::into),
                types.map(Into::into),
                &deny,
                &warn,
                &allow,
            );
        }
        Commands::Convert { input, output } => {
            return run_convert_command(&input, output.as_deref());
        }
        Commands::ExportXliff {
            input,
            src_lang,
            trg_lang,
            output,
        } => {
            if let Err(e) =
                run_export_xliff(&input, &src_lang, trg_lang.as_deref(), output.as_deref())
            {
                tracing::error!("{e}");
                return ExitCode::FAILURE;
            }
        }
        Commands::CompileLocale {
            base,
            xliff,
            locale,
            output,
        } => {
            if let Err(e) = run_compile_locale(&base, &xliff, &locale, &output) {
                tracing::error!("{e}");
                return ExitCode::FAILURE;
            }
        }
        Commands::RegenerateXliff {
            base,
            existing,
            src_lang,
            output,
        } => {
            if let Err(e) = run_regenerate_xliff(&base, &existing, &src_lang, output.as_deref()) {
                tracing::error!("{e}");
                return ExitCode::FAILURE;
            }
        }
        Commands::Fmt {
            files,
            check,
            stdin,
        } => {
            if let Err(e) = run_fmt(&files, check, stdin) {
                tracing::error!("{e}");
                return ExitCode::FAILURE;
            }
        }
        Commands::Play {
            file,
            input,
            speed,
            locale,
            save_transcript,
        } => {
            let locale_refs: Vec<&std::path::Path> = locale.iter().map(PathBuf::as_path).collect();
            if let Err(e) = run_play(
                &file,
                input.as_deref(),
                speed,
                &locale_refs,
                save_transcript.as_deref(),
            ) {
                tracing::error!("{e}");
                return ExitCode::FAILURE;
            }
        }
        Commands::Replay {
            transcript,
            story,
            locale,
        } => {
            if let Err(e) = run_replay(&transcript, &story, locale.as_deref()) {
                tracing::error!("{e}");
                return ExitCode::FAILURE;
            }
        }
        Commands::Ide { command } => return ide::run(&command),
    }

    ExitCode::SUCCESS
}

/// Build the #1306 [`Environment`](brink_environment::Environment) for `entry`
/// and run the pure compile over it — `Project::load` → `compile(&env)`, the
/// one path every `brink compile`/`convert`/`play`/`replay`/`export-xliff`
/// invocation flows through now.
///
/// The CLI mounts a [`RealFs::project`](brink_driver::RealFs::project) tree
/// rooted at [`native_source_root`] — a lazy real-filesystem `SourceTree`, not
/// a whole-tree eager drain (issue #1357): `list` enumerates `.brink`/`.ink`/
/// `brink.toml` keys by stat alone, and `read` serves any one of them off
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
) -> Result<brink_compiler::CompileOutput, Box<dyn std::error::Error>> {
    let root = brink_driver::native_source_root(entry);
    let tree = brink_driver::RealFs::project(&root);
    let entry_key = brink_driver::relative_key(&root, entry);
    let overrides = brink_environment::OptionOverrides {
        dialect,
        types,
        lints,
        deny_warnings,
    };
    let env = brink_environment::Project::load(&tree, &entry_key, &overrides)?;
    Ok(brink_environment::compile(&env)?)
}

/// Resolve `brink compile`'s repeatable `--deny`/`--warn`/`--allow <CODE>`
/// flags into the per-code override map [`compile_entry`] threads through to
/// [`brink_environment::OptionOverrides::lints`] (issue #1373). `--deny
/// warnings` (short form `-D warnings`, mirroring rustc's own `-D warnings`)
/// is special-cased as `deny-warnings` rather than a per-code override,
/// since `"warnings"` is never a real `DiagnosticCode` — every other value
/// is validated downstream, at the one resolution point
/// (`AnalysisOptions::apply_lint_overrides`), not here.
///
/// A code repeated across more than one of `--deny`/`--warn`/`--allow`
/// resolves to whichever flag is applied last, in `deny`, `warn`, `allow`
/// order below — a user passing the same code to more than one flag has
/// already made a contradictory request; this is deliberately simple rather
/// than rejecting it outright.
fn resolve_lint_overrides(
    deny: &[String],
    warn: &[String],
    allow: &[String],
) -> (
    std::collections::BTreeMap<String, brink_driver::LintLevel>,
    Option<bool>,
) {
    let mut lints = std::collections::BTreeMap::new();
    let mut deny_warnings = None;
    for code in deny {
        if code == "warnings" {
            deny_warnings = Some(true);
        } else {
            lints.insert(code.clone(), brink_driver::LintLevel::Deny);
        }
    }
    for code in warn {
        lints.insert(code.clone(), brink_driver::LintLevel::Warn);
    }
    for code in allow {
        lints.insert(code.clone(), brink_driver::LintLevel::Allow);
    }
    (lints, deny_warnings)
}

/// `Commands::Compile`'s dispatch, factored out of [`run_command`] (matching
/// the `Commands::Ide => return ide::run(&command)` shape already used
/// there) — [`run_command`]'s `match` arms stay one-liners, keeping the
/// function within `clippy::too_many_lines`.
fn run_compile_command(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
    dialect: Option<brink_compiler::Dialect>,
    types: Option<brink_compiler::TypePolicy>,
    deny: &[String],
    warn: &[String],
    allow: &[String],
) -> ExitCode {
    if let Err(e) = run_compile(input, output, dialect, types, deny, warn, allow) {
        tracing::error!("{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_compile(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
    dialect: Option<brink_compiler::Dialect>,
    types: Option<brink_compiler::TypePolicy>,
    deny: &[String],
    warn: &[String],
    allow: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let (lints, deny_warnings) = resolve_lint_overrides(deny, warn, allow);
    let output_result = compile_entry(input, dialect, types, lints, deny_warnings)?;
    for w in &output_result.warnings {
        tracing::warn!("[{}] {}", w.code.as_str(), w.message);
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

fn load_story_data(
    input: &std::path::Path,
) -> Result<brink_format::StoryData, Box<dyn std::error::Error>> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "ink" {
        // Raw .ink source — compile in-memory via the native pipeline,
        // discovering + applying a `brink.toml` (#1005) just like `brink
        // compile` does. Every mount that compiles from source (`brink
        // convert`, `brink play`, `brink replay`, `brink export-xliff`) reads
        // the same file `brink compile` does, rather than silently falling
        // back to `AnalysisOptions::default()` and rejecting extension
        // syntax on a `dialect = "brink"` project.
        let output_result =
            compile_entry(input, None, None, std::collections::BTreeMap::new(), None)?;
        for w in &output_result.warnings {
            tracing::warn!("[{}] {}", w.code.as_str(), w.message);
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
            "unsupported story format: {} (expected .ink, .inkb, or .inkt; \
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

fn run_fmt(files: &[PathBuf], check: bool, stdin: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = brink_fmt::FormatConfig::default();

    if stdin {
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
