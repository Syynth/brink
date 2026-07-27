//! Shared `--deny`/`--warn`/`--allow <CODE>` (`-D warnings`) flag resolution
//! (issue #1373), used by both `brink compile` ([`crate::main`]) and `brink
//! ide` ([`crate::ide`], issue #1417) — one place that turns the repeatable
//! CLI flags into the `(lints, deny_warnings)` pair
//! [`brink_analyzer::AnalysisOptions::apply_lint_overrides`] consumes, so the
//! two CLI surfaces can never silently drift on flag semantics.

use std::collections::BTreeMap;

use brink_driver::LintLevel;

/// Resolve repeatable `--deny`/`--warn`/`--allow <CODE>` flags into the
/// per-code override map [`brink_environment::OptionOverrides::lints`] (or,
/// for `brink ide`, `AnalysisOptions::apply_lint_overrides`) expects. `--deny
/// warnings` (short form `-D warnings`, mirroring rustc's own `-D warnings`)
/// is special-cased as `deny-warnings` rather than a per-code override,
/// since `"warnings"` is never a real `DiagnosticCode` — every other value
/// is validated downstream, at the one resolution point
/// (`AnalysisOptions::apply_lint_overrides`), not here.
///
/// A code repeated across more than one of `--deny`/`--warn`/`--allow`
/// resolves to whichever flag is applied last, in `deny`, `warn`, `allow`
/// order below — a user passing the same code to more than one flag has
/// already made a contradictory request; this is deliberately simple rather
/// than rejecting it outright.
pub(crate) fn resolve_lint_overrides(
    deny: &[String],
    warn: &[String],
    allow: &[String],
) -> (BTreeMap<String, LintLevel>, Option<bool>) {
    let mut lints = BTreeMap::new();
    let mut deny_warnings = None;
    for code in deny {
        if code == "warnings" {
            deny_warnings = Some(true);
        } else {
            lints.insert(code.clone(), LintLevel::Deny);
        }
    }
    for code in warn {
        lints.insert(code.clone(), LintLevel::Warn);
    }
    for code in allow {
        lints.insert(code.clone(), LintLevel::Allow);
    }
    (lints, deny_warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_warnings_sentinel_sets_the_flag_not_a_lint_entry() {
        let (lints, deny_warnings) = resolve_lint_overrides(&["warnings".to_owned()], &[], &[]);
        assert!(lints.is_empty());
        assert_eq!(deny_warnings, Some(true));
    }

    #[test]
    fn per_code_flags_map_to_their_level() {
        let (lints, deny_warnings) = resolve_lint_overrides(
            &["E014".to_owned()],
            &["E022".to_owned()],
            &["E027".to_owned()],
        );
        assert_eq!(lints.get("E014"), Some(&LintLevel::Deny));
        assert_eq!(lints.get("E022"), Some(&LintLevel::Warn));
        assert_eq!(lints.get("E027"), Some(&LintLevel::Allow));
        assert_eq!(deny_warnings, None);
    }

    #[test]
    fn last_flag_wins_for_a_code_repeated_across_flags() {
        let (lints, _) = resolve_lint_overrides(&["E014".to_owned()], &[], &["E014".to_owned()]);
        assert_eq!(lints.get("E014"), Some(&LintLevel::Allow));
    }
}
