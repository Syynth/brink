// The Compound — Phase 1b: doors/switches ported to ink as the minimal
// REACTIVE entity (README "Suggested Phase-1 migration order" #2) — "locked
// until its switch flips" is the "await on a value" seam, ported via the
// host BH-4 wake surface (`FlowSleep`/wake_when, `docs/effects-spec.md`
// §13.1) rather than an ink-level `await` (that construct stays fenced
// until FS-3r; this port is the planned before/after comparison datapoint
// for when it lands).
//
// One flow instance per LOCKED door (`src/ink_doors.rs` attaches the flow
// straight onto the same entity `doors.rs::spawn_doors_from_layout` already
// spawns — the flow entity IS the door entity). Every door starts DORMANT:
// parked at entry, costing zero per turn (Collect skips it — §13.1 point 1)
// until `should_open` goes true.
//
// The condition reads LIVE engine state through a `bind_brink_query`
// world-access binding (`is_switch_on`, reading the `Switch` matching this
// door's own `switch_id` off the `Door` component on the calling flow
// entity) — no manual per-frame mirror is needed here, unlike the alarm's
// `call_ink_function` write seam (Phase 1a): the direction is engine state
// -> ink read, not ink logic -> engine state.
//
// On wake it runs its one and only turn (`WakeArming::Once`) and parks for
// good at `-> END`; the host's read seam (`ink_doors::ink_door_sync_system`)
// treats a flow that reached `Ended` as "door open" — permanently. This is a
// deliberate SIMPLIFICATION versus the Rust baseline (`doors::door_sync_system`,
// which is fully bidirectional: it re-locks a door if its switch is flipped
// back off). See MIGRATION.md for why, and the divergence test that proves
// it's a documented choice, not an oversight.

EXTERNAL is_switch_on()

Door unlocks.
-> END

=== function should_open() ===
~ return is_switch_on()
