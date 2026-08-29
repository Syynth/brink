//! Scripted debug sessions (issue #3247): a fixture, a list of actions a
//! person would take in a debugger, and a transcript of what happened.
//!
//! **Why this exists.** Debugger semantics are otherwise defined only by
//! Rust unit tests written alongside the code they test — so a refactor
//! that quietly changes what `step over` does at a choice point *passes*,
//! because the test gets updated to match the new behaviour. There is no
//! artifact saying what stepping is supposed to do. A scripted transcript
//! makes the behaviour itself the artifact.
//!
//! **Source level, never bytecode.** Every assertion and every transcript
//! line is expressed in source terms — `main.ink:2`, a local's name and
//! value, a stack of frame names. Bytecode offsets are deliberately absent:
//! they churn on every codegen change, so goldens written against them
//! would break constantly and teach everyone to re-accept snapshots without
//! reading them, which is strictly worse than having no goldens because it
//! launders real regressions through a habit.
//!
//! **Two granularities, both first-class** (RULED 2026-08-28). `stepi` is
//! VM-instruction stepping; `step` is line stepping. Neither is a
//! second-class wrapper over the other: the studio will present the
//! `.inkt` disassembly and the source side by side, so an author can watch
//! a line and the instructions it became at the same time. GDB's vocabulary
//! is borrowed on purpose — `stepi`/`nexti` for instructions, `step`/`next`
//! for lines — because it is the convention every debugger user already
//! has.
//!
//! Both verbs exist as of #3264: `stepi into|over|out` for instructions,
//! `step into|over|out` (and `next`, GDB's spelling of `step over`) for
//! lines. Instruction stepping was spelled `stepi` from the start
//! precisely so that adding `step` could not silently change what an
//! existing golden meant.
//!
//! **Lines are 1-based in scripts.** A script is a thing a person writes,
//! and `main.ink:2` means what every editor means by line 2. The engine is
//! 0-based (`Program::resolve_source_line`); the conversion happens here,
//! at the one edge that faces a human.
//!
//! The verb set is intended to be shared with the CLI debugger (#3248) so
//! there is one definition of "step over" rather than three. When that
//! lands, this module is what it should be extracted from — it lives in
//! the test harness for now only because creating a crate purely to hold
//! it, before a second consumer exists, would add a publishable-crate to
//! the workspace for no present benefit.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use brink_runtime::{
    BreakpointSet, DEFAULT_DEBUG_BUDGET, DebugStopReason, DebugValue, FastRng, Program, StepMode,
    Story,
};

/// One action or assertion from a `.dbg` script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `break <file>:<line>` — arm a breakpoint. Line is 1-based.
    Break {
        file: String,
        line: u32,
    },
    /// `run` / `continue` — advance to the next breakpoint, choice point,
    /// or terminal outcome.
    Run,
    /// `stepi into|over|out` — one VM instruction.
    StepInstruction(StepMode),
    /// `step into|over|out` / `next` — one source line (#3264).
    StepLine(StepMode),
    /// `locals` / `stack` — record the current frame's state in the
    /// transcript without asserting anything.
    Locals,
    Stack,
    /// `expect-line <n>` (1-based).
    ExpectLine(u32),
    /// `expect-local <name> = <value>`.
    ExpectLocal {
        name: String,
        value: String,
    },
    /// `expect-stack a > b > c`.
    ExpectStack(Vec<String>),
    /// `expect-terminal` — the last action ended the story.
    ExpectTerminal,
}

/// A script failed to parse. Carries the 1-based line number so a broken
/// fixture points at itself.
#[derive(Debug)]
pub struct ScriptError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "script line {}: {}", self.line, self.message)
    }
}

