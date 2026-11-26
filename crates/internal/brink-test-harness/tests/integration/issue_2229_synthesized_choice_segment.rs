//! Issue #2229 (review follow-up): a synthesized anonymous-container path
//! segment must never equal a legal authored identifier.
//!
//! `hir::stamp::stamp_stmt` used to spell choice segments bare — `c{n}` —
//! contra `root_content_scope_path`'s own doc, which always claimed the
//! synthesized segments are `c-N`/`g-N`/`b-N`/`s-N`. That was survivable
//! while root content and knot interiors hashed in disjoint namespaces,
//! but #2229's per-knot `#file:` qualifier put them in the SAME namespace:
//! an authored knot legally named `c0` then hashed the identical scope
//! (`#file:{path}.c0`) as a root-level anonymous choice's subtree, so
//! every same-position descendant container minted the same
//! `DefinitionId` and tripped the #1673 duplicate-id `E060` codegen guard
//! — a single-file regression (this exact story compiles on pre-#2229
//! `main`). The fix spells the segment `c-{n}`: `-` is not legal in any
//! authored identifier, so a synthesized segment can never equal one.
//!
//! ⚠ Rule 20a: verified this test FAILS before the spelling fix (with
//! only #2229's per-knot qualifier applied): `[E060] internal codegen
//! error: duplicate DefinitionId … at paths "c-0.c-0" and "c0.0.c-0"`,
//! reproduced live via `brink compile` on this exact source.

#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_ink;

/// A unique scratch `.ink` file under the system temp dir, removed on
/// drop — `explore_from_ink` compiles through the real `compile_path`
/// road, which registers a real file-path qualifier (the ingredient that
/// makes root content and knot interiors share a namespace), so the
/// source must exist on disk. Mirrors `corpus.rs`'s own `ScratchFile`.
struct ScratchInk(PathBuf);

impl ScratchInk {
    fn write(name: &str, content: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "brink-issue-2229-{name}-{}.ink",
            std::process::id(),
        ));
        std::fs::write(&path, content).expect("write scratch ink file");
        Self(path)
    }
}

impl Drop for ScratchInk {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn authored_knot_named_c0_must_not_collide_with_anonymous_choice_segments() {
    // Root content: an anonymous choice (subtree scope `{root}.c-0`) with
    // a nested anonymous choice inside it (`{root}.c-0.c-0`). Knot `c0`:
    // its interior scope is `{root}.c0`, so its first anonymous choice is
    // `{root}.c0.c-0` — under the old bare `c{n}` spelling both spelled
    // `{root}.c0.c0` and collided.
    let src = "\
* top
  * * nested
    ok
- done
-> DONE

== c0 ==
* knot choice
- g
-> DONE
";
    let scratch = ScratchInk::write("knot-named-c0", src);
    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 50,
    };
    let episodes = explore_from_ink(&scratch.0, &config).expect(
        "a story with an authored knot named `c0` alongside root-level \
         anonymous choices must compile and play cleanly — a synthesized \
         `c-N` segment can never equal an authored identifier",
    );
    assert!(
        !episodes.is_empty(),
        "exploration must produce at least one episode"
    );
}
