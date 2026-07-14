//! M-2b: the runtime refuses host **semantic** access (variable get/set,
//! entry lookup, function eval) to `#@private` definitions, while host
//! **persistence** (save/load) sees everything. Dev tooling opts out of
//! enforcement. See `docs/modules-spec.md` §4 boundary rules 2 and 3.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_format::Value;
use brink_runtime::{DotNetRng, FallbackHandler, RuntimeError, Story, link};

/// An undeclared (default-public) module with a per-definition `#@private`
/// override on the `secret` VAR and the `hidden` knot; `start` stays public.
const SRC: &str = "#@private\n\
                   VAR secret = 5\n\
                   == start ==\n\
                   Hi\n\
                   -> DONE\n\
                   == hidden ==\n\
                   #@private\n\
                   Secret content\n\
                   -> DONE\n";

fn compiled() -> brink_format::StoryData {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options("story.ink", |_p| Ok(SRC.to_owned()), options)
        .unwrap()
        .data
}

fn story(data: &brink_format::StoryData) -> Story<DotNetRng> {
    let (program, line_tables) = link(data).unwrap();
    Story::new(Arc::new(program), line_tables)
}

#[test]
fn host_semantic_access_refused_for_private_defs() {
    let data = compiled();
    assert!(
        !data.private_defs.is_empty(),
        "the declared module must produce private defs"
    );

    let mut s = story(&data);
    assert!(s.visibility_enforced(), "enforcement is on by default");

    // Variable get/set on a private VAR: refused (None / false no-op).
    assert_eq!(s.variable("secret"), None);
    assert!(!s.set_variable("secret", Value::Int(99)));

    // Entry lookup into a private knot: PrivateAccess.
    assert!(matches!(
        s.choose_path_string("hidden"),
        Err(RuntimeError::PrivateAccess { .. })
    ));

    // Function eval of a private target: PrivateAccess.
    assert!(matches!(
        s.call_function("hidden", &[], &FallbackHandler),
        Err(RuntimeError::PrivateAccess { .. })
    ));

    // The public entry point is unaffected.
    s.choose_path_string("start").unwrap();
}

#[test]
fn persistence_sees_private_state_regardless_of_enforcement() {
    let data = compiled();
    let s = story(&data);
    // save_state routes through DefinitionId, not the enforced host surface —
    // the private global is captured even with enforcement on.
    let save = s.save_state();
    assert!(
        save.globals.iter().any(|(name, _)| name == "secret"),
        "persistence must serialize private globals; got {:?}",
        save.globals
    );
}

#[test]
fn dev_override_allows_private_access() {
    let data = compiled();
    let mut s = story(&data);

    // The documented dev-tooling override (play-from-here).
    s.set_visibility_enforcement(false);
    assert!(!s.visibility_enforced());

    assert_eq!(s.variable("secret"), Some(&Value::Int(5)));
    assert!(s.set_variable("secret", Value::Int(42)));
    assert_eq!(s.variable("secret"), Some(&Value::Int(42)));

    // A private knot can now be entered directly — the play-from-here path.
    s.choose_path_string("hidden").unwrap();
}
