//! M-2b: the runtime refuses host **semantic** access (variable get/set,
//! entry lookup, function eval) to `#@private` definitions, while host
//! **persistence** (save/load) sees everything. Dev tooling opts out of
//! enforcement. See `docs/modules-spec.md` §4 boundary rules 2 and 3.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_format::{DefinitionId, StoryData, Value};
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

/// Resolve a scope-owning container's `DefinitionId` by its author path,
/// straight from the compiled `StoryData` — the by-id equivalent of the
/// name-based `find_path_target` a host driving `Story::spawn_flow`
/// (#803) would use if it already held the id (e.g. cached from an earlier
/// name lookup, or read off `StoryData.address_paths` directly).
fn definition_id_for(data: &StoryData, path: &str) -> DefinitionId {
    let name_id = data
        .name_table
        .iter()
        .position(|n| n == path)
        .unwrap_or_else(|| panic!("no interned name for path {path:?}"));
    data.address_paths
        .iter()
        .find(|ap| ap.path.0 as usize == name_id)
        .unwrap_or_else(|| panic!("no address path for {path:?}"))
        .target
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

// ── #803: Story::spawn_flow's by-id entry path ─────────────────────────
//
// PR #781 (Story-level) and PR #796 (FlowInstance-level) both refuse a
// name-based entry into a `#@private` target. `Story::spawn_flow` takes a
// `DefinitionId` directly rather than a name — a host that already holds
// the id (cached from an earlier lookup, or read off `StoryData` directly,
// as `definition_id_for` above does) could start a flow at a private knot
// without ever going through the refused name-based path. Same for
// `Story::spawn_flow_shared`'s `container_idx`, which `brink-web`'s wasm
// `spawn_flow` binding resolves via `Program::find_address` before calling
// in — that resolution step doesn't check privacy either.

#[test]
fn spawn_flow_by_id_refused_for_private_def() {
    let data = compiled();
    let hidden_id = definition_id_for(&data, "hidden");

    let mut s = story(&data);
    assert!(s.visibility_enforced(), "enforcement is on by default");

    assert!(matches!(
        s.spawn_flow("f", hidden_id),
        Err(RuntimeError::PrivateAccess { .. })
    ));
    // The refused spawn must not have registered a flow under the name.
    assert!(s.flow_names().is_empty());
}

#[test]
fn spawn_flow_by_id_allows_public_def() {
    let data = compiled();
    let start_id = definition_id_for(&data, "start");

    let mut s = story(&data);
    s.spawn_flow("f", start_id).unwrap();
    assert_eq!(s.flow_names(), vec!["f"]);
}

#[test]
fn spawn_flow_by_id_dev_override_allows_private_access() {
    let data = compiled();
    let hidden_id = definition_id_for(&data, "hidden");

    let mut s = story(&data);
    s.set_visibility_enforcement(false);

    s.spawn_flow("f", hidden_id).unwrap();
    assert_eq!(s.flow_names(), vec!["f"]);
}

#[test]
fn spawn_flow_shared_by_id_refused_for_private_def() {
    let data = compiled();
    let (program, _line_tables) = link(&data).unwrap();
    let hidden_idx = program
        .find_address("hidden")
        .map(|(idx, _)| idx)
        .expect("hidden knot resolves to a container");

    let mut s = story(&data);
    assert!(s.visibility_enforced(), "enforcement is on by default");

    assert!(matches!(
        s.spawn_flow_shared("f", Some(hidden_idx)),
        Err(RuntimeError::PrivateAccess { .. })
    ));
    assert!(s.flow_names().is_empty());
}

#[test]
fn spawn_flow_shared_by_id_allows_public_def() {
    let data = compiled();
    let (program, _line_tables) = link(&data).unwrap();
    let start_idx = program
        .find_address("start")
        .map(|(idx, _)| idx)
        .expect("start knot resolves to a container");

    let mut s = story(&data);
    s.spawn_flow_shared("f", Some(start_idx)).unwrap();
    assert_eq!(s.flow_names(), vec!["f"]);
}

#[test]
fn spawn_flow_shared_by_id_dev_override_allows_private_access() {
    let data = compiled();
    let (program, _line_tables) = link(&data).unwrap();
    let hidden_idx = program
        .find_address("hidden")
        .map(|(idx, _)| idx)
        .expect("hidden knot resolves to a container");

    let mut s = story(&data);
    s.set_visibility_enforcement(false);

    s.spawn_flow_shared("f", Some(hidden_idx)).unwrap();
    assert_eq!(s.flow_names(), vec!["f"]);
}
