//! The auto-fix core model — `docs/autofix-spec.md` §2 (the model), §4
//! (scope is the compilation), §7 (the surfaces that call it).
//!
//! Diagnostics stay data; **fixes are behaviour**, computed lazily by a
//! per-code [`Fixer`] only when a surface asks. Nothing here runs during
//! analysis.
//!
//! The one currency is [`Fix::edits`]: a `Vec<FileEdit>` of *minimal*
//! byte-range edits that may span files (§4 — `FixCx` is the compilation, so
//! there is no file gate in the fixer contract). `resolve_code_action`'s
//! whole-source return stays behind for structural refactors only.
//!
//! Dispatch is a trait-object registry ([`FIXERS`]) rather than a `match` on
//! the code, so a new fixer is one entry plus its own module — and the
//! registry test in this module can enumerate every fixer to enforce the §3
//! test obligations on all of them at once.
//!
//! Batching (§5) and the policy the batch reads (§6.1) live in [`batch`] and
//! [`policy`]: [`Select`] picks the diagnostics, [`apply_round`] turns them
//! into one non-overlapping edit set, and [`fix_all`] runs rounds to a
//! fixpoint. The Problems panel, `brink fix` (§8) and the LSP `fixAll` road
//! are callers of those, and are later milestones of #3374 — as is where the
//! [`FixPolicy`] comes *from* (`brink.toml`'s `[fix]` table, #3419); this
//! module takes one as an input.

use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode, FileId};
use rowan::TextSize;

use crate::rename::FileEdit;

pub mod batch;
pub mod policy;
pub mod select;

pub use batch::{
    Candidate, DEFAULT_MAX_ROUNDS, FixSite, Report, Round, apply_round, collect, fix_all, plan,
};
pub use policy::{FixMode, FixPolicy};
pub use select::Select;

/// How far a surface may go with a fix without asking the author — the tier
/// (`docs/autofix-spec.md` §3). Each tier names the test that backs it; see
/// [`obligations`] for the ones wired today.
///
/// Ordered by how much a surface is allowed to do unattended:
/// `Placeholder < Suggested < Safe`. That ordering is what
/// [`Fixer::max_applicability`] bounds — a fixer's per-instance
/// [`Fix::applicability`] never exceeds its declared maximum, so a surface
/// can count "at most N safe fixes" from the static bounds alone, without
/// computing a single edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// Observably equivalent (`docs/observable-semantics-spec.md` §2) and
    /// translation identity holds (§2.2) — batchable.
    Safe,
    /// Probably what the author meant, but it changes meaning or loses text —
    /// one explicit click each, unless the project promotes the code.
    Suggested,
    /// Leaves a hole the author must fill; [`Fix::caret`] says where. Never
    /// batchable.
    Placeholder,
}

impl Applicability {
    /// Position in the `Placeholder < Suggested < Safe` order.
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::Placeholder => 0,
            Self::Suggested => 1,
            Self::Safe => 2,
        }
    }

    /// The wire spelling used by the wasm DTO (`FixJs.applicability`) and the
    /// CLI/LSP JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Suggested => "suggested",
            Self::Placeholder => "placeholder",
        }
    }
}

impl PartialOrd for Applicability {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Applicability {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

/// One offered fix for one diagnostic instance.
pub struct Fix {
    /// Which diagnostic this fix discharges.
    pub code: DiagnosticCode,
    /// The menu entry's label. The fixer's own wording (§10).
    pub title: String,
    /// This instance's tier — never above the fixer's
    /// [`max_applicability`](Fixer::max_applicability); [`fixes_for`] asserts it.
    pub applicability: Applicability,
    /// The minimal edits that apply the fix. May span files (§4).
    pub edits: Vec<FileEdit>,
    /// [`Applicability::Placeholder`] only: where the author fills the hole.
    pub caret: Option<(FileId, TextSize)>,
}

/// What a fixer may read: the compilation (§4).
///
/// For ink that is the entry's `INCLUDE` tree, for native the module graph —
/// either way one [`ProjectDb`], which is also the road the editor's live
/// squiggles come from, so a fixer never disagrees with the diagnostic it is
/// discharging.
pub struct FixCx<'a> {
    /// The compilation.
    pub db: &'a ProjectDb,
}

impl<'a> FixCx<'a> {
    /// Wrap a compilation for the fixers to read.
    #[must_use]
    pub fn new(db: &'a ProjectDb) -> Self {
        Self { db }
    }
}

/// A per-code fix producer. One implementor per [`DiagnosticCode`] that has
/// fixes; registered in [`FIXERS`].
pub trait Fixer: Sync {
    /// The diagnostic code this fixer discharges.
    fn code(&self) -> DiagnosticCode;

    /// Declared upper bound on every instance's
    /// [`Applicability`] — lets surfaces count "N safe fixes" without
    /// computing an edit.
    fn max_applicability(&self) -> Applicability;

