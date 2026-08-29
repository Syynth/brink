//! Issue #3248: reachability proof for `brink debug` — the terminal
//! debugger. Drives the compiled `brink` binary, not
//! `brink_runtime::debug_session` directly, for the same reason
//! `debug_info_flag_cli.rs` does: the harness already proves the shared
//! verb set behaves (`brink-test-harness`'s `debug_sessions` goldens), so
//! what is unproven here is that a *user* can reach it — the subcommand
//! exists, it compiles with debug info without being asked, and both the
//! scripted and interactive front-ends work end to end.
//!
//! The failure these guard against is silent: `brink-cli` must enable
//! `brink-runtime`'s `debug-hooks` feature, and a whole-workspace
//! `cargo test` turns that on for every crate through feature unification.
//! A `cargo test -p brink-cli` that only ever ran under the workspace
//! build would therefore pass even if `brink-cli/Cargo.toml` never asked
//! for the feature at all.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

/// A two-frame story: a knot that calls a function, so `stack` and
/// `step over` both have something real to say. Line numbers are load
/// bearing — the fixtures below break on them — so it is written out with
/// its numbering in view.
///
/// ```text
/// 1  VAR greeting = "hi"
/// 2
/// 3  -> start
/// 4
/// 5  === start ===
/// 6  ~ temp who = greet("vendor")
/// 7  ~ temp n = 2
/// 8  Hello {who}, {n}.
/// 9  -> END
/// 10
/// 11 === function greet(who) ===
/// 12 ~ return greeting + " " + who
/// ```
const STORY: &str = "VAR greeting = \"hi\"\n\
                     \n\
                     -> start\n\
                     \n\
                     === start ===\n\
                     ~ temp who = greet(\"vendor\")\n\
                     ~ temp n = 2\n\
                     Hello {who}, {n}.\n\
                     -> END\n\
                     \n\
                     === function greet(who) ===\n\
                     ~ return greeting + \" \" + who\n";

#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn project_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brink-debug-cli-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn story_in(dir: &Path) -> PathBuf {
    let entry = dir.join("story.ink");
    fs::write(&entry, STORY).unwrap();
    entry
}

