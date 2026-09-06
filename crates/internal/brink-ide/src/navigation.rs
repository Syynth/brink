use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
use brink_format::DefinitionId;
use brink_ir::symbols::{RefKind, SymbolKind};
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

    if let Some(loc) = include_target(db, file_id, offset) {
        return Some(loc);
    }

    let info = find_def_at_offset(analysis, file_id, offset)?;
    Some(LocationResult {
        file: info.file,
        range: info.range,
    })
}

/// The file an `INCLUDE` statement under `offset` names, as a jump to its
/// start — an include is a reference to a file the way a divert is a
/// reference to a knot, and an author who Cmd-clicks one expects to land
/// in it. `None` when `offset` is not on an include, or when the include
/// names a file the project does not hold (which is already a diagnostic).
fn include_target(
    db: &ProjectDb,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<LocationResult> {
    let hir = db.hir(file_id)?;
    let from = db.file_path(file_id)?;
    let site = hir
        .includes
        .iter()
        .find(|inc| inc.ptr.text_range().contains_inclusive(offset))?;
    let target = db.file_id(&brink_db::resolve_include_path(from, &site.file_path))?;
    Some(LocationResult {
        file: target,
        range: TextRange::empty(rowan::TextSize::from(0)),
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
    find_references_with_kinds(db, analysis, file_id, offset, include_declaration)
        .into_iter()
        .map(|r| LocationResult {
            file: r.file,
            range: r.range,
        })
        .collect()
}

/// How a reference site uses the symbol — the Search panel's per-card
/// badges (docs/search-results-cards-spec.md, PR E).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// The declaration itself.
    Decl,
    /// A function/external call site (UFCS-desugared calls included).
    Call,
    /// A divert/tunnel/thread target.
    Divert,
    /// A value read (list/struct/type uses included).
    Read,
    /// An assignment target (`~ x = …`, `~ x += …`, `~ x++`).
    Write,
}

/// A reference location plus how the site uses the symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceWithKind {
    pub file: FileId,
    pub range: TextRange,
    pub kind: ReferenceKind,
}

/// [`find_references`], with each site classified ([`ReferenceKind`]).
///
/// Classification sources, in order: the site's own manifest
/// [`RefKind`] (the per-file `unresolved` entry sharing the resolution's
/// exact range — the analyzer resolves those entries in place, so the
/// ranges correspond); else the *target symbol's* kind (a knot/stitch/label
/// reference is a divert, a function/external reference is a call). A
/// variable-shaped read upgrades to [`ReferenceKind::Write`] when the
/// pre-narrowing range sits inside an assignment target (or a `++`/`--`
/// statement) in that file's HIR.
pub fn find_references_with_kinds(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
    include_declaration: bool,
) -> Vec<ReferenceWithKind> {
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
    // Per-file assignment-target ranges, computed lazily (write upgrade).
    let mut write_ranges: std::collections::BTreeMap<FileId, Vec<TextRange>> =
        std::collections::BTreeMap::new();

    // Include the definition itself if requested
    if include_declaration && let Some(info) = analysis.index.symbols.get(&analysis_def_id) {
        locations.push(ReferenceWithKind {
            file: info.file,
            range: info.range,
            kind: ReferenceKind::Decl,
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
            let kind = reference_kind_of(db, resolved, target_kind, &mut write_ranges);
            let range = hir
                .and_then(|h| {
                    crate::ufcs_hover::narrowed_reference_range(h, resolved.range, target_kind)
                })
                .unwrap_or(resolved.range);
            locations.push(ReferenceWithKind {
                file: resolved.file,
                range,
                kind,
            });
        }
    }

    // UFCS-desugared call sites targeting the same free function (issue
    // #1539).
    locations.extend(
        ufcs_reference_locations(db, db_def_id)
            .into_iter()
            .map(|loc| ReferenceWithKind {
                file: loc.file,
                range: loc.range,
                kind: ReferenceKind::Call,
            }),
    );

    locations
}

/// Classify one plain resolution site. The manifest's own [`RefKind`] for
/// the site (matched by exact range) wins; the target symbol's kind is the
/// fallback. Variable-shaped reads upgrade to writes inside an assignment
/// target.
fn reference_kind_of(
    db: &ProjectDb,
    resolved: &brink_ir::symbols::ResolvedRef,
    target_kind: SymbolKind,
    write_ranges: &mut std::collections::BTreeMap<FileId, Vec<TextRange>>,
) -> ReferenceKind {
    let site_kind = db
        .manifest(resolved.file)
        .and_then(|m| m.unresolved.iter().find(|r| r.range == resolved.range))
        .map(|r| r.kind);
    let base = match site_kind {
        Some(RefKind::Divert) => ReferenceKind::Divert,
        Some(RefKind::Function) => ReferenceKind::Call,
        Some(RefKind::Variable | RefKind::List | RefKind::Struct | RefKind::Type) => {
            ReferenceKind::Read
        }
        // No manifest entry (locals resolve in-file without one): fall back
        // to what the *target* is — a knot/stitch/label reference is a
        // divert, a function/external reference is a call.
        None => match target_kind {
            SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label => ReferenceKind::Divert,
            SymbolKind::External => ReferenceKind::Call,
            _ => ReferenceKind::Read,
        },
    };
    if base != ReferenceKind::Read {
        return base;
    }
    let ranges = write_ranges
        .entry(resolved.file)
        .or_insert_with(|| assignment_target_ranges(db, resolved.file));
    if ranges.iter().any(|r| r.contains_range(resolved.range)) {
        ReferenceKind::Write
    } else {
        ReferenceKind::Read
    }
}

/// Every assignment-target span in `file`'s HIR: `~ x = …` / `~ x += …`
/// statements (weave and `~ { … }` logic-block forms) plus `x++`/`x--`
/// statement targets.
fn assignment_target_ranges(db: &ProjectDb, file: FileId) -> Vec<TextRange> {
    use brink_ir::hir::{BlockStmt, Expr, Stmt};

    struct Targets {
        ranges: Vec<TextRange>,
    }
    impl Targets {
        fn push_target(&mut self, target: &Expr) {
            match target {
                Expr::Path(p) => self.ranges.push(p.range),
                Expr::Postfix(inner, _) => self.push_target(inner),
                _ => {}
            }
        }
        fn collect_if(&mut self, i: &brink_ir::hir::IfStmt) {
            for b in &i.body {
                self.collect_block_stmt(b);
            }
            match &i.else_branch {
                Some(brink_ir::hir::ElseBranch::ElseIf(nested)) => self.collect_if(nested),
                Some(brink_ir::hir::ElseBranch::Else(body)) => {
                    for b in body {
                        self.collect_block_stmt(b);
                    }
                }
                None => {}
            }
        }
        fn collect_block_stmt(&mut self, bs: &BlockStmt) {
            match bs {
                BlockStmt::Assignment(a) => self.push_target(&a.target),
                BlockStmt::ExprStmt(Expr::Postfix(inner, _)) => self.push_target(inner),
                BlockStmt::If(i) => self.collect_if(i),
                BlockStmt::While(w) => {
                    for b in &w.body {
                        self.collect_block_stmt(b);
                    }
                }
                BlockStmt::For(f) => {
                    for b in &f.body {
                        self.collect_block_stmt(b);
                    }
                }
                _ => {}
            }
        }
    }
    impl brink_ir::hir::HirVisitor for Targets {
        fn enter_stmt(&mut self, stmt: &Stmt) {
            match stmt {
                Stmt::Assignment(a) => self.push_target(&a.target),
                Stmt::ExprStmt(Expr::Postfix(inner, _)) => self.push_target(inner),
                Stmt::LogicBlock(lb) => {
                    for bs in &lb.stmts {
                        self.collect_block_stmt(bs);
                    }
                }
                _ => {}
            }
        }
    }

    let mut targets = Targets { ranges: Vec::new() };
    if let Some(hir) = db.hir(file) {
        brink_ir::hir::visit::visit(hir, &mut targets);
    }
    targets.ranges
}

#[cfg(test)]
mod tests {
    use rowan::TextSize;

    use super::{ReferenceKind, find_references, find_references_with_kinds, goto_definition};
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

    // ── Card spec PR E: reference kinds ──────────────────────────────

    const KINDS_SRC: &str = "\
VAR gold = 10

=== function pay(n) ===
~ gold = gold - n

== shop ==
~ pay(2)
~ gold++
You have {gold} coins.
-> shop
";

    /// Kinds for every reference of the symbol at `needle`'s first
    /// occurrence, as (site text, kind) pairs in result order.
    fn kinds_at(src: &str, needle: &str) -> Vec<(String, ReferenceKind)> {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let pos = u32::try_from(src.find(needle).expect("needle present")).expect("offset");
        find_references_with_kinds(session.db(), analysis, file_id, TextSize::from(pos), true)
            .into_iter()
            .map(|r| {
                let start: usize = r.range.start().into();
                let end: usize = r.range.end().into();
                (src[start..end].to_owned(), r.kind)
            })
            .collect()
    }

    #[test]
    fn variable_references_classify_decl_reads_and_writes() {
        let kinds = kinds_at(KINDS_SRC, "gold");
        // Declaration first, then sites: LHS of `~ gold = gold - n` is a
        // write, its RHS a read, `~ gold++` a write, `{gold}` a read.
        assert_eq!(kinds[0], ("gold".to_owned(), ReferenceKind::Decl));
        let writes = kinds
            .iter()
            .filter(|(_, k)| *k == ReferenceKind::Write)
            .count();
        let reads = kinds
            .iter()
            .filter(|(_, k)| *k == ReferenceKind::Read)
            .count();
        assert_eq!(writes, 2, "LHS assignment + increment: {kinds:?}");
        assert_eq!(reads, 2, "RHS read + inline print: {kinds:?}");
        assert_eq!(kinds.len(), 5, "{kinds:?}");
    }

    #[test]
    fn divert_and_call_sites_classify_as_such() {
        let shop = kinds_at(KINDS_SRC, "shop");
        assert_eq!(shop[0].1, ReferenceKind::Decl, "{shop:?}");
        assert!(
            shop[1..].iter().all(|(_, k)| *k == ReferenceKind::Divert),
            "{shop:?}"
        );
        assert_eq!(shop.len(), 2, "{shop:?}");

        let pay = kinds_at(KINDS_SRC, "pay");
        assert_eq!(pay[0].1, ReferenceKind::Decl, "{pay:?}");
        assert!(
            pay[1..].iter().all(|(_, k)| *k == ReferenceKind::Call),
            "{pay:?}"
        );
        assert_eq!(pay.len(), 2, "{pay:?}");
    }

    #[test]
    fn ufcs_call_sites_classify_as_calls() {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", UFCS_FREE_FN_SRC.to_string());
        let analysis = session.analysis().expect("analysis");
        let pos = u32::try_from(UFCS_FREE_FN_SRC.find("greet(g, loudness)").expect("decl"))
            .expect("offset");
        let kinds =
            find_references_with_kinds(session.db(), analysis, file_id, TextSize::from(pos), true);
        assert!(
            kinds.iter().any(|r| r.kind == ReferenceKind::Call),
            "the `g.greet(3)` UFCS site must classify as a call: {kinds:?}"
        );
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

    // ── Issue #2272: goto-def/find-references on a struct used only as a
    // parameter/return type ──────────────────────────────────────────────

    const STRUCT_AS_PARAM_TYPE_SRC: &str = "\
struct Guest {
  name: string
}

fn heal(hp: Guest): Guest {
  return hp;
}
";

    /// Position of the `Guest` occurrence inside `hp: Guest`'s param
    /// annotation — deliberately not the plain `.find("Guest")` used by
    /// [`goto_definition_at_native`]'s `needle` for other tests in this
    /// file, since that would land on the *declaration*'s own `Guest`
    /// (the first occurrence in [`STRUCT_AS_PARAM_TYPE_SRC`]), not the
    /// annotation.
    fn param_annotation_guest_pos() -> TextSize {
        let offset = STRUCT_AS_PARAM_TYPE_SRC
            .find("hp: Guest")
            .expect("param annotation")
            + "hp: ".len();
        TextSize::from(u32::try_from(offset).expect("offset"))
    }

    /// Position of the return-type annotation's `Guest` — the last
    /// occurrence in the fixture.
    fn return_annotation_guest_pos() -> TextSize {
        let offset = STRUCT_AS_PARAM_TYPE_SRC
            .rfind("Guest")
            .expect("return annotation");
        TextSize::from(u32::try_from(offset).expect("offset"))
    }

    #[test]
    fn goto_definition_on_a_struct_used_only_as_a_param_type_jumps_to_the_declaration() {
        // RED before issue #2272's fix: `project_knot` recorded a param's
        // annotation on `LocalSymbol.annotation` (the `signature()`
        // firewall's own consumer) but never walked it into a `RefKind::
        // Type` reference — a struct referenced only from a param's type
        // was invisible to `analysis.resolutions`, so goto-def from that
        // occurrence found nothing at all.
        let mut session = IdeSession::new();
        let file_id =
            session.update_and_analyze("test.brink", STRUCT_AS_PARAM_TYPE_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        let loc = goto_definition(
            session.db(),
            analysis,
            file_id,
            param_annotation_guest_pos(),
        )
        .expect("jump target");
        let start: usize = u32::from(loc.range.start()) as usize;
        let end: usize = u32::from(loc.range.end()) as usize;
        assert_eq!(
            &STRUCT_AS_PARAM_TYPE_SRC[start..end],
            "Guest",
            "must jump to the `struct Guest` declaration from the param annotation occurrence"
        );
    }

    #[test]
    fn goto_definition_on_a_struct_used_only_as_a_return_type_jumps_to_the_declaration() {
        // The return-type mirror of the param case above — `Knot::
        // return_type` was never walked into a reference at all (unlike a
        // param's, which was at least stored, just not referenced).
        let mut session = IdeSession::new();
        let file_id =
            session.update_and_analyze("test.brink", STRUCT_AS_PARAM_TYPE_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        let loc = goto_definition(
            session.db(),
            analysis,
            file_id,
            return_annotation_guest_pos(),
        )
        .expect("jump target");
        let start: usize = u32::from(loc.range.start()) as usize;
        let end: usize = u32::from(loc.range.end()) as usize;
        assert_eq!(&STRUCT_AS_PARAM_TYPE_SRC[start..end], "Guest");
    }

    #[test]
    fn find_references_from_a_struct_declaration_includes_its_param_and_return_type_occurrences() {
        let mut session = IdeSession::new();
        let file_id =
            session.update_and_analyze("test.brink", STRUCT_AS_PARAM_TYPE_SRC.to_string());
        let analysis = session.analysis().expect("analysis");
        let decl_pos =
            u32::try_from(STRUCT_AS_PARAM_TYPE_SRC.find("Guest {").expect("decl")).expect("offset");

        let refs = find_references(
            session.db(),
            analysis,
            file_id,
            TextSize::from(decl_pos),
            false,
        );

        assert!(
            refs.iter()
                .any(|loc| loc.file == file_id
                    && loc.range.start() == param_annotation_guest_pos()),
            "expected the param annotation's `Guest` occurrence among references, got {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
        assert!(
            refs.iter().any(
                |loc| loc.file == file_id && loc.range.start() == return_annotation_guest_pos()
            ),
            "expected the return-type annotation's `Guest` occurrence among references, got \
             {:?}",
            refs.iter().map(|l| l.range).collect::<Vec<_>>()
        );
    }
}
