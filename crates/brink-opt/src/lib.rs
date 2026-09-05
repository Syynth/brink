//! The story optimizer — a post-compile `.inkb` → `.inkb` transform
//! (`docs/optimizer-spec.md`).
//!
//! It makes a shipped artifact smaller and cheaper **without changing what it
//! does**. It is not part of compilation. The boundary (spec §7):
//!
//! > The compiler decides what to ship. The optimizer makes what ships cheaper
//! > without changing what it does.
//!
//! This crate depends on `brink-format` and nothing else. That independence is
//! the point: it works on an artifact from an older compiler, and the compiler
//! can be restructured beneath it.
//!
//! # The resident passes
//!
//! `OptConfig::defaults` runs, in order:
//!
//! 1. [`EmitLineNl`] — the peephole that fuses `EmitLine` + `EmitNewline`
//!    (`docs/optimizer-peephole.md`). The relocating rewrite engine every
//!    peephole shares lives in `peephole`.
//!
//! Every pass is a pure function of the artifact, visits it in program
//! order, and is held to the fence in `brink-test-harness` (trace equality,
//! line identity, idempotence, run-to-run stability) plus the generator
//! property in `brink-gen`.
//!
//! # Not `no_std`, deliberately
//!
//! `brink-format` is conditionally `no_std`, so this crate could be too. It
//! isn't, because the optimizer is a build-time tool that never ships to a
//! device — the constraint would buy nothing and cost the usual `alloc`
//! ceremony. Do not add it later out of symmetry with `brink-runtime`.
//!
//! # What the artifact cannot tell a pass (spec §6)
//!
//! Stated here so nobody designs a pass that needs it:
//!
//! - **Types.** Bytecode is largely untyped, so type-directed optimization is
//!   out of scope by construction. Anything needing types belongs in the
//!   compiler.
//! - **Source provenance, sometimes.** `debug_info` is an `Option` and may be
//!   absent from a release build, so a pass must not depend on knowing which
//!   source file a definition came from. (That is exactly why the reachability
//!   prune is a *compiler* step — it must tell project definitions from mounted
//!   ones, and only the compiler always knows.)
//! - **Lowering's structural invariants**, which are guarantees about LIR, not
//!   about the artifact.

use brink_format::StoryData;

#[cfg(feature = "test-control")]
pub mod control;
mod passes;
mod peephole;
mod stats;

pub use passes::EmitLineNl;

pub use stats::ArtifactStats;

/// The prefix reserved for negative-control passes (`control::` module).
///
/// A pass whose name starts with this must never appear in a default pass set;
/// [`PassSet::default`] asserts it. See [`control`] for why the check is a
/// runtime assertion rather than a `cfg`.
pub const CONTROL_PREFIX: &str = "control:";

/// One transform over an artifact.
///
/// Determinism is the contract (spec §4): a pass is a pure function of
/// [`StoryData`] plus its config — no clock, no environment, no file, no global
/// state, no randomness. Iteration is over `Vec`s in program order and over
/// `BTreeMap`/`BTreeSet`; a `HashMap` may be used only as an index that is
/// never iterated. A pass never renumbers a `DefinitionId` or `NameId` it keeps.
///
/// A pass takes `&mut StoryData` rather than consuming and returning one: every
/// foreseeable pass edits a small part of a large structure, and
/// `fn(StoryData) -> StoryData` would cost a clone per pass in the common
/// no-op case.
pub trait Pass {
    /// Stable identifier, used in reports and (eventually) `--passes=`.
    fn name(&self) -> &'static str;

    /// Apply the transform. Returns what changed, for [`PassReport`].
    fn run(&self, story: &mut StoryData) -> PassOutcome;
}

/// What one pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassOutcome {
    /// Whether the pass edited the story at all.
    pub changed: bool,
    /// Pass-specific counters, reported verbatim. Ordered, so output is stable.
    pub notes: Vec<(&'static str, usize)>,
}

