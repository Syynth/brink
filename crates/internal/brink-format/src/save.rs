//! Persistent, name-keyed save state for a story's game state.
//!
//! [`SaveState`] is the **durable** save format: globals, visit/turn counts,
//! turn index, and RNG, keyed by stable identities (variable name; scope
//! [`DefinitionId`]). It captures *game state only* — not execution position
//! (call stack / PC) — via name-stable identities, so a save survives a story
//! recompile/patch as long as the relevant names/paths are unchanged. The
//! runtime (`Story::save_state` / `load_state`) produces and reconciles it.
//! See `docs/external-binding-foundation.md`.
//!
//! **FS-1** (`docs/flow-suspension-spec.md` §2/§9, format-only slice) adds
//! [`Self::suspended`]: a `FlowFrame` — the durable representation of *this*
//! flow's execution position when parked mid-tunnel/mid-`await` — using the
//! same name-stable-identity discipline as everything else here (container
//! [`DefinitionId`]s, never instruction offsets), so it survives a recompile
//! exactly like globals/visits do. This is currently a pure format addition:
//! `Story::save_state`/`load_state` (this module's runtime counterpart)
//! always produce/consume `None` — the compiler synthesis that populates a
//! live frame (FS-2) and the runtime spill/restore that produces or consumes
//! one (FS-3) are later slices.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{DefinitionId, Value};

/// Current [`SaveState`] format version. Bump when the *format* changes
/// (independent of the story's own content); `version` lets a loader migrate.
pub const SAVE_FORMAT_VERSION: u16 = 1;

/// A persistent, name-keyed snapshot of a story's game state.
///
/// Globals are keyed by variable name; visit/turn counts by scope
/// [`DefinitionId`] (which serializes as a stable `"$tt_hash"` string), with an
/// advisory author path attached when the scope is named.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SaveState {
    /// Save-format version (see [`SAVE_FORMAT_VERSION`]).
    pub version: u16,
    /// Global variables by name. `BTreeMap` for deterministic serialization.
    pub globals: BTreeMap<String, Value>,
    /// Each saved global's compiled `DefinitionId` at save time, keyed by
    /// the same name as [`Self::globals`] (M-3 rehydration miss-path lookup,
    /// `docs/modules-spec.md` §5). A VAR/CONST/LIST living in a **declared**
    /// module hashes its identity as `(module, name)`, so a bare name alone
    /// can't reconstruct the id a `#@was` alias-table entry was compiled
    /// against — this is the "module qualifier" the miss path needs,
    /// round-tripped as the id itself rather than the module name, so no
    /// hashing scheme has to be re-derived at load time. Consulted only when
    /// a saved global's name no longer matches any current global slot;
    /// absent entries (older saves predating this field) simply fall back
    /// to the pre-M-3 unknown-global report, same tolerant-of-patches
    /// behavior as before. `#[serde(default)]` so an older save missing
    /// this field entirely still deserializes.
    #[serde(default)]
    pub global_ids: BTreeMap<String, DefinitionId>,
    /// Visit counts by scope id, sorted by id for deterministic output.
    pub visits: Vec<VisitEntry>,
    /// Turn-since counts by scope id, sorted by id.
    pub turns: Vec<VisitEntry>,
    /// Global turn index.
    pub turn_index: u32,
    /// RNG seed.
    pub rng_seed: i32,
    /// Last drawn random value (so the RNG sequence resumes correctly).
    pub previous_random: i32,
    /// This flow's `FlowFrame` when parked mid-tunnel/mid-`await`
    /// (`docs/flow-suspension-spec.md` §2/§9, FS-1). Absent when the flow
    /// isn't suspended — an ordinary save at a turn boundary, choice, or
    /// `-> END` has no execution position to capture, same as before this
    /// field existed. `#[serde(default)]` so an older save predating this
    /// field still deserializes; `skip_serializing_if` keeps an unsuspended
    /// save's wire form byte-identical to before (no `"suspended": null`
    /// noise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended: Option<SuspendedFlow>,
}

/// One visit/turn-count entry: a scope id and its count, plus (when the scope
/// is a named knot/stitch) an advisory author path for human inspection. The
/// `id` is the load key; `path` is cosmetic.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VisitEntry {
    /// The counted scope's definition id (load key).
    pub id: DefinitionId,
    /// Author path for a named scope, e.g. `"forest.clearing"`. Absent for
    /// anonymous counted containers (gathers, choice points).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The count.
    pub count: u32,
}

/// Current suspended-flow section version, versioned independently of
/// [`SAVE_FORMAT_VERSION`] (`docs/flow-suspension-spec.md` §9 FS-1: "one new
/// suspended-flow section, section-locally versioned"). Bump when the
/// [`SuspendedFlow`] shape itself changes; the rest of [`SaveState`] is
/// unaffected.
pub const SUSPENDED_FLOW_SECTION_VERSION: u16 = 1;

