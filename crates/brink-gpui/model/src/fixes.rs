//! Auto-fixes — `docs/autofix-spec.md` §7, on the worker.
//!
//! Every surface here is a caller of `brink_ide::fix::{fixes_for, collect,
//! fix_all}`; nothing reconstructs a fix. Three queries:
//!
//! - **fixes under the cursor** (the editor's code-action menu): every
//!   offered fix for the visible diagnostics whose own range covers the
//!   offset — "on the squiggle", the ruled narrowing.
//! - **offers** (the Problems panel): every offered fix in the compilation,
//!   paired with its diagnostic's `(path, range, code)` so each row looks
//!   itself up — ONE query per analysis, not one per row — plus the number
//!   the batch would take, for "Fix all safe (N)". The count is `collect`'s
//!   own length (the `admits` gate applied, identical fixes collapsed),
//!   never a tally of the offers: the button promises what pressing it does.
//! - **fix all** (§5's fixpoint), run on the live session and **rolled
//!   back** before it answers. The host owns the write: it applies each
//!   changed file through its own edit seam, so every editor over that file
//!   follows and undo sees the change. A session left holding the fixed
//!   text would make the host's undo snapshot the fixed text.
//!
//! Two filters apply before a fix is ever offered, both matching what the
//! Problems panel shows (the intersection #3459 names): a suppressed
//! diagnostic (`// brink-disable`, `@[allow(…)]`) is dropped by
//! `apply_suppressions`, and a `[lints] "allow"`-levelled code — the one
//! `effective_severity` answers `None` for — is skipped, so no surface offers,
//! counts or applies a fix for a problem with no row.

use std::collections::BTreeMap;

use brink_ide::fix::{Applicability, FixCx, FixMode, FixPolicy, Select, collect, fixes_for};
use brink_ide::session::IdeSession;
use brink_ir::{DiagnosticCode, FileId};

use crate::query::{Location, TextEdit};

/// `brink_ide::fix::Applicability`, as plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tier {
    Safe,
    Suggested,
    Placeholder,
}

impl Tier {
    fn of(tier: Applicability) -> Self {
        match tier {
            Applicability::Safe => Self::Safe,
            Applicability::Suggested => Self::Suggested,
            Applicability::Placeholder => Self::Placeholder,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Suggested => "suggested",
            Self::Placeholder => "placeholder",
        }
    }
}

/// One offered fix, ready to apply: the edits are in bytes of each file as
/// it is now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FixPlan {
    pub code: String,
    pub title: String,
    pub tier: Tier,
    pub edits: Vec<TextEdit>,
    /// Placeholder fixes leave a hole the author fills; this is where.
    pub caret: Option<Location>,
}

/// A fix paired with the diagnostic it discharges — how a Problems row finds
/// its own without a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixOffer {
    pub path: String,
    pub start: u32,
    pub end: u32,
    pub code: String,
    /// Whether the batch would take it — `FixPolicy::admits`, as opposed to
    /// `offers`, so a surface can tell "you may click this" from "Fix all
    /// will do this" without asking again.
    pub batchable: bool,
    pub fix: FixPlan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixOffers {
    pub offers: Vec<FixOffer>,
    /// The `N` in "Fix all safe (N)": what one batch round would take.
    pub batchable: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixScope {
    Project,
    File(String),
}

/// What a fix-all pass did — and the text it would leave behind, for the
/// host to write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixAllReport {
    /// `(path, new source)` for every file whose text changed.
    pub files: Vec<(String, String)>,
    pub applied: usize,
    pub skipped_overlap: usize,
    pub remaining: usize,
    pub rounds: u8,
    pub cap_hit: bool,
}

/// The project's `[fix]` table as the fixers' policy — `brink-web`'s
/// `fix_policy` without the app ceiling (§6.2's setting is not in the native
/// studio yet; the ceiling stays resolvable in one place,
/// `ProjectConfig::effective_fix_policy`, for when it is).
fn policy(session: &IdeSession) -> FixPolicy {
    let mut config = brink_project_config::ProjectConfig::default();
    config.fix.clone_from(&session.project_settings().fix);
    let mut policy = FixPolicy::new();
    for code in session.project_settings().fix.keys() {
        // A code this compiler does not know: `[fix]` accepts it and no fixer
        // can match it; the config layer already warned.
        let Some(parsed) = DiagnosticCode::from_str_code(code) else {
            continue;
        };
        if let Some(mode) = FixMode::from_config(config.effective_fix_policy(code, None)) {
            policy.set(parsed, mode);
        }
    }
    policy
}