/// Parse a `.dbg` script. Line-oriented; `#` starts a comment; blank lines
/// are skipped.
///
/// # Errors
/// Returns [`ScriptError`] naming the offending line for an unknown verb or
/// a malformed argument. Unknown verbs are an error rather than a skip: a
/// silently-ignored line is a script that appears to test something it does
/// not.
pub fn parse_script(text: &str) -> Result<Vec<Command>, ScriptError> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let lineno = i + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let err = |message: String| ScriptError {
            line: lineno,
            message,
        };
        let (verb, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        let cmd = match verb {
            "break" => {
                let (file, line_s) = rest
                    .rsplit_once(':')
                    .ok_or_else(|| err(format!("expected `break <file>:<line>`, got {rest:?}")))?;
                let n: u32 = line_s.trim().parse().map_err(|_| {
                    err(format!(
                        "line number must be a positive integer: {line_s:?}"
                    ))
                })?;
                if n == 0 {
                    return Err(err(
                        "line numbers in scripts are 1-based; 0 is not a line".into()
                    ));
                }
                Command::Break {
                    file: file.trim().to_string(),
                    line: n,
                }
            }
            "run" | "continue" => Command::Run,
            "stepi" => match rest {
                "into" => Command::StepInstruction(StepMode::Into),
                "over" => Command::StepInstruction(StepMode::Over),
                "out" => Command::StepInstruction(StepMode::Out),
                other => return Err(err(format!("stepi takes into|over|out, got {other:?}"))),
            },
            // Line granularity (#3264). `next` is GDB's spelling of
            // `step over`, accepted as the alias every debugger user
            // already types.
            "step" => match rest {
                "into" => Command::StepLine(StepMode::Into),
                "over" => Command::StepLine(StepMode::Over),
                "out" => Command::StepLine(StepMode::Out),
                other => return Err(err(format!("step takes into|over|out, got {other:?}"))),
            },
            "next" => Command::StepLine(StepMode::Over),
            "locals" => Command::Locals,
            "stack" => Command::Stack,
            "expect-line" => Command::ExpectLine(
                rest.parse()
                    .map_err(|_| err(format!("expect-line takes a 1-based line, got {rest:?}")))?,
            ),
            "expect-local" => {
                let (name, value) = rest.split_once('=').ok_or_else(|| {
                    err(format!(
                        "expected `expect-local <name> = <value>`, got {rest:?}"
                    ))
                })?;
                Command::ExpectLocal {
                    name: name.trim().to_string(),
                    value: value.trim().to_string(),
                }
            }
            "expect-stack" => Command::ExpectStack(
                rest.split('>')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            ),
            "expect-terminal" => Command::ExpectTerminal,
            other => return Err(err(format!("unknown verb {other:?}"))),
        };
        out.push(cmd);
    }
    Ok(out)
}

/// Render a [`DebugValue`] compactly for a transcript. Structured kinds
/// keep their structure — a locals panel that can only say `"[list]"` is
/// not the target, and neither is a golden that records one.
fn render(value: &DebugValue) -> String {
    match value {
        DebugValue::Int(i) => i.to_string(),
        DebugValue::Float(f) => format!("{f}"),
        DebugValue::Bool(b) => b.to_string(),
        DebugValue::Str(s) => format!("{s:?}"),
        DebugValue::Null => "null".to_string(),
        DebugValue::List(items) => format!("[{}]", items.join(", ")),
        DebugValue::DivertTarget(t) => format!("-> {}", t.as_deref().unwrap_or("?")),
        DebugValue::Struct { name, fields } => {
            let inner: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", render(v)))
                .collect();
            format!(
                "{} {{ {} }}",
                name.as_deref().unwrap_or("?"),
                inner.join(", ")
            )
        }
        DebugValue::Handle { kind, id } => format!("<{kind} #{id}>"),
        DebugValue::Other(s) => s.clone(),
    }
}

/// A driven debug session: the story, its breakpoints, and the sources
/// needed to report positions in source terms.
pub struct Session {
    story: Story<FastRng>,
    program: std::sync::Arc<Program>,
    breakpoints: BreakpointSet,
    /// Source text per file, used only to turn a resolved byte offset back
    /// into a line for the transcript. The harness legitimately holds these
    /// — it compiled them — which is why this does not need the engine to
    /// carry a byte→line index.
    sources: BTreeMap<String, String>,
    transcript: String,
    last_reason: Option<DebugStopReason>,
}

impl Session {
    #[must_use]
    pub fn new(
        program: std::sync::Arc<Program>,
        line_tables: Vec<Vec<brink_format::LineEntry>>,
        sources: BTreeMap<String, String>,
    ) -> Self {
        let story = Story::<FastRng>::new(std::sync::Arc::clone(&program), line_tables);
        Self {
            story,
            program,
            breakpoints: BreakpointSet::new(),
            sources,
            transcript: String::new(),
            last_reason: None,
        }
    }