/// The `FlowFrame` — a parked flow's durable, recompile-stable representation
/// (`docs/flow-suspension-spec.md` §2, RULED). No instruction offsets ever
/// serialize; recompile-stability rides container/[`DefinitionId`] identity,
/// the same contract as the rest of [`SaveState`], `#@was`, and fn tokens.
///
/// FS-1 is format-only: this type's writer and reader are exercised today
/// only by round-trip tests (`crates/internal/brink-format/tests/`). The
/// compiler synthesis that populates [`Self::frame`] (FS-2) and the runtime
/// spill/restore that produces or consumes a live value (FS-3) are later
/// slices (`docs/flow-suspension-spec.md` §9).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SuspendedFlow {
    /// Section-local format version (see [`SUSPENDED_FLOW_SECTION_VERSION`]).
    pub version: u16,
    /// The container the flow is currently parked inside (name-stable; §2
    /// point 1).
    pub current: DefinitionId,
    /// The tunnel-return chain, outermost first (name-stable; §2 point 2).
    /// Depth-capped at the runtime layer (FS-3; §7 "recursive awaiting
    /// tunnels" — a park-depth limit, sibling of the VM step limit) — the
    /// format itself imposes no bound.
    pub return_stack: Vec<DefinitionId>,
    /// Every local crossing a yield, name-keyed — a plain [`Value`]
    /// (typically [`Value::map`]), serialized by the existing `Value`
    /// encoders with no new wire representation (§2 point 3). Frame-shape
    /// drift across a recompile (a tunnel's crossing-locals set changes)
    /// rides the standard name-keyed rehydration discipline — missing
    /// field → default, extra field → dropped, renamed field → treated as
    /// missing, each reported rather than silently swallowed. This is
    /// tolerant *decode*, which is FS-3 runtime scope; FS-1's job is making
    /// sure the encoding is name-keyed (never positional) so that decode is
    /// possible at all — see this module's
    /// `suspended_flow_frame_drift_is_representable` test.
    pub frame: Value,
    /// The wake policy governing when the parked flow resumes (§2 point 4).
    pub wake: WakePolicy,
}

/// A parked flow's wake policy (`docs/flow-suspension-spec.md` §2 point 4):
/// await-site id + condition fn token + host-source discriminant, all
/// name-stable. See `docs/effects-spec.md` §13.1 for the wake contract this
/// plugs into (persistent-by-default policies, `wake_once`, host
/// cancellation) — that contract's runtime enforcement is FS-3/FS-4 scope;
/// this type only carries the wire shape.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WakePolicy {
    /// The `await` site's synthesized resume-container id — site-stable
    /// identity, so the general anonymous-fn identity problem does not
    /// apply here (`docs/flow-suspension-spec.md` §3).
    pub site: DefinitionId,
    /// The condition's compiler-synthesized pure-fn token
    /// (`docs/flow-suspension-spec.md` §3: "direct-expression conditions
    /// capture as compiler-synthesized pure fns"). Absent for a policy whose
    /// [`Self::source`] is [`WakeSource::Host`] — a host-driven wake trigger
    /// (next-frame, external event) has no compiled ink condition fn to
    /// token (§3: "an ink spelling for them is PROPOSED-only").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<DefinitionId>,
    /// Where the wake nudge originates.
    pub source: WakeSource,
}

/// The wake policy's host-source discriminant
/// (`docs/flow-suspension-spec.md` §2 point 4; `docs/effects-spec.md`
/// §13.1). FS-1 records the discriminant only — the host-side wake plumbing
/// (`wake_when`, dormant spawn, cancellation-to-false) is FS-4 scope; today
/// only [`Self::Condition`] is ever compiler-produced, but the format
/// reserves [`Self::Host`] for the host-driven wake sources §3 names as a
/// future (PROPOSED-only) ink spelling, so the wire shape doesn't need a
/// breaking change to add it later.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeSource {
    /// An ink-authored `await <condition>` / `while await <condition>` — the
    /// compiled condition fn ([`WakePolicy::condition`]) drives
    /// re-evaluation per the wake contract (`docs/effects-spec.md` §13.1).
    Condition,
    /// A host-driven wake source (e.g. next-frame) with no compiled ink
    /// condition fn — the host owns re-evaluation directly (§3, §13.1).
    Host,
}

