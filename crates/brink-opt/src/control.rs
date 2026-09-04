//! Negative-control passes — deliberately wrong transforms whose only purpose
//! is to be **caught** by the fence.
//!
//! # Why these exist
//!
//! v1 ships an empty pass list, so the fence's trace-equality, line-identity,
//! idempotence and stability checks are all green. A fence that compared
//! *nothing at all* would be exactly as green. These passes are what make the
//! greenness evidence: each one trips a specific obligation, so every check is
//! demonstrated to go red on demand through the same `judge()` seam the real
//! fence uses.
//!
//! # The design fact that makes them independent
//!
//! `trace::line_identity_diff` compares only `(scope_id, line_index,
//! source_hash)` — it never reads `LineEntry::content`. And `source_hash` is a
//! translation-identity key that **the runtime never reads**. So `content` and
//! `source_hash` are orthogonal handles on the two semantic oracles:
//!
//! | pass | edits | trace | identity | idempotent | stable |
//! |---|---|---|---|---|---|
//! | [`Retext`] | `content` | **DIRTY** | clean | yes | yes |
//! | [`Rehash`] | `source_hash` | clean | **DIRTY** | yes | yes |
//! | [`Grow`] | appends to `content` | dirty | clean | **NO** | yes |
//! | [`Drift`] | `audio_ref` | clean | clean | no | **NO** |
//!
//! Every column has a red cell, so no assertion in the fence can be quietly
//! dead. `Retext`/`Rehash` are the pair proving the two semantic oracles are
//! *independently* wired — a single control tripping both would prove neither.
//! `Drift` is what justifies keeping the byte-level checks at all: it trips
//! nothing a semantic oracle can see.
//!
//! # Why passes and not bare `fn(&mut StoryData)` helpers
//!
//! A helper would prove the diff functions detect edits, but not that the fence
//! routes the artifact through [`crate::optimize`] at all — a fence whose
//! `post` was accidentally `pre` would still look green. Driving the controls
//! through the real `optimize(&mut data, &OptConfig { passes })` proves the
//! whole wire: read → optimize → write → diff.
//!
//! # Why the guard is a runtime assertion
//!
//! These are behind `feature = "test-control"`, which is never in `default`.
//! That is not the safety property, though: `cargo test --workspace` unifies
//! features, so the feature is ON for every `brink-opt` in that build graph.
//! The real guard is [`crate::OptConfig::defaults`], which asserts no default
//! pass is named `control:*`.

use core::sync::atomic::{AtomicUsize, Ordering};

use brink_format::{LineContent, LineFlags, StoryData};

use crate::{Pass, PassOutcome};

/// Every control pass's name, for the disjointness test.
pub const ALL: [&str; 4] = [Retext::NAME, Rehash::NAME, Grow::NAME, Drift::NAME];

/// Text no real story can contain: two Unicode control pictures around a word.
const SENTINEL: &str = "\u{2400}control\u{2401}";

/// A hash no `content_hash` output can plausibly collide with.
const SENTINEL_HASH: u64 = 0xC0FF_EE00_C0FF_EE00;

/// Replaces every line's rendered text, leaving translation identity alone.
///
/// Trips the **trace** differential (the runtime renders text from the line
/// tables) and nothing else: `line_identity_index` never reads `content`, and
/// the `(scope_id, index, source_hash)` tuples are byte-for-byte unchanged.
///
/// `LineFlags` are recomputed deliberately. A stale `EMPTY`/`ALL_WS` flag would
/// let the output buffer suppress the sentinel and the control would go inert
/// on exactly the lines it most needs to perturb.
pub struct Retext;

impl Retext {
    const NAME: &'static str = "control:retext";
}

impl Pass for Retext {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn run(&self, story: &mut StoryData) -> PassOutcome {
        let mut changed = 0;
        for table in &mut story.line_tables {
            for entry in &mut table.lines {
                let replacement = LineContent::Plain(SENTINEL.to_owned());
                if entry.content != replacement {
                    entry.content = replacement;
                    entry.flags = LineFlags::from_content(&entry.content);
                    changed += 1;
                }
            }
        }
        PassOutcome::changed("lines retexted", changed)
    }
}

