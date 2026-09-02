use std::io::{BufRead, Lines, Write as _};

pub fn play_loop<B: BufRead>(
    story: &mut brink_runtime::Story,
    mut lines: Lines<B>,
    interactive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();

    loop {
        let step = story.continue_single()?;
        // Issue #3354: non-fatal runtime warnings (today: a `~ temp` read
        // before its declaration ran) go to stderr, drained after every
        // step so they land next to the line that raised them without
        // interleaving into the story text on stdout — the same separation
        // the C# reference's own `RUNTIME WARNING` line has from story
        // output.
        report_runtime_warnings(story)?;
        match step {
            brink_runtime::Step::Line(line) => {
                write!(stdout, "{}", line.text)?;
            }
            // A park (`Step::Suspended`, FS-3r) is a terminal turn boundary;
            // runtime-unreachable today behind the E052 fence, grouped with
            // the other terminals so the exhaustive match keeps compiling.
            // Terminals carry no text of their own — any trailing content
            // already arrived as its own preceding `Step::Line`.
            brink_runtime::Step::Done
            | brink_runtime::Step::End
            | brink_runtime::Step::Suspended => {
                stdout.flush()?;
                break;
            }
            brink_runtime::Step::Choices(choices) => {
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

/// Drain and print every [`brink_runtime::RuntimeWarning`] the story has
/// raised since the last call (issue #3354).
fn report_runtime_warnings(
    story: &mut brink_runtime::Story,
) -> Result<(), Box<dyn std::error::Error>> {
    let warnings = story.take_runtime_warnings();
    if warnings.is_empty() {
        return Ok(());
    }
    let mut stderr = std::io::stderr().lock();
    for warning in warnings {
        writeln!(stderr, "RUNTIME WARNING: {warning}")?;
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
