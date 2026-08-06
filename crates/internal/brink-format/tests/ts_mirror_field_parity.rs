//! A cheap tripwire against `packages/wasm-types/src/index.ts`'s hand-written
//! `SaveState`/`SuspendedFlow`/`WakePolicy` TS mirror drifting from the real
//! Rust shape (issue #2313; #2311 is the same family's other instance).
//!
//! There is no schema-generation crate (`schemars`, `ts-rs`, `typeshare`, …)
//! anywhere in this workspace today, so this test does not introduce one —
//! picking a cross-language codegen tool is a cross-cutting dependency
//! choice (which crate owns the generated output, how build-time codegen
//! wires into `packages/wasm-types`, how versioning/publishing interacts)
//! that deserves its own design discussion, not a unilateral call buried in
//! a drift-fix PR. Instead this test gets real, load-bearing leverage for
//! free out of a fact that's already true of the structs it covers: none of
//! [`SaveState`], [`SuspendedFlow`], or [`WakePolicy`] derives `Default`, so
//! every literal construction of one names every one of its fields — adding
//! or removing a field is a compile error at every call site, including the
//! ones below. That forces this file to be touched (and, per the assertions
//! below, to keep passing) in the same commit that changes the Rust shape.
//!
//! What it checks: the exact set of top-level keys the struct actually
//! serializes to (derived from a real instance via `serde_json`, never
//! hand-copied) each appear, verbatim, inside the matching TS `interface`
//! body. What it does NOT check: TS field *types*, optionality spelled
//! correctly (`?` vs required), or a field renamed identically on both
//! sides (a coincidental match would slip through). It is a floor, not a
//! ceiling — cheap insurance against the exact failure mode #2313 and #2311
//! both were ("Rust struct grew a field, nobody told the TS mirror"), not a
//! substitute for a real generated-or-asserted schema if one is ever
//! adopted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use brink_format::{
    DefinitionId, DefinitionTag, SAVE_FORMAT_VERSION, SUSPENDED_FLOW_SECTION_VERSION, SaveState,
    SuspendedFlow, Value, VisitEntry, WakePolicy, WakeSource,
};

fn some_def_id(hash: u64) -> DefinitionId {
    DefinitionId::new(DefinitionTag::GlobalVar, hash)
}

/// A fully-populated `SaveState` — every `Option`/collection non-empty, so
/// serializing it yields every field this struct can ever put on the wire
/// (`skip_serializing_if` fields included).
fn sample_save_state() -> SaveState {
    let mut globals = BTreeMap::new();
    globals.insert("score".to_string(), Value::Int(3));
    let mut global_ids = BTreeMap::new();
    global_ids.insert("score".to_string(), some_def_id(1));

    SaveState {
        version: SAVE_FORMAT_VERSION,
        globals,
        global_ids,
        visits: vec![VisitEntry {
            id: some_def_id(2),
            path: Some("forest.clearing".to_string()),
            count: 1,
        }],
        turns: vec![VisitEntry {
            id: some_def_id(3),
            path: None,
            count: 2,
        }],
        turn_index: 4,
        rng_seed: 5,
        previous_random: 6,
        suspended: Some(sample_suspended_flow()),
    }
}

fn sample_suspended_flow() -> SuspendedFlow {
    let mut pending_element = BTreeMap::new();
    pending_element.insert("speaker".to_string(), "Ari".to_string());

    SuspendedFlow {
        version: SUSPENDED_FLOW_SECTION_VERSION,
        current: some_def_id(10),
        return_stack: vec![some_def_id(11)],
        frame: Value::Int(7),
        wake: WakePolicy {
            site: some_def_id(12),
            condition: Some(some_def_id(13)),
            source: WakeSource::Condition,
        },
        next_block_id: 8,
        pending_element,
    }
}

/// Path to the TS mirror, relative to this crate's manifest dir
/// (`crates/internal/brink-format`).
fn ts_mirror_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../packages/wasm-types/src/index.ts")
}

