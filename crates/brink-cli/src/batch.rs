use std::io::{BufRead, Lines, Write as _};

pub fn play_loop<B: BufRead>(
    story: &mut brink_runtime::Story,
    mut lines: Lines<B>,
    interactive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();

    loop {
        match story.continue_maximally()? {
            brink_runtime::StepResult::Done { text, .. }
            | brink_runtime::StepResult::Ended { text, .. } => {
                write!(stdout, "{text}")?;
                stdout.flush()?;
                break;
            }
            brink_runtime::StepResult::Choices { text, choices, .. } => {
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
