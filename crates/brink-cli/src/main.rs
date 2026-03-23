mod batch;
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

#[derive(Subcommand)]
enum Commands {
    /// Compile an .ink story (native pipeline)
    Compile {
        /// Entry-point .ink file
        input: PathBuf,
        /// Output file (format inferred from extension, defaults to stdout as .inkt)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Convert between ink formats (.ink.json, .inkb, .inkt)
    Convert {
        /// Input file (.ink.json, .inkb, or .inkt)
        input: PathBuf,
        /// Output file (format inferred from extension, defaults to stdout as .inkt)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export line tables from a compiled story as XLIFF 2.0
    ExportXliff {
        /// Input story file (.inkb, .ink.json, or .inkt)
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
        /// Story file (.ink.json, .inkb, or .inkt)
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
        /// Story file (.ink.json, .inkb, or .inkt)
        #[arg(short, long)]
        story: PathBuf,
        /// Locale overlay file (.inkl) to apply before rendering
        #[arg(long)]
        locale: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Commands::Compile { input, output } => {
                if let Err(e) = run_compile(&input, output.as_deref()) {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            }
            Commands::Convert { input, output } => {
                if let Err(e) = run_convert(&input, output.as_deref()) {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
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
                if let Err(e) = run_regenerate_xliff(&base, &existing, &src_lang, output.as_deref())
                {
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
                let locale_refs: Vec<&std::path::Path> =
                    locale.iter().map(PathBuf::as_path).collect();
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
        }
    }

    ExitCode::SUCCESS
}

fn run_compile(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_result = brink_compiler::compile_path(input)?;
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
    if ext == "inkb" {
        let bytes = std::fs::read(input)?;
        Ok(brink_format::read_inkb(&bytes)?)
    } else if ext == "inkt" {
        let text = std::fs::read_to_string(input)?;
        Ok(brink_format::read_inkt(&text)?)
    } else {
        // Assume ink.json
        let json_text = std::fs::read_to_string(input)?;
        let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(&json_text);
        let story: brink_json::InkJson = serde_json::from_str(json_text)?;
        Ok(brink_converter::convert(&story)?)
    }
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

    if let Some(input_path) = input {
        // Batch mode: read choices from a file
        let mut story = brink_runtime::Story::new(&program, line_tables);
        let file = std::fs::File::open(input_path)?;
        let reader = std::io::BufReader::new(file);
        batch::play_loop(&mut story, reader.lines(), false)?;
        if let Some(path) = save_transcript {
            save_transcript_file(&story, &program, path)?;
        }
    } else if std::io::stdin().is_terminal() {
        // Interactive TUI mode
        let char_delay_ms = if speed == 0 { 0 } else { 1000 / speed };

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
        let mut story = brink_runtime::Story::new(&program, line_tables);
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
    let (parts, source_checksum, fragments) =
        brink_runtime::transcript::read_transcript(&transcript_bytes)?;

    if source_checksum != program.source_checksum() {
        return Err(
            brink_runtime::transcript::TranscriptError::ChecksumMismatch {
                transcript: source_checksum,
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
        &parts,
        &program,
        &line_tables,
        None,
        &fragments,
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
