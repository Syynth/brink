use std::io::{BufRead, IsTerminal, Lines, Write as _};
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
    /// Convert between ink formats (.ink.json, .inkb, .inkt)
    Convert {
        /// Input file (.ink.json, .inkb, or .inkt)
        input: PathBuf,
        /// Output file (format inferred from extension, defaults to stdout as .inkt)
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
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Commands::Convert { input, output } => {
                if let Err(e) = run_convert(&input, output.as_deref()) {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            }
            Commands::Play { file, input } => {
                if let Err(e) = run_play(&file, input.as_deref()) {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    ExitCode::SUCCESS
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

fn run_play(
    file: &std::path::Path,
    input: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = load_story_data(file)?;
    let program = brink_runtime::link(&data)?;
    let mut story = brink_runtime::Story::new(&program);

    if let Some(input_path) = input {
        // Batch mode: read choices from a file
        let file = std::fs::File::open(input_path)?;
        let reader = std::io::BufReader::new(file);
        play_loop(&mut story, &program, reader.lines(), false)?;
    } else if std::io::stdin().is_terminal() {
        // Interactive mode: read choices from terminal
        let stdin = std::io::stdin();
        play_loop(&mut story, &program, stdin.lock().lines(), true)?;
    } else {
        // Batch mode: stdin is piped
        let stdin = std::io::stdin();
        play_loop(&mut story, &program, stdin.lock().lines(), false)?;
    }

    Ok(())
}

fn play_loop<B: BufRead>(
    story: &mut brink_runtime::Story,
    program: &brink_runtime::Program,
    mut lines: Lines<B>,
    interactive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();

    loop {
        match story.step(program)? {
            brink_runtime::StepResult::Done { text }
            | brink_runtime::StepResult::Ended { text } => {
                write!(stdout, "{text}")?;
                stdout.flush()?;
                break;
            }
            brink_runtime::StepResult::Choices { text, choices } => {
                write!(stdout, "{text}")?;

                for choice in &choices {
                    writeln!(stdout, "{}: {}", choice.index + 1, choice.text)?;
                }

                let idx = read_choice(&mut lines, choices.len(), interactive)?;
                story.choose(choices[idx].index)?;
            }
        }
    }

    Ok(())
}

fn read_choice<B: BufRead>(
    lines: &mut Lines<B>,
    num_choices: usize,
    interactive: bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut stderr = std::io::stderr().lock();
    let mut stdout = std::io::stdout().lock();

    loop {
        if interactive {
            write!(stdout, "?> ")?;
            stdout.flush()?;
        }

        let line = lines
            .next()
            .ok_or("unexpected end of input while waiting for choice")??;

        let trimmed = line.trim();

        let n: usize = if let Ok(n) = trimmed.parse() {
            n
        } else {
            if interactive {
                writeln!(stderr, "please enter a number between 1 and {num_choices}")?;
                continue;
            }
            return Err(format!("invalid choice input: {trimmed:?}").into());
        };

        if n < 1 || n > num_choices {
            if interactive {
                writeln!(stderr, "please enter a number between 1 and {num_choices}")?;
                continue;
            }
            return Err(format!("choice {n} out of range (1..={num_choices})").into());
        }

        return Ok(n - 1);
    }
}