/// The batch selection: safe fixes only, over the compilation or one file.
fn batch_select(session: &IdeSession, scope: &FixScope) -> Option<Select> {
    let select = Select::all().with_tiers(vec![Applicability::Safe]);
    Some(match scope {
        FixScope::Project => select,
        FixScope::File(path) => select.in_file(session.file_id(path)?),
    })
}

/// A file's diagnostics as the Problems panel shows them: suppressions
/// applied, `[lints] allow` codes dropped.
fn visible_diagnostics(session: &IdeSession, file: FileId) -> Vec<brink_ir::Diagnostic> {
    let db = session.db();
    let (Some(raw), Some(source)) = (db.diagnostics(file), db.source(file)) else {
        return Vec::new();
    };
    let unsuppressed = match db.suppressions(file) {
        Some(sup) => brink_ir::suppressions::apply_suppressions(file, source, raw.to_vec(), sup),
        None => raw.to_vec(),
    };
    let types = session.type_policy();
    let lints = session.lint_policy();
    unsuppressed
        .into_iter()
        .filter(|d| brink_analyzer::effective_severity(d.code, types, lints).is_some())
        .collect()
}

fn plan(session: &IdeSession, fix: brink_ide::fix::Fix) -> Option<FixPlan> {
    let db = session.db();
    let mut edits = Vec::with_capacity(fix.edits.len());
    for e in &fix.edits {
        // A fix into a file the author cannot see (the mounted stdlib) is
        // not applicable; a fix that names a retired file is stale.
        if session.is_mounted_std(e.file) {
            return None;
        }
        edits.push(TextEdit {
            path: db.file_path(e.file)?.to_owned(),
            start: e.range.start().into(),
            end: e.range.end().into(),
            new_text: e.new_text.clone(),
        });
    }
    let caret = match fix.caret {
        Some((file, at)) => Some(Location {
            path: db.file_path(file)?.to_owned(),
            start: at.into(),
            end: at.into(),
        }),
        None => None,
    };
    Some(FixPlan {
        code: fix.code.as_str().to_owned(),
        title: fix.title,
        tier: Tier::of(fix.applicability),
        edits,
        caret,
    })
}

