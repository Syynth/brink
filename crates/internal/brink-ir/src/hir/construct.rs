//! The `construct` protocol registry — the 4th entry of the closed
//! protocol registry (`docs/stdlib-spec.md` §9.6, RULED 2026-07-23 as
//! #1103; `docs/decision-log.md` "Collection/construction initializer:
//! `TypeName { }` is a std-registered construction protocol").
//!
//! `TypeName { … }` construction is **protocol dispatch** (the C# `Add`-
//! method lineage), *not* closed compiler grammar over a fixed set of type
//! names. The brace tokens are fixed surface grammar the parser produces
//! (`brink_syntax_native`'s `CONSTRUCT_LITERAL`/`CONSTRUCT_ENTRY`: the
//! element form, and the pair/field form); everything about what a literal
//! *means* is decided here, by looking the leading type name up in this
//! registry.
//!
//! # The fence: std-only this round
//!
//! The ruling admits **only std types** in this round — user-type opt-in
//! rides the deferred `impl` spelling (still ⏳ for the code-dialect
//! sitting, exactly as `crate::…`'s sibling protocols `display`/`compare`/
//! `iterate` are; see `brink_analyzer::protocols`' own "v1 has no impl
//! *spelling*" section). [`ConstructTarget`] is therefore a **closed enum
//! that IS the registry** — the same protocol-fence shape NS-A8 used for
//! the numeric tower (`docs/tower-mini-spec.md`): adding an entry is a
//! compiler change by construction, and there is no data-driven table a
//! user could extend.
//!
//! A type name that is *not* in the registry is not an error here: it
//! falls through to the declared-struct reading (the field form), which is
//! how `Point { x: 1, y: 2 }` works without `Point` being a std type.
//! Whether that name actually names a declared struct is
//! `brink-analyzer`'s job (the existing struct-literal checks), not this
//! registry's.
//!
//! # Not in this round
//!
//! - The **validating** member (`construct → Option`, for data-driven /
//!   runtime tables) is ratified as a protocol member but its user-facing
//!   spelling is deferred with the impl spelling — so only the **total**
//!   literal exists here (`Weighted { … }` faults on an invalid table).
//! - The spread / from-existing form (`Map { ..other, k: v }`) is
//!   deferred — no grammar for it exists, and adding one later costs
//!   nothing.

/// Which of the two ruled brace entry forms a registry entry consumes.
///
/// The parser produces both forms from one grammar; this is the per-type
/// meaning the protocol supplies. A literal whose entries do not match its
/// target's form is [`DiagnosticCode::E139`](crate::DiagnosticCode::E139).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConstructForm {
    /// The **element** form — `Flags { Red, Blue }`: bare expressions,
    /// no colons.
    Element,
    /// The **pair/field** form — `Map { "a": 1 }`, `Weighted { 3: "gold" }`,
    /// `Point { x: 1 }`: every entry is `left : right`.
    Pair,
}

impl ConstructForm {
    /// A short, author-facing name for this form, used in the
    /// form-mismatch diagnostic's message.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ConstructForm::Element => "element",
            ConstructForm::Pair => "key/value",
        }
    }
}

/// One entry of the closed `construct` registry. The enum is the registry
/// (see the module doc's fence): every std type that opts into the
/// `TypeName { … }` literal this round has a variant here, and nothing
/// else can register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConstructTarget {
    /// `Map { key: value, … }` — the map literal (`docs/stdlib-spec.md`
    /// §5). Same HIR shape as the brink dialect's `#{…}` sigil literal, so
    /// the key-domain (`E106`) and duplicate-key (`E138`) checks serve both
    /// surfaces from one place.
    Map,
    /// `Flags { A, B }` — a flags value (`docs/stdlib-spec.md` §6), the
    /// element form. Same HIR shape as ink's `(A, B)` list literal.
    Flags,
    /// `Weighted { weight: value, … }` — the weighted table
    /// (`docs/stdlib-spec.md` §8). The **total** literal: an invalid table
    /// faults (`E120`'s compile-time half, `WeightedBadWeight` at runtime),
    /// which is the 90% value-position case the ruling ships this round.
    Weighted,
}

impl ConstructTarget {
    /// Every registry entry, in declaration order — the closed set.
    pub const ALL: [ConstructTarget; 3] = [
        ConstructTarget::Map,
        ConstructTarget::Flags,
        ConstructTarget::Weighted,
    ];

    /// The registered type name, as written in source.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        match self {
            ConstructTarget::Map => "Map",
            ConstructTarget::Flags => "Flags",
            ConstructTarget::Weighted => "Weighted",
        }
    }

    /// The brace entry form this target's `construct` member consumes.
    #[must_use]
    pub fn form(self) -> ConstructForm {
        match self {
            ConstructTarget::Flags => ConstructForm::Element,
            ConstructTarget::Map | ConstructTarget::Weighted => ConstructForm::Pair,
        }
    }

    /// Resolve a written type-name path to a registry entry.
    ///
    /// `segments` is the literal's whole path, in source order. Matching is
    /// on the **last** segment: imports are naming-only (`docs/stdlib-spec.md`
    /// §9.5 — "no `std::prelude` module; the prelude is pre-granted
    /// naming"), so `Map` and `std::map::Map` name the same type and must
    /// construct the same way. `None` means "not a registered std type" —
    /// the declared-struct fall-through, not an error.
    #[must_use]
    pub fn lookup(segments: &[String]) -> Option<ConstructTarget> {
        let last = segments.last()?;
        ConstructTarget::ALL
            .into_iter()
            .find(|t| t.type_name() == last)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConstructForm, ConstructTarget};

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn every_registry_entry_is_reachable_by_its_own_name() {
        for target in ConstructTarget::ALL {
            assert_eq!(
                ConstructTarget::lookup(&path(&[target.type_name()])),
                Some(target),
                "{} must resolve to itself",
                target.type_name()
            );
        }
    }

    #[test]
    fn a_qualified_spelling_resolves_to_the_same_entry() {
        assert_eq!(
            ConstructTarget::lookup(&path(&["std", "map", "Map"])),
            Some(ConstructTarget::Map)
        );
        assert_eq!(
            ConstructTarget::lookup(&path(&["std", "collections", "Weighted"])),
            Some(ConstructTarget::Weighted)
        );
    }

    #[test]
    fn an_unregistered_name_falls_through_rather_than_erroring() {
        assert_eq!(ConstructTarget::lookup(&path(&["Point"])), None);
        // The fence: user types cannot register this round, so even a name
        // that *looks* std-ish is a struct name as far as this registry is
        // concerned.
        assert_eq!(ConstructTarget::lookup(&path(&["Heap"])), None);
        assert_eq!(ConstructTarget::lookup(&[]), None);
    }

    #[test]
    fn each_entry_declares_the_form_the_spec_gives_it() {
        assert_eq!(ConstructTarget::Map.form(), ConstructForm::Pair);
        assert_eq!(ConstructTarget::Weighted.form(), ConstructForm::Pair);
        assert_eq!(ConstructTarget::Flags.form(), ConstructForm::Element);
    }

    #[test]
    fn type_names_are_unique_so_lookup_is_unambiguous() {
        let mut names: Vec<&str> = ConstructTarget::ALL
            .into_iter()
            .map(ConstructTarget::type_name)
            .collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "registry type names must be unique");
    }
}
