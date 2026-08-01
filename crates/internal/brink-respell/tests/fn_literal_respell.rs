//! Respelling ink's `#fn(…)` fn-value literal onto the native surface
//! (issue #1862).
//!
//! The 2026-08-01 ruling split the construct in two on the way across:
//!
//! - a **zero-bound** `#fn(f)` is just "take a fn value of `f`", and native
//!   spells that as the bare name `f` (a call keeps its parentheses, so the
//!   reference is unambiguous) — so it respells;
//! - the **binding** form `#fn(f, a)` deliberately got *no* native
//!   spelling, because for a `ref` parameter it binds a durable cell while
//!   a lambda (`|x| f(a, x)`) captures by value only. The emitter refuses
//!   it rather than inventing a spelling that would silently change what it
//!   means.
//!
//! Both halves are asserted here, through the crate's own public entry
//! (`respell_ink_source`) rather than a unit call on the emitter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_respell::respell_ink_source;

#[test]
fn zero_bound_fn_literal_respells_as_a_bare_name() {
    let ink = "VAR f = #fn(scene)\n\
               \n\
               Here.\n\
               -> END\n\
               \n\
               === function scene(x) ===\n\
               -> DONE\n";
    let brink = respell_ink_source(ink).expect("a zero-bound `#fn` must respell");
    assert!(
        brink.contains("= scene"),
        "expected the bare target name as the native fn-value spelling, got:\n{brink}"
    );
    assert!(
        !brink.contains("#fn"),
        "the `#fn` sigil is ink-only and must not survive the respell, got:\n{brink}"
    );
}

#[test]
fn binding_fn_literal_still_refuses() {
    let ink = "VAR gold = 0\n\
               VAR f = #fn(heal, gold)\n\
               \n\
               Here.\n\
               -> END\n\
               \n\
               === function heal(ref pool, amount) ===\n\
               -> DONE\n";
    let err = respell_ink_source(ink)
        .expect_err("the binding form has no native spelling and must refuse");
    let rendered = format!("{err}");
    assert!(
        rendered.contains("bound arguments"),
        "expected the refusal to name the binding form specifically, got: {rendered}"
    );
}
