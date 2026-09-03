//! The batch half of the auto-fix wasm surface — `docs/autofix-spec.md` §5
//! (batching), §6 (policy) and §7 (the studio surfaces).
//!
//! Three queries, all of them thin callers of `brink_ide::fix`:
//!
//! - [`fix_offers`](EditorSession::fix_offers) — every OFFERED fix of a
//!   selection, paired with the diagnostic site it discharges. The Problems
//!   panel's per-row Fix button and its context menu read this: one call per
//!   compile, indexed by `(path, start, end, code)`.
//! - [`fix_count`](EditorSession::fix_count) — how many fixes one round would
//!   take. This is the `N` in "Fix all safe (N)", so it must be the batch's
//!   own count (`collect`, identical fixes collapsed) rather than a tally of
//!   the offers above.
//! - [`fix_all`](EditorSession::fix_all) — the fixpoint loop, returning the
//!   `Report` plus the sources to write.
//!
//! **All three intersect the Problems panel's own severity resolution.**
//! `ProjectDb::diagnostics` is the RAW per-file list; the panel renders
//! `compile_project`, which drops every diagnostic
//! `brink_analyzer::effective_severity` answers `None` for — a `[lints]`
//! `"allow"` code (#3173). [`suppressed_codes`](EditorSession::suppressed_codes)
//! withdraws exactly those codes from the [`Select`] every one of the three
//! queries below runs, so a code an author turned off is never offered,
//! never counted in "Fix all safe (N)", and never applied (issue #3459).
//!
//! **Policy comes from the project, narrowed by the app.** `brink.toml`'s
//! `[fix]` table (§6.1) is resolved through
//! `ProjectConfig::effective_fix_policy` against an optional app-scope
//! ceiling (§6.2) — that function is the one place the still-tentative
//! ceiling relationship lives, so this module never re-derives the
//! intersection itself.
//!
//! **`fix_all` is side-effect-free from outside.** Re-analysis is a mutation
//! (§5, "as built"), so the loop necessarily rewrites the session's own
//! sources round over round — but the sources are **restored** before this
//! returns, and the report carries `files` instead: every path whose text
//! changed, with its full new source. That keeps the wasm API the same shape
//! as `apply_fix` ("compute the sources to write; the caller applies through
//! its own seam"), and it is load-bearing rather than cosmetic: the studio's
//! apply seam snapshots each file for undo *as it writes*, so a session left
//! holding the fixed text would snapshot the fixed text and make Undo a
//! no-op.

use std::collections::BTreeMap;

use brink_ide::fix::{Applicability, FixCx, FixMode, FixPolicy, Select, collect, fix_all};
use brink_ir::{DiagnosticCode, FileId, suppressions::apply_suppressions};
use brink_project_config::FixPolicy as ConfigFixPolicy;
use wasm_bindgen::prelude::*;

use super::{EditorSession, byte_to_utf16};
use crate::editor_dto::{FixFileJs, FixOfferJs, FixReportJs, FixSelectJs, FixSiteJs};

/// The wire spelling of an [`Applicability`], parsed. `None` for anything
/// else — an unrecognized tier contributes no tier to the selection, which
/// narrows rather than widens it.
fn parse_tier(s: &str) -> Option<Applicability> {
    match s {
        "safe" => Some(Applicability::Safe),
        "suggested" => Some(Applicability::Suggested),
        "placeholder" => Some(Applicability::Placeholder),
        _ => None,
    }
}

/// What a caller's `FixSelectJs` resolved to.
///
/// The three cases are deliberately distinct: "the ceiling forbids
/// everything" is a legitimate no-op a fix-on-save hook reaches every time
/// the setting is Off, while "this file is not loaded" is a caller bug the
/// report must name.
enum FixRequest {
    /// Act on this selection under this policy.
    Run(Select, FixPolicy),
    /// Well-formed, but the app-scope ceiling admits nothing at all.
    NothingAdmitted,
    /// Unusable — the reason, for the report's `error`.
    Invalid(&'static str),
}

/// A report for a run that never happened: no rounds, no writes.
fn empty_report(error: Option<String>) -> String {
    let js = FixReportJs {
        applied: Vec::new(),
        skipped_overlap: 0,
        remaining: Vec::new(),
        rounds: 0,
        cap_hit: false,
        files: Vec::new(),
        error,
    };
    serde_json::to_string(&js).unwrap_or_default()
}

/// The app-scope ceiling's wire spelling (§6.2).
///
/// An unrecognized spelling resolves to [`ConfigFixPolicy::Off`] — the
/// fail-safe direction. This is a batch road that rewrites the author's
/// files; a typo must make it do nothing, never make it do everything.
fn parse_ceiling(s: &str) -> ConfigFixPolicy {
    match s {
        "auto" => ConfigFixPolicy::Auto,
        "ask" => ConfigFixPolicy::Ask,
        _ => ConfigFixPolicy::Off,
    }
}

impl EditorSession {
    /// The effective fix policy: the project's `[fix]` table (§6.1) narrowed
    /// by an optional app-scope ceiling (§6.2).
    ///
    /// `None` means *nothing is admitted at all* — the ceiling is `"off"`,
    /// which applies to every code including ones `[fix]` never mentions.
    /// Building a [`FixPolicy`] could not express that: its overrides map is
    /// keyed by code, and a code absent from `[fix]` would fall through to
    /// its tier default and stay batchable.
    ///
    /// The mapping is deliberately NOT one-to-one — see
    /// [`FixMode::from_config`], the one shared bridge both this module and
    /// `brink-cli`'s `fix.rs` call, for why `Ask` elides to *no override*
    /// rather than [`FixMode::Ask`].
    fn fix_policy(&self, ceiling: Option<ConfigFixPolicy>) -> Option<FixPolicy> {
        if ceiling == Some(ConfigFixPolicy::Off) {
            return None;
        }
        let mut policy = FixPolicy::new();
        // `configured_fix` is a `BTreeMap`, so this walk is deterministic.
        for code in self.configured_fix.keys() {
            let Some(parsed) = DiagnosticCode::from_str_code(code) else {
                // A code this compiler doesn't know. `[fix]` accepts it (the
                // config crate is dependency-free of the real code set) and
                // no fixer can ever match it, so there is nothing to record.
                // #3447 surfaces this as a `ConfigWarning` already, at the
                // point `configured_fix` above was populated (`brink-web`'s
                // `apply_parsed_config`, via `AnalysisOptions::apply_project_config`'s
                // `validate_fix_code` gate in `brink-analyzer`) — nothing
                // further to do here.
                continue;
            };
            // `FixMode::from_config` is the one place `brink_project_config`'s
            // `Off`/`Auto`/`Ask` maps onto `brink_ide`'s own `FixMode` (issue
            // #3464: this bridge used to be hand-rolled here and
            // independently in `brink-cli`'s `fix.rs`).
            if let Some(mode) = FixMode::from_config(self.configured_fix_policy_for(code, ceiling))
            {
                policy.set(parsed, mode);
            }
        }
        Some(policy)
    }