impl PassOutcome {
    /// The pass looked and found nothing to do.
    #[must_use]
    pub fn unchanged() -> Self {
        Self::default()
    }

    /// The pass edited the story, touching `count` of `unit`.
    #[must_use]
    pub fn changed(unit: &'static str, count: usize) -> Self {
        Self {
            changed: count > 0,
            notes: vec![(unit, count)],
        }
    }
}

/// The passes to run, in order.
///
/// Passes run **once each, in list order, with no fixpoint loop** (spec §2).
#[derive(Default)]
pub struct PassSet {
    passes: Vec<Box<dyn Pass>>,
}

impl PassSet {
    /// An empty set — what v1 ships and what [`OptConfig::default`] carries.
    #[must_use]
    pub fn empty() -> Self {
        Self { passes: Vec::new() }
    }

    /// Add a pass to the end of the list.
    #[must_use]
    pub fn with(mut self, pass: Box<dyn Pass>) -> Self {
        self.passes.push(pass);
        self
    }

    /// The names of the passes in this set, in run order.
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        self.passes.iter().map(|p| p.name()).collect()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// How many passes the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.passes.len()
    }
}

impl core::fmt::Debug for PassSet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PassSet")
            .field("passes", &self.names())
            .finish()
    }
}

/// How to run the optimizer.
#[derive(Debug, Default)]
pub struct OptConfig {
    /// The passes to run, in order.
    pub passes: PassSet,
}

impl OptConfig {
    /// The default configuration: **no passes** (spec §8.1).
    ///
    /// This is the configuration the fence runs, and the one an eventual
    /// `brink opt` would run without arguments. It is separate from
    /// `Default::default()` only so the invariant below has somewhere to live.
    ///
    /// # Panics
    ///
    /// Never in practice, but this asserts the one safety property that the
    /// `test-control` feature cannot provide on its own: **no default pass may
    /// be a negative control.** `cargo test --workspace` unifies features, so
    /// `test-control` is on for every `brink-opt` in that build graph — "the
    /// feature is off in release" is not a guard. This is.
    #[must_use]
    pub fn defaults() -> Self {
        let config = Self {
            passes: PassSet::empty().with(Box::new(EmitLineNl)),
        };
        assert!(
            !config
                .passes
                .names()
                .iter()
                .any(|n| n.starts_with(CONTROL_PREFIX)),
            "a negative-control pass reached the default pass set: {:?}. \
             Controls exist to be caught by the fence, never to run on a real \
             artifact — see brink_opt::control.",
            config.passes.names()
        );
        config
    }
}

/// What one pass did, as reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassReport {
    /// The pass's [`Pass::name`].
    pub name: &'static str,
    /// Whether it edited the story.
    pub changed: bool,
    /// Its own counters.
    pub notes: Vec<(&'static str, usize)>,
}

/// The result of one `optimize` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptReport {
    /// One entry per pass, in run order.
    pub passes: Vec<PassReport>,
    /// Measurement before any pass ran.
    pub before: ArtifactStats,
    /// Measurement after every pass ran.
    pub after: ArtifactStats,
}

impl OptReport {
    /// Whether any pass edited the story.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.passes.iter().any(|p| p.changed)
    }
}

/// Run the configured passes over `story`, in place.
///
/// With an empty pass set this is a no-op on the data — but note that the
/// *fence* still exercises real surface, because its road is
/// `read_inkb → optimize → write_inkb`, so byte-identity there is the claim
/// that `write_inkb ∘ read_inkb == id` over real artifacts.
pub fn optimize(story: &mut StoryData, config: &OptConfig) -> OptReport {
    let before = ArtifactStats::measure(story);
    let mut passes = Vec::with_capacity(config.passes.len());
    for pass in &config.passes.passes {
        let outcome = pass.run(story);
        passes.push(PassReport {
            name: pass.name(),
            changed: outcome.changed,
            notes: outcome.notes,
        });
    }
    let after = ArtifactStats::measure(story);
    OptReport {
        passes,
        before,
        after,
    }
}
