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
    /// Export line tables from a compiled story to lines.json
    ExportLines {
        /// Input story file (.inkb, .inkt, or .ink.json)
        input: PathBuf,
        /// Output lines.json file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compile translated lines.json into a .inkl locale overlay
    CompileLocale {
        /// Base .inkb file
        #[arg(long)]
        base: PathBuf,
        /// Translated lines.json file
        #[arg(long)]
        lines: PathBuf,
        /// BCP 47 locale tag (e.g. "es", "ja")
        #[arg(long)]
        locale: String,
        /// Output .inkl file
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Regenerate lines.json preserving existing translations after recompilation
    RegenerateLines {
        /// Recompiled .inkb file
        #[arg(long)]
        base: PathBuf,
        /// Existing translated lines.json
        #[arg(long)]
        existing: PathBuf,
        /// Output updated lines.json (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
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
            Commands::ExportLines { input, output } => {
                if let Err(e) = run_export_lines(&input, output.as_deref()) {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            }
            Commands::CompileLocale {
                base,
                lines,
                locale,
                output,
            } => {
                if let Err(e) = run_compile_locale(&base, &lines, &locale, &output) {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            }
            Commands::RegenerateLines {
                base,
                existing,
                output,
            } => {
                if let Err(e) = run_regenerate_lines(&base, &existing, output.as_deref()) {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            }
            Commands::Play { file, input, speed } => {
                if let Err(e) = run_play(&file, input.as_deref(), speed) {
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
    let data = brink_compiler::compile_path(input)?;

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

fn run_export_lines(
    input: &std::path::Path,
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

    let lines_json = brink_intl::export_lines(&data, checksum);
    let json = serde_json::to_string_pretty(&lines_json)?;

    if let Some(path) = output {
        std::fs::write(path, &json)?;
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(json.as_bytes())?;
        handle.write_all(b"\n")?;
    }

    Ok(())
}

fn run_compile_locale(
    base: &std::path::Path,
    lines: &std::path::Path,
    locale: &str,
    output: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_bytes = std::fs::read(base)?;
    let lines_text = std::fs::read_to_string(lines)?;
    let lines_json: brink_intl::LinesJson = serde_json::from_str(&lines_text)?;
    let inkl_bytes = brink_intl::compile_locale(&base_bytes, &lines_json, locale)?;
    std::fs::write(output, &inkl_bytes)?;
    Ok(())
}

fn run_regenerate_lines(
    base: &std::path::Path,
    existing: &std::path::Path,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Export fresh lines from the recompiled .inkb.
    let base_bytes = std::fs::read(base)?;
    let index = brink_format::read_inkb_index(&base_bytes)?;
    let data = brink_format::read_inkb(&base_bytes)?;
    let new_export = brink_intl::export_lines(&data, index.checksum);

    // Read the existing translated lines.json.
    let existing_text = std::fs::read_to_string(existing)?;
    let existing_lines: brink_intl::LinesJson = serde_json::from_str(&existing_text)?;

    // Regenerate, preserving translations.
    let merged = brink_intl::regenerate_lines(&new_export, &existing_lines);
    let json = serde_json::to_string_pretty(&merged)?;

    if let Some(path) = output {
        std::fs::write(path, &json)?;
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(json.as_bytes())?;
        handle.write_all(b"\n")?;
    }

    Ok(())
}

fn run_play(
    file: &std::path::Path,
    input: Option<&std::path::Path>,
    speed: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = load_story_data(file)?;
    let program = brink_runtime::link(&data)?;
    let mut story = brink_runtime::Story::new(&program);

    if let Some(input_path) = input {
        // Batch mode: read choices from a file
        let file = std::fs::File::open(input_path)?;
        let reader = std::io::BufReader::new(file);
        batch::play_loop(&mut story, reader.lines(), false)?;
    } else if std::io::stdin().is_terminal() {
        // Interactive TUI mode
        let char_delay_ms = if speed == 0 { 0 } else { 1000 / speed };
        tui::run(&mut story, tui::TuiConfig { char_delay_ms })?;
    } else {
        // Batch mode: stdin is piped
        let stdin = std::io::stdin();
        batch::play_loop(&mut story, stdin.lock().lines(), false)?;
    }

    Ok(())
}
