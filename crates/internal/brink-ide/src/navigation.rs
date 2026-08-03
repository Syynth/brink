use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
use brink_format::DefinitionId;
use brink_ir::{FileId, SymbolInfo};
use rowan::TextRange;

/// A location result for navigation operations.
pub struct LocationResult {
    pub file: FileId,
    pub range: TextRange,
}

/// Find the definition for the symbol at `offset`.
///
/// Tries, in order: resolved references, declaration sites, then local
/// variables (params/temps) by identifier text.
pub fn find_def_at_offset(
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<&SymbolInfo> {
    // 1. Resolved reference at this position
    let def_id = analysis
        .resolutions
        .iter()
        .find(|r| r.file == file_id && (r.range.contains(offset) || r.range.start() == offset))
        .map(|r| r.target)
        // 2. Declaration site at this position
        .or_else(|| {
            analysis
                .index
                .symbols
                .values()
                .find(|info| {
                    info.file == file_id
                        && (info.range.contains(offset) || info.range.start() == offset)
                })
                .map(|info| info.id)
        });

    def_id.and_then(|id| analysis.index.symbols.get(&id))
}

/// Compute goto-definition for the symbol at `offset`.
///
/// B3a UFCS resolution (issue #1507): checked first, and narrowly (see
/// `crate::ufcs_hover`'s module doc) — a UFCS-shaped callee's
/// `ResolutionMap` entry spans the whole `recv.verb` range and targets the
/// *receiver*, so without this the fallback below would jump to the
/// receiver's declaration instead of the resolved method's.
/// `ufcs_hover::ufcs_goto_definition`'s outer `Option` is exactly the "is
/// `offset` even on a UFCS call's method segment" gate: `None` there falls
/// through to the generic lookup below; `Some(_)` — even `Some(None)`, a
/// field-call/prelude-intrinsic verdict with no `DefinitionId` — is
/// returned as-is, so a resolved-but-unjumpable verdict stops here instead
/// of falling through to the same wrong receiver-declaration jump this
/// override exists to prevent.
pub fn goto_definition(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<LocationResult> {
    if let Some(hir) = db.hir(file_id)
        && let Some(loc) = crate::ufcs_hover::ufcs_goto_definition(db, hir, file_id, offset)
    {
        return loc;
    }

    let info = find_def_at_offset(analysis, file_id, offset)?;
    Some(LocationResult {
        file: info.file,
        range: info.range,
    })
}

/// Re-derive `(file, range)`'s `DefinitionId` in `db`'s own identity space
/// (issue #1539). A caller-supplied [`AnalysisResult`] (`IdeSession`'s
/// cached analysis, the LSP's `analysis_loop`) is built by
/// `brink_analyzer::analyze_with_options`, an intentionally module-*blind*
/// convenience path; `ProjectDb::ufcs_call_sites_for_target` only speaks
/// `db.resolutions_index()`'s module-aware identity space — see
/// `crate::ufcs_hover`'s module doc's "Why a `FreeFnDesugar`/`FreeFnAutoRef`
/// target resolves through `db`, not `analysis`" section. A declaration's
/// own source range is stable across both computations, so this correlates
/// by that instead of trusting a caller-supplied id directly.
///
/// `pub(crate)` (review finding on #1539/PR #1543): `crate::rename` used to
/// re-implement this exact `HashMap::values().find(...)` scan inline in two
/// places — reused here instead, so the correlation rule (and its
/// determinism caveat below) lives in one place.
///
/// Both this and [`analysis_identity_of`] scan `HashMap::values()` for a
/// `(file, range)` match, so they are order-dependent if two symbols ever
/// share a declaration span — that can't happen for well-formed source
/// today, but is worth flagging given `UfcsLookup::call_sites_for_target`
/// (immediately relevant to these two functions' callers) is carefully
/// sorted for determinism.
pub(crate) fn db_identity_of(
    db: &ProjectDb,
    file: FileId,
    range: TextRange,
) -> Option<DefinitionId> {
    db.resolutions_index()
        .index
        .symbols
        .values()
        .find(|info| info.file == file && info.range == range)
        .map(|info| info.id)
}

/// The mirror of [`db_identity_of`]: re-derive `(file, range)`'s
/// `DefinitionId` in a caller-supplied `analysis`'s own identity space,
/// starting from a `db`-space id (e.g. a UFCS verdict's `target`). Needed so
/// a UFCS-originated lookup can still find the declaration/plain-reference
/// entries that live in `analysis.index`/`analysis.resolutions`, which are
/// keyed by `analysis`'s own (possibly different) ids.
///
/// `pub(crate)` — see [`db_identity_of`]'s doc; `crate::rename` reuses this
/// too.
pub(crate) fn analysis_identity_of(
    analysis: &AnalysisResult,
    file: FileId,
    range: TextRange,
) -> Option<DefinitionId> {
    analysis
        .index
        .symbols
        .values()
        .find(|info| info.file == file && info.range == range)
        .map(|info| info.id)
}

/// Every UFCS call site (project-wide) that desugars to `target` — a
/// free-function `DefinitionId` in `db`'s own identity space — as reference
/// locations pointing at each call's narrow method segment.
fn ufcs_reference_locations(db: &ProjectDb, target: DefinitionId) -> Vec<LocationResult> {
    let mut out = Vec::new();
    for (file, path_range) in db.ufcs_call_sites_for_target(target) {
        let Some(hir) = db.hir(file) else {
            continue;
        };
        let Some(method_range) = crate::ufcs_hover::ufcs_method_range_at_path(hir, path_range)
        else {
            continue;
        };
        out.push(LocationResult {
            file,
            range: method_range,
        });
    }
    out
}

/// Find all references to the symbol at `offset`.
///
/// B3a UFCS resolution (issue #1539): if `offset` sits on a UFCS call
/// site's method segment, the target is resolved straight through `db`'s
/// verdict table (see `crate::ufcs_hover`'s module doc) — the plain
/// `analysis.resolutions` lookup below would otherwise find the
/// *receiver*'s own resolution entry there, exactly the bug
/// `goto_definition` above already works around. Either way, every UFCS
/// call site that desugars to the resolved target is enumerated from `db`
/// too, alongside the plain `ResolutionMap` references —
/// `analysis.resolutions` never carries a UFCS call site's true target, by
/// the analyzer's own design (see `ufcs_hover`'s doc). The two id spaces
/// ([`db_identity_of`]/[`analysis_identity_of`]) are correlated by
/// declaration range rather than assumed equal, since `analysis` may be a
/// caller-supplied, module-blind snapshot.
///
/// Review finding on #1539/PR #1543: correlation between the two identity
/// spaces must not fail *open*. If `analysis`/`db` disagree on where a
/// declaration lives (e.g. an LSP caller's `snap.analysis` isn't
/// revision-locked with the freshly re-locked `db`, so a stale snapshot
/// shifts ranges), the old code silently returned only the *reachable*
/// half of the reference set — UFCS call sites but no plain references, or
/// vice versa — with no signal that the result was incomplete. Both
/// branches below now fail closed (empty result) the moment a needed
/// correlation step misses, rather than ever returning a silently partial
/// reference list.
pub fn find_references(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
    include_declaration: bool,
) -> Vec<LocationResult> {
    let ufcs_target = db
        .hir(file_id)
        .and_then(|hir| crate::ufcs_hover::ufcs_goto_definition_target(db, hir, file_id, offset));

    let (analysis_def_id, db_def_id, target_kind) = match ufcs_target {
        // A field call or prelude intrinsic has no `DefinitionId` at all —
        // stop here rather than falling through to the generic lookup
        // below, which would resolve to the receiver instead.
        Some(None) => return Vec::new(),
        Some(Some(target)) => {
            let resolved = db.resolutions_index();
            let Some(info) = resolved.index.symbols.get(&target) else {
                return Vec::new();
            };
            let Some(analysis_id) = analysis_identity_of(analysis, info.file, info.range) else {
                // Correlation failed: returning only the UFCS call sites
                // below (via `db_def_id`) would silently omit any plain
                // reference/declaration this same symbol also has.
                return Vec::new();
            };
            (analysis_id, target, info.kind)
        }
        None => {
            let def_id = analysis
                .resolutions
                .iter()
                .find(|r| {
                    r.file == file_id && (r.range.contains(offset) || r.range.start() == offset)
                })
                .map(|r| r.target)
                .or_else(|| {
                    analysis
                        .index
                        .symbols
                        .values()
                        .find(|info| {
                            info.file == file_id
                                && (info.range.contains(offset) || info.range.start() == offset)
                        })
                        .map(|info| info.id)
                });
            let Some(def_id) = def_id else {
                return Vec::new();
            };
            let Some(info) = analysis.index.symbols.get(&def_id) else {
                return Vec::new();
            };
            let Some(db_def_id) = db_identity_of(db, info.file, info.range) else {
                // Correlation failed: returning only the plain references
                // below (via `analysis_def_id`) would silently omit any
                // UFCS call site desugaring to this same free function.
                return Vec::new();
            };
            (def_id, db_def_id, info.kind)
        }
    };

    let mut locations = Vec::new();

    // Include the definition itself if requested
    if include_declaration && let Some(info) = analysis.index.symbols.get(&analysis_def_id) {
        locations.push(LocationResult {
            file: info.file,
            range: info.range,
        });
    }

    // Collect all reference sites that resolve to this definition.
    //
    // Issue #1550: a `ResolvedRef` here may be a UFCS call site's
    // *receiver* — `resolve::resolve_function`'s UFCS-shaped fallback
    // records the receiver's resolution against the *whole* `recv.verb`
    // path (mirroring the D2 side table's own key), not just the
    // receiver's own segment. Reporting that whole-path range as a
    // reference to the receiver would wrongly include the method segment,
    // so it's narrowed to the receiver's own first segment via
    // `ufcs_hover::ufcs_receiver_head_range_at_path` — the same narrowing
    // `rename`'s plain-reference loop applies for the same reason.
    //
    // Issue #1560 (the non-UFCS-call half of the same bug):
    // `resolve::lookup_variable`'s dotted-field-access fallback (step 11)
    // records the SAME whole-path shape for a plain (non-call) reference
    // like `p.x.y` — narrowed the same way, or a reference to `p` wrongly
    // reports `.x.y` as part of its own range too.
    //
    // Issue #1571 (the tail half): a qualified reference to a stitch, list
    // item or label (`-> hub.market`, `Colors.Red`) names its target with
    // the path's *last* segment, so an unnarrowed range wrongly reports the
    // qualifier as part of the reference.
    //
    // All three narrowings are composed by
    // `ufcs_hover::narrowed_reference_range`, shared with `rename` and
    // `prepare_rename`.
    //
    // Review finding on #1838 (blocking): a `ResolvedRef` targeting a
    // natural-notation element-dispatch handler may be the compiler's own
    // *synthesized* call (issue #1838), whose range is the entire claimed
    // prose line rather than any real occurrence of the handler's name —
    // reporting it as a reference location would highlight that whole
    // prose line. `ufcs_hover::is_synthesized_element_ref` excludes it, the
    // same exclusion `rename`/`prepare_rename` apply.
    for resolved in &analysis.resolutions {
        if resolved.target == analysis_def_id {
            let hir = db.hir(resolved.file);
            if hir.is_some_and(|h| crate::ufcs_hover::is_synthesized_element_ref(h, resolved.range))
            {
                continue;
            }
            let range = hir
                .and_then(|h| {
                    crate::ufcs_hover::narrowed_reference_range(h, resolved.range, target_kind)
                })
                .unwrap_or(resolved.range);
            locations.push(LocationResult {
                file: resolved.file,
                range,
            });
        }
    }

    // UFCS-desugared call sites targeting the same free function (issue
    // #1539).
    locations.extend(ufcs_reference_locations(db, db_def_id));

    locations
}

#[cfg(test)]
mod tests {
    use rowan::TextSize;

    use super::{find_references, goto_definition};
    use crate::session::IdeSession;

    const UFCS_FREE_FN_SRC: &str = "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.greet(3);
}
";

    /// go-to-def for the first occurrence of `needle` in a native `.brink`
    /// fixture (B3a UFCS resolution is native-only).
    fn goto_definition_at_native(src: &str, needle: &str) -> Option<(String, u32)> {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let pos = u32::try_from(src.find(needle).expect("needle present")).expect("offset");
        goto_definition(session.db(), analysis, file_id, TextSize::from(pos)).map(|loc| {
            let start: u32 = loc.range.start().into();
            let end: u32 = loc.range.end().into();
            (src[start as usize..end as usize].to_owned(), start)
        })
    }

    // ── Issue #1507: go-to-def follows the D2-resolved target ────────────

    #[test]
    fn goto_definition_on_a_ufcs_method_segment_jumps_to_the_free_function() {
        let (text, _start) =
            goto_definition_at_native(UFCS_FREE_FN_SRC, "greet(3)").expect("jump target");
        assert_eq!(text, "greet", "must jump to the `fn greet` declaration");
    }

    #[test]
    fn goto_definition_on_the_receiver_segment_of_a_ufcs_call_is_unaffected() {
        // Hovering/jumping from `g` itself (before the dot) must keep
        // jumping to the receiver's own declaration — the override is
        // narrowly scoped to the method segment.
        let (text, _start) =
            goto_definition_at_native(UFCS_FREE_FN_SRC, "g.greet(3)").expect("jump target");
        assert_eq!(text, "g", "must jump to the receiver's own `let g` binding");
    }

    #[test]
    fn goto_definition_on_a_ufcs_prelude_desugar_finds_no_target() {
        // A prelude verb has no `DefinitionId` (issue #1507: the explicit,
        // no-unwrap arm) — go-to-def must return nothing rather than
        // falling through to the receiver's own declaration.
        let src = "\
struct Guest {
  name: string
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.len();
}
";
        assert!(
            goto_definition_at_native(src, "len()").is_none(),
            "a prelude intrinsic has no DefinitionId to jump to"
        );
    }

    #[test]
    fn goto_definition_on_a_ufcs_field_call_finds_no_target() {
        // Fixture mirrors `brink-analyzer`'s
        // `a_function_typed_field_wins_and_is_recorded_as_a_field_call`
        // (crates/internal/brink-analyzer/tests/ufcs_resolution.rs). A
        // struct field is not an index symbol — `ufcs_goto_definition`
        // returns `Some(None)` here (resolved, but nowhere to jump), which
        // must surface as `None` rather than falling through to the
        // receiver's own declaration.
        let src = "\
struct Guest {
  greet: fn(int): int
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
";
        assert!(
            goto_definition_at_native(src, "greet(3)").is_none(),
            "a field call has no DefinitionId to jump to"
        );
    }

    #[test]
    fn goto_definition_on_a_ufcs_free_fn_auto_ref_jumps_to_the_free_function() {
        // Fixture mirrors `brink-analyzer`'s
        // `a_ref_first_param_auto_refs_a_frame_local_receiver`
        // (crates/internal/brink-analyzer/tests/ufcs_resolution.rs). Proves
        // the `FreeFnAutoRef` arm is live — it reaches `target` through an
        // or-pattern shared with `FreeFnDesugar`, so nothing else exercises
        // it independently.
        let src = "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn main() {
  let g = 1;
  g.bump(5);
}
";
        let (text, _start) = goto_definition_at_native(src, "bump(5)").expect("jump target");
        assert_eq!(text, "bump", "must jump to the `fn bump` declaration");
    }

    // ── Issue #1539: find_references follows UFCS call sites ─────────────

    #[test]
    fn find_references_from_a_free_fn_declaration_includes_its_ufcs_call_site() {
        // `fn greet` is called once via UFCS (`g.greet(3)`) and never called
        // directly. Before #1539, `analysis.resolutions` never carried this
        // call site's true target, so a references query from the
        // declaration found nothing.
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", UFCS_FREE_FN_SRC.to_string());
        let analysis = session.analysis().expect("analysis");
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");

        let refs = find_references(
            session.db(),
            analysis,
            file_id,
            TextSize::from(decl_pos),
            false,
        );

        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");
        assert!(
            refs.iter()
                .any(|loc| loc.file == file_id && loc.range.start() == TextSize::from(call_pos)),
            "expected the UFCS call site's method segment among references, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
    }

    #[test]
    fn find_references_from_a_ufcs_call_site_targets_the_free_function() {
        // Querying references *from* the UFCS call site's method segment
        // (rather than the declaration) must resolve to the free function,
        // not the receiver — mirroring `goto_definition`'s own fix.
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", UFCS_FREE_FN_SRC.to_string());
        let analysis = session.analysis().expect("analysis");
        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");

        let refs = find_references(
            session.db(),
            analysis,
            file_id,
            TextSize::from(call_pos),
            true,
        );

        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");
        assert!(
            refs.iter()
                .any(|loc| loc.file == file_id && loc.range.start() == TextSize::from(decl_pos)),
            "expected the `fn greet` declaration among references, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
    }

    // ── Issue #1550: find_references narrows a UFCS receiver reference to
    // its own segment (the mirror of #1539, from the receiver side) ──────

    #[test]
    fn find_references_from_a_ufcs_receiver_declaration_reports_only_the_receiver_segment() {
        // `resolve::resolve_function`'s UFCS-shaped-callee fallback records
        // the receiver's (`g`'s) `ResolvedRef` against the *whole*
        // `g.greet` path, not just `g`'s own segment (mirroring the D2 side
        // table's own key). Reporting that whole-path range as a reference
        // to `g` would wrongly include the `.greet` method segment — the
        // reported span at the call site must be exactly the 1-byte `g`
        // segment.
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", UFCS_FREE_FN_SRC.to_string());
        let analysis = session.analysis().expect("analysis");
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("g = Guest").expect("decl")).expect("offset");

        let refs = find_references(
            session.db(),
            analysis,
            file_id,
            TextSize::from(decl_pos),
            false,
        );

        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("g.greet(3)").expect("call")).expect("offset");
        let found = refs
            .iter()
            .find(|loc| loc.file == file_id && loc.range.start() == TextSize::from(call_pos));
        assert!(
            found.is_some(),
            "expected the UFCS call site's receiver segment among references, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
        let range = found.expect("checked above").range;
        assert_eq!(
            usize::from(range.end()) - usize::from(range.start()),
            1,
            "the reported reference must span only the receiver's own `g` segment (1 byte), \
             not the whole `g.greet` path — got {range:?}"
        );
    }

    #[test]
    fn find_references_from_a_free_fn_auto_ref_declaration_includes_its_ufcs_call_site() {
        // Review finding on #1539/PR #1543: `UfcsLookup::call_sites_for_target`
        // matches `FreeFnDesugar { target } | FreeFnAutoRef { target }` in one
        // or-pattern arm, but every other test exercising the reverse
        // (target → call sites) direction used only a `FreeFnDesugar`
        // fixture — so the `FreeFnAutoRef` half of that arm was never
        // independently proven live for `find_references`/`rename`. Fixture
        // mirrors `goto_definition_on_a_ufcs_free_fn_auto_ref_jumps_to_the_free_function`
        // above.
        let src = "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn main() {
  let g = 1;
  g.bump(5);
}
";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let decl_pos = u32::try_from(src.find("bump(ref n").expect("decl")).expect("offset");

        let refs = find_references(
            session.db(),
            analysis,
            file_id,
            TextSize::from(decl_pos),
            false,
        );

        let call_pos = u32::try_from(src.find("bump(5)").expect("call")).expect("offset");
        assert!(
            refs.iter()
                .any(|loc| loc.file == file_id && loc.range.start() == TextSize::from(call_pos)),
            "expected the FreeFnAutoRef UFCS call site among references, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
    }

    #[test]
    fn find_references_refuses_rather_than_silently_dropping_ufcs_sites_when_identity_spaces_disagree()
     {
        // Review finding on #1539/PR #1543: `analysis`/`db` are not
        // revision-locked for every caller (e.g. the LSP's cached
        // `snap.analysis` vs. a freshly re-locked `self.db`). Before this
        // fix, a correlation miss between the two identity spaces silently
        // returned only the *reachable* half of the reference set (plain
        // references without the UFCS call sites, or vice versa) — this
        // must now return nothing rather than a silently incomplete list.
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", UFCS_FREE_FN_SRC.to_string());
        // Captured before the edit below: self-consistent with the
        // original source's ranges, but about to go stale relative to the
        // session's `db` once the source shifts.
        let stale_analysis = session.analysis().expect("analysis").clone();
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");

        let shifted_src = format!("// shifted\n{UFCS_FREE_FN_SRC}");
        session.update_and_analyze("test.brink", shifted_src);

        let refs = find_references(
            session.db(),
            &stale_analysis,
            file_id,
            TextSize::from(decl_pos),
            true,
        );
        assert!(
            refs.is_empty(),
            "a stale analysis/db identity-space mismatch must return no references, not a \
             partial list, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
    }

    // ── Issue #1560: find_references on the HEAD of a plain (non-UFCS-call)
    // dotted field access reports only the head segment — the non-call
    // mirror of #1550's
    // `find_references_from_a_ufcs_receiver_declaration_reports_only_the_receiver_segment`
    // ──────────────────────────────────────────────────────────────────

    const FIELD_ACCESS_SRC: &str = "\
struct Point {
  y: int
}

struct Guest {
  x: Point
}

fn main() {
  let p = Guest { x: Point { y: 2 } };
  let n = p.x.y;
}
";

    #[test]
    fn find_references_from_a_plain_field_access_head_declaration_reports_only_the_head_segment() {
        // `resolve::lookup_variable`'s dotted-field-access fallback (step
        // 11, resolve.rs:474-503) records the head variable's (`p`'s)
        // `ResolvedRef` against the *whole* `p.x.y` path, not just `p`'s
        // own segment — mirroring the D2 side table's own key, and the
        // #1550 UFCS-receiver bug this issue is the non-call half of.
        // Reporting that whole-path range as a reference to `p` would
        // wrongly include the `.x.y` field segments; the reported span at
        // the reference site must be exactly the 1-byte `p` segment.
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", FIELD_ACCESS_SRC.to_string());
        let analysis = session.analysis().expect("analysis");
        let decl_pos =
            u32::try_from(FIELD_ACCESS_SRC.find("p = Guest").expect("decl")).expect("offset");

        let refs = find_references(
            session.db(),
            analysis,
            file_id,
            TextSize::from(decl_pos),
            false,
        );

        let ref_pos = u32::try_from(FIELD_ACCESS_SRC.find("p.x.y").expect("ref")).expect("offset");
        let found = refs
            .iter()
            .find(|loc| loc.file == file_id && loc.range.start() == TextSize::from(ref_pos));
        assert!(
            found.is_some(),
            "expected the field-access reference's head segment among references, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
        let range = found.expect("checked above").range;
        assert_eq!(
            usize::from(range.end()) - usize::from(range.start()),
            1,
            "the reported reference must span only the head's own `p` segment (1 byte), not \
             the whole `p.x.y` path — got {range:?}"
        );
    }

    #[test]
    fn find_references_on_a_stitch_reports_only_the_tail_segment_of_a_qualified_reference() {
        // Issue #1571, the tail half: a qualified `-> hub.market` divert
        // records its `ResolvedRef` against the whole `hub.market` path,
        // targeting the *stitch*. Reporting that whole range as a reference
        // to `market` wrongly swallows the `hub.` qualifier — the same
        // over-wide span #1550/#1560 fixed from the head end.
        let src = "=== hub ===\n= market\nHi.\n-> DONE\n=== main ===\n-> hub.market\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("t.ink", src.to_string());
        let hir = session.hir(file_id).expect("hir");
        let decl_pos =
            crate::rename::declaration_offset(hir, "hub", Some("market")).expect("stitch decl");
        let analysis = session.analysis().expect("analysis");

        let refs = find_references(session.db(), analysis, file_id, decl_pos, false);

        let tail_pos =
            u32::try_from(src.find("hub.market").expect("ref") + "hub.".len()).expect("offset");
        let found = refs
            .iter()
            .find(|loc| loc.file == file_id && loc.range.start() == TextSize::from(tail_pos));
        assert!(
            found.is_some(),
            "expected the qualified reference's tail segment among references, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
        let range = found.expect("checked above").range;
        assert_eq!(
            usize::from(range.end()) - usize::from(range.start()),
            "market".len(),
            "the reported reference must span only the `market` segment, not the whole \
             `hub.market` path — got {range:?}"
        );
    }

    #[test]
    fn find_references_from_a_claiming_handler_does_not_report_the_claimed_prose_line() {
        // Issue #1838 review finding (blocking, correctness) — the third
        // (`find_references`) surface of the pair check alongside
        // `rename`/`prepare_rename` in `crate::rename`'s tests: a claiming
        // handler's compiler-synthesized call has a `ResolvedRef` whose
        // range is the entire claimed prose line, not a real occurrence of
        // the handler's name. Reporting it as a reference location would
        // highlight that whole prose line rather than any identifier.
        let src = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\nfn interior(place) {\n  return place;\n}\n\nflow main() {\n  INT. MARKET SQUARE\n}\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let decl_pos = u32::try_from(src.find("interior(place)").expect("decl")).expect("offset");

        let refs = find_references(
            session.db(),
            analysis,
            file_id,
            TextSize::from(decl_pos),
            false,
        );

        let heading_pos =
            u32::try_from(src.find("INT. MARKET SQUARE").expect("heading")).expect("offset");
        assert!(
            refs.iter()
                .all(|loc| loc.range.start() != TextSize::from(heading_pos)),
            "the claimed prose line must never be reported as a reference location, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
    }
}