/// What `Story::load_state` couldn't apply, so a host can surface it rather
/// than have data silently vanish. Globals whose name no longer exists are
/// **dropped** (no slot to hold them) and reported here. Visit/turn counts are
/// never dropped — counts for scopes the current program lacks are retained
/// harmlessly (unused until/unless the scope returns), so they aren't reported
/// *except* when the miss-path alias lookup (M-3, docs/modules-spec.md §5)
/// still can't place them — see `unresolved_renames`.
#[derive(Default, Clone, Debug, PartialEq, Serialize)]
pub struct LoadReport {
    /// Saved global names with no matching global in the current program.
    pub unknown_globals: Vec<String>,
    /// M-3 rehydration miss-path teaching messages (docs/modules-spec.md
    /// §5): a saved fn token, divert value, or visit/turn-count key that
    /// didn't match the current program even after consulting the compiled
    /// `#@was` alias table. Only populated for a program that actually
    /// carries alias-table entries (i.e. uses `#@was` somewhere) — an
    /// ordinary content edit with no rename directive stays silent, same
    /// as before M-3.
    pub unresolved_renames: Vec<String>,
}

impl LoadReport {
    /// Whether the load applied cleanly (nothing dropped, nothing left
    /// unresolved after the rename miss-path lookup).
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unknown_globals.is_empty() && self.unresolved_renames.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DefinitionTag;
    use crate::value::{MapKey, OrderedMap};

    /// A `SaveState` whose globals hold collection values must round-trip
    /// through serde as full trees — no lossy fold to null (T1a-3 / #525). The
    /// `globals` map is a `BTreeMap` (name-keyed, deterministic order), and
    /// each `Value` serializes through its own derived tree encoding, so a
    /// map-of-array global survives a save/load byte-for-byte.
    #[test]
    fn save_state_round_trips_collection_globals() {
        let inventory: OrderedMap = [
            (
                MapKey::from("weapons"),
                Value::array(vec![Value::from("sword"), Value::from("bow")]),
            ),
            (MapKey::from("gold"), Value::Int(42)),
        ]
        .into_iter()
        .collect();

        let mut globals = BTreeMap::new();
        globals.insert(String::from("inventory"), Value::map(inventory));
        globals.insert(
            String::from("scores"),
            Value::array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        );

        let save = SaveState {
            version: SAVE_FORMAT_VERSION,
            globals,
            global_ids: BTreeMap::new(),
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 0,
            rng_seed: 7,
            previous_random: 0,
            suspended: None,
        };

        let json = serde_json::to_string(&save).expect("serialize save");
        let back: SaveState = serde_json::from_str(&json).expect("deserialize save");
        assert_eq!(back, save);
    }

    /// A `SaveState` with no suspended flow serializes with no `"suspended"`
    /// key at all (`skip_serializing_if`) — an unsuspended save's wire form
    /// is byte-identical to the pre-FS-1 shape, and an older save missing the
    /// key entirely still deserializes via `#[serde(default)]`.
    #[test]
    fn suspended_absent_by_default_and_omitted_from_wire() {
        let save = SaveState {
            version: SAVE_FORMAT_VERSION,
            globals: BTreeMap::new(),
            global_ids: BTreeMap::new(),
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 0,
            rng_seed: 0,
            previous_random: 0,
            suspended: None,
        };

        let json = serde_json::to_string(&save).expect("serialize save");
        assert!(
            !json.contains("suspended"),
            "unsuspended save must omit the key entirely: {json}"
        );

        // An older save's JSON, predating the field, deserializes unchanged.
        let old_json = r#"{"version":1,"globals":{},"global_ids":{},"visits":[],"turns":[],"turn_index":0,"rng_seed":0,"previous_random":0}"#;
        let back: SaveState = serde_json::from_str(old_json).expect("deserialize old save");
        assert_eq!(back, save);
    }