/// Rewrites every line's `source_hash`, leaving rendered text alone.
///
/// Trips **line identity** (every change is a `HashChanged`) and nothing else.
/// `source_hash` is written to `.inkb` and read back, but no runtime code path
/// consumes it — `.inkl` overlays key on it and the fence loads no overlay.
///
/// That "and nothing else" is itself worth asserting: if a future change makes
/// `source_hash` runtime-observable, this control's trace-clean assertion goes
/// red and says so.
pub struct Rehash;

impl Rehash {
    const NAME: &'static str = "control:rehash";
}

impl Pass for Rehash {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn run(&self, story: &mut StoryData) -> PassOutcome {
        let mut changed = 0;
        for table in &mut story.line_tables {
            for entry in &mut table.lines {
                if entry.source_hash != SENTINEL_HASH {
                    entry.source_hash = SENTINEL_HASH;
                    changed += 1;
                }
            }
        }
        PassOutcome::changed("hashes rewritten", changed)
    }
}

/// Appends a marker to the first line, accumulating on every run.
///
/// Trips **idempotence** — `opt(opt(A)) != opt(A)` — which is the one thing
/// `Retext` cannot, since it assigns a constant. It also trips the trace, which
/// is fine: its targeted obligation is idempotence, and the fence asserts the
/// whole row of the matrix rather than a single cell.
pub struct Grow;

impl Grow {
    const NAME: &'static str = "control:grow";
    const MARKER: char = '!';
}

impl Pass for Grow {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn run(&self, story: &mut StoryData) -> PassOutcome {
        for table in &mut story.line_tables {
            if let Some(entry) = table.lines.first_mut() {
                match &mut entry.content {
                    LineContent::Plain(s) => s.push(Self::MARKER),
                    LineContent::Template(parts) => {
                        parts.push(brink_format::LinePart::Literal(Self::MARKER.to_string()));
                    }
                }
                entry.flags = LineFlags::from_content(&entry.content);
                return PassOutcome::changed("lines grown", 1);
            }
        }
        PassOutcome::unchanged()
    }
}

/// Writes a different `audio_ref` on every call.
///
/// Trips **run-to-run stability** and nothing else. `audio_ref` is written to
/// `.inkb` but is never read by the runtime and is not part of line identity,
/// so both semantic oracles stay clean — which is precisely what justifies the
/// fence keeping byte-level checks alongside them.
///
/// The counter is an `AtomicUsize` on purpose. The obvious alternative — making
/// output depend on `HashMap` iteration order, the actual house-rule hazard —
/// would be a *worse* control: `RandomState` varies per map instance, so two
/// in-process runs usually differ but are not guaranteed to, and a
/// nondeterministic control is worse than none.
pub struct Drift;

impl Drift {
    const NAME: &'static str = "control:drift";
}

/// Bumped on every [`Drift`] run, so no two runs agree.
static DRIFT_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl Pass for Drift {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn run(&self, story: &mut StoryData) -> PassOutcome {
        let tick = DRIFT_COUNTER.fetch_add(1, Ordering::Relaxed);
        for table in &mut story.line_tables {
            if let Some(entry) = table.lines.first_mut() {
                entry.audio_ref = Some(format!("control-drift-{tick}"));
                return PassOutcome::changed("audio refs drifted", 1);
            }
        }
        PassOutcome::unchanged()
    }
}

/// Build a [`crate::PassSet`] holding one control, by name.
///
/// Returns `None` for an unknown name so a typo in a test is a clear failure
/// rather than a silently empty set that then passes every obligation.
#[must_use]
pub fn pass_set(name: &str) -> Option<crate::PassSet> {
    let pass: Box<dyn Pass> = match name {
        Retext::NAME => Box::new(Retext),
        Rehash::NAME => Box::new(Rehash),
        Grow::NAME => Box::new(Grow),
        Drift::NAME => Box::new(Drift),
        _ => return None,
    };
    Some(crate::PassSet::empty().with(pass))
}

/// An [`crate::OptConfig`] running exactly one control.
///
/// # Panics
///
/// If `name` is not one of [`ALL`].
#[must_use]
pub fn config(name: &str) -> crate::OptConfig {
    let passes = pass_set(name);
    assert!(
        passes.is_some(),
        "unknown control pass {name:?}; known: {ALL:?}"
    );
    crate::OptConfig {
        passes: passes.unwrap_or_else(crate::PassSet::empty),
    }
}