    /// The 1-based line the flow is currently stopped on, or `None` when
    /// there is no position (terminal, or parked with nothing to point at).
    fn current_line(&self) -> Option<(String, u32)> {
        let pos = self.story.debug_snapshot().position?;
        let loc = self.program.resolve_debug_position(pos)?;
        let file = loc.file?;
        let src = self.sources.get(&file)?;
        let upto = src.get(..loc.range_start as usize)?;
        let line = u32::try_from(upto.matches('\n').count()).ok()? + 1;
        Some((file, line))
    }

    fn frame_names(&self) -> Vec<String> {
        self.story
            .debug_snapshot()
            .call_stack
            .iter()
            .rev()
            .filter_map(|f| f.location.clone())
            .collect()
    }

    fn note_position(&mut self) {
        match self.current_line() {
            Some((file, line)) => {
                let _ = writeln!(self.transcript, "  at {file}:{line}");
            }
            None => {
                let _ = writeln!(self.transcript, "  at <no source position>");
            }
        }
    }
}

/// Run a parsed script, returning the transcript.
///
/// # Errors
/// Returns the assertion message for the first `expect-*` that does not
/// hold, or a runtime/breakpoint-binding failure. The transcript up to that
/// point is included so a failure reads as a session, not a bare assert.
pub fn run_script(session: &mut Session, script: &[Command]) -> Result<String, String> {
    for cmd in script {
        match cmd {
            Command::Break { .. }
            | Command::Run
            | Command::StepInstruction(_)
            | Command::StepLine(_)
            | Command::Locals
            | Command::Stack => apply_action(session, cmd)?,
            Command::ExpectLine(_)
            | Command::ExpectLocal { .. }
            | Command::ExpectStack(_)
            | Command::ExpectTerminal => apply_expectation(session, cmd)?,
        }
    }
    Ok(session.transcript.clone())
}

/// The verbs that move the session: break, run, step, and the two that
/// only record state.
fn apply_action(session: &mut Session, cmd: &Command) -> Result<(), String> {
    match cmd {
        Command::Break { file, line } => {
            // Scripts are 1-based; the engine is 0-based.
            let position = session
                .program
                .resolve_source_line(file, line.saturating_sub(1))
                .ok_or_else(|| {
                    format!(
                        "{}\nbreak {file}:{line} bound to nothing — that line has no \
                         executable code (a comment, a blank, or code that folded away). \
                         A breakpoint that can never hit is worse than none, so this is an \
                         error rather than a silent no-op.",
                        session.transcript
                    )
                })?;
            session.breakpoints.insert(
                position.container_idx,
                position.offset,
                format!("{file}:{line}"),
            );
            let _ = writeln!(session.transcript, "break {file}:{line}");
        }
        Command::Run => {
            let outcome = session
                .story
                .debug_run(&session.breakpoints, DEFAULT_DEBUG_BUDGET)
                .map_err(|e| format!("{}\nrun failed: {e:?}", session.transcript))?;
            let _ = writeln!(session.transcript, "run -> {}", describe(&outcome.reason));
            session.last_reason = Some(outcome.reason);
            session.note_position();
        }
        Command::StepInstruction(mode) => {
            let outcome = session
                .story
                .debug_step(*mode, &session.breakpoints, DEFAULT_DEBUG_BUDGET)
                .map_err(|e| format!("{}\nstepi failed: {e:?}", session.transcript))?;
            let _ = writeln!(
                session.transcript,
                "stepi {} -> {}",
                match mode {
                    StepMode::Into => "into",
                    StepMode::Over => "over",
                    StepMode::Out => "out",
                },
                describe(&outcome.reason)
            );
            session.last_reason = Some(outcome.reason);
            session.note_position();
        }
        Command::StepLine(mode) => {
            let outcome = session
                .story
                .debug_step_line(*mode, &session.breakpoints, DEFAULT_DEBUG_BUDGET)
                .map_err(|e| format!("{}\nstep failed: {e:?}", session.transcript))?;
            let _ = writeln!(
                session.transcript,
                "step {} -> {}",
                match mode {
                    StepMode::Into => "into",
                    StepMode::Over => "over",
                    StepMode::Out => "out",
                },
                describe(&outcome.reason)
            );
            session.last_reason = Some(outcome.reason);
            session.note_position();
        }
        Command::Locals => {
            let snap = session.story.debug_snapshot();
            let locals = snap.call_stack.first().and_then(|f| f.locals.as_ref());
            match locals {
                Some(ls) if !ls.is_empty() => {
                    let _ = writeln!(session.transcript, "locals");
                    for l in ls {
                        let _ = writeln!(session.transcript, "  {} = {}", l.name, render(&l.value));
                    }
                }
                Some(_) => {
                    let _ = writeln!(session.transcript, "locals (none in scope)");
                }
                None => {
                    let _ = writeln!(
                        session.transcript,
                        "locals <unavailable: compiled without debug info>"
                    );
                }
            }
        }
        Command::Stack => {
            let _ = writeln!(session.transcript, "stack");
            for name in session.frame_names() {
                let _ = writeln!(session.transcript, "  {name}");
            }
        }
        _ => unreachable!("apply_action only handles action verbs"),
    }
    Ok(())
}

