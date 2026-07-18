// The Compound — Phase 1c: security cameras, the pure sweep-and-detect loop.
//
// This file is the ink counterpart of `src/cameras.rs`'s `camera_ai_system`.
// Unlike the alarm (Phase 1a, one shared flow) or doors (Phase 1b, no VARs at
// all), EVERY security camera in a round gets its own flow instance under
// the *same* `CamerasStory` marker (`src/ink_cameras.rs`: the flow entity IS
// the camera entity, spawned per `SecurityCamera`). All flows under one
// marker share ONE `BrinkGlobals<CamerasStory>` World — so a plain `VAR`
// here would be one sweep phase shared by every camera in the compound.
// `#@local` is the flow-private storage class that avoids that: each
// camera's `phase`/`facing` lives in its own flow's `FlowLocal`, invisible to
// every other camera's flow (`docs/directive-annotations-spec.md` §3).
//
// Semantics parity contract (asserted frame-by-frame in `ink_cameras::tests`):
//   * `phase` accumulates exactly like `SecurityCamera.phase` (`+= dt * speed`).
//   * `facing` is `center_angle + SWEEP_HALF * sin(phase)`, bit-for-bit the
//     same formula `camera_ai_system` uses (ink floats are f32, matching Rust).
//   * detection is cone-in-range AND wall-line-of-sight, exactly
//     `world::point_in_cone` + `world::raycast_clear` — done by the engine,
//     not ink (vector/geometry math is icebox #827; the whole point of the
//     `sees_player` world-access binding is to keep that math in Rust).
//
// `center_angle` and the (loadout/stealth-adjusted) `range` are NOT ink state
// at all — `src/ink_cameras.rs` passes them as fresh arguments to
// `sweep_and_detect` every call, computed by the host exactly like the Rust
// baseline does. That is "no per-entity memory to marshal" (README's Phase-1
// migration order, #3): the only thing ink actually remembers across frames
// is the two `#@local` cells above, and even `facing` only round-trips back
// to the host via `camera_facing()` for the debug cone gizmo / parity tests —
// gameplay only needs the boolean this function returns.
//
// NOT USED: `#[derive(BrinkCommand)]`. The plan (`docs/drive-app-plan.md`
// §3) sketched cameras raising the alarm via an ink→engine command. That
// turned out to be unreachable from this port's drive shape: cameras need a
// per-frame **function call** (`call_ink_function`), not a turn-stepped
// story, and `call_ink_function`'s evaluation handler only resolves `pure`
// and `bind_brink_query` bindings — `bind_brink_command` triggers are
// buffered/flushed only on the serial (`BrinkHandler`) story-stepping path.
// Calling a `bind_brink_command`-bound EXTERNAL from here silently falls
// back instead of firing the event. Filed as a new drive-it issue (see
// MIGRATION.md); this port raises the alarm the same way `ink_alarm.rs`
// already does — the host reads this function's boolean return and writes
// `SpottedEvent` itself.

#@local
VAR phase = 0.0
#@local
VAR facing = 0.0

CONST SWEEP_HALF = 0.7
CONST SWEEP_SPEED = 1.1

EXTERNAL sin(x)
EXTERNAL sees_player(facing, range)

// The flow has no visible narration; it exists only as a home for the
// per-camera sweep state and the `sweep_and_detect`/`camera_facing`
// functions the engine calls into. It parks immediately.
-> DONE

// Advance the sweep by `dt` seconds around `center_angle`, then report
// whether the camera currently sees the player within `range` (mirrors
// `camera_ai_system`'s per-frame phase/facing update + cone-and-raycast
// check). Returns true exactly the frames the Rust baseline would have
// written a `SpottedEvent`.
=== function sweep_and_detect(dt, center_angle, range) ===
~ phase = phase + dt * SWEEP_SPEED
~ facing = center_angle + SWEEP_HALF * sin(phase)
~ return sees_player(facing, range)

// The read seam: the current sweep facing, for the host's debug cone gizmo
// and the frame-by-frame parity test. Gameplay itself never needs this —
// only `sweep_and_detect`'s boolean does.
=== function camera_facing() ===
~ return facing
