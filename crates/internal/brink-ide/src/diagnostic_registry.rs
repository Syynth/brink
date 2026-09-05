//! The diagnostic registry as a settings UI needs it (#3169/#3148): every
//! code the compiler knows, with the one thing `DiagnosticCode` does not
//! carry — an **author-facing grouping**.
//!
//! The grouping is a UI taxonomy, and it is deliberately not on
//! `DiagnosticCode`. The enum IS sectioned by comment, but into 37
//! sections named after the milestone that added each code — `B0.6 native
//! frontend`, `Lambda lifting, LIR (issue #1709 review)`. That is developer
//! organisation, and showing it to an author would be nonsense. So this
//! groups by what a diagnostic is ABOUT, read from its title, not by where
//! it sits in the enum.
//!
//! It lives here, in the crate both studios sit on (`brink-web` for the
//! web studio, `brink-gpui-model` for the native one), so the two
//! Diagnostics sections list the same rows from the same table. A
//! hand-maintained copy in either UI would be wrong the moment a code is
//! added, and wrong *silently* — the missing code simply never appears.
//!
//! Only OVERRIDABLE codes appear in [`CATEGORIES`]: those are the only ones
//! `[lints]` can set, so they are the only ones a settings section lists.
//! A new overridable code with no entry fails
//! `every_overridable_code_has_a_category` rather than quietly landing in
//! a fallback bucket — assigning it is a judgement someone should make,
//! not a default.

use brink_ir::Severity;
use brink_ir::hir::DiagnosticCode;

/// The author-facing group of every overridable code.
pub const CATEGORIES: &[(&str, &str)] = &[
    // Where the story goes next.
    ("E033", "Flow"),
    ("E131", "Flow"),
    // The shape of a choice point.
    ("E034", "Choices"),
    ("E151", "Choices"),
    ("E195", "Choices"),
    // `~` lines and what they evaluate to.
    ("E014", "Logic"),
    ("E030", "Logic"),
    // A `~ temp` read on a path its declaration does not dominate (#3354).
    // "Logic" rather than "Names & shadowing": the name resolves fine — it
    // is the same frame's slot — and what is wrong is *when* the `~ temp`
    // line runs relative to the read.
    ("E193", "Logic"),
    // Calling something, and with what.
    ("E031", "Functions & calls"),
    ("E176", "Functions & calls"),
    // Two things claiming one name.
    ("E022", "Names & shadowing"),
    ("E023", "Names & shadowing"),
    ("E026", "Names & shadowing"),
    ("E035", "Names & shadowing"),
    ("E054", "Names & shadowing"),
    ("E188", "Names & shadowing"),
    // Annotations, inference, key domains.
    ("E063", "Types"),
    ("E106", "Types"),
    ("E152", "Types"),
    // Module boundaries and what crosses them.
    ("E092", "Modules & visibility"),
    ("E095", "Modules & visibility"),
    ("E132", "Modules & visibility"),
    ("E190", "Modules & visibility"),
    // `@[convention(…)]` handlers competing for the same prose.
    ("E168", "Conventions"),
    ("E170", "Conventions"),
    // Inline markup checked against the host manifest.
    ("E164", "Host markup"),
    ("E165", "Host markup"),
    ("E173", "Host markup"),
    // `///` tags on declarations.
    ("E038", "Doc comments"),
    ("E043", "Doc comments"),
    // Spellings that still work but have been superseded.
    ("E110", "Deprecated spellings"),
    ("E172", "Deprecated spellings"),
    // The advisory tiers. `Info`-default codes are overridable too — an
    // author who does not want TODO notes reported has no other lever
    // (ruled 2026-08-27, #3173).
    ("E157", "Author notes"),
    ("E189", "Author notes"),
    // The `// brink-…` comment channel itself. Its own group rather than
    // folded into "Deprecated spellings": the bare `// brink-disable-file`
    // IS a superseded spelling, but E192 also covers a directive that was
    // never valid (`// brink-disable-fil E027`), and grouping by "how the
    // author got it wrong" would put those two in different places.
    ("E192", "Suppression directives"),
    // The compat-deny tier (#3373): brink accepts a construct inklecate
    // rejects outright. Its own group rather than folded into "Logic" or
    // "Names & shadowing": what unites current and future members is not
    // what kind of mistake it is (there may be none — the program plays
    // correctly) but that ink's own compiler draws a line brink doesn't.
    ("E194", "Ink compatibility"),
];

