//! The §3 test obligations every [`Fixer`](super::Fixer) owes, plus the
//! curated fixtures the registry test runs them over.
//!
//! `docs/autofix-spec.md` §2 places the *trace* helper in
//! `brink-test-harness` and the `Safe` fixtures on disk. As of #3417 that is
//! where the `Safe` half lives: `brink_test_harness::fix::assert_safe_fix`
//! compiles a `tests/fix/<code>/` fixture's two sides, replays the pre-fix
//! program's run set on the post-fix one, and diffs the line tables.
//!
//! It cannot be *called* from here — `brink-test-harness` depends on
//! `brink-ide`, so the dependency only runs one way — so §3's `Safe`
//! obligation is split. This module owns the half that can be checked
//! without the oracle: the fixture must **exist** and be well-formed
//! ([`safe_fixture_dir`], enforced by the registry test in
//! [`super`]). The harness's own
//! `crates/internal/brink-test-harness/tests/fix_safe_obligations.rs`
//! enumerates the same [`super::FIXERS`] registry and runs each fixture
//! through the oracle. Neither half is optional, and neither can be
//! satisfied by the other.
//!
//! The `Suggested`/`Placeholder` obligation ([`assert_fix_discharges`]) stays
//! here in full: it needs a live [`IdeSession`], not a compiled program.

use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// Where a code's `Safe` fixture lives: `tests/fix/<code>/`, resolved from
/// this crate's manifest so it works from any working directory.
///
/// The same path `brink_test_harness::fix::fix_fixtures_root` resolves; the
/// registry test below pins them together by requiring a real fixture there.
pub(crate) fn safe_fixture_dir(code: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("fix")
        .join(code)
}

/// The half of §3's `Safe` obligation this crate can check: the fixture for
/// `code` exists and carries both sides on the same surface.
///
/// Running it — compile, replay, diff — is
/// `brink_test_harness::fix::assert_safe_fix`'s job; see this module's doc
/// for why the check is split rather than duplicated.
pub(crate) fn assert_safe_fixture_present(code: &str) {
    let _ = load_disk_fixture(code);
}

/// A `tests/fix/<code>/` fixture as this crate reads it. The harness reads
/// the same directory its own way; what is shared is the layout, not a type.
struct DiskFixture {
    /// The entry's file name, `story.ink` or `story.brink` — the name both
    /// sides compile under in the harness, so the same name is used here.
    entry_name: String,
    /// `before.*`.
    before: String,
    /// `expected.*`.
    expected: String,
    /// Other files beside the entry, in name order.
    support: Vec<(String, String)>,
    /// The fixture's `brink.toml`, if it has one.
    config: Option<String>,
}

/// Read `tests/fix/<code>/`, asserting the layout `docs/autofix-spec.md` §3
/// requires.
fn load_disk_fixture(code: &str) -> DiskFixture {
    let dir = safe_fixture_dir(code);
    assert!(
        dir.is_dir(),
        "{code} has no Safe fixture at {} — docs/autofix-spec.md §3 requires one per Safe fixer",
        dir.display()
    );

    let read = std::fs::read_dir(&dir);
    assert!(read.is_ok(), "read {}: {:?}", dir.display(), read.err());
    let mut paths: Vec<std::path::PathBuf> = read
        .expect("just asserted above")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    paths.sort();

    let mut before: Option<(String, String)> = None;
    let mut expected: Option<String> = None;
    let mut support: Vec<(String, String)> = Vec::new();
    let mut config: Option<String> = None;
    for path in paths {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path);
        assert!(text.is_ok(), "read {}: {:?}", path.display(), text.err());
        let text = text.expect("just asserted above");
        match name.as_str() {
            "before.ink" => before = Some(("story.ink".to_owned(), text)),
            "before.brink" => before = Some(("story.brink".to_owned(), text)),
            "expected.ink" | "expected.brink" => expected = Some(text),
            "brink.toml" => config = Some(text),
            "rewrites.txt" | "README.md" => {}
            _ => support.push((name, text)),
        }
    }

    assert!(
        before.is_some() && expected.is_some(),
        "{} must hold before.ink + expected.ink (or the .brink pair)",
        dir.display()
    );
    let (entry_name, before) = before.expect("just asserted above");
    DiskFixture {
        entry_name,
        before,
        expected: expected.expect("just asserted above"),
        support,
        config,
    }
}