/// Every fix for every visible diagnostic covering `offset` — the cursor
/// menu. `None` when the file is not in the session.
pub(crate) fn fixes_at(session: &IdeSession, path: &str, offset: u32) -> Option<Vec<FixPlan>> {
    let file = session.file_id(path)?;
    let source = session.source(file)?;
    let at = rowan::TextSize::from(crate::query::clamp_offset(source, offset));
    let policy = policy(session);
    let cx = FixCx::new(session.db());
    let mut seen: Vec<(String, String, Vec<TextEdit>)> = Vec::new();
    let mut out = Vec::new();
    for d in visible_diagnostics(session, file)
        .iter()
        .filter(|d| d.range.contains_inclusive(at))
    {
        for fix in fixes_for(&cx, d) {
            if !policy.offers(fix.code, fix.applicability) {
                continue;
            }
            let Some(plan) = plan(session, fix) else {
                continue;
            };
            // One site can carry several diagnostics of one code whose one
            // fix discharges them all; the menu shows that entry once.
            let key = (plan.code.clone(), plan.title.clone(), plan.edits.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            out.push(plan);
        }
    }
    Some(out)
}

/// Every offered fix in the compilation, plus the batch count.
pub(crate) fn offers(session: &IdeSession) -> FixOffers {
    let policy = policy(session);
    let db = session.db();
    let cx = FixCx::new(db);
    let select = Select::all();
    let mut out = Vec::new();
    for file in select.files(db) {
        if session.is_mounted_std(file) {
            continue;
        }
        let Some(path) = db.file_path(file).map(str::to_owned) else {
            continue;
        };
        for d in &visible_diagnostics(session, file) {
            for fix in fixes_for(&cx, d) {
                if !policy.offers(fix.code, fix.applicability) {
                    continue;
                }
                let batchable = policy.admits(fix.code, fix.applicability);
                let Some(fix) = plan(session, fix) else {
                    continue;
                };
                out.push(FixOffer {
                    path: path.clone(),
                    start: d.range.start().into(),
                    end: d.range.end().into(),
                    code: d.code.as_str().to_owned(),
                    batchable,
                    fix,
                });
            }
        }
    }
    let batchable = collect(
        &cx,
        &Select::all().with_tiers(vec![Applicability::Safe]),
        &policy,
    )
    .len();
    FixOffers {
        offers: out,
        batchable,
    }
}

/// Run the batch to its fixpoint and answer the text it produced, leaving
/// the session exactly as found. `None` when a file scope names a file the
/// session does not hold.
pub(crate) fn fix_all(session: &mut IdeSession, scope: &FixScope) -> Option<FixAllReport> {
    let select = batch_select(session, scope)?;
    let policy = policy(session);

    // Cheap bail before a whole-compilation pass whose answer is fixed:
    // sound because a fixer's per-instance tier never exceeds its declared
    // maximum.
    if !brink_ide::fix::FIXERS
        .iter()
        .any(|f| select.admits_tier(f.max_applicability()))
    {
        return Some(FixAllReport::default());
    }

    // Every loaded file, so a rewrite in a file the selection never named
    // (a cross-file fix, §4) is rolled back too.
    let before: BTreeMap<String, String> = session
        .db()
        .file_ids()
        .filter_map(|id| {
            Some((
                session.db().file_path(id)?.to_owned(),
                session.db().source(id)?.to_owned(),
            ))
        })
        .collect();

    let report = brink_ide::fix::fix_all(
        session,
        &select,
        &policy,
        brink_ide::fix::DEFAULT_MAX_ROUNDS,
    );

    let mut files = Vec::new();
    for (path, old) in &before {
        let now = session.file_id(path).and_then(|id| session.source(id));
        if let Some(now) = now
            && now != old
        {
            files.push((path.clone(), now.to_owned()));
        }
    }
    if !files.is_empty() {
        for (path, old) in &before {
            session.update_source(path, old.clone());
        }
        session.refresh_analysis();
    }

    Some(FixAllReport {
        files,
        applied: report.applied.len(),
        skipped_overlap: report.skipped_overlap,
        remaining: report.remaining.len(),
        rounds: report.rounds,
        cap_hit: report.cap_hit,
    })
}

// ── Structural refactors (`brink_ide::code_actions`) ─────────────────

/// A whole-source refactor the editor offers beside the fixes: sort knots,
/// sort or format a knot's stitches, reorder a stitch. Carried as its
/// serialized `CodeActionData` so the resolve query needs no cursor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Refactor {
    pub title: String,
    pub data: String,
}

/// The refactors at `offset`. Ink only, explicitly: `code_actions` parses
/// with the ink frontend and offers ink structure, which a `.brink` file
/// never has (#2360's "gate explicitly" lesson).
pub(crate) fn refactors(session: &IdeSession, path: &str, offset: u32) -> Option<Vec<Refactor>> {
    let file = session.file_id(path)?;
    if session.is_native(file) {
        return Some(Vec::new());
    }
    let source = session.source(file)?;
    let at = crate::query::clamp_offset(source, offset) as usize;
    Some(
        brink_ide::code_actions::code_actions(source, at)
            .into_iter()
            .filter(|a| {
                // Only the whole-source ones: the moves (promote, demote,
                // move stitch) need the breakage gate a rename gets, and are
                // not offered here until they get it.
                !matches!(
                    a.data,
                    brink_ide::code_actions::CodeActionData::MoveStitch { .. }
                        | brink_ide::code_actions::CodeActionData::PromoteStitch { .. }
                        | brink_ide::code_actions::CodeActionData::DemoteKnot { .. }
                )
            })
            .filter_map(|a| {
                Some(Refactor {
                    title: a.title,
                    data: serde_json::to_string(&a.data).ok()?,
                })
            })
            .collect(),
    )
}

/// The file's new text after a refactor, or `None` when it changes nothing.
pub(crate) fn resolve_refactor(session: &IdeSession, path: &str, data: &str) -> Option<String> {
    let file = session.file_id(path)?;
    let source = session.source(file)?;
    let data: brink_ide::code_actions::CodeActionData = serde_json::from_str(data).ok()?;
    brink_ide::code_actions::resolve_code_action(source, &data)
}