/// The group `code` belongs to, for an overridable code; `None` otherwise.
#[must_use]
pub fn category_of(code: DiagnosticCode) -> Option<&'static str> {
    CATEGORIES
        .iter()
        .find(|(c, _)| *c == code.as_str())
        .map(|(_, group)| *group)
}

/// One diagnostic code, as the settings UI needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticInfo {
    pub code: DiagnosticCode,
    /// One line, from `DiagnosticCode::title` — always present.
    pub title: &'static str,
    /// The code's DEFAULT severity.
    pub default_severity: Severity,
    /// Whether `[lints]` can override it at all. Only a minority can:
    /// `validate_lint_code` refuses every code whose default severity is
    /// an error. A UI that ignores this offers a level picker for a code
    /// the analyzer then discards.
    pub overridable: bool,
    /// The written explanation (markdown), absent when nobody has written
    /// one. Absent rather than empty so a caller cannot render a blank
    /// panel by forgetting to check.
    pub explanation: Option<&'static str>,
    /// The author-facing group — present only for overridable codes, which
    /// are the only ones a settings section lists.
    pub category: Option<&'static str>,
    /// Whether only `.brink` can produce it. A project filters its list by
    /// this, so a `strict-ink` project is not offered settings for markup
    /// spans it cannot write. See `DiagnosticCode::is_native_only` for why
    /// the uncertain cases say "both".
    pub native_only: bool,
}

/// Every diagnostic code the compiler knows, ordered by code. Static data —
/// it depends on no session and cannot go stale within a build.
#[must_use]
pub fn registry() -> Vec<DiagnosticInfo> {
    DiagnosticCode::ALL
        .iter()
        .map(|&code| DiagnosticInfo {
            code,
            title: code.title(),
            default_severity: code.severity(),
            overridable: code.is_overridable(),
            explanation: code.explanation(),
            category: category_of(code),
            native_only: code.is_native_only(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_overridable_code_has_a_category() {
        // The drift guard for the taxonomy. A NEW overridable code lands
        // here with no group, and this fails — which is the point: picking
        // its group is a judgement someone should make, not something a
        // fallback bucket should paper over.
        let uncategorised: Vec<&str> = registry()
            .into_iter()
            .filter(|r| r.overridable && r.category.is_none())
            .map(|r| r.code.as_str())
            .collect();
        assert!(
            uncategorised.is_empty(),
            "overridable codes with no category: {uncategorised:?} \
             — add them to CATEGORIES in brink-ide's diagnostic_registry.rs"
        );
    }

    #[test]
    fn only_overridable_codes_carry_a_category() {
        // The settings section lists only what can be configured, so a
        // category on anything else is dead data that would imply the row
        // belongs in a list it can never appear in.
        for r in registry() {
            if !r.overridable {
                assert!(
                    r.category.is_none(),
                    "{} is not overridable but has a category",
                    r.code.as_str()
                );
            }
        }
        // And nothing in the table names a code the compiler lacks.
        for (code, _) in CATEGORIES {
            assert!(
                DiagnosticCode::ALL.iter().any(|c| c.as_str() == *code),
                "CATEGORIES names {code}, which is not a diagnostic code"
            );
        }
    }

    #[test]
    fn the_registry_is_every_code_in_order() {
        let rows = registry();
        assert_eq!(rows.len(), DiagnosticCode::ALL.len());
        assert!(
            rows.windows(2)
                .all(|w| w[0].code.as_str() < w[1].code.as_str())
        );
    }
}
