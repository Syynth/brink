#![expect(clippy::expect_used, reason = "test harness")]

//! `.inkb` v10 (`docs/compiler-spec.md` §"Parameter binding"): the VM binds a
//! container's parameters at entry, replacing the leading `DeclareTemp`
//! prologue codegen used to emit. The prologue was correct by construction —
//! bytecode at offset 0 runs whenever control arrives at offset 0 — so
//! moving the work into the VM turned that into an obligation on **every**
//! entry site. These are the shapes that exercise them.
//!
//! Two of these were found the hard way rather than by reading the code:
//!
//! - `->-> target(args)` (tunnel onwards with an argument) binds *after* the
//!   frame pops, because it works by rewriting the frame's return address —
//!   so no search for "positions execution at offset 0" finds it.
//! - A **stitch**'s parameters do not start at slot 0. A knot and its
//!   stitches share one call frame and one temp map, so the stitch's
//!   parameters continue after the knot's. Binding positionally into
//!   `0 … n-1` silently bound the wrong slot; the VM reads the recorded
//!   slot (`ParamMeta::slot`) instead.

use std::sync::Arc;

use brink_runtime::{FastRng, Step, Story};

fn play(name: &str, src: &str, picks: &[usize]) -> String {
    let compiled = brink_compiler::compile("t.ink", |p| {
        if p == "t.ink" {
            Ok(src.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such include",
            ))
        }
    });
    assert!(compiled.is_ok(), "{name}: compile failed: {compiled:?}");
    let out = compiled.expect("just asserted above");
    let linked = brink_runtime::link(&out.data);
    assert!(
        linked.is_ok(),
        "{name}: link failed: {:?}",
        linked.as_ref().err()
    );
    let (program, line_tables) = linked.expect("just asserted above");
    let mut story = Story::<FastRng>::new(Arc::new(program), line_tables);

    let mut text = String::new();
    let mut picks = picks.iter();
    for _ in 0..32 {
        let steps = story.continue_maximally();
        assert!(steps.is_ok(), "{name}: run failed: {steps:?}");
        let steps = steps.expect("just asserted above");
        text.push_str(&steps.iter().map(Step::text).collect::<String>());
        match steps.last() {
            Some(Step::Choices(choices)) => {
                // Choice labels are not part of `Step::text`, and a label is
                // where a thread's argument shows up first.
                for c in choices {
                    text.push_str(&c.text);
                    text.push('\n');
                }
                let Some(&pick) = picks.next() else { break };
                let chosen = story.choose(pick);
                assert!(chosen.is_ok(), "{name}: choose failed: {chosen:?}");
            }
            _ => break,
        }
    }
    text
}

#[test]
fn a_function_call_binds_its_arguments() {
    let text = play(
        "function",
        "{add(2, 3)}\n-> END\n\n=== function add(a, b) ===\n~ return a + b\n",
        &[],
    );
    assert!(
        text.contains('5'),
        "expected 2 + 3 to print 5, got {text:?}"
    );
}

#[test]
fn a_divert_to_a_parameterized_knot_binds_its_arguments() {
    let text = play(
        "divert",
        "-> greet(\"hi\", 2)\n\n=== greet(word, n) ===\n{word} {n}\n-> END\n",
        &[],
    );
    assert!(
        text.contains("hi 2"),
        "the divert's arguments should reach the knot: {text:?}"
    );
}

#[test]
fn a_tunnel_binds_its_arguments_and_onwards_binds_the_next_knots() {
    // `->-> b(x)` rewrites the tunnel frame's return address, so its binding
    // happens after the frame pops — the entry site a search for "enters at
    // offset 0" does not find.
    let text = play(
        "tunnel onwards",
        "-> a(4) ->\n-> END\n\n=== a(n) ===\nin a: {n}\n->-> b(n + 1)\n\n=== b(m) ===\nin b: {m}\n",
        &[],
    );
    assert!(
        text.contains("in a: 4") && text.contains("in b: 5"),
        "both the tunnel's own argument and the onwards argument should bind: {text:?}"
    );
}

#[test]
fn a_stitch_parameter_binds_to_its_own_slot_not_slot_zero() {
    // `inner`'s `m` is slot 1: it shares a frame with `outer`'s `n`, which
    // holds slot 0. Positional binding wrote the argument into slot 0 and
    // `{m}` read an unbound slot 1.
    let text = play(
        "stitch param",
        "-> outer(7)\n\n=== outer(n) ===\n-> inner(n + 1)\n\n= inner(m)\ninner {m}\n-> END\n",
        &[],
    );
    assert!(
        text.contains("inner 8"),
        "the stitch's parameter must bind to its own slot, not the knot's: {text:?}"
    );

    // NOT asserted here, and not caused by v10: `{n}` (the enclosing knot's
    // parameter) reads as empty inside the stitch on this shape, on the
    // pre-v10 prologue build too. That is a separate question about knot
    // parameter visibility from a stitch, unrelated to where binding
    // happens.
}

#[test]
fn a_thread_binds_a_parameterized_stitchs_arguments() {
    // The fork copies the call stack, but the arguments are on the shared
    // value stack — and the stitch's parameter is not slot 0.
    let text = play(
        "thread",
        "-> gameloop\n\n=== gameloop ===\n- (top)\n    <- opt(3)\n    -> DONE\n\n= opt(n)\n+ [Pick {n}]\n    You picked {n}.\n    -> END\n",
        &[0],
    );
    assert!(
        text.contains("Pick 3") && text.contains("You picked 3."),
        "the thread's argument should bind for both the choice label and its body: {text:?}"
    );
}
