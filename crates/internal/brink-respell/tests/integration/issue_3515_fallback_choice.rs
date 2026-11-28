//! Issues #3515 and #3516: the shrunk story from the respell equivalence
//! property, a stitch with a guarded choice and a text-less fallback
//! choice. It respelled to source the native compiler rejected: the guard
//! `{not true}` came out as the bare word `not` (#3516), and the error
//! cascade landed on the fallback's `else { … }` arm (#3515). The fallback
//! is native's own `else` arm of a `{? … }` block; with the guard spelled
//! `!true` the whole file compiles.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_respell::respell_ink_source;

#[test]
fn a_fallback_choice_respells_to_native_that_compiles() {
    let ink = "-> a_k0\n\
               \n\
               === a_k0 ===\n\
               -> a_k0.a_s0\n\
               \n\
               = a_s0\n\
               * {not true} [a]\n    -> END\n\
               + -> END\n";
    let brink = respell_ink_source(ink).expect("respells");
    assert!(brink.contains("{if !true}"), "{brink}");
    assert!(brink.contains("else {"), "{brink}");

    let out =
        brink_test_harness::corpus::compile_source_to_inkb("respell-3515", "story.brink", &brink);
    assert!(
        out.is_ok(),
        "respelled source does not compile: {:?}\n{brink}",
        out.err()
    );
}
