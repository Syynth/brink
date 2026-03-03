use std::path::Path;

use brink_format::Opcode;
use brink_json::InkJson;

use crate::convert;

#[test]
fn convert_i001_minimal_story() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = convert(&story).unwrap();

    // Should have containers (root + sub-containers)
    assert!(!data.containers.is_empty(), "should produce containers");

    // At least one line table should have an entry with "Hello, world!"
    let has_hello = data.line_tables.iter().any(|lt| {
        lt.lines.iter().any(|entry| {
            matches!(
                &entry.content,
                brink_format::LineContent::Plain(s) if s == "Hello, world!"
            )
        })
    });
    assert!(has_hello, "should contain 'Hello, world!' in line table");
}

#[test]
fn convert_simple_divert() {
    let json_text = include_str!("../../../../tests/tier1/divert/simple-divert/story.ink.json");
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = convert(&story).unwrap();

    // Should have a line table with "hurried home" text
    let has_hurry_home = data.line_tables.iter().any(|lt| {
        lt.lines.iter().any(|entry| {
            matches!(
                &entry.content,
                brink_format::LineContent::Plain(s) if s.contains("hurried home")
            )
        })
    });
    assert!(
        has_hurry_home,
        "should contain 'hurried home' text in some container"
    );

    // Root sub-container should have a Goto opcode
    let has_goto = data.containers.iter().any(|c| {
        let mut offset = 0;
        while offset < c.bytecode.len() {
            if let Ok(op) = Opcode::decode(&c.bytecode, &mut offset) {
                if matches!(op, Opcode::Goto(_)) {
                    return true;
                }
            } else {
                break;
            }
        }
        false
    });
    assert!(has_goto, "should have a Goto opcode in bytecode");
}

/// Sequence branch conditional diverts (`.^.s0`, `.^.s1`, `.^.s2`) must be
/// emitted as `JumpIfFalse + EnterContainer`, NOT `GotoIf`. `GotoIf` clears
/// the container stack, losing the parent context needed to resume after the
/// branch. `JumpIfFalse + EnterContainer` pushes the branch as a child,
/// allowing the branch's internal goto to correctly unwind back.
#[test]
fn sequence_branch_diverts_use_enter_container() {
    let json_text = include_str!(
        "../../../../tests/tier1/choices/I089-once-only-choices-with-own-content/story.ink.json"
    );
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    let data = convert(&story).unwrap();

    // The sequence container (eat.0.1) should NOT have any GotoIf opcodes.
    // Instead it should use JumpIfFalse + EnterContainer for the s0/s1/s2
    // branch diverts.
    //
    // Find the container whose bytecode has the sequence pattern. We can
    // identify it by looking for CurrentVisitCount (the "visit" command).
    let seq_container = data.containers.iter().find(|c| {
        let mut offset = 0;
        while offset < c.bytecode.len() {
            if let Ok(op) = Opcode::decode(&c.bytecode, &mut offset) {
                if matches!(op, Opcode::CurrentVisitCount) {
                    return true;
                }
            } else {
                break;
            }
        }
        false
    });

    let seq_container =
        seq_container.expect("should find the sequence container with CurrentVisitCount");

    // Decode all opcodes and check: no GotoIf, but has EnterContainer.
    let mut opcodes = Vec::new();
    let mut offset = 0;
    while offset < seq_container.bytecode.len() {
        if let Ok(op) = Opcode::decode(&seq_container.bytecode, &mut offset) {
            opcodes.push(op);
        } else {
            break;
        }
    }

    let has_goto_if = opcodes.iter().any(|op| matches!(op, Opcode::GotoIf(_)));
    assert!(
        !has_goto_if,
        "sequence container should NOT have GotoIf for branch diverts; \
         should use JumpIfFalse + EnterContainer instead. Opcodes: {opcodes:?}"
    );

    let has_enter_container = opcodes
        .iter()
        .any(|op| matches!(op, Opcode::EnterContainer(_)));
    assert!(
        has_enter_container,
        "sequence container should have EnterContainer for branch diverts"
    );

    let has_jump_if_false = opcodes
        .iter()
        .any(|op| matches!(op, Opcode::JumpIfFalse(_)));
    assert!(
        has_jump_if_false,
        "sequence container should have JumpIfFalse paired with EnterContainer"
    );
}

fn collect_ink_json_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_ink_json_files(&path));
            } else if path.extension().is_some_and(|e| e == "json")
                && path.to_str().is_some_and(|s| s.contains(".ink.json"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn convert_all_test_corpus() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests");

    let files = collect_ink_json_files(&tests_dir);
    assert!(
        !files.is_empty(),
        "no .ink.json files found in {tests_dir:?}"
    );

    let mut failures = Vec::new();

    for path in &files {
        let json_text = std::fs::read_to_string(path).unwrap();
        let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(&json_text);

        let story: InkJson = match serde_json::from_str(json_text) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("PARSE {}: {e}", path.display()));
                continue;
            }
        };

        if let Err(e) = convert(&story) {
            failures.push(format!("CONVERT {}: {e}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed conversion:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
