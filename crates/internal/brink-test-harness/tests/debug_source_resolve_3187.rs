//! Proof tests for issue #3187 (debugger D9): the program→source resolver —
//! [`brink_runtime::Program::resolve_debug_position`] — the piece
//! `debug.rs`'s own doc named as deferred ("resolving position to source is
//! a later workstream (D6/D9)") and `docs/debugger-spec.md` §6 names as
//! this ticket's job: "brink-desktop... reads it through the wasm bridge
//! (D9, #3187) using the `container_idx`-indexed layout in §2.2 to resolve
//! a running `FlowInstance`'s `(container_idx, offset)` position... to
//! source."
//!
//! Proof shape, mirroring D6's own (#3184) and D4's (#3182): every
//! assertion is cross-checked against the raw source text sliced at the
//! resolved range — never against the resolver's own output alone — and
//! covers **both** source surfaces (`.ink` and `.brink`), per the RULED
//! both-surfaces requirement (`docs/debugger-spec.md` §0).

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::collections::BTreeMap;

use brink_environment::{OptionOverrides, Project};
use brink_runtime::{DebugPosition, FastRng, Program, Story};
use brink_source_tree::InMemory;

type LineTables = Vec<Vec<brink_format::LineEntry>>;

/// Compile `entry` (with `--debug-info` semantics on) over an in-memory
/// tree and link it — the real `Project::load` → `brink_environment::compile`
/// → `brink_runtime::link` production road (#1306), not `brink_compiler`'s
/// lower-level entry points. Works for both `.ink` and `.brink` entries:
/// `Project::load`'s own doc says a `.brink` entry's universe is "the whole
/// native source tree," discovered via the tree's own `list` — no real
/// filesystem needed, unlike `brink_compiler::compile_path`.
fn compile_with_debug_info(files: &[(&str, &str)], entry: &str) -> (Program, LineTables, String) {
    let mut source = files
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect::<BTreeMap<_, _>>();
    let entry_text = source.get(entry).cloned().expect("entry must be in files");
    // Re-insert to guarantee the map holds exactly what the caller wrote
    // (defensive against accidental normalization above).
    source.insert(entry.to_string(), entry_text.clone());
    let tree = InMemory::new(source);

    let overrides = OptionOverrides {
        debug_info: true,
        ..Default::default()
    };
    let env = Project::load(&tree, entry, &overrides).expect("Project::load");
    let out = brink_environment::compile(&env).expect("compile");
    let (program, line_tables) = brink_runtime::link(&out.data).expect("link");
    (program, line_tables, entry_text)
}

/// Step the default flow one VM opcode at a time (via the `testing`-only
/// `Story::step_once`), calling `on_step` after every step until it returns
/// `true` or the story finishes/errors. Bounded per CLAUDE.md's "guard
/// against unbounded growth."
fn step_until(
    story: &mut Story<FastRng>,
    max_steps: usize,
    mut on_step: impl FnMut(&Story<FastRng>) -> bool,
) -> bool {
    for _ in 0..max_steps {
        match story.step_once() {
            Ok(Some(_)) => {
                if on_step(story) {
                    return true;
                }
            }
            Ok(None) | Err(_) => return false,
        }
    }
    false
}

// ── End-to-end: resolve a temp-assignment position back to its own source text ──

#[test]
fn ink_position_resolves_to_the_assigning_source_text() {
    // No knot header: a top-level `VAR` declaration alone produces no root
    // content, so the default flow's very first instruction would be an
    // implicit `Done` unless the assignment is itself top-level content.
    let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
    let (program, line_tables, source) = compile_with_debug_info(&[("main.ink", src)], "main.ink");
    let mut story = Story::<FastRng>::new(std::sync::Arc::new(program), line_tables);

    let mut resolved = None;
    step_until(&mut story, 500, |s| {
        if let Some(pos) = s.debug_snapshot().position
            && let Some(loc) = s.program().resolve_debug_position(pos)
        {
            let start = loc.range_start as usize;
            let end = start + loc.range_len as usize;
            if let Some(slice) = source.get(start..end)
                && slice.contains("x = 5")
            {
                resolved = Some((loc, slice.to_string()));
                return true;
            }
        }
        false
    });

    let (loc, slice) = resolved.expect("must resolve the `x = 5` assignment's own position");
    assert_eq!(
        slice, "x = 5",
        "resolved source range must be exactly the assignment"
    );
    assert_eq!(loc.file.as_deref(), Some("main.ink"));
}

#[test]
fn brink_native_position_resolves_to_the_assigning_source_text() {
    let src = "var x = 0\n\nflow main() {\n    ~ x = 5\n    hello\n    -> END\n}\n";
    let (program, line_tables, source) =
        compile_with_debug_info(&[("main.brink", src)], "main.brink");
    let mut story = Story::<FastRng>::new(std::sync::Arc::new(program), line_tables);

    let mut resolved = None;
    step_until(&mut story, 500, |s| {
        if let Some(pos) = s.debug_snapshot().position
            && let Some(loc) = s.program().resolve_debug_position(pos)
        {
            let start = loc.range_start as usize;
            let end = start + loc.range_len as usize;
            if let Some(slice) = source.get(start..end)
                && slice.contains("x = 5")
            {
                resolved = Some((loc, slice.to_string()));
                return true;
            }
        }
        false
    });

    let (loc, slice) = resolved.expect("must resolve the `x = 5` assignment's own position");
    assert_eq!(
        slice, "x = 5",
        "resolved source range must be exactly the assignment"
    );
    assert_eq!(loc.file.as_deref(), Some("main.brink"));
}

// ── Gating: no `DebugInfo` section means no resolution, not a panic ──

#[test]
fn resolve_debug_position_is_none_without_debug_info() {
    let src = "VAR x = 0\n=== main ===\n~ x = 5\nhello\n-> END\n";
    let mut source = BTreeMap::new();
    source.insert("main.ink".to_string(), src.to_string());
    let tree = InMemory::new(source);

    // Default overrides: `debug_info: false` — the release/non-studio compile.
    let overrides = OptionOverrides::default();
    let env = Project::load(&tree, "main.ink", &overrides).expect("Project::load");
    let out = brink_environment::compile(&env).expect("compile");
    let (program, line_tables) = brink_runtime::link(&out.data).expect("link");
    let mut story = Story::<FastRng>::new(std::sync::Arc::new(program), line_tables);

    let mut any_position_seen = false;
    let mut any_resolved = false;
    step_until(&mut story, 500, |s| {
        if let Some(pos) = s.debug_snapshot().position {
            any_position_seen = true;
            if s.program().resolve_debug_position(pos).is_some() {
                any_resolved = true;
            }
        }
        false
    });

    assert!(any_position_seen, "fixture must actually produce positions");
    assert!(
        !any_resolved,
        "resolve_debug_position must be None on every position when no DebugInfo section was compiled"
    );
}

// ── Edge case: an out-of-range container index doesn't panic ──

#[test]
fn resolve_debug_position_is_none_for_out_of_range_container() {
    let src = "VAR x = 0\n=== main ===\n~ x = 5\nhello\n-> END\n";
    let (program, _line_tables, _source) =
        compile_with_debug_info(&[("main.ink", src)], "main.ink");

    let bogus = DebugPosition {
        container_idx: u32::MAX,
        offset: 0,
    };
    assert!(program.resolve_debug_position(bogus).is_none());
}