/// Extract the body of `export interface <name> { ... }` (the first line
/// starting with a lone `}` after the opener — every interface in this file
/// is flat, single-line members, so no nested top-level brace can appear).
fn interface_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("export interface {name} {{");
    let missing_msg = format!(
        "packages/wasm-types/src/index.ts has no `{marker}` — the TS mirror for `{name}` is \
         missing entirely"
    );
    let start = source.find(&marker).expect(&missing_msg);
    let body_start = start + marker.len();
    let rest = &source[body_start..];
    let unterminated_msg =
        format!("`{name}` interface in packages/wasm-types/src/index.ts has no closing brace");
    let end = rest.find("\n}").expect(&unterminated_msg);
    &rest[..end]
}

/// Assert every `field` from `keys` appears, verbatim, as an actual TS
/// interface member inside `body` — spelled `field:` or `field?:` (TS's
/// optional-member spelling).
///
/// Scans line-by-line rather than a whole-body substring search: every
/// interface in this file is one member per line, so a bare
/// `body.contains("{key}:")` over the full text (doc comments included)
/// false-passes two ways — a future field named `id`/`block_id` would be
/// "found" by an unrelated `next_block_id:` member, and doc-comment prose
/// of the form `word:` (e.g. "already true of the structs it covers:")
/// would satisfy a field that was never declared. Requiring the *trimmed*
/// line itself to *start with* `field:`/`field?:` closes both holes.
fn assert_fields_present(interface_name: &str, body: &str, keys: &[&str]) {
    for key in keys {
        let required = format!("{key}:");
        let optional = format!("{key}?:");
        let found = body.lines().any(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('*') || trimmed.starts_with('/') {
                return false;
            }
            trimmed.starts_with(&required) || trimmed.starts_with(&optional)
        });
        assert!(
            found,
            "packages/wasm-types/src/index.ts's `{interface_name}` interface is missing \
             field `{key}` — the Rust struct `brink_format::{interface_name}` serializes it. \
             This is exactly the drift issue #2313/#2311 fixed; keep the TS mirror in sync \
             in the same commit that changes the Rust shape."
        );
    }
}

#[test]
fn save_state_ts_mirror_has_every_field() {
    let save = sample_save_state();
    let json = serde_json::to_value(&save).expect("serialize sample SaveState");
    let object = json
        .as_object()
        .expect("SaveState serializes to a JSON object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();

    let source = std::fs::read_to_string(ts_mirror_path()).expect(
        "read packages/wasm-types/src/index.ts — this test assumes it runs from within the \
         brink workspace checkout",
    );
    let body = interface_body(&source, "SaveState");
    assert_fields_present("SaveState", body, &keys);

    // `visits`/`turns` are nested `VisitEntry` — check its own field set the
    // same way. Use the `visits[0]` sample, which has `path: Some(..)`, so
    // its JSON carries all three fields (`id`, `path`, `count`).
    let visit_json = serde_json::to_value(&save.visits[0]).expect("serialize sample VisitEntry");
    let visit_object = visit_json
        .as_object()
        .expect("VisitEntry serializes to a JSON object");
    let mut visit_keys: Vec<&str> = visit_object.keys().map(String::as_str).collect();
    visit_keys.sort_unstable();
    let visit_body = interface_body(&source, "VisitEntry");
    assert_fields_present("VisitEntry", visit_body, &visit_keys);
}

#[test]
fn suspended_flow_ts_mirror_has_every_field() {
    let suspended = sample_suspended_flow();
    let json = serde_json::to_value(&suspended).expect("serialize sample SuspendedFlow");
    let object = json
        .as_object()
        .expect("SuspendedFlow serializes to a JSON object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();

    let source = std::fs::read_to_string(ts_mirror_path()).expect(
        "read packages/wasm-types/src/index.ts — this test assumes it runs from within the \
         brink workspace checkout",
    );
    let body = interface_body(&source, "SuspendedFlow");
    assert_fields_present("SuspendedFlow", body, &keys);

    // `wake` is a nested `WakePolicy` — check its own field set the same way.
    let wake_json = serde_json::to_value(suspended.wake).expect("serialize sample WakePolicy");
    let wake_object = wake_json
        .as_object()
        .expect("WakePolicy serializes to a JSON object");
    let mut wake_keys: Vec<&str> = wake_object.keys().map(String::as_str).collect();
    wake_keys.sort_unstable();
    let wake_body = interface_body(&source, "WakePolicy");
    assert_fields_present("WakePolicy", wake_body, &wake_keys);
}