    /// A `SuspendedFlow` — current container, return stack, name-keyed frame
    /// record, wake policy — round-trips through `serde_json` byte-for-byte
    /// (`docs/flow-suspension-spec.md` §2, FS-1). Covers both `WakeSource`
    /// variants: `Condition` (a compiled condition fn token present) and
    /// `Host` (no condition fn — §3's PROPOSED-only host wake spelling).
    #[test]
    fn suspended_flow_round_trips() {
        let mut frame = OrderedMap::new();
        frame.insert(MapKey::from("hp"), Value::Int(7));
        frame.insert(
            MapKey::from("party"),
            Value::array(vec![Value::from("hero"), Value::from("mage")]),
        );

        let suspended_condition = SuspendedFlow {
            version: SUSPENDED_FLOW_SECTION_VERSION,
            current: DefinitionId::new(DefinitionTag::Address, 1),
            return_stack: vec![
                DefinitionId::new(DefinitionTag::Address, 2),
                DefinitionId::new(DefinitionTag::Address, 3),
            ],
            frame: Value::map(frame.clone()),
            wake: WakePolicy {
                site: DefinitionId::new(DefinitionTag::Address, 4),
                condition: Some(DefinitionId::new(DefinitionTag::ExternalFn, 5)),
                source: WakeSource::Condition,
            },
        };
        let save_condition = SaveState {
            version: SAVE_FORMAT_VERSION,
            globals: BTreeMap::new(),
            global_ids: BTreeMap::new(),
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 3,
            rng_seed: 1,
            previous_random: 0,
            suspended: Some(suspended_condition),
        };

        let json = serde_json::to_string(&save_condition).expect("serialize save");
        let back: SaveState = serde_json::from_str(&json).expect("deserialize save");
        assert_eq!(back, save_condition);

        let suspended_host = SuspendedFlow {
            version: SUSPENDED_FLOW_SECTION_VERSION,
            current: DefinitionId::new(DefinitionTag::Address, 1),
            return_stack: Vec::new(),
            frame: Value::map(frame),
            wake: WakePolicy {
                site: DefinitionId::new(DefinitionTag::Address, 4),
                condition: None,
                source: WakeSource::Host,
            },
        };
        let save_host = SaveState {
            suspended: Some(suspended_host),
            ..save_condition
        };

        let json = serde_json::to_string(&save_host).expect("serialize save");
        let back: SaveState = serde_json::from_str(&json).expect("deserialize save");
        assert_eq!(back, save_host);
        assert!(
            !json.contains("condition"),
            "absent condition must be omitted, not null: {json}"
        );
    }

    /// Frame-shape drift (`docs/flow-suspension-spec.md` §7): the compiler
    /// changes a tunnel's crossing-locals set between a save and a later
    /// load (a field is dropped, a field is added, a field is renamed). FS-1
    /// is encode-side only — the tolerant *decode* (missing → default,
    /// extra → dropped, renamed → treated as missing, each reported) is
    /// FS-3 runtime scope — but the frame record must be encoded so that
    /// decode is *possible*: because `frame` is an ordinary name-keyed
    /// `Value::Map` (never a positional tuple), a shape change on one side
    /// never desyncs field identity with the other — each entry carries its
    /// own name, so added/missing/renamed keys are ordinary map diffs, not a
    /// decode failure. This proves the encoding carries what FS-3 needs
    /// without implementing FS-3's reconciliation itself.
    #[test]
    fn suspended_flow_frame_drift_is_representable() {
        let mut old_shape = OrderedMap::new();
        old_shape.insert(MapKey::from("hp"), Value::Int(7));
        old_shape.insert(MapKey::from("gold"), Value::Int(100));

        let old = SuspendedFlow {
            version: SUSPENDED_FLOW_SECTION_VERSION,
            current: DefinitionId::new(DefinitionTag::Address, 1),
            return_stack: Vec::new(),
            frame: Value::map(old_shape),
            wake: WakePolicy {
                site: DefinitionId::new(DefinitionTag::Address, 2),
                condition: Some(DefinitionId::new(DefinitionTag::ExternalFn, 3)),
                source: WakeSource::Condition,
            },
        };
        let json = serde_json::to_string(&old).expect("serialize old-shape frame");

        // The author edits the tunnel: `gold` is dropped, `hp` is renamed to
        // `health`, and a new `mana` local is added — none of this is
        // reflected in `json` above, simulating a save made against the old
        // shape being loaded against a recompiled program with the new one.
        let mut new_shape = OrderedMap::new();
        new_shape.insert(MapKey::from("health"), Value::Int(0));
        new_shape.insert(MapKey::from("mana"), Value::Int(50));
        let new_shape_flow = SuspendedFlow {
            frame: Value::map(new_shape),
            ..old.clone()
        };
        let new_shape_json =
            serde_json::to_string(&new_shape_flow).expect("serialize new-shape frame");

        // Both the old-shaped and new-shaped saves decode cleanly — the
        // frame is a generic name-keyed map, not a fixed-arity tuple, so
        // neither shape is a parse error. This is exactly the property FS-3's
        // tolerant reconciliation (missing/extra/renamed) needs to build on.
        let decoded_old: SuspendedFlow = serde_json::from_str(&json).expect("decode old shape");
        let decoded_new: SuspendedFlow =
            serde_json::from_str(&new_shape_json).expect("decode new shape");
        assert_eq!(decoded_old, old);
        assert_eq!(decoded_new, new_shape_flow);

        // The old save's frame keys are untouched by the shape change
        // happening elsewhere — a future FS-3 reconciler can diff
        // `decoded_old.frame` against the *current* program's expected
        // key set field-by-field, because each value carries its own name.
        let Value::Map(m) = &decoded_old.frame else {
            panic!("expected a map frame");
        };
        assert_eq!(m.get(&MapKey::from("hp")), Some(&Value::Int(7)));
        assert_eq!(m.get(&MapKey::from("gold")), Some(&Value::Int(100)));
    }
}