/// The whole point of the subcommand, in one run: arm a breakpoint by
/// `file:line`, run to it, step a source line, and read the locals — over
/// a plain `.ink` entry with no `--debug-info` anywhere on the command
/// line, because `brink debug` compiles with it implicitly
/// (`docs/debugger-spec.md` §1.2). A user who had to remember the flag
/// would just get a debugger that silently could not bind.
#[test]
fn script_mode_runs_a_session_over_a_source_entry() {
    let dir = project_dir("script");
    let entry = story_in(&dir);
    let script = dir.join("session.dbg");
    fs::write(
        &script,
        "break story.ink:7\nrun\nstep over\nlocals\nstack\n",
    )
    .expect("write script");

    let output = brink()
        .arg("debug")
        .arg(&entry)
        .arg("--script")
        .arg(&script)
        .output()
        .expect("brink debug --script should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let transcript = String::from_utf8_lossy(&output.stdout);
    let expected = "break story.ink:7\n\
                    run -> breakpoint story.ink:7\n  \
                    at story.ink:7\n\
                    step over -> step\n  \
                    at story.ink:8\n\
                    locals\n  \
                    who = \"hi vendor\"\n  \
                    n = 2\n\
                    stack\n  \
                    start\n";
    assert_eq!(transcript, expected, "full transcript:\n{transcript}");
}

/// `expect-*` verbs are assertions, and a script is only useful in CI if a
/// violated one fails the process. A satisfied one must of course pass.
#[test]
fn script_expectations_decide_the_exit_status() {
    let dir = project_dir("expect");
    let entry = story_in(&dir);

    let good = dir.join("good.dbg");
    fs::write(&good, "break story.ink:7\nrun\nexpect-line 7\n").expect("write script");
    let output = brink()
        .arg("debug")
        .arg(&entry)
        .arg("--script")
        .arg(&good)
        .output()
        .expect("brink debug --script should run");
    assert!(
        output.status.success(),
        "a satisfied expectation must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bad = dir.join("bad.dbg");
    fs::write(&bad, "break story.ink:7\nrun\nexpect-line 9\n").expect("write script");
    let output = brink()
        .arg("debug")
        .arg(&entry)
        .arg("--script")
        .arg(&bad)
        .output()
        .expect("brink debug --script should run");
    assert!(
        !output.status.success(),
        "a violated expectation must fail the process, or `brink debug --script` \
         is useless in CI; stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// A breakpoint on a line with no code is an error, not a silent no-op —
/// and when the artifact simply carries no debug info, the message says
/// THAT instead, so the user reaches for the compile flag rather than
/// hunting a bug in a source line that is perfectly fine.
#[test]
fn unbindable_breakpoints_report_the_right_reason() {
    let dir = project_dir("unbindable");
    let entry = story_in(&dir);
    let script = dir.join("session.dbg");

    // Line 10 is blank: real debug info, nothing on that line.
    fs::write(&script, "break story.ink:10\n").expect("write script");
    let output = brink()
        .arg("debug")
        .arg(&entry)
        .arg("--script")
        .arg(&script)
        .output()
        .expect("brink debug --script should run");
    assert!(
        !output.status.success(),
        "a breakpoint that cannot bind must fail"
    );
    // The CLI's `tracing` subscriber writes to stdout, so that is where a
    // reported error lands — checking stderr here would pass vacuously.
    let reported = String::from_utf8_lossy(&output.stdout);
    assert!(
        reported.contains("no executable code"),
        "expected the empty-line reason, got:\n{reported}"
    );

    // The same break against an .inkb built WITHOUT --debug-info: the line
    // is fine, the artifact is the problem.
    let inkb = dir.join("story.inkb");
    let status = brink()
        .args(["compile", "-o"])
        .arg(&inkb)
        .arg(&entry)
        .status()
        .expect("brink compile should run");
    assert!(status.success());

    fs::write(&script, "break story.ink:7\n").expect("write script");
    let output = brink()
        .arg("debug")
        .arg(&inkb)
        .arg("--script")
        .arg(&script)
        .output()
        .expect("brink debug --script should run");
    assert!(!output.status.success());
    let reported = String::from_utf8_lossy(&output.stdout);
    assert!(
        reported.contains("no debug info"),
        "a story compiled without --debug-info must say so, not blame the source \
         line; got:\n{reported}"
    );
}

/// A prebuilt `.inkb` is taken as-is rather than silently recompiled, so
/// one built without the flag degrades honestly: stepping still runs, it
/// just cannot say where it is.
#[test]
fn a_prebuilt_artifact_without_debug_info_degrades_honestly() {
    let dir = project_dir("nodebug");
    let entry = story_in(&dir);
    let inkb = dir.join("story.inkb");
    let status = brink()
        .args(["compile", "-o"])
        .arg(&inkb)
        .arg(&entry)
        .status()
        .expect("brink compile should run");
    assert!(status.success());

    let script = dir.join("session.dbg");
    fs::write(&script, "step over\n").expect("write script");
    let output = brink()
        .arg("debug")
        .arg(&inkb)
        .arg("--script")
        .arg(&script)
        .output()
        .expect("brink debug --script should run");
    assert!(
        output.status.success(),
        "stepping without debug info is not an error, just uninformative; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(
        transcript.contains("nolineinfo") && transcript.contains("<no source position>"),
        "expected an honest no-line-info stop, got:\n{transcript}"
    );
}

/// The REPL: the same verbs typed a line at a time, plus `list`, which is
/// the CLI's own — it marks the stopped line, so it answers "where am I"
/// and not merely "what does the file say".
#[test]
fn repl_mode_steps_and_lists() {
    let dir = project_dir("repl");
    let entry = story_in(&dir);

    let mut child = brink()
        .arg("debug")
        .arg(&entry)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("brink debug should start a REPL");
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        stdin
            .write_all(b"break story.ink:7\nrun\nlist\nstep over\nquit\n")
            .expect("write REPL input");
    }
    let output = child.wait_with_output().expect("REPL should exit");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let session = String::from_utf8_lossy(&output.stdout);
    assert!(
        session.contains("run -> breakpoint story.ink:7"),
        "REPL must reach the breakpoint; got:\n{session}"
    );
    assert!(
        session.contains("->    7 ~ temp n = 2"),
        "`list` must mark the stopped line; got:\n{session}"
    );
    assert!(
        session.contains("      6 ~ temp who = greet(\"vendor\")"),
        "`list` must show unmarked context around it; got:\n{session}"
    );
    assert!(
        session.contains("step over -> step") && session.contains("at story.ink:8"),
        "REPL stepping must work like the scripted form; got:\n{session}"
    );
}

/// An unknown verb is reported and the REPL keeps going — a typo must not
/// end the session and lose the breakpoints with it.
#[test]
fn repl_survives_a_bad_verb() {
    let dir = project_dir("badverb");
    let entry = story_in(&dir);

    let mut child = brink()
        .arg("debug")
        .arg(&entry)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("brink debug should start a REPL");
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        stdin
            .write_all(b"break story.ink:7\nfrobnicate\nrun\nquit\n")
            .expect("write REPL input");
    }
    let output = child.wait_with_output().expect("REPL should exit");
    assert!(output.status.success());

    let session = String::from_utf8_lossy(&output.stdout);
    assert!(
        session.contains("frobnicate"),
        "the bad verb must be reported; got:\n{session}"
    );
    assert!(
        session.contains("run -> breakpoint story.ink:7"),
        "the session must survive it with its breakpoints intact; got:\n{session}"
    );
}

/// Root-level content — before any knot — is a frame with no name, and a
/// stack listing that prints a blank line where a frame is, is worse than
/// one that names it.
#[test]
fn the_root_frame_is_named_in_the_stack() {
    let dir = project_dir("rootframe");
    let entry = dir.join("story.ink");
    // No knot at all: line 1 is the whole story, running at root depth.
    fs::write(&entry, "~ temp n = 1\nHello.\n-> END\n").expect("write entry");

    let script = dir.join("session.dbg");
    fs::write(&script, "break story.ink:1\nrun\nstack\n").expect("write script");

    let output = brink()
        .arg("debug")
        .arg(&entry)
        .arg("--script")
        .arg(&script)
        .output()
        .expect("brink debug --script should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(
        transcript.contains("stack\n  <root>\n"),
        "the unnamed root frame must render as `<root>`, not a blank line; got:\n{transcript}"
    );
}

/// Both source surfaces are debuggable (RULED, `docs/debugger-spec.md`
/// §1) — so the native entry reaches the same subcommand, not just `.ink`.
#[test]
fn a_native_entry_is_debuggable_too() {
    let dir = project_dir("native");
    let entry = dir.join("main.brink");
    fs::write(
        &entry,
        "flow main() {\n  ~ let count = 3\n  Hello.\n  -> END\n}\n",
    )
    .expect("write native entry");

    let script = dir.join("session.dbg");
    // Break on the binding, step past it, then look: a breakpoint stops
    // BEFORE its line runs, so asking for locals at the stop would prove
    // nothing about whether native locals resolve at all.
    fs::write(&script, "break main.brink:2\nrun\nstep over\nlocals\n").expect("write script");

    let output = brink()
        .arg("debug")
        .arg(&entry)
        .arg("--script")
        .arg(&script)
        .output()
        .expect("brink debug --script should run for a native entry");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let transcript = String::from_utf8_lossy(&output.stdout);
    assert!(
        transcript.contains("run -> breakpoint main.brink:2"),
        "a .brink breakpoint must bind and hit; got:\n{transcript}"
    );
    assert!(
        transcript.contains("count = 3"),
        "native locals must resolve; got:\n{transcript}"
    );
}
