//! The §3 test obligations every [`Fixer`](super::Fixer) owes, plus the
//! curated fixtures the registry test runs them over.
//!
//! `docs/autofix-spec.md` §2 places the *trace* helper in
//! `brink-test-harness` and the `Safe` fixtures in an integration-test crate.
//! As built (#3377) both helpers live here instead, because the registry test
//! that enforces them is `brink-ide`'s own and the trace half has nothing to
//! delegate to yet: [`assert_safe_fix`] is a stub until #3371's observable-
//! semantics oracle lands, at which point it moves to the harness in place.

use std::collections::BTreeMap;

use brink_analyzer::{Dialect, TypePolicy};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, Severity};

use super::{FixCx, Fixer, fixes_for};
use crate::session::IdeSession;
use crate::structural_result::gate;

/// A curated compilation plus the cursor the surface asks at.
pub(crate) struct FixFixture {
    /// Project files in load order, as `(path, source)`.
    pub(crate) files: Vec<(&'static str, String)>,
    pub(crate) dialect: Dialect,
    pub(crate) types: Option<TypePolicy>,
    /// The file the fix is taken in, and a byte offset the diagnostic covers.
    pub(crate) at: (&'static str, u32),
}

impl FixFixture {
    fn session(&self) -> IdeSession {
        let mut session = IdeSession::new();
        session.set_language_dialect(self.dialect);
        if let Some(types) = self.types {
            session.set_type_policy(types);
        }
        for (path, src) in &self.files {
            session.update_source(path, src.clone());
        }
        for (path, src) in &self.files {
            session.update_and_analyze(path, src.clone());
        }
        session
    }
}

/// Byte offset of the first occurrence of `needle` in `haystack`.
fn offset_of(haystack: &str, needle: &str) -> u32 {
    let at = haystack.find(needle);
    assert!(at.is_some(), "fixture needle {needle:?} not found");
    u32::try_from(at.expect("just asserted above")).expect("fixture offsets fit in u32")
}

/// Apply `edits` to the fixture's sources, returning the post-fix file set.
fn apply(
    files: &[(&'static str, String)],
    session: &IdeSession,
    edits: &[crate::rename::FileEdit],
) -> Vec<(&'static str, String)> {
    // `FileId` is not `Ord`; group by its raw id so the order is stable.
    let mut by_file: BTreeMap<u32, Vec<&crate::rename::FileEdit>> = BTreeMap::new();
    for e in edits {
        by_file.entry(e.file.0).or_default().push(e);
    }
    let mut patched: BTreeMap<&str, String> = BTreeMap::new();
    for (raw, mut file_edits) in by_file {
        let fid = FileId(raw);
        let path = session.file_path(fid).expect("edit names a loaded file");
        let src = session.source(fid).expect("edit names a loaded file");
        let mut out = src.to_owned();
        // Splice from the end so earlier offsets stay valid.
        file_edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
        for e in file_edits {
            out.replace_range(
                usize::from(e.range.start())..usize::from(e.range.end()),
                &e.new_text,
            );
        }
        let owned: &str = files
            .iter()
            .map(|(p, _)| *p)
            .find(|p| *p == path)
            .expect("edited file is part of the fixture");
        patched.insert(owned, out);
    }

    files
        .iter()
        .map(|(path, src)| match patched.remove(*path) {
            Some(fixed) => (*path, fixed),
            None => (*path, src.clone()),
        })
        .collect()
}

/// How many diagnostics carrying `code` the whole compilation reports.
fn count_code(
    session: &IdeSession,
    files: &[(&'static str, String)],
    code: DiagnosticCode,
) -> usize {
    files
        .iter()
        .filter_map(|(path, _)| session.file_id(path))
        .filter_map(|fid| session.db().diagnostics(fid))
        .flat_map(<[Diagnostic]>::iter)
        .filter(|d| d.code == code)
        .count()
}

/// §3's Suggested/Placeholder obligation: the fixer offers a fix for the
/// fixture's diagnostic, applying it makes that diagnostic go away, and it
/// introduces no new **error** — the property `StructuralResult.safe` already
/// computes, reused here through [`gate`].
pub(crate) fn assert_fix_discharges(fixer: &dyn Fixer, fixture: &FixFixture) {
    let code = fixer.code();
    let session = fixture.session();
    let (path, offset) = fixture.at;
    let file = session.file_id(path).expect("fixture file is loaded");
    let at = rowan::TextSize::from(offset);

    let target = {
        let diagnostics = session
            .db()
            .diagnostics(file)
            .expect("fixture file has diagnostics");
        let found = diagnostics
            .iter()
            .find(|d| d.code == code && d.range.contains_inclusive(at));
        assert!(
            found.is_some(),
            "fixture must actually carry a {} at the cursor: {diagnostics:?}",
            code.as_str()
        );
        found.expect("just asserted above").clone()
    };

    let cx = FixCx::new(session.db());
    let offered = fixes_for(&cx, &target);
    assert!(
        !offered.is_empty(),
        "{} offers no fix for its own fixture",
        code.as_str()
    );
    let fix = &offered[0];
    assert!(
        !fix.edits.is_empty(),
        "{} produced a fix with no edits",
        code.as_str()
    );

    // No new error (§3). The op-agnostic gate overlays the edits and diffs.
    let introduced = gate(&session, &fix.edits);
    let errors: Vec<&crate::structural_result::IntroducedDiagnostic> = introduced
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{} introduced new errors: {errors:?}",
        code.as_str()
    );

    // The diagnostic is gone (§3). Re-analyze the patched compilation.
    let before = count_code(&session, &fixture.files, code);
    let patched = apply(&fixture.files, &session, &fix.edits);
    let after_fixture = FixFixture {
        files: patched,
        dialect: fixture.dialect,
        types: fixture.types,
        at: fixture.at,
    };
    let after_session = after_fixture.session();
    let after = count_code(&after_session, &after_fixture.files, code);
    assert!(
        after < before,
        "{} did not discharge its own diagnostic ({before} before, {after} after)",
        code.as_str()
    );
}

/// §3's `Safe` obligation — **stub**. Today it runs the discharge check only;
/// the trace-equivalence half (compile → apply → recompile → empty
/// `trace_diff` → line-table identity for untouched lines) is upgraded in
/// place when #3371's observable-semantics oracle lands. Nothing declares
/// `max_applicability = Safe` yet, so no fixer is currently under-tested by
/// the gap.
pub(crate) fn assert_safe_fix(fixer: &dyn Fixer, fixture: &FixFixture) {
    assert_fix_discharges(fixer, fixture);
}

// ── The curated fixtures ─────────────────────────────────────────────

const QUEST: &str = "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n";
const HEAL_REF: &str = "=== function heal(ref hp, amount) ===\n~ hp = hp + amount\n~ return hp\n\n";
const DOUBLE: &str = "=== function double(x) ===\n~ return x + x\n\n";
const HEAL2: &str = "=== function heal2(hp, amount) ===\n~ return hp + amount\n\n";

/// E025 on the ink surface: `town.ink` references `quest`'s public `ambush`
/// without importing it.
///
/// `Dialect::Brink`, because the fix *writes* an `IMPORT` — a brink extension
/// that a strict-ink project rejects as `E051`. (`#@module` is an extension
/// too, so a strict-ink project reaching E025 at all is already outside the
/// dialect; the discharge check is what makes that concrete.)
pub(crate) fn e025_ink_fixture() -> FixFixture {
    let town = "#@module(town)\n== square ==\nHi\n-> ambush\n".to_owned();
    let at = offset_of(&town, "ambush");
    FixFixture {
        files: vec![("quest.ink", QUEST.to_owned()), ("town.ink", town)],
        dialect: Dialect::Brink,
        types: None,
        at: ("town.ink", at),
    }
}

/// E025 on the **native** surface — the `use module::name;` rendering, which
/// the ink fixture cannot reach (`import_fix`'s dialect branch, #1590).
pub(crate) fn e025_native_fixture() -> FixFixture {
    let barter = "flow start() {\n  -> ambush\n}\n".to_owned();
    let at = offset_of(&barter, "ambush");
    FixFixture {
        files: vec![
            ("quest.ink", QUEST.to_owned()),
            ("market/barter.brink", barter),
        ],
        dialect: Dialect::Brink,
        types: None,
        at: ("market/barter.brink", at),
    }
}

/// E080: `heal`'s `ref hp` param is unbound at the creation site, and a
/// durable `VAR hp` is in scope to bind.
pub(crate) fn e080_fixture() -> FixFixture {
    let src = format!("{HEAL_REF}VAR hp = 10\n=== main ===\n~ temp f = #fn(heal)\n-> DONE\n");
    let at = offset_of(&src, "#fn(heal)") + 5;
    FixFixture {
        files: vec![("test.ink", src)],
        dialect: Dialect::Brink,
        types: None,
        at: ("test.ink", at),
    }
}

/// E081: the creation site binds more arguments than `double` declares.
pub(crate) fn e081_fixture() -> FixFixture {
    let src = format!("{DOUBLE}=== main ===\n~ temp f = #fn(double, 1, 2)\n-> DONE\n");
    let at = offset_of(&src, "2)");
    FixFixture {
        files: vec![("test.ink", src)],
        dialect: Dialect::Brink,
        types: None,
        at: ("test.ink", at),
    }
}

/// E063: a strict-mode `call(f, …)` supplies more arguments than the callee's
/// known type accepts. The cursor sits on the `call` identifier — where the
/// analyzer anchors the diagnostic (`fact.range`), so where the squiggle is.
pub(crate) fn e063_fixture() -> FixFixture {
    let src = format!(
        "{HEAL2}=== main ===\n~ temp f = #fn(heal2)\n~ temp r = call(f, 1, 2, 3)\n-> DONE\n"
    );
    let at = offset_of(&src, "call(f");
    FixFixture {
        files: vec![("test.ink", src)],
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        at: ("test.ink", at),
    }
}