    /// `ProjectConfig::effective_fix_policy` over this session's applied
    /// `[fix]` map. Kept as its own step so the ceiling intersection is
    /// resolved by the config crate's function (§6.2's "one place"), never
    /// re-derived here.
    fn configured_fix_policy_for(
        &self,
        code: &str,
        ceiling: Option<ConfigFixPolicy>,
    ) -> ConfigFixPolicy {
        let mut config = brink_project_config::ProjectConfig::default();
        config.fix.clone_from(&self.configured_fix);
        config.effective_fix_policy(code, ceiling)
    }

    /// Every diagnostic code this project's `[lints]` table suppresses
    /// outright — the codes `brink_analyzer::effective_severity` answers
    /// `None` for (issue #3173's `"allow"` level).
    ///
    /// This is the intersection issue #3459 names. `db.diagnostics` is the
    /// RAW list; the Problems panel renders `compile_project`'s output,
    /// which drops every diagnostic whose effective severity is `None`. Any
    /// fix surface reading the raw list therefore offers, counts and applies
    /// fixes for problems with no visible row — "Fix all safe (3)" against a
    /// panel showing two.
    ///
    /// Computed by asking the real seam about every known code rather than
    /// by re-deriving which levels suppress: `effective_severity`'s
    /// resolution order (the hard-error exemption, the compat-deny carve-out,
    /// `deny-warnings`) stays its own business, and a future level that
    /// suppresses is picked up here for free.
    pub(super) fn suppressed_codes(&self) -> Vec<DiagnosticCode> {
        let options = self.session.analysis_options();
        let types = options.type_policy();
        // `DiagnosticCode::ALL` is a fixed slice, so this walk — and the
        // resulting selection — is deterministic.
        DiagnosticCode::ALL
            .iter()
            .copied()
            .filter(|code| {
                brink_analyzer::effective_severity(*code, types, &options.lints).is_none()
            })
            .collect()
    }

    /// Turn a caller's `FixSelectJs` into a [`Select`] plus the policy to
    /// judge with.
    fn parse_fix_request(&self, select_json: &str) -> FixRequest {
        let Ok(request) = serde_json::from_str::<FixSelectJs>(select_json) else {
            return FixRequest::Invalid("the fix selection is not valid JSON");
        };
        let Some(policy) = self.fix_policy(request.ceiling.as_deref().map(parse_ceiling)) else {
            // An `"off"` ceiling is a legitimate "do nothing", not a bad
            // request — a fix-on-save hook set to Off must report a clean
            // no-op, never an error.
            return FixRequest::NothingAdmitted;
        };
        // The severity intersection (#3459), applied FIRST so it survives
        // every other narrowing: a `[lints]` `"allow"` code is withdrawn
        // from `fix_offers`, `fix_count` and `fix_all` alike, including
        // when the caller names it explicitly in `codes`.
        let mut select = Select::all().excluding_codes(self.suppressed_codes());
        if let Some(codes) = &request.codes {
            // An unrecognized spelling drops out: the resulting list may be
            // empty, which selects NOTHING. That is the honest reading of
            // "only these codes" when none of them exist.
            select = select.with_codes(
                codes
                    .iter()
                    .filter_map(|c| DiagnosticCode::from_str_code(c))
                    .collect(),
            );
        }
        if let Some(tiers) = &request.tiers {
            select = select.with_tiers(tiers.iter().filter_map(|t| parse_tier(t)).collect());
        }
        if let Some(path) = &request.path {
            // A path the session never loaded selects nothing — `in_file`
            // would otherwise be dropped and the request would silently
            // widen to the whole compilation.
            let Some(file) = self.session.file_id(path) else {
                return FixRequest::Invalid("the fix selection names a file that is not loaded");
            };
            select = select.in_file(file);
        }
        FixRequest::Run(select, policy)
    }

    /// Every offered fix of `select`, in the compilation's own file order.
    ///
    /// "Offered" is `FixPolicy::offers` — everything except a code the
    /// project turned `"off"`. `#@allow`-suppressed diagnostics are dropped
    /// first, and [`suppressed_codes`](Self::suppressed_codes) has already
    /// withdrawn every `[lints] "allow"` code from the selection, so this
    /// sees exactly what the Problems panel sees (§5).
    fn fix_offers_impl(&self, select: &Select, policy: &FixPolicy) -> Vec<FixOfferJs> {
        let db = self.session.db();
        let cx = FixCx::new(db);
        let mut out = Vec::new();
        for file in select.files(db) {
            let (Some(raw), Some(source)) = (db.diagnostics(file), db.source(file)) else {
                continue;
            };
            let Some(path) = db.file_path(file) else {
                continue;
            };
            let diagnostics = match db.suppressions(file) {
                Some(sup) => apply_suppressions(file, source, raw.to_vec(), sup),
                None => raw.to_vec(),
            };
            for d in &diagnostics {
                if !select.matches(db, d) {
                    continue;
                }
                for fix in brink_ide::fix::fixes_for(&cx, d) {
                    if !select.admits_tier(fix.applicability)
                        || !policy.offers(fix.code, fix.applicability)
                    {
                        continue;
                    }
                    let Some(js) = self.fix_to_js(&fix) else {
                        continue;
                    };
                    out.push(FixOfferJs {
                        code: d.code.as_str().to_owned(),
                        path: path.to_owned(),
                        start: byte_to_utf16(source, d.range.start().into()),
                        end: byte_to_utf16(source, d.range.end().into()),
                        batchable: policy.admits(fix.code, fix.applicability),
                        fix: js,
                    });
                }
            }
        }
        out
    }