/// The bridge between a fixture on disk and the fixer that owns it: applying
/// the fixer's own [`Fix`](super::Fix) to `before.*` must reproduce
/// `expected.*` **exactly**.
///
/// Without this the two halves of §3's obligation would never meet.
/// `assert_safe_fix` in the harness compares `before.*` against `expected.*`
/// as two source files; nothing in it knows a fixer exists, so a
/// hand-written `expected.*` the fixer does not actually produce would be
/// certified observably equivalent and prove nothing about the fix.
pub(crate) fn assert_fixture_matches_fixer(fixer: &dyn Fixer) {
    let code = fixer.code();
    let fixture = load_disk_fixture(code.as_str());

    // The same seam a real project reaches the analyzer through: sources
    // first, then the `brink.toml`-resolved `AnalysisOptions` (issue #2885 —
    // a bare session defaults to `Dialect::StrictInk`, which is not what any
    // of these fixtures runs under), then one analyze pass per file.
    let mut session = IdeSession::new();
    for (name, text) in &fixture.support {
        session.update_source(name, text.clone());
    }
    let entry = session.update_source(&fixture.entry_name, fixture.before.clone());
    let mut options = brink_analyzer::AnalysisOptions::default();
    if let Some(toml) = &fixture.config {
        let parsed = brink_project_config::parse_str(toml);
        assert!(
            parsed.is_ok(),
            "{}'s brink.toml must parse: {:?}",
            code.as_str(),
            parsed.err()
        );
        let (config, _warnings) = parsed.expect("just asserted above");
        let _ = options.apply_project_config(&config, false, false);
    }
    session.apply_analysis_options(&options);
    for (name, text) in &fixture.support {
        session.update_and_analyze(name, text.clone());
    }
    session.update_and_analyze(&fixture.entry_name, fixture.before.clone());

    let target = {
        let diagnostics = session.db().diagnostics(entry);
        assert!(
            diagnostics.is_some(),
            "{}'s before.* produced no diagnostics at all",
            code.as_str()
        );
        let diagnostics = diagnostics.expect("just asserted above");
        let found = diagnostics.iter().find(|d| d.code == code);
        assert!(
            found.is_some(),
            "{}'s before.* must actually carry a {} — it reported {diagnostics:?}",
            code.as_str(),
            code.as_str()
        );
        found.expect("just asserted above").clone()
    };

    let cx = FixCx::new(session.db());
    let offered = fixes_for(&cx, &target);
    assert!(
        !offered.is_empty(),
        "{} offers no fix for its own disk fixture",
        code.as_str()
    );
    let fix = &offered[0];
    assert!(
        !fix.edits.is_empty(),
        "{} produced a fix with no edits",
        code.as_str()
    );

    let mut edits: Vec<&crate::rename::FileEdit> = fix.edits.iter().collect();
    for edit in &edits {
        assert!(
            edit.file == entry,
            "{}'s fix edits a file other than the entry — the on-disk fixture format \
             carries one before/expected pair",
            code.as_str()
        );
    }
    // Splice from the end so earlier offsets stay valid.
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
    let mut produced = fixture.before.clone();
    for edit in edits {
        produced.replace_range(
            usize::from(edit.range.start())..usize::from(edit.range.end()),
            &edit.new_text,
        );
    }

    assert_eq!(
        produced,
        fixture.expected,
        "{}'s fixer does not produce its own expected.* — \
         tests/fix/{}/expected.* must be exactly what the fix writes",
        code.as_str(),
        code.as_str()
    );
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

/// E031: an ordinary call (`greet("Al", "Bob")`) over-supplies `greet`'s one
/// declared param. The cursor sits on `greet`'s own identifier — where
/// `resolve::check_arity` anchors the diagnostic (`uref.range`, the callee
/// path's whole span, issue #1561).
pub(crate) fn e031_fixture() -> FixFixture {
    let src = "=== function greet(name) ===\n~ return \"Hi \" + name\n\n=== main ===\n~ temp r = greet(\"Al\", \"Bob\")\n{r}\n-> DONE\n".to_owned();
    let at = offset_of(&src, "greet(\"Al\"");
    FixFixture {
        files: vec![("test.ink", src)],
        dialect: Dialect::Brink,
        types: None,
        at: ("test.ink", at),
    }
}

/// E176: a divert-with-args (`-> accuse("Hastings", "Poirot")`) over-supplies
/// `accuse`'s one declared param. `arity_trim_fix`'s own module doc: the
/// runtime binds the *trailing* supplied argument here, not the leading one
/// a first read of "over-supplied args" suggests — this fixture's own
/// `expected.ink` is what `every_fixture_matches_its_fixer` pins that against.
pub(crate) fn e176_fixture() -> FixFixture {
    let src =
        "=== accuse(who) ===\nI accuse {who}!\n-> DONE\n\n=== main ===\n-> accuse(\"Hastings\", \"Poirot\")\n"
            .to_owned();
    let at = offset_of(&src, "accuse(\"Hastings\"");
    FixFixture {
        files: vec![("test.ink", src)],
        dialect: Dialect::Brink,
        types: None,
        at: ("test.ink", at),
    }
}

/// E095: a knot's `#@was(greet)` names its own current name (`greet`) —
/// nothing to migrate. `crate::stale_was_fix`'s own module doc: every call
/// site that reads a `#@was` compares the old name against the current one
/// *before* storing anything, so a self-aliasing occurrence never reaches
/// codegen at all — deleting the tag removes a value nothing downstream
/// reads.
pub(crate) fn e095_fixture() -> FixFixture {
    let src = "=== greet ===\n#@was(greet)\nHello!\n-> DONE\n".to_owned();
    let at = offset_of(&src, "#@was(greet)");
    FixFixture {
        files: vec![("test.ink", src)],
        dialect: Dialect::Brink,
        types: None,
        at: ("test.ink", at),
    }
}
