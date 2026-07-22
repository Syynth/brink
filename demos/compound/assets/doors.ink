// The Compound — Phase 1b: doors/switches ported to ink as the minimal
// REACTIVE entity (README "Suggested Phase-1 migration order" #2) — "locked
// until its switch flips" is the "await on a value" seam, ported via the
// host BH-4 wake surface (`FlowSleep`/wake_when, `docs/effects-spec.md`
// §13.1) rather than an ink-level `await` (that construct stays fenced
// until FS-3r; this port is the planned before/after comparison datapoint
// for when it lands).
//
// One flow instance per door (`src/ink_doors.rs::attach_ink_door_flows`
// attaches the flow straight onto the same entity `doors.rs::spawn_doors_from_layout`
// already spawns — the flow entity IS the door entity). Every door starts
// DORMANT: parked at entry, costing zero per turn (Collect skips it — §13.1
// point 1) until `should_open` goes true.
//
// The condition reads LIVE engine state through a `bind_brink_query`
// world-access binding (`is_switch_on`, reading the `Switch` matching this
// door's own `switch_id` off the `Door` component on the calling flow
// entity) — no manual per-frame mirror is needed here, unlike the alarm's
// `call_ink_function` write seam (Phase 1a): the direction is engine state
// -> ink read, not ink logic -> engine state.
//
// FULLY REVERSIBLE (`WakeArming::Latch`, issue #1081): the flow never ends —
// each wake prints the current switch reading and loops back via `-> DONE`
// / `-> door_watch` (the same self-looping idiom `bevy-brink`'s own
// `LOOPING_STORY` test fixture uses). The host's `Latch` arming mode does
// the edge detection: it wakes only on a transition (switch-on, then
// switch-off, then switch-on again, ...), so `should_open` stays a plain
// level predicate — no ink-side "was I previously open" state is needed to
// detect the direction (`ink_doors::ink_door_sync_system` reads that
// directly off the policy via `FlowSleep::latch_waiting_for`). This closes
// the divergence from the Rust baseline (`doors::door_sync_system`, fully
// bidirectional) that an earlier `WakeArming::Once` version of this port
// had to accept — see `MIGRATION.md`'s G5 entry and the parity test that
// proves it.

EXTERNAL is_switch_on()

-> door_watch

=== door_watch ===
{ is_switch_on():
    Door unlocks.
- else:
    Door locks.
}
-> DONE
-> door_watch

=== function should_open() ===
~ return is_switch_on()