/// The `expect-*` verbs. Each records itself in the transcript and then
/// fails with the session so far, so a violated expectation reads as a
/// session rather than a bare assert.
fn apply_expectation(session: &mut Session, cmd: &Command) -> Result<(), String> {
    match cmd {
        Command::ExpectLine(want) => {
            let got = session.current_line();
            let _ = writeln!(session.transcript, "expect-line {want}");
            match got {
                Some((_, line)) if line == *want => {}
                Some((file, line)) => {
                    return Err(format!(
                        "{}\nexpected to be stopped on line {want}, but the flow is at \
                         {file}:{line}",
                        session.transcript
                    ));
                }
                None => {
                    return Err(format!(
                        "{}\nexpected to be stopped on line {want}, but the flow has no source \
                         position (terminal, or parked)",
                        session.transcript
                    ));
                }
            }
        }
        Command::ExpectLocal { name, value } => {
            let _ = writeln!(session.transcript, "expect-local {name} = {value}");
            let snap = session.story.debug_snapshot();
            let locals = snap
                .call_stack
                .first()
                .and_then(|f| f.locals.as_ref())
                .ok_or_else(|| {
                    format!(
                        "{}\nexpect-local {name}: this frame reports no locals at all (compiled \
                         without debug info?)",
                        session.transcript
                    )
                })?;
            let found = locals.iter().find(|l| &l.name == name).ok_or_else(|| {
                let have: Vec<&str> = locals.iter().map(|l| l.name.as_str()).collect();
                format!(
                    "{}\nexpect-local {name}: no such local in scope. In scope: {have:?}",
                    session.transcript
                )
            })?;
            let got = render(&found.value);
            if &got != value {
                return Err(format!(
                    "{}\nexpect-local {name}: expected {value}, got {got}",
                    session.transcript
                ));
            }
        }
        Command::ExpectStack(want) => {
            let _ = writeln!(session.transcript, "expect-stack {}", want.join(" > "));
            let got = session.frame_names();
            if &got != want {
                return Err(format!(
                    "{}\nexpect-stack: expected {want:?}, got {got:?}",
                    session.transcript
                ));
            }
        }
        Command::ExpectTerminal => {
            let _ = writeln!(session.transcript, "expect-terminal");
            match &session.last_reason {
                Some(DebugStopReason::Terminal) => {}
                other => {
                    return Err(format!(
                        "{}\nexpect-terminal: the last action stopped for {other:?}, not a \
                         terminal outcome",
                        session.transcript
                    ));
                }
            }
        }
        _ => unreachable!("apply_expectation only handles expect verbs"),
    }
    Ok(())
}

/// Source-level description of why the flow stopped. Deliberately omits
/// bytecode positions — see this module's own doc.
fn describe(reason: &DebugStopReason) -> String {
    match reason {
        DebugStopReason::Breakpoint { name, .. } => format!("breakpoint {name}"),
        DebugStopReason::Watchpoint { global_idx } => format!("watchpoint on global {global_idx}"),
        DebugStopReason::Choices => "choice point".to_string(),
        DebugStopReason::Step => "step".to_string(),
        other => format!("{other:?}").to_lowercase(),
    }
}
