//! The per-code fix policy — `docs/autofix-spec.md` §6.1.
//!
//! A policy answers one question for a batch surface: *may this fix be applied
//! without asking?* The answer is a [`FixMode`], defaulted from the fix's own
//! tier ([`Applicability`]) and overridable per diagnostic code:
//!
//! | tier | default mode |
//! |---|---|
//! | [`Applicability::Safe`] | [`FixMode::Auto`] |
//! | [`Applicability::Suggested`] | [`FixMode::Ask`] |
//! | [`Applicability::Placeholder`] | [`FixMode::Off`] |
//!
//! Where the overrides come *from* is #3419's job (`brink.toml`'s `[fix]`
//! table and `effective_fix_policy`); this type is the input the batching road
//! takes, so both roads agree on what "batchable" means. The one exception is
//! the `brink_project_config::FixPolicy -> FixMode` bridge itself
//! ([`FixMode::from_config`]): both `brink-cli`'s `fix.rs` and
//! `brink-web`'s `fix_batch.rs` used to hand-roll that three-way mapping
//! independently (issue #3464), so it now lives here, as the one place it is
//! decided, rather than being re-derived at each call site.
//!
//! [`Applicability::Placeholder`] is never batchable however the policy is
//! written (§3, "Batchable: never") — a promotion to [`FixMode::Auto`] cannot
//! reach it, because the fix leaves a hole the author must fill.

use std::collections::BTreeMap;

use brink_ir::DiagnosticCode;

use super::Applicability;

/// How far a surface may go with one code's fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixMode {
    /// Batchable: `fix_all` applies it unattended.
    Auto,
    /// Offered, never batched — one explicit click each.
    Ask,
    /// Never offered here at all.
    Off,
}

impl FixMode {
    /// The wire spelling used by `brink.toml`'s `[fix]` table and the CLI/LSP
    /// JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ask => "ask",
            Self::Off => "off",
        }
    }

    /// Parse the wire spelling. `None` for anything else — the caller decides
    /// whether an unknown value is a config error or a silent default.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "ask" => Some(Self::Ask),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// The mode a code gets when the policy says nothing about it.
    #[must_use]
    pub fn from_tier(tier: Applicability) -> Self {
        match tier {
            Applicability::Safe => Self::Auto,
            Applicability::Suggested => Self::Ask,
            Applicability::Placeholder => Self::Off,
        }
    }

    /// Bridge from `brink_project_config::FixPolicy` (a raw `[fix]`-table
    /// entry, resolved through `ProjectConfig::effective_fix_policy`) to the
    /// override this module's own [`FixPolicy`] records.
    ///
    /// The three-way mapping is the ONE place it is decided
    /// (`docs/autofix-spec.md` §6.1) — `brink-cli`'s `fix.rs` and
    /// `brink-web`'s `fix_batch.rs` both call this rather than re-deriving
    /// it, after the two hand-rolled the identical match independently
    /// (issue #3464). `Off`/`Auto` become that literal override; `Ask`
    /// elides to `None` ("no override recorded") rather than
    /// [`FixMode::Ask`] — `Ask` means "this project says nothing special",
    /// which per §6.1 still leaves a Safe fixer batchable (its own TOML
    /// comment: "absent ⇒ ask: … batchable (Safe)"). Recording
    /// [`FixMode::Ask`] here would instead demote every Safe-tier fixer to
    /// non-batchable — the exact regression both call sites' doc comments
    /// warn against.
    #[must_use]
    pub fn from_config(policy: brink_project_config::FixPolicy) -> Option<Self> {
        match policy {
            brink_project_config::FixPolicy::Off => Some(Self::Off),
            brink_project_config::FixPolicy::Auto => Some(Self::Auto),
            brink_project_config::FixPolicy::Ask => None,
        }
    }
}

/// The project's fix policy: tier defaults plus per-code overrides.
///
/// Keyed by the code's wire spelling ([`DiagnosticCode::as_str`]) in a
/// [`BTreeMap`], so iteration order is deterministic (`DiagnosticCode` itself
/// is not `Ord`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixPolicy {
    overrides: BTreeMap<&'static str, FixMode>,
}

impl FixPolicy {
    /// A policy with no overrides — every code takes its tier default.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override one code's mode.
    pub fn set(&mut self, code: DiagnosticCode, mode: FixMode) {
        self.overrides.insert(code.as_str(), mode);
    }

    /// Builder form of [`set`](Self::set).
    #[must_use]
    pub fn with(mut self, code: DiagnosticCode, mode: FixMode) -> Self {
        self.set(code, mode);
        self
    }

