// The Compound — Phase 1a: the alarm escalation logic, ported to ink.
//
// This file is the ink counterpart of `src/alarm.rs`. It owns the escalation
// STATE (two globals) and the escalation LOGIC (four functions) with exactly
// the Rust module's semantics. The engine never mutates these globals directly:
// guards/cameras emit `SpottedEvent` / `GlobalAlarm` messages, and the ink-mode
// driver (`src/ink_alarm.rs`) folds each frame's events into ink by *calling
// these functions* (`call_ink_function`) — the single-writer world-policy seam,
// just expressed in ink instead of Rust.
//
// Semantics parity contract (asserted frame-by-frame in `ink_alarm::tests`):
//   * spotting soft-caps at SOFT_CAP (1.9, tier 1) — seeing you never sweeps.
//   * only a guard reaching an alarm panel (trigger_global) jumps to 3.0.
//   * spotting never lowers an already-higher level (e.g. a decaying global).
//   * decay bleeds DECAY_PER_SEC/sec and clears the `global` latch at zero.
//
// brink stores floats as f32 — the same width as `alarm.rs` — so the
// arithmetic below is bit-for-bit comparable to the Rust baseline.

VAR alarm_level = 0.0
VAR alarm_global = false

CONST MAX_LEVEL = 3.0
CONST SOFT_CAP = 1.9
CONST DECAY_PER_SEC = 0.25
// Mirrors `f32::EPSILON`, the threshold `alarm.rs` clears the global latch at.
CONST LEVEL_EPSILON = 0.00000011920929

// The flow has no visible narration; it exists only as a home for the state
// and functions the engine calls into. It parks immediately.
-> DONE

// Raise the alarm from *spotting*, capped at SOFT_CAP. Never lowers an
// already-higher level (mirrors `Alarm::escalate_spotting`).
=== function escalate_spotting(amount) ===
{ alarm_level < SOFT_CAP:
    ~ alarm_level = MIN(alarm_level + amount, SOFT_CAP)
}

// Raise the *global* alarm — a guard reached an alarm panel
// (mirrors `Alarm::trigger_global`).
=== function trigger_global() ===
~ alarm_level = MAX_LEVEL
~ alarm_global = true

// Bleed the alarm down over `dt` seconds (mirrors `Alarm::decay`).
=== function decay(dt) ===
~ alarm_level = MAX(alarm_level - DECAY_PER_SEC * dt, 0.0)
{ alarm_level < LEVEL_EPSILON:
    ~ alarm_global = false
}

// Reset to calm when a new round starts (mirrors `Alarm::reset`).
=== function alarm_reset() ===
~ alarm_level = 0.0
~ alarm_global = false
