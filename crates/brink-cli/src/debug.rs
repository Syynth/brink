//! `brink debug` — step through a story from the terminal (issue #3248).
//!
//! Drives the **shared** verb set (`brink_runtime::debug_session`), the same
//! one the scripted test harness runs and the studio's `debug.*` commands
//! expose. One definition of "step over", three front-ends — which is the
//! point: three implementations would drift, and the harness's goldens
//! would then be asserting something the CLI does not do.
//!
//! Two modes. `--script` runs a `.dbg` file non-interactively and prints
//! the transcript, so the CLI is runnable in CI and shares fixtures with
//! the harness rather than growing a parallel format. Without it, a REPL.
//!
//! A few verbs are REPL-only conveniences rather than shared vocabulary:
//! `help`, `quit`, and `list` (show source around the current line). They
//! are the CLI's own because they are about the terminal, not about
//! debugger semantics — `list` in particular needs the source file, which
//! this process legitimately has open and the runtime deliberately does
//! not carry.

use std::io::{BufRead as _, Write};

use brink_runtime::debug_session::{Session, parse_script, run_script};

/// How many source lines either side of the current one `list` shows.
const LIST_CONTEXT: u32 = 3;

/// Run a debug session over `file`, either scripted or interactive.
///
/// # Errors
/// Compile/load failures, a malformed script, and IO failures on the
/// script file. A failed *expectation* inside a script is also an error:
/// a script is an assertion, and one that quietly did not hold would make
/// `brink debug --script` useless in CI.
pub fn run_debug(
    file: &std::path::Path,
    script: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (program, line_tables) = crate::load_program_with_debug_info(file)?;
    let mut session = Session::new(std::sync::Arc::new(program), line_tables);

    match script {
        Some(path) => {
            let text = std::fs::read_to_string(path)?;
            let commands = parse_script(&text).map_err(|e| e.to_string())?;
            // `run_script`'s error already carries the session so far, so a
            // failed expectation reads as a transcript ending in the
            // violation rather than a bare assert.
            let transcript = run_script(&mut session, &commands)?;
            write!(std::io::stdout(), "{transcript}")?;
            Ok(())
        }
        None => repl(&mut session, file),
    }
}

fn repl(session: &mut Session, file: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(file).unwrap_or_default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();

    writeln!(out, "brink debug — `help` for verbs, `quit` to leave")?;
    loop {
        write!(out, "(brink) ")?;
        out.flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            // EOF (piped input exhausted, or ^D) — leaving is the only
            // sensible reading, and it must not look like an error.
            writeln!(out)?;
            return Ok(());
        }
        let line = line.trim();
        match line {
            "" => continue,
            "quit" | "q" | "exit" => return Ok(()),
            "help" | "?" => {
                write!(out, "{HELP}")?;
                continue;
            }
            "list" | "l" => {
                list_around(&mut out, session, &source)?;
                continue;
            }
            _ => {}
        }

        // Everything else is the shared vocabulary. Parsing one line at a
        // time is exactly `parse_script` on a one-line script, so the REPL
        // cannot drift from the scripted form.
        let before = session.transcript().len();
        match parse_script(line) {
            Err(e) => writeln!(out, "{e}")?,
            Ok(commands) => match run_script(session, &commands) {
                Ok(_) => {
                    let delta = &session.transcript()[before..];
                    write!(out, "{delta}")?;
                }
                Err(message) => {
                    // The session-so-far is already on screen; show only
                    // the failure itself rather than reprinting it.
                    let tail = message.rsplit('\n').next().unwrap_or(&message);
                    writeln!(out, "{tail}")?;
                }
            },
        }
    }
}

/// Print the source around wherever the flow is stopped.
fn list_around(
    out: &mut impl Write,
    session: &Session,
    source: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some((_, line)) = session.current_position() else {
        writeln!(out, "no source position (not started, terminal, or parked)")?;
        return Ok(());
    };
    let lines: Vec<&str> = source.lines().collect();
    let first = line.saturating_sub(LIST_CONTEXT).max(1);
    let last = (line + LIST_CONTEXT).min(u32::try_from(lines.len()).unwrap_or(u32::MAX));
    for n in first..=last {
        let Some(text) = lines.get((n - 1) as usize) else {
            continue;
        };
        // The stopped line is marked, so `list` answers "where am I" and
        // not merely "what does the file say".
        let marker = if n == line { "->" } else { "  " };
        writeln!(out, "{marker} {n:>4} {text}")?;
    }
    Ok(())
}

const HELP: &str = "\
  break <file>:<line>   arm a breakpoint (1-based lines)
  run, continue         advance to the next breakpoint/choice/terminal
  step into|over|out    advance one SOURCE LINE
  next                  same as `step over`
  stepi into|over|out   advance one VM INSTRUCTION
  locals                named locals in the innermost frame
  stack                 call stack, innermost first
  list, l               source around the current line
  help, ?               this
  quit, q               leave
";