    /// The override recorded for `code`, if any.
    #[must_use]
    pub fn override_for(&self, code: DiagnosticCode) -> Option<FixMode> {
        self.overrides.get(code.as_str()).copied()
    }

    /// The effective mode for a fix of `code` at `tier`: the override if the
    /// project set one, else the tier default.
    #[must_use]
    pub fn mode_for(&self, code: DiagnosticCode, tier: Applicability) -> FixMode {
        self.override_for(code)
            .unwrap_or_else(|| FixMode::from_tier(tier))
    }

    /// Whether a batch surface may apply this fix unattended.
    ///
    /// [`Applicability::Placeholder`] is excluded unconditionally (§3): a fix
    /// that leaves a hole is never batchable, so promoting its code to
    /// [`FixMode::Auto`] does not make it so.
    #[must_use]
    pub fn admits(&self, code: DiagnosticCode, tier: Applicability) -> bool {
        tier != Applicability::Placeholder && self.mode_for(code, tier) == FixMode::Auto
    }

    /// Whether a surface may *offer* this fix at all (a cursor menu, a
    /// Problems row) — everything except [`FixMode::Off`].
    #[must_use]
    pub fn offers(&self, code: DiagnosticCode, tier: Applicability) -> bool {
        self.mode_for(code, tier) != FixMode::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_defaults_are_safe_auto_suggested_ask_placeholder_off() {
        let policy = FixPolicy::new();
        assert_eq!(
            policy.mode_for(DiagnosticCode::E014, Applicability::Safe),
            FixMode::Auto
        );
        assert_eq!(
            policy.mode_for(DiagnosticCode::E025, Applicability::Suggested),
            FixMode::Ask
        );
        assert_eq!(
            policy.mode_for(DiagnosticCode::E173, Applicability::Placeholder),
            FixMode::Off
        );
    }

    #[test]
    fn only_auto_is_batchable() {
        let policy = FixPolicy::new();
        assert!(policy.admits(DiagnosticCode::E014, Applicability::Safe));
        assert!(!policy.admits(DiagnosticCode::E025, Applicability::Suggested));
        assert!(!policy.admits(DiagnosticCode::E173, Applicability::Placeholder));
    }

    #[test]
    fn promoting_a_suggested_code_makes_it_batchable() {
        let policy = FixPolicy::new().with(DiagnosticCode::E025, FixMode::Auto);
        assert!(policy.admits(DiagnosticCode::E025, Applicability::Suggested));
        // Untouched codes keep their tier default.
        assert!(!policy.admits(DiagnosticCode::E081, Applicability::Suggested));
    }

    /// §3: a Placeholder fix is never batchable, whatever the project writes.
    #[test]
    fn placeholder_is_never_batchable_even_when_promoted() {
        let policy = FixPolicy::new().with(DiagnosticCode::E173, FixMode::Auto);
        assert_eq!(
            policy.mode_for(DiagnosticCode::E173, Applicability::Placeholder),
            FixMode::Auto
        );
        assert!(!policy.admits(DiagnosticCode::E173, Applicability::Placeholder));
    }

    #[test]
    fn off_withdraws_a_safe_fix_entirely() {
        let policy = FixPolicy::new().with(DiagnosticCode::E014, FixMode::Off);
        assert!(!policy.admits(DiagnosticCode::E014, Applicability::Safe));
        assert!(!policy.offers(DiagnosticCode::E014, Applicability::Safe));
    }

    #[test]
    fn wire_spellings_round_trip() {
        for mode in [FixMode::Auto, FixMode::Ask, FixMode::Off] {
            assert_eq!(FixMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(FixMode::parse("sometimes"), None);
    }

    /// The ONE pin on the `brink_project_config::FixPolicy -> Option<FixMode>`
    /// bridge (issue #3464): `Off`/`Auto` become that literal override, and
    /// `Ask` elides to "no override recorded" rather than [`FixMode::Ask`] —
    /// both `brink-cli`'s `fix.rs` and `brink-web`'s `fix_batch.rs` call
    /// [`FixMode::from_config`] and must see exactly this mapping.
    #[test]
    fn from_config_maps_off_and_auto_literally_and_elides_ask() {
        assert_eq!(
            FixMode::from_config(brink_project_config::FixPolicy::Off),
            Some(FixMode::Off)
        );
        assert_eq!(
            FixMode::from_config(brink_project_config::FixPolicy::Auto),
            Some(FixMode::Auto)
        );
        assert_eq!(
            FixMode::from_config(brink_project_config::FixPolicy::Ask),
            None,
            "Ask must elide to no override — recording FixMode::Ask would \
             demote every Safe-tier fixer to non-batchable"
        );
    }
}