    /// Compute the fixes for one diagnostic instance. Called on demand (a
    /// cursor menu, a Problems row, a batch) — **never** during analysis.
    ///
    /// `d.code` is always [`Self::code`]: [`fixes_for`] dispatches before
    /// calling.
    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix>;
}

/// Every registered fixer, one per code. Enforced unique by the registry
/// test in this module.
pub static FIXERS: &[&dyn Fixer] = &[
    &crate::import_fix::ImportFixer,
    &crate::creation_site_fix::TrimFnLiteralArgsFixer,
    &crate::creation_site_fix::BindRefArgsFixer,
    &crate::value_call_fix::ValueCallArityFixer,
    &crate::arity_trim_fix::CallArityTrimFixer,
    &crate::arity_trim_fix::DivertArityTrimFixer,
    &crate::stale_was_fix::StaleWasFixer,
];

/// Run one fixer over one diagnostic, enforcing the per-instance ≤ static-max
/// invariant [`Fixer::max_applicability`] promises.
fn fixes_from(fixer: &dyn Fixer, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
    let offered = fixer.fixes(cx, d);
    let max = fixer.max_applicability();
    for fix in &offered {
        assert!(
            fix.applicability <= max,
            "{}: fix applicability {:?} exceeds the fixer's declared max {max:?}",
            fixer.code().as_str(),
            fix.applicability,
        );
    }
    offered
}

/// Every fix offered for one diagnostic. Dispatches on `d.code`; empty when
/// no fixer claims that code.
#[must_use]
pub fn fixes_for(cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
    FIXERS
        .iter()
        .filter(|f| f.code() == d.code)
        .flat_map(|f| fixes_from(*f, cx, d))
        .collect()
}

/// The `Select::AtOffset` pull (§4): every fix for every diagnostic covering
/// `offset` in `file`, in the analyzer's own diagnostic order.
///
/// This is the cursor-menu selection (§7) — the diagnostics come off the same
/// per-file `diagnostics` road that paints the editor's squiggles, so a fix is
/// offered exactly where the author can see the problem it discharges.
///
/// **Identical fixes are collapsed.** One site can carry several diagnostics
/// of the same code whose single fix discharges all of them at once — E080
/// reports one diagnostic per unbound `ref` param, while `BindRefArgsFixer`
/// binds the whole trailing run in one edit — and the menu must show that
/// entry once, not once per diagnostic.
#[must_use]
pub fn fixes_at(cx: &FixCx<'_>, file: FileId, offset: u32) -> Vec<Fix> {
    let at = TextSize::from(offset);
    let Some(diagnostics) = cx.db.diagnostics(file) else {
        return Vec::new();
    };
    let mut seen: Vec<FixKey> = Vec::new();
    let mut out = Vec::new();
    for d in diagnostics
        .iter()
        .filter(|d| d.range.contains_inclusive(at))
    {
        for fix in fixes_for(cx, d) {
            let key = fix_key(&fix);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            out.push(fix);
        }
    }
    out
}

/// What makes two offered fixes the same menu entry.
type FixKey = (&'static str, String, Vec<(u32, u32, u32, String)>);

fn fix_key(fix: &Fix) -> FixKey {
    (
        fix.code.as_str(),
        fix.title.clone(),
        fix.edits
            .iter()
            .map(|e| {
                (
                    e.file.0,
                    e.range.start().into(),
                    e.range.end().into(),
                    e.new_text.clone(),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
pub(crate) mod obligations;

#[cfg(test)]
mod tests {
    use super::*;
    use obligations::{
        FixFixture, assert_fix_discharges, assert_fixture_matches_fixer,
        assert_safe_fixture_present,
    };
    use std::collections::BTreeSet;

    /// One discharge fixture per registered fixer (§3's Suggested/Placeholder
    /// obligation). Keyed by code so the registry test can prove coverage.
    fn discharge_fixtures() -> Vec<(DiagnosticCode, FixFixture)> {
        vec![
            (DiagnosticCode::E025, obligations::e025_ink_fixture()),
            (DiagnosticCode::E025, obligations::e025_native_fixture()),
            (DiagnosticCode::E080, obligations::e080_fixture()),
            (DiagnosticCode::E081, obligations::e081_fixture()),
            (DiagnosticCode::E063, obligations::e063_fixture()),
            (DiagnosticCode::E031, obligations::e031_fixture()),
            (DiagnosticCode::E176, obligations::e176_fixture()),
            (DiagnosticCode::E095, obligations::e095_fixture()),
        ]
    }

    #[test]
    fn registry_codes_are_unique() {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for fixer in FIXERS {
            assert!(
                seen.insert(fixer.code().as_str()),
                "duplicate fixer for {}",
                fixer.code().as_str()
            );
        }
    }

    #[test]
    fn every_fixer_has_a_discharge_fixture() {
        let fixtures: BTreeSet<&str> = discharge_fixtures()
            .iter()
            .map(|(code, _)| code.as_str())
            .collect();
        for fixer in FIXERS {
            assert!(
                fixtures.contains(fixer.code().as_str()),
                "{} has no discharge fixture — §3 requires one per fixer",
                fixer.code().as_str()
            );
        }
    }

    /// §3: every fixer discharges its own diagnostic and introduces no new
    /// error. This is the behaviour pin for the three migrated fixers — the
    /// edits are applied and the compilation is re-analyzed.
    #[test]
    fn every_fixer_discharges_its_diagnostic() {
        for (code, fixture) in discharge_fixtures() {
            let fixer = FIXERS
                .iter()
                .find(|f| f.code() == code)
                .copied()
                .expect("fixture names a registered fixer");
            assert_fix_discharges(fixer, &fixture);
        }
    }

    /// §3: a `Safe`-max fixer owes the stronger obligation, and this is the
    /// half `brink-ide` can enforce on its own — the fixture must be on disk
    /// at `tests/fix/<code>/`.
    ///
    /// The other half — actually compiling both sides, replaying the pre-fix
    /// program's run set on the post-fix one, and diffing the line tables —
    /// is `brink_test_harness::fix::assert_safe_fix`, run over this same
    /// registry by that crate's `tests/fix_safe_obligations.rs`. It cannot be
    /// called from here: `brink-test-harness` depends on `brink-ide`, so the
    /// dependency only runs one way. Neither half is optional; see
    /// [`obligations`]'s module doc.
    ///
    /// The four migrated fixers (E025/E080/E081/E063) are all `Suggested`;
    /// `arity_trim_fix`'s `CallArityTrimFixer`/`DivertArityTrimFixer`
    /// (E031/E176, issue #3428) are the first two to declare `Safe`, so this
    /// loop now actually iterates. `the_safe_fixture_path_resolves` and
    /// `every_fixture_matches_its_fixer` stay beside it regardless: without a
    /// live fixture to resolve, a typo in the fixture path would make this
    /// obligation silently unenforceable for the *next* Safe fixer.
    #[test]
    fn every_safe_max_fixer_has_a_safe_fixture() {
        for fixer in FIXERS {
            if fixer.max_applicability() == Applicability::Safe {
                // Subsumes the existence check: `assert_fixture_matches_fixer`
                // loads the same directory and fails first if it is absent.
                assert_fixture_matches_fixer(*fixer);
            }
        }
    }

    /// The fixture path must name the real fixture tree. `E014` is the one
    /// fixture that exists ahead of its fixer (`docs/autofix-spec.md` §9's
    /// first-wave Safe candidate — delete the bare `~`), and it is what keeps
    /// this path honest while the loop above has nothing to iterate.
    #[test]
    fn the_safe_fixture_path_resolves() {
        assert_safe_fixture_present("E014");
    }

    /// A fixture's `expected.*` must be **exactly** what its fixer writes.
    ///
    /// This is the join between the two halves of §3: the harness's
    /// `assert_safe_fix` compares `before.*` against `expected.*` as two
    /// source files and knows nothing about fixers, so a hand-written
    /// `expected.*` would be certified observably equivalent while proving
    /// nothing about the fix. Every registered fixer that has a fixture is
    /// checked here regardless of its tier — the four migrated `Suggested`
    /// fixers and the two `Safe` ones (E031/E176, issue #3428) alike.
    #[test]
    fn every_fixture_matches_its_fixer() {
        let mut checked = 0usize;
        for fixer in FIXERS {
            if obligations::safe_fixture_dir(fixer.code().as_str()).is_dir() {
                assert_fixture_matches_fixer(*fixer);
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "no registered fixer has a tests/fix/ fixture — this test proves nothing"
        );
    }

    #[test]
    fn applicability_orders_placeholder_below_suggested_below_safe() {
        assert!(Applicability::Placeholder < Applicability::Suggested);
        assert!(Applicability::Suggested < Applicability::Safe);
    }

    /// A fixer whose instance claims more than it declared is a bug, and
    /// `fixes_from` — the function `fixes_for` funnels every fixer through —
    /// is where that is caught.
    struct OverclaimingFixer;

    impl Fixer for OverclaimingFixer {
        fn code(&self) -> DiagnosticCode {
            DiagnosticCode::E025
        }
        fn max_applicability(&self) -> Applicability {
            Applicability::Suggested
        }
        fn fixes(&self, _cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
            vec![Fix {
                code: d.code,
                title: "overclaims".to_owned(),
                applicability: Applicability::Safe,
                edits: Vec::new(),
                caret: None,
            }]
        }
    }

    #[test]
    #[should_panic(expected = "exceeds the fixer's declared max")]
    fn per_instance_applicability_may_not_exceed_the_declared_max() {
        let db = ProjectDb::new();
        let cx = FixCx::new(&db);
        let d = Diagnostic {
            file: FileId(0),
            range: rowan::TextRange::empty(TextSize::from(0)),
            message: String::new(),
            code: DiagnosticCode::E025,
        };
        let _ = fixes_from(&OverclaimingFixer, &cx, &d);
    }
}
