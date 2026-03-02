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
    /// Convert an ink.json file to .inkt or .inkb format
    Convert {
        /// Input file (.ink.json)
        input: PathBuf,
        /// Output file (format inferred from extension, defaults to stdout as .inkt)
        #[arg(short, long)]
        output: Option<PathBuf>,
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
        }
    }

    ExitCode::SUCCESS
}

fn run_convert(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let json_text = std::fs::read_to_string(input)?;
    let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(&json_text);

    let story: brink_json::InkJson = serde_json::from_str(json_text)?;
    let data = brink_converter::convert(&story)?;

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf)?;

    if let Some(path) = output {
        std::fs::write(path, &buf)?;
    } else {
        use std::io::Write as _;
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(buf.as_bytes())?;
        handle.write_all(b"\n")?;
    }

    Ok(())
}