    /// Every loaded file's source, as the before-picture `fix_all` is diffed
    /// against. `BTreeMap` so the resulting write list is deterministic.
    fn source_snapshot(&self) -> BTreeMap<FileId, String> {
        let db = self.session.db();
        db.file_ids()
            .filter_map(|id| Some((id, db.source(id)?.to_owned())))
            .collect()
    }
}

#[wasm_bindgen]
impl EditorSession {
    /// The auto-fixes offered for the diagnostics in a selection
    /// (`docs/autofix-spec.md` §7's Problems-panel surface). Returns a JSON
    /// `FixOfferJs[]`.
    ///
    /// `select_json` is a `FixSelectJs`; `"{}"` means the whole compilation.
    /// Each entry names the diagnostic's own file and UTF-16 range, so a
    /// Problems row can find its fixes without a query per row.
    pub fn fix_offers(&self, select_json: &str) -> String {
        let FixRequest::Run(select, policy) = self.parse_fix_request(select_json) else {
            return "[]".to_owned();
        };
        serde_json::to_string(&self.fix_offers_impl(&select, &policy)).unwrap_or_default()
    }

    /// How many fixes one batch round would take for `select` — the `N` in
    /// "Fix all safe (N)".
    ///
    /// This is `brink_ide::fix::collect`, not a tally of
    /// [`fix_offers`](Self::fix_offers): `collect` applies the policy's
    /// `admits` gate (never `offers`) and collapses identical fixes, which
    /// is exactly what [`fix_all`](Self::fix_all) will do.
    pub fn fix_count(&self, select_json: &str) -> u32 {
        let FixRequest::Run(select, policy) = self.parse_fix_request(select_json) else {
            return 0;
        };
        let cx = FixCx::new(self.session.db());
        u32::try_from(collect(&cx, &select, &policy).len()).unwrap_or(u32::MAX)
    }

    /// Run the batch to a fixpoint (`docs/autofix-spec.md` §5) and return the
    /// `Report` as JSON, plus every file whose text changed.
    ///
    /// The session is left exactly as it was found — the loop's intermediate
    /// rewrites are rolled back before this returns. The host owns the write:
    /// push each `files` entry through its own apply seam, the same way
    /// [`apply_fix`](Self::apply_fix)'s result is applied. See this module's
    /// doc for why the rollback is load-bearing.
    pub fn fix_all(&mut self, select_json: &str) -> String {
        let (select, policy) = match self.parse_fix_request(select_json) {
            FixRequest::Run(select, policy) => (select, policy),
            FixRequest::NothingAdmitted => return empty_report(None),
            FixRequest::Invalid(why) => return empty_report(Some(why.to_owned())),
        };
        let before = self.source_snapshot();
        let report = fix_all(
            &mut self.session,
            &select,
            &policy,
            brink_ide::fix::DEFAULT_MAX_ROUNDS,
        );

        // Everything read off the post-loop session happens here, while the
        // borrow is alive; the rollback below needs `&mut self.session`.
        let (applied, remaining, files, rollback) = {
            let db = self.session.db();
            let site = |s: &brink_ide::fix::FixSite| FixSiteJs {
                code: s.code.as_str().to_owned(),
                path: db.file_path(s.file).unwrap_or_default().to_owned(),
            };
            let mut files = Vec::new();
            let mut rollback = Vec::new();
            for (id, old) in &before {
                let (Some(now), Some(path)) = (db.source(*id), db.file_path(*id)) else {
                    continue;
                };
                if now != old {
                    files.push(FixFileJs {
                        path: path.to_owned(),
                        new_source: now.to_owned(),
                    });
                    rollback.push((path.to_owned(), old.clone()));
                }
            }
            (
                report.applied.iter().map(site).collect::<Vec<_>>(),
                report.remaining.iter().map(site).collect::<Vec<_>>(),
                files,
                rollback,
            )
        };

        // Roll the session back to what the caller handed us. `before` is a
        // snapshot of every loaded file, so this covers a file an edit landed
        // in that the selection never named (§4).
        if !rollback.is_empty() {
            for (path, source) in rollback {
                self.session.update_source(&path, source);
            }
            self.session.refresh_analysis();
        }

        let js = FixReportJs {
            applied,
            skipped_overlap: u32::try_from(report.skipped_overlap).unwrap_or(u32::MAX),
            remaining,
            rounds: report.rounds,
            cap_hit: report.cap_hit,
            files,
            error: None,
        };
        serde_json::to_string(&js).unwrap_or_default()
    }

    /// [`fixes_at`](Self::fixes_at) for an arbitrary loaded file rather than
    /// the active one — the road a Problems row (which names its own file)
    /// and the LSP-style per-file menu take. Returns a JSON `FixJs[]`.
    pub fn fixes_at_path(&self, path: &str, offset: u32) -> String {
        self.fixes_at_path_impl(path, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::super::EditorSession;

    /// A `pub` flow in its own native module — the definition side of the
    /// `E025` import-required shape `ImportFixer` discharges.
    const BARTER: &str = "\
pub flow haggle() {
  You haggle over the price.
}
";

    /// Two unimported references in one file: the batch road's real shape.
    const MAIN: &str = "\
flow start() {
  The market is busy.
  -> haggle
}
";

    fn session() -> EditorSession {
        let mut session = EditorSession::new();
        session.update_file("market/barter.brink", BARTER);
        session.update_file("main.brink", MAIN);
        assert!(session.set_active_file("main.brink"));
        session
    }

    /// Apply a `brink.toml`, asserting it parsed. `apply_project_config`
    /// rejects a malformed `[fix]` value as an error rather than a warning,
    /// so a typo in a fixture surfaces here instead of silently leaving the
    /// session's policy at its default and making the assertion below
    /// vacuous.
    fn apply_config(session: &mut EditorSession, toml: &str) {
        let applied = session.apply_project_config(toml);
        assert!(applied.is_ok(), "brink.toml fixture must parse: {toml}");
    }

