//! Per-frame behavior-system timing.
//!
//! Phase 1 of the drive-app plan migrates each entity archetype's behavior
//! from Rust to ink one module at a time. To make the "what does it cost?"
//! question answerable, every AI/behavior system in this demo records its own
//! wall-clock duration into [`BehaviorTimings`], and the HUD prints the
//! per-system microseconds. That readout is the Rust-side control number the
//! ink port is measured against.
//!
//! Because each behavior system takes `ResMut<BehaviorTimings>`, the Bevy
//! scheduler runs them sequentially rather than in parallel. That is a
//! deliberate trade: the demo is not throughput-bound (a dozen guards, a few
//! cameras, some rats), and serial execution keeps the timing numbers directly
//! comparable and summable into a single "total behavior cost" figure.

use bevy::prelude::*;
use core::time::Duration;

/// Wall-clock cost of each behavior system, refreshed every frame.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct BehaviorTimings {
    pub guards: Duration,
    pub cameras: Duration,
    pub doors: Duration,
    pub alarm: Duration,
    pub rats: Duration,
}

impl BehaviorTimings {
    /// Sum of every recorded behavior system for this frame.
    pub fn total(&self) -> Duration {
        self.guards + self.cameras + self.doors + self.alarm + self.rats
    }
}

/// Format a duration as right-aligned integer microseconds, e.g. `"  742 µs"`.
pub fn micros(d: Duration) -> String {
    format!("{:>5} µs", d.as_micros())
}
