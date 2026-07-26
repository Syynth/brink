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
fn db_identity_of(db: &ProjectDb, file: FileId, range: TextRange) -> Option<DefinitionId> {
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
fn analysis_identity_of(
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

    let (analysis_def_id, db_def_id) = match ufcs_target {
        // A field call or prelude intrinsic has no `DefinitionId` at all —
        // stop here rather than falling through to the generic lookup
        // below, which would resolve to the receiver instead.
        Some(None) => return Vec::new(),
        Some(Some(target)) => {
            let analysis_id = db
                .resolutions_index()
                .index
                .symbols
                .get(&target)
                .and_then(|info| analysis_identity_of(analysis, info.file, info.range));
            (analysis_id, Some(target))
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
            let db_def_id = def_id.and_then(|id| {
                analysis
                    .index
                    .symbols
                    .get(&id)
                    .and_then(|info| db_identity_of(db, info.file, info.range))
            });
            (def_id, db_def_id)
        }
    };

    if analysis_def_id.is_none() && db_def_id.is_none() {
        return Vec::new();
    }

    let mut locations = Vec::new();

    if let Some(def_id) = analysis_def_id {
        // Include the definition itself if requested
        if include_declaration && let Some(info) = analysis.index.symbols.get(&def_id) {
            locations.push(LocationResult {
                file: info.file,
                range: info.range,
            });
        }

        // Collect all reference sites that resolve to this definition
        for resolved in &analysis.resolutions {
            if resolved.target == def_id {
                locations.push(LocationResult {
                    file: resolved.file,
                    range: resolved.range,
                });
            }
        }
    }

    // UFCS-desugared call sites targeting the same free function (issue
    // #1539).
    if let Some(db_def_id) = db_def_id {
        locations.extend(ufcs_reference_locations(db, db_def_id));
    }

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
}