    /// `brink.toml` promoting `E025` to `"auto"` — the §6.1 lever that makes
    /// a Suggested fix batchable for this project.
    fn promote_e025(session: &mut EditorSession) {
        apply_config(session, "[fix]\nE025 = \"auto\"\n");
    }

    fn parse(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("JSON")
    }

    /// The Problems panel's road: one call, and the row for the `E025`
    /// diagnostic finds its fix by `(path, start, end, code)`.
    #[test]
    fn fix_offers_names_the_diagnostic_site_and_the_fix() {
        let session = session();
        let offers = parse(&session.fix_offers("{}"));
        let offers = offers.as_array().expect("array");
        assert_eq!(offers.len(), 1, "{offers:?}");
        let offer = &offers[0];
        assert_eq!(offer["code"], "E025");
        assert_eq!(offer["path"], "main.brink");
        assert_eq!(offer["fix"]["applicability"], "suggested");
        assert_eq!(
            offer["fix"]["title"],
            "Import `haggle` from `story::market::barter`"
        );
        // The site is the DIAGNOSTIC's range, and it covers the reference.
        let start = offer["start"].as_u64().expect("start");
        let end = offer["end"].as_u64().expect("end");
        assert!(start < end, "a diagnostic range is non-empty: {offer:?}");
        let at = usize::try_from(start).expect("offset");
        let to = usize::try_from(end).expect("offset");
        assert_eq!(&MAIN[at..to], "haggle");
    }

    /// Untouched by `[fix]`, a Suggested fix is offered but NOT batchable —
    /// the tier default (§6.1). The Problems row shows a Fix button; the
    /// "Fix all safe" header must not count it.
    #[test]
    fn a_suggested_fix_is_offered_but_not_batchable_by_default() {
        let session = session();
        let offers = parse(&session.fix_offers("{}"));
        assert_eq!(offers[0]["batchable"], false, "{offers:?}");
        assert_eq!(session.fix_count("{}"), 0);
    }

    /// `[fix] E025 = "auto"` promotes it — offered AND batchable, and the
    /// header's count moves with it.
    #[test]
    fn promoting_a_code_makes_it_batchable_and_counted() {
        let mut session = session();
        promote_e025(&mut session);
        let offers = parse(&session.fix_offers("{}"));
        assert_eq!(offers[0]["batchable"], true, "{offers:?}");
        assert_eq!(session.fix_count("{}"), 1);
    }

    /// `[fix] E025 = "off"` withdraws the fix from every surface — the row
    /// gets no Fix button at all.
    #[test]
    fn off_withdraws_the_fix_from_the_offers() {
        let mut session = session();
        apply_config(&mut session, "[fix]\nE025 = \"off\"\n");
        let offers = parse(&session.fix_offers("{}"));
        assert_eq!(offers.as_array().expect("array").len(), 0, "{offers:?}");
        assert_eq!(session.fix_count("{}"), 0);
    }

