//! The project-level injection point for an evaluated conventions
//! registry (issue #1863, a direct prerequisite of #1840).
//!
//! `element::collect` (issue #1838) builds a file's claiming-handler table
//! by walking **that file's own CST children only** — the confinement
//! ruling (issue #1844) restricts *declaring* a claiming handler to the
//! one module `brink.toml`'s `[project] elements` names, but nothing
//! today lets a line in any *other* file actually match against that
//! module's handlers. This module is the seam: an optional, externally
//! supplied, already-ordered handler set that [`super::lower_with_conventions`]
//! merges in alongside a file's own local declarations.
//!
//! # The two-independent-reads join (Q1, `docs/decision-log.md`
//! 2026-08-01 "Conventions comptime: the four blocking rulings")
//!
//! > The compiler reads the conventions module's CST for `ClaimHandler`
//! > records... it separately comptime-evaluates `fn conventions()` for
//! > an ORDERED LIST OF IDENTITIES; it joins them. `DefinitionId` is the
//! > join key.
//!
//! [`ExternalClaimHandler`] is the CST-side payload — pattern, parameter
//! order, display name, annotation range, block-capture flag (issue
//! #2068) — exactly what [`super::element::collect`] already produces
//! for a *local* handler (issue #1838). [`ExternalConventions`] is the
//! join's OUTPUT: an already-ordered, already-joined list, because this
//! crate (`brink-ir`) has no project identity to perform the join itself
//! — single-file lowering never has a `SymbolIndex` to resolve a `DefinitionId` against
//! (`element.rs`'s own module doc makes the same point about the
//! confinement check). The join itself — attaching a `DefinitionId` to
//! each declared handler and ordering the result against a
//! comptime-evaluated identity list — is one layer up
//! (`brink_analyzer::conventions_registry`), which has the `SymbolIndex`
//! this crate deliberately does not.
//!
//! Building an [`ExternalConventions`] directly via [`ExternalConventions::new`]
//! — bypassing any join at all — is exactly how a test (or, before #1840
//! lands, nothing else) proves this seam: a hand-constructed registry is
//! not a lesser input, it is the same shape the real join produces.
//!
//! # Deliberately not here
//!
//! Comptime-evaluating `fn conventions()` is issue #1840's job, not this
//! one's — this module never runs any brink code. Wiring a real
//! project-wide registry into `brink-db`'s per-file `lowered_query` is
//! also not here: that query is deliberately project-*identity*-free
//! today (no `ProjectInput` parameter), which is what keeps a body edit
//! in one file from invalidating every other file's memo. Widening that
//! dependency edge is an architectural call belonging to whichever PR
//! actually has a real, non-placeholder order to feed it — see issue
//! #1863's own scope note.

use rowan::TextRange;

use crate::Name;

/// One claiming handler's CST-derived payload, as it crosses the
/// project-identity boundary into another file's lowering (issue #1863).
///
/// Carries everything [`super::element::try_claim`]-equivalent matching
/// needs — pattern, parameter order, display name, annotation range,
/// block-capture flag (issue #2068, see [`Self::block`]'s own doc) —
/// deliberately *without* a `DefinitionId`: this crate has no project
/// identity to compute one against (see the module doc). The id is the
/// join key one layer up, in whatever produced this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalClaimHandler {
    /// The handler's own name, carrying its declaration-site range *in
    /// its own file* — not the file this record is injected into.
    pub name: Name,
    /// Parameter names in declaration order, matching
    /// [`crate::ClaimHandlerDecl::params`].
    pub params: Vec<String>,
    /// The claiming pattern's regex source (uncompiled — `regex::Regex`
    /// has no `Eq`, and every HIR-adjacent type this travels through
    /// needs one; the file that actually matches against it compiles it
    /// once, exactly like a local handler's own pattern).
    pub pattern: String,
    /// Range of the `@[element(claims = "…")]` annotation line, in the
    /// declaring file.
    pub annotation: TextRange,
    /// The bare `block` clause (issue #1839), carried across the
    /// injection join since issue #2068 — `true` when the declaring
    /// file's own trailing parameter is a `content`-typed block-capture
    /// receiver rather than a regex-bound capture. Before #2068 this
    /// field did not exist, so `super::element::collect` had no way to
    /// give an injected handler anything but `block: false`, regardless
    /// of how it was actually declared.
    pub block: bool,
}

/// The project's evaluated conventions registry, as it arrives at a
/// file's own lowering (issue #1863) — already ordered, already joined.
///
/// Deliberately just an ordered list, never a richer payload: Q1's ruling
/// is explicit that the comptime boundary carries "the one thing it
/// uniquely knows: order, and nothing else" — the pattern/param/name
/// payload for each entry comes from the *separate* CST read, not from
/// comptime evaluation. See the module doc for the full two-reads shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalConventions {
    handlers: Vec<ExternalClaimHandler>,
}

impl ExternalConventions {
    /// Build directly from an already-ordered handler list.
    ///
    /// This is the ONE constructor, used identically by a hand-built test
    /// registry (proving this seam before issue #1840's evaluator exists)
    /// and by the eventual real join (`brink_analyzer::conventions_registry`,
    /// once #1840 lands) — neither is a lesser or provisional path through
    /// this type.
    #[must_use]
    pub fn new(handlers: Vec<ExternalClaimHandler>) -> Self {
        Self { handlers }
    }

    /// `true` when the registry carries no handlers — the injection
    /// point's exact no-op case (an unconfigured or empty project stays
    /// byte-identical to lowering with no external registry at all).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// The registry's handlers, in resolution order.
    #[must_use]
    pub fn handlers(&self) -> &[ExternalClaimHandler] {
        &self.handlers
    }
}
