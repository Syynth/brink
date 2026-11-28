//! Issue #3184 (D6, `docs/debugger-spec.md` §1.2/§2): reachability proof for
//! the `brink compile --debug-info` flag — the real user path that turns on
//! `SectionKind::DebugInfo` (tag `0x11`). Exercises the actual compiled
//! `brink` binary end to end, not codegen's internal API, mirroring
//! `native_play_cli.rs`'s pattern for the same reason: a test that only
//! calls `brink_codegen_inkb::emit_with_options` directly proves the section
//! CAN be built, not that a user can reach it.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn project_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("brink-debug-info-cli-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `brink compile --debug-info story.ink` (default `.inkt` stdout output)
/// renders the `(debug_info ...)` block; the same command without the flag
/// does not.
#[test]
fn compile_debug_info_flag_adds_the_section_to_inkt_output() {
    let dir = project_dir("inkt");
    let entry = dir.join("story.ink");
    fs::write(&entry, "VAR x = 0\n~ x = 5\n-> END\n").unwrap();

    let with_flag = brink()
        .args(["compile", "--debug-info"])
        .arg(&entry)
        .output()
        .expect("brink compile --debug-info should run");
    assert!(
        with_flag.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&with_flag.stderr)
    );
    let with_flag_text = String::from_utf8_lossy(&with_flag.stdout);
    assert!(
        with_flag_text.contains("(debug_info"),
        "expected a (debug_info ...) block in .inkt output, got:\n{with_flag_text}"
    );

    let without_flag = brink()
        .arg("compile")
        .arg(&entry)
        .output()
        .expect("brink compile should run");
    assert!(without_flag.status.success());
    let without_flag_text = String::from_utf8_lossy(&without_flag.stdout);
    assert!(
        !without_flag_text.contains("(debug_info"),
        "no (debug_info ...) block without the flag — ship-policy default \
         (docs/debugger-spec.md §1.2), got:\n{without_flag_text}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// The same flag against `.inkb` binary output: the compiled artifact
/// carries a real `SectionKind::DebugInfo` section a reader can parse back.
#[test]
fn compile_debug_info_flag_adds_the_section_to_inkb_output() {
    let dir = project_dir("inkb");
    let entry = dir.join("story.ink");
    let out = dir.join("story.inkb");
    fs::write(&entry, "VAR x = 0\n~ x = 5\n-> END\n").unwrap();

    let status = brink()
        .args(["compile", "--debug-info", "-o"])
        .arg(&out)
        .arg(&entry)
        .status()
        .expect("brink compile --debug-info -o story.inkb should run");
    assert!(status.success());

    let bytes = fs::read(&out).unwrap();
    let story = brink_format::read_inkb(&bytes).expect("valid .inkb");
    assert!(
        story.debug_info.is_some(),
        "the .inkb produced with --debug-info must carry a DebugInfo section"
    );

    fs::remove_dir_all(&dir).ok();
}

/// A `.brink` (native surface) entry reaches the same flag.
#[test]
fn compile_debug_info_flag_works_for_native_entry() {
    let dir = project_dir("native");
    let entry = dir.join("main.brink");
    fs::write(&entry, "flow main() {\n  Hello. -> END\n}\n").unwrap();

    let output = brink()
        .args(["compile", "--debug-info"])
        .arg(&entry)
        .output()
        .expect("brink compile --debug-info should run for a native entry");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("(debug_info"),
        "expected a (debug_info ...) block for a native entry, got:\n{text}"
    );

    fs::remove_dir_all(&dir).ok();
}