    /// §6.2: the app ceiling only ever narrows. `"ask"` cancels the
    /// project's promotion, so fix-on-save at "Safe only" applies nothing
    /// here even though `brink.toml` says `"auto"`.
    #[test]
    fn the_app_ceiling_cancels_a_project_promotion() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count("{}"), 1);
        assert_eq!(session.fix_count(r#"{"ceiling":"ask"}"#), 0);
        // …and cannot raise it back past the project either.
        assert_eq!(session.fix_count(r#"{"ceiling":"auto"}"#), 1);
    }

    /// A `"off"` ceiling admits nothing, for every code — including ones
    /// `[fix]` never mentions, which a per-code override map could not
    /// express.
    #[test]
    fn an_off_ceiling_admits_nothing_at_all() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count(r#"{"ceiling":"off"}"#), 0);
        let offers = parse(&session.fix_offers(r#"{"ceiling":"off"}"#));
        assert_eq!(offers.as_array().expect("array").len(), 0);
    }

    /// A ceiling spelling this build doesn't know must fail SAFE: the batch
    /// road rewrites files, so a typo does nothing rather than everything.
    #[test]
    fn an_unrecognized_ceiling_admits_nothing() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count(r#"{"ceiling":"whenever"}"#), 0);
    }

    /// The `Select{tiers: Safe}` pull the "Fix all safe" header runs: no
    /// registered fixer is Safe today, so it counts zero even with `E025`
    /// promoted — the promotion makes a *Suggested* fix batchable, and the
    /// tier filter is a separate gate.
    #[test]
    fn the_safe_tier_selection_excludes_a_promoted_suggested_fix() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 0);
        assert_eq!(session.fix_count(r#"{"tiers":["suggested"]}"#), 1);
    }

    /// `fix_all` applies the promoted fix, reports the site, and hands back
    /// the source to write.
    #[test]
    fn fix_all_applies_and_returns_the_source_to_write() {
        let mut session = session();
        promote_e025(&mut session);
        let report = parse(&session.fix_all("{}"));
        assert_eq!(report["rounds"], 1, "{report:?}");
        assert_eq!(report["cap_hit"], false, "{report:?}");
        assert_eq!(report["skipped_overlap"], 0, "{report:?}");
        let applied = report["applied"].as_array().expect("applied");
        assert_eq!(applied.len(), 1, "{report:?}");
        assert_eq!(applied[0]["code"], "E025");
        assert_eq!(applied[0]["path"], "main.brink");
        assert_eq!(
            report["remaining"].as_array().expect("remaining").len(),
            0,
            "{report:?}"
        );
        let files = report["files"].as_array().expect("files");
        assert_eq!(files.len(), 1, "{report:?}");
        assert_eq!(files[0]["path"], "main.brink");
        assert_eq!(
            files[0]["new_source"],
            format!("use story::market::barter::haggle;\n{MAIN}")
        );
    }

    /// The rollback: `fix_all` computes the sources to write and leaves the
    /// session exactly as it found it, so the host's apply seam still
    /// snapshots the PRE-fix text for undo.
    #[test]
    fn fix_all_leaves_the_session_untouched() {
        let mut session = session();
        promote_e025(&mut session);
        let report = parse(&session.fix_all("{}"));
        assert_eq!(
            report["files"].as_array().expect("files").len(),
            1,
            "the fixture must actually produce a write: {report:?}"
        );
        assert_eq!(parse(&session.get_file_source("main.brink")), MAIN);
        // …and the diagnostic is still there to be fixed again.
        assert_eq!(session.fix_count("{}"), 1);
    }

    /// Applying one row's fix names the ROW's file as the primary, not
    /// whichever file happens to be active.
    #[test]
    fn apply_fix_at_path_reports_the_rows_own_file() {
        let mut session = session();
        assert!(session.set_active_file("market/barter.brink"));
        let offers = parse(&session.fix_offers("{}"));
        let chosen = serde_json::to_string(&offers[0]["fix"]).expect("re-serialize");
        let applied = parse(&session.apply_fix_at_path("main.brink", &chosen));
        assert_eq!(applied["ok"], true, "{applied:?}");
        assert_eq!(applied["path"], "main.brink", "{applied:?}");
        assert_eq!(
            applied["new_source"],
            format!("use story::market::barter::haggle;\n{MAIN}")
        );
    }

    /// With nothing admitted, `fix_all` is a no-op that says so — and writes
    /// no file, so a fix-on-save hook can run unconditionally.
    #[test]
    fn fix_all_with_nothing_admitted_writes_nothing() {
        let mut session = session();
        let report = parse(&session.fix_all("{}"));
        assert_eq!(report["rounds"], 0, "{report:?}");
        assert_eq!(report["files"].as_array().expect("files").len(), 0);
        assert_eq!(report["applied"].as_array().expect("applied").len(), 0);
        assert_eq!(report["cap_hit"], false, "{report:?}");
    }

    /// A selection naming a file the session never loaded must select
    /// NOTHING — silently widening to the whole compilation would make
    /// "fix all in this file" rewrite the project.
    #[test]
    fn an_unknown_path_selects_nothing() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count(r#"{"path":"nowhere.brink"}"#), 0);
        assert_eq!(session.fix_offers(r#"{"path":"nowhere.brink"}"#), "[]");
        let report = parse(&session.fix_all(r#"{"path":"nowhere.brink"}"#));
        assert_eq!(
            report["error"], "the fix selection names a file that is not loaded",
            "{report:?}"
        );
        assert_eq!(report["files"].as_array().expect("files").len(), 0);
    }

    /// An `"off"` ceiling is a legitimate no-op, NOT a bad request: the
    /// fix-on-save hook reaches this branch on every save while the setting
    /// is Off, and a report carrying an `error` there would surface as a
    /// failure notification on each one.
    #[test]
    fn an_off_ceiling_reports_a_clean_no_op_rather_than_an_error() {
        let mut session = session();
        promote_e025(&mut session);
        let report = parse(&session.fix_all(r#"{"ceiling":"off"}"#));
        assert_eq!(report.get("error"), None, "{report:?}");
        assert_eq!(report["rounds"], 0, "{report:?}");
        assert_eq!(report["files"].as_array().expect("files").len(), 0);
        // …and the source is untouched.
        assert_eq!(session.fix_count("{}"), 1);
    }

    /// The file selection reaches the file that HAS the diagnostic, and
    /// skips one that doesn't.
    #[test]
    fn a_file_selection_picks_only_that_files_diagnostics() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count(r#"{"path":"main.brink"}"#), 1);
        assert_eq!(session.fix_count(r#"{"path":"market/barter.brink"}"#), 0);
    }

    /// A code filter naming only codes this build doesn't know selects
    /// nothing rather than falling back to "every code".
    #[test]
    fn an_unknown_code_filter_selects_nothing() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count(r#"{"codes":["E9999"]}"#), 0);
        assert_eq!(session.fix_count(r#"{"codes":["E025"]}"#), 1);
    }

    /// The Problems row's per-row Fix button asks for the fixes of a
    /// diagnostic in a file that is NOT the active one.
    #[test]
    fn fixes_at_path_reads_a_non_active_file() {
        let mut session = session();
        assert!(session.set_active_file("market/barter.brink"));
        let at = MAIN.find("haggle\n}");
        assert!(at.is_some(), "fixture must carry the divert target");
        let offset = u32::try_from(at.expect("just asserted above")).expect("offset");
        let fixes = parse(&session.fixes_at_path("main.brink", offset));
        let fixes = fixes.as_array().expect("array");
        assert_eq!(fixes.len(), 1, "{fixes:?}");
        assert_eq!(fixes[0]["code"], "E025");
    }

    /// The **ink** surface reaches the same road, and the fix it writes is
    /// ink's own `IMPORT`, not the native `use`.
    ///
    /// Both surfaces matter here: `ImportFixer` branches on the dialect
    /// (#1590), so a batch road exercised only over `.brink` would leave the
    /// `.ink` half of the studio's Problems panel unproven.
    #[test]
    fn the_ink_surface_batches_through_the_same_road() {
        const QUEST: &str = "#@module(quest)\n== ambush ==\n#@public\nGotcha!\n-> DONE\n";
        const TOWN: &str = "#@module(town)\n== square ==\nHi\n-> ambush\n";
        let mut session = EditorSession::new();
        session.update_file("quest.ink", QUEST);
        session.update_file("town.ink", TOWN);
        assert!(session.set_active_file("town.ink"));
        promote_e025(&mut session);

        let offers = parse(&session.fix_offers("{}"));
        let offers = offers.as_array().expect("array");
        assert_eq!(offers.len(), 1, "{offers:?}");
        assert_eq!(offers[0]["code"], "E025");
        assert_eq!(offers[0]["path"], "town.ink");
        assert_eq!(offers[0]["batchable"], true);

        let report = parse(&session.fix_all("{}"));
        let files = report["files"].as_array().expect("files");
        assert_eq!(files.len(), 1, "{report:?}");
        assert_eq!(files[0]["path"], "town.ink");
        let written = files[0]["new_source"].as_str().unwrap_or_default();
        assert!(
            written.contains("IMPORT { ambush } FROM quest"),
            "the ink surface must get ink's own IMPORT line: {written:?}"
        );
        assert!(
            !written.contains("use "),
            "…and never the native `use` form: {written:?}"
        );
    }

    // ── §5's severity intersection (issue #3459) ─────────────────────

    /// The Problems panel's own list, as `compile_project` renders it —
    /// `effective_severity` applied, so a `[lints]` `"allow"` code is
    /// already gone. The fix surfaces are asserted against THIS, not against
    /// `ProjectDb::diagnostics`, because it is what the author can see.
    fn problem_codes(session: &mut EditorSession, entry: &str) -> Vec<String> {
        let compiled = parse(&session.compile_project(entry));
        let warnings = compiled["warnings"].as_array().cloned().unwrap_or_default();
        warnings
            .iter()
            .filter_map(|d| d["code"].as_str().map(str::to_owned))
            .collect()
    }

    /// A project whose only diagnostic is a Safe-fixable `E014` — an
    /// effect-free logic line. Single file, so the compile road and the fix
    /// road are looking at exactly the same compilation.
    const EMPTY_LOGIC: &str = "\
VAR score = 0
Hello.
~
~ score = score + 1
Score is {score}.
-> DONE
";

    fn e014_session() -> EditorSession {
        let mut session = EditorSession::new();
        session.update_file("main.ink", EMPTY_LOGIC);
        assert!(session.set_active_file("main.ink"));
        session
    }

    /// The premise: without `[lints]`, `E014` is a visible Problems row AND
    /// a batchable Safe fix. Everything below is the difference `allow`
    /// makes, so this must be non-vacuous first.
    #[test]
    fn the_allow_fixture_is_fixable_before_the_lint_is_allowed() {
        let mut session = e014_session();
        assert!(
            problem_codes(&mut session, "main.ink").contains(&"E014".to_owned()),
            "fixture must produce a VISIBLE E014 row"
        );
        let offers = parse(&session.fix_offers("{}"));
        assert_eq!(offers.as_array().expect("array").len(), 1, "{offers:?}");
        assert_eq!(offers[0]["code"], "E014");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 1);
    }

    /// #3459: `[lints] E014 = "allow"` removes the row from the Problems
    /// panel, so it must remove the offer, the count and the batch too.
    ///
    /// The assertion is against the panel's OWN list (`problem_codes`), not
    /// a restatement of the rule — a fix surface that disagrees with it is
    /// exactly the reachable false positive this issue reports.
    #[test]
    fn an_allowed_code_is_never_offered_counted_or_fixed() {
        let mut session = e014_session();
        apply_config(&mut session, "[lints]\nE014 = \"allow\"\n");

        let visible = problem_codes(&mut session, "main.ink");
        assert!(
            !visible.contains(&"E014".to_owned()),
            "the panel must not show an allowed code: {visible:?}"
        );

        assert_eq!(session.fix_offers("{}"), "[]");
        assert_eq!(session.fix_count("{}"), 0);
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 0);

        let report = parse(&session.fix_all(r#"{"tiers":["safe"]}"#));
        assert_eq!(report.get("error"), None, "{report:?}");
        assert_eq!(report["rounds"], 0, "{report:?}");
        assert_eq!(report["files"].as_array().expect("files").len(), 0);
        assert_eq!(report["applied"].as_array().expect("applied").len(), 0);
        // …and the source is untouched: no silent rewrite of a problem the
        // author turned off.
        assert_eq!(parse(&session.get_file_source("main.ink")), EMPTY_LOGIC);
    }

    /// Naming the allowed code explicitly does not get it back: the
    /// intersection is a subtraction, not a default the caller can override.
    #[test]
    fn naming_an_allowed_code_explicitly_still_selects_nothing() {
        let mut session = e014_session();
        apply_config(&mut session, "[lints]\nE014 = \"allow\"\n");
        assert_eq!(session.fix_count(r#"{"codes":["E014"]}"#), 0);
        assert_eq!(session.fix_offers(r#"{"codes":["E014"]}"#), "[]");
    }

    /// The cursor menu is the same road (`fixes_at_path`) and reads the same
    /// raw diagnostic list — an allowed code must offer nothing there either.
    #[test]
    fn the_cursor_menu_offers_nothing_for_an_allowed_code() {
        let mut session = e014_session();
        let at = EMPTY_LOGIC.find("~\n");
        assert!(at.is_some(), "fixture must carry the empty logic line");
        let offset = u32::try_from(at.expect("just asserted above")).expect("offset");

        let before = parse(&session.fixes_at_path("main.ink", offset));
        assert_eq!(before.as_array().expect("array").len(), 1, "{before:?}");
        assert_eq!(before[0]["code"], "E014");

        apply_config(&mut session, "[lints]\nE014 = \"allow\"\n");
        assert_eq!(session.fixes_at_path("main.ink", offset), "[]");
    }

    /// Only the allowed code is withdrawn — the rest of the batch is
    /// untouched, so "Fix all safe (N)" drops by exactly the allowed code's
    /// share rather than collapsing.
    #[test]
    fn allowing_one_code_leaves_the_others_fixable() {
        const TWO: &str = "\
=== accuse(who: string) ===
I accuse {who}!
-> DONE

=== main ===
~
-> accuse(\"Hastings\", \"Poirot\")
";
        let mut session = EditorSession::new();
        session.update_file("main.ink", TWO);
        assert!(session.set_active_file("main.ink"));

        let codes = problem_codes(&mut session, "main.ink");
        assert!(codes.contains(&"E014".to_owned()), "{codes:?}");
        assert!(codes.contains(&"E176".to_owned()), "{codes:?}");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 2);

        apply_config(&mut session, "[lints]\nE014 = \"allow\"\n");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 1);
        let offers = parse(&session.fix_offers("{}"));
        let offers = offers.as_array().expect("array");
        assert_eq!(offers.len(), 1, "{offers:?}");
        assert_eq!(offers[0]["code"], "E176");
    }

    /// `deny`/`warn` are not `allow`: only the level that SUPPRESSES
    /// withdraws a fix. A code the author escalated is still fixable.
    #[test]
    fn a_denied_code_stays_fixable() {
        let mut session = e014_session();
        apply_config(&mut session, "[lints]\nE014 = \"deny\"\n");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 1);
        let offers = parse(&session.fix_offers("{}"));
        assert_eq!(offers[0]["code"], "E014", "{offers:?}");
    }

    /// A `[lints]` entry removed from `brink.toml` restores the fix — the
    /// same wholesale-replace rule `[fix]` follows.
    #[test]
    fn removing_the_allow_entry_restores_the_fix() {
        let mut session = e014_session();
        apply_config(&mut session, "[lints]\nE014 = \"allow\"\n");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 0);
        apply_config(&mut session, "[project]\n");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 1);
    }

    /// A `[fix]` entry removed from `brink.toml` must stop applying — the
    /// wholesale-replace rule every other configured field follows.
    #[test]
    fn removing_the_fix_entry_stops_promoting_the_code() {
        let mut session = session();
        promote_e025(&mut session);
        assert_eq!(session.fix_count("{}"), 1);
        apply_config(&mut session, "[project]\n");
        assert_eq!(session.fix_count("{}"), 0);
    }
}

/// The `?fixture=fixable` studio project, pinned.
///
/// The studio's playground seeds these five files from `FIXABLE_FIXTURE` in
/// `packages/brink-studio/src/main.tsx`, and
/// `packages/brink-studio/e2e/auto-fix.spec.ts` drives every auto-fix
/// surface over them. The sources below are **byte-identical** to that
/// registry entry, the way the editor acceptance gate's fixture mirrors
/// `?fixture=native`: an e2e that asserts "Fix all safe (5)" is only as
/// trustworthy as the claim that the fixture really produces five Safe fixes
/// and one suppressed sixth, and that claim is checked here, in Rust,
/// against a real `EditorSession` rather than in a browser.
#[cfg(test)]
mod fixable_fixture {
    use super::super::EditorSession;

    const BRINK_TOML: &str = "\
[project]
entry = \"main.ink\"
dialect = \"brink\"

[lints]
E110 = \"allow\"
";

    const MAIN: &str = "\
INCLUDE prologue.ink
INCLUDE market.ink
INCLUDE quest.ink

#@public
VAR gold = 12

-> prologue
";

    const PROLOGUE: &str = "\
=== prologue ===
#@was(prologue)
#@effects(reads: gold, writes: gold)
The road out of town is quiet.
~
~ gold = gold + 1
You have {gold} coins.
-> dead_end

=== dead_end ===
-> DONE
This line can never be reached.
";

    const MARKET: &str = "\
#@module(market)

=== function greet(name: string): string ===
~ return \"Hello, \" + name

=== square ===
The market square is loud.
~ temp hail = greet(\"friend\", \"stranger\")
{hail}
-> accuse(\"Hastings\", \"Poirot\")

=== accuse(who: string) ===
I accuse {who}!
-> haggle
";

    const QUEST: &str = "\
#@module(quest)

=== haggle ===
#@public
You haggle over the price of a lantern.
-> DONE
";

    /// The fixture, mounted the way the playground mounts it: sources first,
    /// then `brink.toml`.
    fn fixture(toml: &str) -> EditorSession {
        let mut session = EditorSession::new();
        session.update_file("main.ink", MAIN);
        session.update_file("prologue.ink", PROLOGUE);
        session.update_file("market.ink", MARKET);
        session.update_file("quest.ink", QUEST);
        let applied = session.apply_project_config(toml);
        assert!(applied.is_ok(), "fixture brink.toml must parse: {toml}");
        assert!(session.set_active_file("main.ink"));
        session
    }

    /// Every code the Problems panel would render, in the panel's own order.
    fn problem_codes(session: &mut EditorSession) -> Vec<String> {
        let compiled: serde_json::Value =
            serde_json::from_str(&session.compile_project("main.ink")).expect("JSON");
        compiled["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|d| d["code"].as_str().map(str::to_owned))
            .collect()
    }

    /// Every offered fix's `(code, path, tier)`, in offer order.
    fn offers(session: &EditorSession) -> Vec<(String, String, String)> {
        let parsed: serde_json::Value =
            serde_json::from_str(&session.fix_offers("{}")).expect("JSON");
        parsed
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|o| {
                (
                    o["code"].as_str().unwrap_or_default().to_owned(),
                    o["path"].as_str().unwrap_or_default().to_owned(),
                    o["fix"]["applicability"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
            .collect()
    }

    /// The closed diagnostic set the e2e counts against — every row the
    /// Problems panel shows, and nothing else.
    #[test]
    fn the_fixture_produces_exactly_the_documented_diagnostics() {
        let mut session = fixture(BRINK_TOML);
        let mut codes = problem_codes(&mut session);
        codes.sort();
        assert_eq!(
            codes,
            vec!["E014", "E025", "E031", "E033", "E092", "E095", "E176"],
            "the fixture's diagnostic set is CLOSED — update main.tsx's table \
             and the e2e counts together with this list"
        );
        // …and the `[lints]`-allowed code is NOT among them.
        assert!(
            !codes.contains(&"E110".to_owned()),
            "E110 is `allow`ed: it must have no Problems row: {codes:?}"
        );
    }

    /// One fix per Safe-fixable row, one Suggested import, and E033 — the
    /// no-fixer warning — offering nothing.
    ///
    /// Sorted before comparing: the offer order is the compilation's file
    /// order, which is the closure once a compile has run and id order
    /// before one. Which of the two this fixture is in is not what this test
    /// is about.
    #[test]
    fn the_fixture_offers_one_fix_per_fixable_row() {
        let session = fixture(BRINK_TOML);
        let mut got = offers(&session);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("E014".into(), "prologue.ink".into(), "safe".into()),
                ("E025".into(), "market.ink".into(), "suggested".into()),
                ("E031".into(), "market.ink".into(), "safe".into()),
                ("E092".into(), "main.ink".into(), "safe".into()),
                ("E095".into(), "prologue.ink".into(), "safe".into()),
                ("E176".into(), "market.ink".into(), "safe".into()),
            ],
        );
    }

    /// **The e2e's headline number.** "Fix all safe (5)" — five, not six:
    /// `E110` has a Safe fixer and a live diagnostic, and is excluded purely
    /// because `[lints]` turned it off (issue #3459).
    #[test]
    fn fix_all_safe_counts_five_not_the_suppressed_sixth() {
        let session = fixture(BRINK_TOML);
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 5);
    }

    /// The same fixture with the `[lints]` line removed counts SIX — the
    /// difference is the whole regression, and it proves the count above is
    /// not five for some unrelated reason.
    #[test]
    fn without_the_allow_line_the_suppressed_sixth_comes_back() {
        let session = fixture("[project]\nentry = \"main.ink\"\ndialect = \"brink\"\n");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 6);
        let codes: Vec<String> = offers(&session).into_iter().map(|(c, _, _)| c).collect();
        assert!(codes.contains(&"E110".to_owned()), "{codes:?}");
    }

    /// "Fix all safe" over the whole project, end to end: five fixes in one
    /// batch, and the resulting sources are the ones the e2e reads back out
    /// of the editor.
    #[test]
    fn fix_all_safe_rewrites_the_three_files_and_leaves_the_rest() {
        let mut session = fixture(BRINK_TOML);
        let report: serde_json::Value =
            serde_json::from_str(&session.fix_all(r#"{"tiers":["safe"]}"#)).expect("JSON");
        assert_eq!(report.get("error"), None, "{report:?}");
        assert_eq!(report["cap_hit"], false, "{report:?}");
        assert_eq!(report["applied"].as_array().expect("applied").len(), 5);

        let files = report["files"].as_array().expect("files");
        let mut written: Vec<(String, String)> = files
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap_or_default().to_owned(),
                    f["new_source"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        written.sort();
        assert_eq!(
            written,
            vec![
                (
                    "main.ink".to_owned(),
                    "\
INCLUDE prologue.ink
INCLUDE market.ink
INCLUDE quest.ink

VAR gold = 12

-> prologue
"
                    .to_owned()
                ),
                (
                    "market.ink".to_owned(),
                    "\
#@module(market)

=== function greet(name: string): string ===
~ return \"Hello, \" + name

=== square ===
The market square is loud.
~ temp hail = greet(\"stranger\")
{hail}
-> accuse(\"Poirot\")

=== accuse(who: string) ===
I accuse {who}!
-> haggle
"
                    .to_owned()
                ),
                (
                    "prologue.ink".to_owned(),
                    "\
=== prologue ===
#@effects(reads: gold, writes: gold)
The road out of town is quiet.
~ gold = gold + 1
You have {gold} coins.
-> dead_end

=== dead_end ===
-> DONE
This line can never be reached.
"
                    .to_owned()
                ),
            ],
            "the batch must NOT rewrite the `allow`ed `#@effects` line"
        );
    }

    /// After the batch, the five fixed codes are gone from the Problems
    /// panel and nothing new took their place — the full post-fix list, not
    /// just an absence check on one code.
    #[test]
    fn the_batch_clears_exactly_the_rows_it_fixed() {
        let mut session = fixture(BRINK_TOML);
        let report: serde_json::Value =
            serde_json::from_str(&session.fix_all(r#"{"tiers":["safe"]}"#)).expect("JSON");
        let files = report["files"].as_array().expect("files").clone();
        for file in &files {
            let path = file["path"].as_str().unwrap_or_default().to_owned();
            let source = file["new_source"].as_str().unwrap_or_default().to_owned();
            session.update_file(&path, &source);
        }
        let mut left = problem_codes(&mut session);
        left.sort();
        assert_eq!(
            left,
            vec!["E025", "E033"],
            "only the Suggested import and the no-fixer warning may remain"
        );
    }

    /// `[fix] E014 = "off"` — the Settings ▸ Lints "Fix" column's write —
    /// drops the count by exactly one and withdraws that row's Fix button,
    /// and `"auto"` puts it back.
    #[test]
    fn the_fix_column_withdraws_one_row_and_restores_it() {
        let mut session = fixture(BRINK_TOML);
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 5);

        let off = format!("{BRINK_TOML}\n[fix]\nE014 = \"off\"\n");
        let applied = session.apply_project_config(&off);
        assert!(applied.is_ok(), "{off}");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 4);
        let codes: Vec<String> = offers(&session).into_iter().map(|(c, _, _)| c).collect();
        assert!(!codes.contains(&"E014".to_owned()), "{codes:?}");

        let auto = format!("{BRINK_TOML}\n[fix]\nE014 = \"auto\"\n");
        let applied = session.apply_project_config(&auto);
        assert!(applied.is_ok(), "{auto}");
        assert_eq!(session.fix_count(r#"{"tiers":["safe"]}"#), 5);
        let codes: Vec<String> = offers(&session).into_iter().map(|(c, _, _)| c).collect();
        assert!(codes.contains(&"E014".to_owned()), "{codes:?}");
    }

    /// Fix-on-save at the "Safe only" ceiling (§6.2) rewrites only the file
    /// it was asked for, and applies nothing at "off".
    #[test]
    fn fix_on_save_is_per_file_and_respects_the_ceiling() {
        let mut session = fixture(BRINK_TOML);

        let off: serde_json::Value =
            serde_json::from_str(&session.fix_all(r#"{"path":"prologue.ink","ceiling":"off"}"#))
                .expect("JSON");
        assert_eq!(off["files"].as_array().expect("files").len(), 0, "{off:?}");

        let safe: serde_json::Value =
            serde_json::from_str(&session.fix_all(r#"{"path":"prologue.ink","ceiling":"ask"}"#))
                .expect("JSON");
        let files = safe["files"].as_array().expect("files");
        assert_eq!(files.len(), 1, "{safe:?}");
        assert_eq!(files[0]["path"], "prologue.ink");
        let written = files[0]["new_source"].as_str().unwrap_or_default();
        assert!(
            !written.contains("#@was(prologue)") && !written.contains("\n~\n"),
            "the Safe fixes must have landed: {written:?}"
        );
        assert!(
            written.contains("#@effects(reads: gold, writes: gold)"),
            "the `allow`ed E110 line must survive a save: {written:?}"
        );
    }
}
