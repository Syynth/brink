use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
use brink_ir::{FileId, HirFile};
use rowan::{TextRange, TextSize};

use crate::navigation::{analysis_identity_of, db_identity_of, find_def_at_offset};
use crate::session::IdeSession;
use crate::structural_result::{StructuralResult, gate};

/// A single text edit within a file.
pub struct FileEdit {
    pub file: FileId,
    pub range: TextRange,
    pub new_text: String,
}

/// The result of a rename operation.
pub struct RenameResult {
    pub edits: Vec<FileEdit>,
    /// Set when the renamed symbol is an `EXTERNAL` (ruled 2026-08-24): the
    /// declaration's (file, name-range) — `rename_safe` synthesizes the
    /// always-unsafe E190 host-binding entry from it, so the rename applies
    /// only through the Force gate.
    pub external_binding: Option<(FileId, rowan::TextRange)>,
}

/// Check if a rename is possible at `offset` and return the renameable range.
///
/// B3a UFCS resolution (issue #1539): if `offset` sits on a UFCS call site's
/// method segment, checked first via the same verdict table
/// `crate::ufcs_hover` uses — a field call or prelude intrinsic (no
/// `DefinitionId`) is not renameable; a free-function target is, and the
/// renameable range is the method segment's own span (mirroring how a plain
/// reference's own range, not its target's declaration range, is returned
/// below).
///
/// Review finding on #1539/PR #1543: a UFCS target must clear the same
/// `SymbolKind::External` guard `rename` itself applies once it resolves the
/// same target (below) — without this, `prepare_rename` reported an
/// `external fn`'s UFCS call site as renameable, and the subsequent `rename`
/// call then returned `None`, i.e. a silent no-op from the caller's (LSP's)
/// point of view.
pub fn prepare_rename(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
) -> Option<TextRange> {
    if let Some(hir) = db.hir(file_id)
        && let Some(verdict_target) =
            crate::ufcs_hover::ufcs_goto_definition_target(db, hir, file_id, offset)
    {
        // `verdict_target.is_none()` (field call / prelude intrinsic): not
        // renameable — return `None` rather than falling through to the
        // generic lookup below, which would offer the receiver's range.
        let target = verdict_target?;
        let resolved = db.resolutions_index();
        let _info = resolved.index.symbols.get(&target)?;
        // Externals included (ruled 2026-08-24): renameable behind the
        // always-unsafe Force gate — see `RenameResult::external_binding`.
        return crate::ufcs_hover::ufcs_method_range_at_offset(hir, offset);
    }

    let info = find_def_at_offset(analysis, file_id, offset)?;

    // Builtins cannot be renamed (they never resolve to a symbol here).
    // Externals CAN (ruled 2026-08-24) — behind the always-unsafe Force
    // gate; `rename_safe` synthesizes the E190 host-binding entry.

    // Return the range of the symbol under the cursor (reference or definition site)
    //
    // Issue #1571: the reference range may be a *whole dotted path* the
    // symbol only owns one segment of — `p` in `p.x.y`, `recv` in
    // `recv.verb(…)`, `market` in `-> hub.market`. Returning it unnarrowed
    // made F2 on the head/receiver segment highlight (and offer to replace)
    // the whole path, contradicting the range `rename` then actually edits.
    // The same composed narrowing `rename` and `find_references` apply is
    // applied here, so the highlighted range and the edited range agree.
    //
    // Review finding on #1838 (blocking): a `ResolvedRef` may instead be a
    // natural-notation element dispatch's compiler-*synthesized* call
    // (issue #1838), whose range is the **entire claimed prose line**, not
    // any real occurrence of the handler's name — narrowing has nothing to
    // narrow it down to, so the unfiltered lookup below would offer the
    // whole prose line as renameable and a subsequent `rename` would
    // corrupt it. `ufcs_hover::is_synthesized_element_ref` excludes it from
    // the candidate search entirely, the same exclusion `rename` and
    // `find_references` apply.
    let hir = db.hir(file_id);
    analysis
        .resolutions
        .iter()
        .find(|r| {
            r.file == file_id
                && (r.range.contains(offset) || r.range.start() == offset)
                && hir.is_none_or(|h| !crate::ufcs_hover::is_synthesized_element_ref(h, r.range))
        })
        .map(|r| {
            hir.and_then(|h| crate::ufcs_hover::narrowed_reference_range(h, r.range, info.kind))
                .unwrap_or(r.range)
        })
        // The declaration-site fallback: reached only when no `ResolvedRef`
        // covers `offset` at all, which (`find_def_at_offset`'s own two-step
        // lookup) means `info` was found via its *declaration* site, not a
        // reference — so `info.range` legitimately contains `offset` there.
        // Review finding on #1838: with the synthesized-ref exclusion above,
        // that invariant can break — a claimed prose line's `ResolvedRef`
        // does cover `offset`, `find_def_at_offset`'s own unfiltered lookup
        // resolves `info` through it, but the filtered search above (rightly)
        // excludes it and finds nothing. Without the `info.range.contains`
        // guard, this fallback would still fire on `info.file == file_id`
        // alone and offer the handler's *declaration* range as renameable
        // from a cursor sitting on an unrelated claimed prose line elsewhere
        // in the file — not a source corruption, but a bogus renameable
        // answer with no relation to the cursor.
        .or_else(|| {
            (info.file == file_id && (info.range.contains(offset) || info.range.start() == offset))
                .then_some(info.range)
        })
}

/// Compute a rename of the symbol at `offset` to `new_name`.
///
/// B3a UFCS resolution (issue #1539): if `offset` sits on a UFCS call
/// site's method segment, the target free function is resolved through the
/// same verdict table `crate::ufcs_hover` uses, rather than
/// `find_def_at_offset` (which would target the *receiver*). Either way,
/// once the definition is known, every UFCS call site project-wide that
/// desugars to it is rewritten alongside the plain `ResolutionMap`
/// references — this is the fix for the "renaming a free function silently
/// misses every UFCS call site" bug: without it, `analysis.resolutions`
/// alone never carries a UFCS call site's true target (see `ufcs_hover`'s
/// module doc), so those call sites were never in the edit set at all.
///
/// Review finding on #1539/PR #1543: the two identity-space correlations
/// (`analysis` ⇄ `db`, via [`analysis_identity_of`]/[`db_identity_of`],
/// shared with `crate::navigation::find_references`) must not fail *open*.
/// The two are not revision-locked for every caller (e.g. the LSP's cached
/// `snap.analysis` vs. a freshly re-locked `self.db`), so a stale snapshot
/// can shift a declaration's range and miss the correlation. Previously,
/// a missed correlation silently dropped an entire category of edits
/// (either the plain `ResolutionMap` references, or the UFCS call sites)
/// while still returning `Some(RenameResult)` — exactly the "rename
/// silently produces a broken program" failure mode #1539 exists to kill,
/// just relocated to a different trigger. Both branches below now return
/// `None` — refuse the whole rename — the moment a needed correlation step
/// misses, rather than ever emitting a silently incomplete edit set.
pub fn rename(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    file_id: FileId,
    offset: rowan::TextSize,
    new_name: &str,
) -> Option<RenameResult> {
    let ufcs_target = db
        .hir(file_id)
        .and_then(|hir| crate::ufcs_hover::ufcs_goto_definition_target(db, hir, file_id, offset));

    let (decl_file, decl_range, analysis_def_id, db_def_id, target_kind) = match ufcs_target {
        // A field call or prelude intrinsic has no `DefinitionId` — not
        // renameable.
        Some(None) => return None,
        Some(Some(target)) => {
            let info = db.resolutions_index().index.symbols.get(&target)?.clone();
            let analysis_id = analysis_identity_of(analysis, info.file, info.range)?;
            (info.file, info.range, analysis_id, target, info.kind)
        }
        None => {
            let info = find_def_at_offset(analysis, file_id, offset)?;
            let db_id = db_identity_of(db, info.file, info.range)?;
            (info.file, info.range, info.id, db_id, info.kind)
        }
    };

    let mut edits = Vec::new();

    // 1. Rename the definition site
    edits.push(FileEdit {
        file: decl_file,
        range: decl_range,
        new_text: new_name.to_owned(),
    });

    // 2. Rename all plain reference sites (analysis's own identity space).
    //
    // Issue #1550: a `ResolvedRef` here may be a UFCS call site's
    // *receiver* — `resolve::resolve_function`'s UFCS-shaped fallback
    // records the receiver's resolution against the *whole* `recv.verb`
    // path, not just the receiver's own segment (mirroring the D2 side
    // table's own key). Renaming the receiver must therefore narrow that
    // whole-path range down to the receiver's own first segment via
    // `ufcs_hover::ufcs_receiver_head_range_at_path`, or the edit collapses
    // `g.greet(3)` into `newname(3)`, silently dropping the method
    // segment.
    //
    // Issue #1560 (the non-UFCS-call half of the same bug):
    // `resolve::lookup_variable`'s dotted-field-access fallback (step 11)
    // records the SAME whole-path shape for a plain (non-call) reference
    // like `p.x.y` — narrowed the same way, or the edit collapses `p.x.y`
    // into `newname`, silently dropping `.x.y`.
    //
    // Issue #1571 (the tail half): when the target is a stitch, list item
    // or label, the same whole-path shape names the symbol with its *last*
    // segment instead — `-> hub.market`, `Colors.Red` — so an unnarrowed
    // edit collapses the qualifier away from the other end.
    //
    // All three narrowings are composed by
    // `ufcs_hover::narrowed_reference_range`, shared with `find_references`
    // and `prepare_rename`.
    //
    // Review finding on #1838 (blocking): a `ResolvedRef` targeting a
    // natural-notation element-dispatch handler may be the compiler's own
    // *synthesized* call (issue #1838) — `hir::lower_native::element::
    // try_claim` stamps the call's `Path`/`Name` range to the entire
    // claimed prose line, not any real occurrence of the handler's name.
    // Unfiltered, this loop would emit an edit replacing that whole prose
    // line's bytes with `new_name` — source corruption through a shipped
    // CLI (`brink ide rename` on a claiming handler). Skip it: it resolves
    // correctly (the call does target this handler), but there is no real
    // identifier occurrence here to rewrite.
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
            edits.push(FileEdit {
                file: resolved.file,
                range,
                new_text: new_name.to_owned(),
            });
        }
    }

    // 3. Rename every UFCS-desugared call site targeting the same free
    // function (issue #1539, `db`'s own identity space).
    for (file, path_range) in db.ufcs_call_sites_for_target(db_def_id) {
        let Some(hir) = db.hir(file) else {
            continue;
        };
        let Some(method_range) = crate::ufcs_hover::ufcs_method_range_at_path(hir, path_range)
        else {
            continue;
        };
        edits.push(FileEdit {
            file,
            range: method_range,
            new_text: new_name.to_owned(),
        });
    }

    // 4. Stamp `#@was(old_name)` on the renamed declaration (issue #1672,
    // docs/modules-spec.md §5 — RULED, never implemented until now). This is
    // the one shared chokepoint every rename surface (CLI, LSP, brink-web's
    // `rename_safe`) funnels through, so stamping here — rather than in each
    // caller — is what keeps the surfaces from diverging the way #1539/#1550
    // did. See [`was_directive_edit`] for the kinds this applies to and why.
    //
    // `#@was` is a brink-extension directive (dialect_gate.rs: "M-3 …
    // `#@was` is brink-only") — under `Dialect::StrictInk` (the default,
    // and the dialect the oracle-conformance corpus compiles under)
    // stamping it would introduce a fresh `E051` on every rename, i.e. this
    // "safe" rename would no longer be safe. Only stamp under `Brink`.
    //
    // `old_name` is read straight out of the pre-edit source at `decl_range`
    // rather than from `SymbolInfo::name` — that field is the *canonical/
    // qualified* name (`hub.market`, `Colors.Red`), but `decl_range` is
    // exactly the bare name token's own span (edit 1 above replaces only
    // that span), and `#@was` always records the bare old name (stitch.rs's
    // own doc comment: "takes the bare old stitch name").
    if db.analysis_options().dialect == brink_analyzer::Dialect::Brink {
        let old_name = db.source(decl_file).and_then(|src| {
            src.get(usize::from(decl_range.start())..usize::from(decl_range.end()))
        });
        if let Some(old_name) = old_name
            && old_name != new_name
            && let Some(was_edit) =
                was_directive_edit(db, decl_file, decl_range, target_kind, old_name)
        {
            edits.push(was_edit);
        }
    }

    Some(RenameResult {
        edits,
        external_binding: matches!(target_kind, brink_ir::SymbolKind::External)
            .then_some((decl_file, decl_range)),
    })
}

/// Compute the insertion edit that stamps `#@was(old_name)` on the
/// declaration at `decl_range`, if it is a kind that carries a `was` field
/// in its HIR node and doesn't already have one.
///
/// `#@was`-eligible kinds are `Knot`, `Stitch`, `Variable`, `Constant`, and
/// `List` — exactly the ones with a `was: Option<(String, TextRange)>`
/// field on their HIR node (`hir::types`). `External` is excluded because
/// [`rename`] already refuses to rename it (a host binding's name can't be
/// renamed from the ink side); `Label`/`ListItem`/`Param`/`Temp` have no
/// `was` field at all; `Struct` explicitly never carries one (`StructDecl`'s
/// own doc comment — M-2 only wires visibility for that kind).
///
/// Returns `None` — never overwriting or duplicating — when: the kind isn't
/// `#@was`-eligible, the matching declaration can't be found, the
/// declaration already carries a `#@was` (a second rename of an
/// already-migrated declaration keeps its original record rather than
/// silently losing it), or no insertion point can be computed (e.g. a
/// knot/stitch header with no trailing newline in the file — degrades to no
/// stamp rather than risking a corrupt insertion).
fn was_directive_edit(
    db: &ProjectDb,
    decl_file: FileId,
    decl_range: TextRange,
    target_kind: brink_ir::SymbolKind,
    old_name: &str,
) -> Option<FileEdit> {
    let hir = db.hir(decl_file)?;
    let src = db.source(decl_file)?;

    match target_kind {
        brink_ir::SymbolKind::Knot => {
            let knot = hir.knots.iter().find(|k| k.name.range == decl_range)?;
            (knot.was.is_none())
                .then(|| insert_after_header_line(src, decl_file, knot.ptr.range, old_name))
                .flatten()
        }
        brink_ir::SymbolKind::Stitch => {
            // Either a top-level `= stitch` promoted to knot status
            // (`symbol_kind()` reports `Stitch` but it's stored as a
            // `Knot`, F-I#5) or a real nested `Stitch` under some knot's
            // `stitches`.
            if let Some(knot) = hir
                .knots
                .iter()
                .find(|k| k.name.range == decl_range && k.symbol_kind() == target_kind)
            {
                return (knot.was.is_none())
                    .then(|| insert_after_header_line(src, decl_file, knot.ptr.range, old_name))
                    .flatten();
            }
            let stitch = hir
                .knots
                .iter()
                .flat_map(|k| &k.stitches)
                .find(|s| s.name.range == decl_range)?;
            (stitch.was.is_none())
                .then(|| insert_after_header_line(src, decl_file, stitch.ptr.range, old_name))
                .flatten()
        }
        brink_ir::SymbolKind::Variable => {
            let v = hir.variables.iter().find(|v| v.name.range == decl_range)?;
            (v.was.is_none())
                .then(|| insert_before_decl_line(src, decl_file, v.ptr.range, old_name))
                .flatten()
        }
        brink_ir::SymbolKind::Constant => {
            let c = hir.constants.iter().find(|c| c.name.range == decl_range)?;
            (c.was.is_none())
                .then(|| insert_before_decl_line(src, decl_file, c.ptr.range, old_name))
                .flatten()
        }
        brink_ir::SymbolKind::List => {
            let l = hir.lists.iter().find(|l| l.name.range == decl_range)?;
            (l.was.is_none())
                .then(|| insert_before_decl_line(src, decl_file, l.ptr.range, old_name))
                .flatten()
        }
        brink_ir::SymbolKind::External
        | brink_ir::SymbolKind::ListItem
        | brink_ir::SymbolKind::Label
        | brink_ir::SymbolKind::Param
        | brink_ir::SymbolKind::Temp
        | brink_ir::SymbolKind::Struct => None,
    }
}

/// Insert `#@was(old_name)` as the first line of a knot/stitch body — the
/// "leading tag-line run" placement `hir::lower::directive::in_leading_body_run`
/// requires. `node_range` is the whole `KNOT_DEF`/`STITCH_DEF` provenance
/// range; its header line (`== name ==` / `= name`) is single-line by
/// grammar, so the first newline at or after the range's start ends it.
fn insert_after_header_line(
    src: &str,
    file: FileId,
    node_range: TextRange,
    old_name: &str,
) -> Option<FileEdit> {
    let start = usize::from(node_range.start());
    let rel_nl = src.get(start..)?.find('\n')?;
    let at = TextSize::try_from(start + rel_nl + 1).ok()?;
    Some(FileEdit {
        file,
        range: TextRange::empty(at),
        new_text: format!("#@was({old_name})\n"),
    })
}

/// Insert `#@was(old_name)` on its own line immediately above a
/// `VAR`/`CONST`/`LIST` declaration — the "directive line immediately above
/// a declaration" placement `hir::lower::directive::attached_declaration`
/// requires. `decl_range` is the whole declaration node's provenance range;
/// the insertion point is the start of the line it begins on, walked back
/// over any contiguous run of `///` doc-comment lines and pre-existing
/// `#@…` directive lines immediately above the declaration.
///
/// Review finding on #1672 (blocking): inserting at the declaration's own
/// line landed `#@was` *between* a `///` doc block and the declaration it
/// documents (`hir::lower::doc_comment::collect_doc_lines` only walks a
/// *contiguous* run of doc-comment lines back from the declaration, so a
/// directive spliced in between breaks that contiguity) — the doc block
/// silently vanished from `SymbolManifest::docs` with no diagnostic. Walking
/// the insertion point back over the whole leading run keeps both: the
/// directive lands above the doc block, which stays attached to the
/// declaration.
fn insert_before_decl_line(
    src: &str,
    file: FileId,
    decl_range: TextRange,
    old_name: &str,
) -> Option<FileEdit> {
    let mut line_start = src
        .get(..usize::from(decl_range.start()))?
        .rfind('\n')
        .map_or(0, |i| i + 1);

    while line_start > 0 {
        let end_of_prev = line_start - 1; // the '\n' terminating the line above
        let start_of_prev = src.get(..end_of_prev)?.rfind('\n').map_or(0, |i| i + 1);
        let prev_line = src.get(start_of_prev..end_of_prev)?.trim_start();
        if prev_line.starts_with("///") || prev_line.starts_with("#@") {
            line_start = start_of_prev;
        } else {
            break;
        }
    }

    let at = TextSize::try_from(line_start).ok()?;
    Some(FileEdit {
        file,
        range: TextRange::empty(at),
        new_text: format!("#@was({old_name})\n"),
    })
}

// ─── Safe rename (studio path) ──────────────────────────────────────────

/// Resolve the declaration offset (name-range start) of a knot, or a stitch
/// within a knot, by name. Returns `None` if the container doesn't exist.
#[must_use]
pub fn declaration_offset(hir: &HirFile, knot: &str, stitch: Option<&str>) -> Option<TextSize> {
    let k = hir.knots.iter().find(|k| k.name.text == knot)?;
    match stitch {
        None => Some(k.name.range.start()),
        Some(s) => k
            .stitches
            .iter()
            .find(|st| st.name.text == s)
            .map(|st| st.name.range.start()),
    }
}

/// Apply `edits` to `src`, splicing from the end so earlier offsets stay valid.
fn apply_edits(src: &str, mut edits: Vec<&FileEdit>) -> String {
    let mut s = src.to_owned();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
    for e in edits {
        let (start, end) = (usize::from(e.range.start()), usize::from(e.range.end()));
        if start <= end && end <= s.len() && s.is_char_boundary(start) && s.is_char_boundary(end) {
            s.replace_range(start..end, &e.new_text);
        }
    }
    s
}

/// Compute a rename and the diagnostics it would introduce, by overlaying the
/// edits and re-analyzing the whole project (via the op-agnostic [`gate`]). The
/// primary file (`file_id`)'s edits are folded into `new_source`; edits in other
/// files travel out as `cross_file_edits`. The session is not mutated.
#[must_use]
pub fn rename_safe(
    session: &IdeSession,
    file_id: FileId,
    offset: TextSize,
    new_name: &str,
) -> Option<StructuralResult> {
    let analysis = session.analysis()?;
    let result = rename(session.db(), analysis, file_id, offset, new_name)?;

    // The gate overlays every edit (primary + cross-file) and re-analyzes.
    let mut introduced = gate(session, &result.edits);

    // External rename (ruled 2026-08-24): ALWAYS unsafe — the name is the
    // story↔engine contract, so the report carries the E190 host-binding
    // entry and the rename applies only through the Force path.
    if let Some((decl_file, decl_range)) = result.external_binding {
        let (path, old_name, line, col) =
            match (session.file_path(decl_file), session.source(decl_file)) {
                (Some(p), Some(src)) => {
                    let idx = crate::LineIndex::new(src);
                    let (l, c) = idx.line_col(decl_range.start());
                    let name = src
                        .get(usize::from(decl_range.start())..usize::from(decl_range.end()))
                        .unwrap_or("");
                    (p.to_owned(), name.to_owned(), l + 1, c + 1)
                }
                _ => (String::new(), String::new(), 1, 1),
            };
        introduced.push(crate::structural_result::IntroducedDiagnostic {
            severity: brink_ir::Severity::Warning,
            code: brink_ir::DiagnosticCode::E190,
            message: format!(
                "renames the host binding `{old_name}` — the engine must re-register the external under the new name"
            ),
            path,
            line,
            col,
        });
    }

    // Fold the primary file's edits into its new source; the rest are cross-file.
    let primary: Vec<&FileEdit> = result.edits.iter().filter(|e| e.file == file_id).collect();
    let new_source = session.source(file_id).map(|src| apply_edits(src, primary));
    let cross_file_edits: Vec<FileEdit> = result
        .edits
        .into_iter()
        .filter(|e| e.file != file_id)
        .collect();

    Some(StructuralResult {
        new_source,
        cross_file_edits,
        safe: introduced.is_empty(),
        introduced,
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn external_rename_is_always_unsafe_with_the_e190_binding_entry() {
        // Ruled 2026-08-24: externals rename behind the Force gate — the
        // verdict is ALWAYS unsafe, carrying E190 naming the host binding,
        // and the edits cover the declaration and every call site.
        let mut s = crate::session::IdeSession::new();
        let src = "EXTERNAL play_se(name)\n=== k ===\n~ play_se(1)\n~ play_se(2)\n";
        let id = s.update_and_analyze("t.ink", src.to_string());
        let call = src.rfind("play_se(").expect("call") + 1;
        let res = rename_safe(
            &s,
            id,
            TextSize::from(u32::try_from(call).expect("fits")),
            "sfx",
        )
        .expect("external rename must produce a result");
        assert!(!res.safe, "external rename is never safe");
        assert!(
            res.introduced
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E190 && d.message.contains("play_se")),
            "E190 host-binding entry expected: {:?}",
            res.introduced
        );
        let new_source = res.new_source.expect("primary source");
        assert!(new_source.contains("EXTERNAL sfx(name)"), "{new_source}");
        assert!(!new_source.contains("play_se("), "{new_source}");
    }

    #[test]
    fn prepare_rename_accepts_function_call_sites_and_externals() {
        // #3061 review question: call sites ARE renameable (reference-site
        // path); externals too as of the 2026-08-24 Force-gate ruling.
        let mut session = crate::session::IdeSession::new();
        let src = "EXTERNAL play_se(name)\n=== function roll(x) ===\n~ return x\n=== k ===\n~ temp v = roll(3)\n~ play_se(1)\n";
        let file_id = session.update_and_analyze("t.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let call = src.find("roll(3)").expect("call site") + 1;
        let got = prepare_rename(
            session.db(),
            analysis,
            file_id,
            rowan::TextSize::from(u32::try_from(call).expect("fits")),
        );
        assert!(got.is_some(), "function call site must be renameable");

        let ext_call = src.rfind("play_se(").expect("ext call") + 1;
        let got = prepare_rename(
            session.db(),
            analysis,
            file_id,
            rowan::TextSize::from(u32::try_from(ext_call).expect("fits")),
        );
        assert!(
            got.is_some(),
            "external call site IS renameable (ruled 2026-08-24 — Force gate)"
        );
    }
    use rowan::TextSize;

    use super::{declaration_offset, prepare_rename, rename, rename_safe};
    use crate::session::IdeSession;

    fn session(src: &str) -> (IdeSession, brink_ir::FileId) {
        let mut s = IdeSession::new();
        let id = s.update_and_analyze("t.ink", src.to_string());
        (s, id)
    }

    #[test]
    fn declaration_offset_resolves_knot_and_stitch() {
        let (s, id) = session("=== outer ===\n= inner\nHi.\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let knot = declaration_offset(hir, "outer", None).expect("knot offset");
        let stitch = declaration_offset(hir, "outer", Some("inner")).expect("stitch offset");
        assert!(stitch > knot, "stitch decl comes after the knot decl");
        assert!(declaration_offset(hir, "missing", None).is_none());
        assert!(declaration_offset(hir, "outer", Some("missing")).is_none());
    }

    #[test]
    fn safe_rename_updates_refs_with_no_new_diagnostics() {
        let (s, id) = session("-> hello\n=== hello ===\nHi.\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "hello", None).expect("offset");
        let res = rename_safe(&s, id, offset, "greeting").expect("rename");

        // Both the divert reference and the declaration (same file) are folded
        // into new_source — no cross-file edits, and the old name is gone.
        // `session()` doesn't opt into `Dialect::Brink`, so `#@was` (a brink
        // extension, issue #1672) is correctly withheld here — see
        // `was_directive_edit_tests` below for the dialect==Brink case where
        // it stamps.
        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("-> greeting") && new_source.contains("=== greeting ==="),
            "decl + ref both rewritten: {new_source}"
        );
        assert!(
            !new_source.contains("hello"),
            "old name fully removed: {new_source}"
        );
        assert!(
            res.cross_file_edits.is_empty(),
            "single-file rename has no cross-file edits"
        );
        assert!(
            res.introduced.is_empty(),
            "a consistent rename introduces nothing, got {:?}",
            res.introduced
                .iter()
                .map(|d| (d.code.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rename_into_collision_reports_breakage() {
        // Two knots; renaming `a` to `b` collides with the existing `b`.
        let (s, id) = session("-> a\n=== a ===\n-> END\n=== b ===\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "a", None).expect("offset");
        let res = rename_safe(&s, id, offset, "b").expect("rename");

        assert!(
            res.introduced
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E022),
            "expected E022 duplicate-knot, got {:?}",
            res.introduced
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>()
        );
        // Not safe, but the edits are still produced — applying is the caller's
        // choice (force). The rewritten primary source is present.
        assert!(!res.safe);
        assert!(res.new_source.is_some());
    }

    // ── PR #2271 review finding: type-annotation reference ranges ────────

    #[test]
    fn renaming_a_struct_via_a_type_annotation_reference_does_not_corrupt_trailing_source() {
        // `symbols::project::walk_type_annotation` registers a
        // `RefKind::Type` reference at `TypeExpr::Named`'s range (the
        // `TYPE_EXPR`/`TYPE_NAME` node the ink-dialect parser builds).
        // `narrowed_reference_range` returns `None` for a `SymbolKind::Struct`
        // target (none of its three narrowings apply), so `rename` falls
        // back to that raw range. Before the trivia fix in
        // `brink-syntax::parser::types::type_name_or_generic`, `TYPE_NAME`
        // absorbed the whitespace trailing the type name, so renaming
        // `Point` -> `Cue` rewrote `Point ` (with the trailing space),
        // producing `VAR p: Cue= 0` — silent source corruption with no
        // syntax error to catch it. This proves the emitted edit's range is
        // exactly the 5-byte `Point` identifier, never the identifier plus
        // trailing trivia.
        const SRC: &str = "STRUCT Point = #{x: float}\nVAR p: Point = 0\n=== main ===\n-> DONE\n";
        let (s, id) = session(SRC);
        let decl_pos = u32::try_from(SRC.find("Point").expect("decl")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result = rename(s.db(), analysis, id, TextSize::from(decl_pos), "Cue").expect("rename");

        let ann_pos =
            u32::try_from(SRC.rfind("Point").expect("annotation occurrence")).expect("offset");
        let edit = result
            .edits
            .iter()
            .find(|e| e.range.start() == TextSize::from(ann_pos));
        assert!(
            edit.is_some(),
            "expected an edit at the VAR annotation's `Point` occurrence, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        let edit = edit.expect("checked above");
        assert_eq!(
            edit.range.end(),
            TextSize::from(ann_pos + 5),
            "edit range must end exactly at the identifier's end, not swallow the trailing \
             space before `=`: {:?}",
            edit.range
        );
        assert_eq!(edit.new_text, "Cue");
    }

    // ── Issue #1539: rename follows UFCS call sites ──────────────────────

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

    fn native_session(src: &str) -> (IdeSession, brink_ir::FileId) {
        let mut s = IdeSession::new();
        let id = s.update_and_analyze("test.brink", src.to_string());
        (s, id)
    }

    #[test]
    fn renaming_a_free_function_from_its_declaration_rewrites_its_ufcs_call_site() {
        // The core #1539 bug: `fn greet` is called only via UFCS
        // (`g.greet(3)`) — before this fix, `rename` keyed solely off
        // `analysis.resolutions`, which never carries a UFCS call site's
        // true target, so the call site was silently left unrenamed,
        // producing a broken program (`greet` still called under its old
        // name after the declaration moved).
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(decl_pos), "salute").expect("rename");

        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");
        assert!(
            result.edits.iter().any(|e| e.file == id
                && e.range.start() == TextSize::from(call_pos)
                && e.new_text == "salute"),
            "expected the UFCS call site's method segment rewritten to `salute`, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        // The declaration itself is also rewritten.
        assert!(
            result
                .edits
                .iter()
                .any(|e| e.range.start() == TextSize::from(decl_pos) && e.new_text == "salute"),
            "expected the declaration site rewritten too"
        );
    }

    #[test]
    fn renaming_a_free_function_from_its_ufcs_call_site_rewrites_the_declaration() {
        // The reverse direction: initiating the rename *from* the UFCS call
        // site's method segment must resolve to the free function (not the
        // receiver `g`) and still rewrite the declaration.
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(call_pos), "salute").expect("rename");

        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");
        assert!(
            result
                .edits
                .iter()
                .any(|e| e.range.start() == TextSize::from(decl_pos) && e.new_text == "salute"),
            "expected the `fn greet` declaration rewritten, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rename_via_rename_safe_folds_the_ufcs_call_site_into_new_source() {
        // The studio-facing `rename_safe` path (issue #1539): the UFCS call
        // site is in the same file as the declaration, so it must be folded
        // into `new_source` alongside the declaration, not silently dropped.
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "salute").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("fn salute(") && new_source.contains("g.salute(3)"),
            "decl + UFCS call site both rewritten: {new_source}"
        );
        // `native_session()` doesn't opt into `Dialect::Brink` either, so
        // `#@was` (issue #1672) is correctly withheld — same reasoning as
        // `safe_rename_updates_refs_with_no_new_diagnostics` above.
        assert!(
            !new_source.contains("greet"),
            "old name fully removed: {new_source}"
        );
    }

    // ── Issue #1550: renaming the RECEIVER of a UFCS call leaves the
    // method segment intact (the mirror of #1539, which fixed renaming the
    // *method*) ────────────────────────────────────────────────────────

    #[test]
    fn renaming_the_receiver_of_a_ufcs_call_site_edits_only_the_receiver_segment() {
        // The core #1550 bug: `brink-analyzer::resolve`'s UFCS-shaped-callee
        // fallback recorded the receiver's resolved reference range as the
        // *whole* `recv.verb` path. Renaming the receiver `g` (declared by
        // `let g = Guest { .. }`) then produced an edit spanning all of
        // `g.greet`, so applying it collapsed `g.greet(3)` down to
        // `newname(3)` — silently dropping the method segment and
        // corrupting the program from a "safe" rename.
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("g = Guest").expect("decl")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(decl_pos), "newname").expect("rename");

        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("g.greet(3)").expect("call")).expect("offset");
        let found = result
            .edits
            .iter()
            .find(|e| e.file == id && e.range.start() == TextSize::from(call_pos));
        assert!(
            found.is_some(),
            "expected an edit at the UFCS call site's receiver segment, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        let edit = found.expect("checked above");
        assert_eq!(edit.new_text, "newname");
        assert_eq!(
            usize::from(edit.range.end()) - usize::from(edit.range.start()),
            1,
            "the edit must span only the receiver's own `g` segment (1 byte), not the whole \
             `g.greet` path — got {:?}",
            edit.range
        );
    }

    #[test]
    fn rename_safe_on_the_receiver_of_a_ufcs_call_site_produces_a_valid_program() {
        // End-to-end counterpart of the test above, via the studio-facing
        // `rename_safe` path: the folded `new_source` must keep the method
        // segment (`newname.greet(3)`), never collapse to a bare call
        // (`newname(3)`), and the rename must introduce no new diagnostics.
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("g = Guest").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "newname").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("newname.greet(3)"),
            "the method segment must survive the receiver rename: {new_source}"
        );
        assert!(
            !new_source.contains("newname(3)"),
            "the receiver rename must not collapse into a bare call, dropping the method \
             segment: {new_source}"
        );
        assert!(
            res.introduced.is_empty(),
            "a correct receiver rename introduces no new diagnostics, got {:?}",
            res.introduced
                .iter()
                .map(|d| (d.code.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn prepare_rename_on_a_ufcs_call_site_returns_the_method_segment_span() {
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let call_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(3)").expect("call")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let range =
            prepare_rename(s.db(), analysis, id, TextSize::from(call_pos)).expect("renameable");

        assert_eq!(
            &UFCS_FREE_FN_SRC[usize::from(range.start())..usize::from(range.end())],
            "greet",
            "the UFCS call's own method span, not the receiver's or the target's declaration"
        );
    }

    #[test]
    fn prepare_rename_on_a_ufcs_field_call_is_not_renameable() {
        // Fixture mirrors `navigation.rs`'s
        // `goto_definition_on_a_ufcs_field_call_finds_no_target`: a struct
        // field has no `DefinitionId`, so it cannot be renamed through this
        // path.
        let src = "\
struct Guest {
  greet: fn(int): int
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
";
        let (s, id) = native_session(src);
        let call_pos = u32::try_from(src.find("greet(3)").expect("call")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        assert!(
            prepare_rename(s.db(), analysis, id, TextSize::from(call_pos)).is_none(),
            "a field call has no DefinitionId to rename"
        );
    }

    #[test]
    fn prepare_rename_and_rename_agree_on_a_ufcs_call_to_an_external_free_fn() {
        // Review finding on #1539/PR #1543: `prepare_rename`'s UFCS branch
        // skipped the `SymbolKind::External` guard `rename` itself applies
        // once it resolves the same target (below) — an LSP `prepareRename`
        // reported the call site renameable, then the follow-up `rename`
        // call returned `None` (an external target has no analysis-space
        // correlate to rename through), i.e. a silent no-op.
        let src = "\
struct Guest {
  name: string
}

extern greet(g, n)

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.greet(3);
}
";
        let (s, id) = native_session(src);
        let call_pos = u32::try_from(src.find("greet(3)").expect("call")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        // Ruled 2026-08-24 (Force gate): both now ACCEPT — the invariant this
        // pin protects is that prepare_rename and rename AGREE (the original
        // #1539 finding was their disagreement producing a silent no-op).
        assert!(
            prepare_rename(s.db(), analysis, id, TextSize::from(call_pos)).is_some(),
            "external UFCS call site is renameable under the Force-gate ruling"
        );
        let renamed = rename(s.db(), analysis, id, TextSize::from(call_pos), "salute");
        assert!(
            renamed.is_some(),
            "rename must agree with prepare_rename: both accept the external target"
        );
        assert!(
            renamed.expect("just asserted").external_binding.is_some(),
            "the external binding must be flagged for the E190 verdict"
        );
    }

    #[test]
    fn rename_refuses_rather_than_silently_dropping_edits_when_identity_spaces_disagree() {
        // Review finding on #1539/PR #1543: `analysis`/`db` are not
        // revision-locked for every caller (e.g. the LSP's cached
        // `snap.analysis` vs. a freshly re-locked `self.db`) — a stale
        // `analysis` snapshot can carry a declaration range that no longer
        // matches `db`'s current one. Before this fix, `rename` silently
        // returned `Some(RenameResult)` containing only the edits reachable
        // from whichever identity space *did* correlate, omitting the rest
        // with no signal — precisely the "rename silently produces a
        // broken program" failure mode #1539 exists to kill, just
        // relocated to a different trigger. It must now refuse instead.
        let (mut s, id) = native_session(UFCS_FREE_FN_SRC);
        // Captured *before* the edit below: self-consistent with the
        // original source's ranges, but about to go stale relative to the
        // session's `db` (and its own `hir`) once the source shifts.
        let stale_analysis = s.analysis().expect("analysis").clone();
        let decl_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("greet(g").expect("decl")).expect("offset");

        // Shift every declaration's range forward by re-analyzing a source
        // with an extra leading line — `db`/the fresh analysis now disagree
        // with `stale_analysis` about where `fn greet` lives.
        let shifted_src = format!("// shifted\n{UFCS_FREE_FN_SRC}");
        s.update_and_analyze("test.brink", shifted_src);

        assert!(
            rename(
                s.db(),
                &stale_analysis,
                id,
                TextSize::from(decl_pos),
                "salute"
            )
            .is_none(),
            "a stale analysis/db identity-space mismatch must refuse the rename, not emit a \
             partial edit set"
        );
    }

    // ── Issue #1560: renaming the HEAD of a plain (non-UFCS-call) dotted
    // field access leaves the trailing field segments intact — the
    // non-call mirror of #1550, which fixed the analogous UFCS-receiver
    // case (`recv.verb(args)`). `resolve::lookup_variable`'s
    // dotted-field-access fallback (step 11, resolve.rs:474-503) records
    // the SAME whole-path `ResolvedRef` shape for a plain reference like
    // `p.x.y` (no call involved) ─────────────────────────────────────────

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
    fn renaming_the_head_of_a_plain_field_access_edits_only_the_head_segment() {
        // The core #1560 bug: `resolve::lookup_variable`'s
        // dotted-field-access fallback recorded the head variable `p`'s
        // resolved reference range as the *whole* `p.x.y` path. Renaming
        // `p` (declared by `let p = Guest { .. }`) then produced an edit
        // spanning all of `p.x.y`, so applying it collapsed `p.x.y` down to
        // `newname` — silently dropping both field segments.
        let (s, id) = native_session(FIELD_ACCESS_SRC);
        let decl_pos =
            u32::try_from(FIELD_ACCESS_SRC.find("p = Guest").expect("decl")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(decl_pos), "newname").expect("rename");

        let ref_pos = u32::try_from(FIELD_ACCESS_SRC.find("p.x.y").expect("ref")).expect("offset");
        let found = result
            .edits
            .iter()
            .find(|e| e.file == id && e.range.start() == TextSize::from(ref_pos));
        assert!(
            found.is_some(),
            "expected an edit at the field-access reference's head segment, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        let edit = found.expect("checked above");
        assert_eq!(edit.new_text, "newname");
        assert_eq!(
            usize::from(edit.range.end()) - usize::from(edit.range.start()),
            1,
            "the edit must span only the head's own `p` segment (1 byte), not the whole \
             `p.x.y` path — got {:?}",
            edit.range
        );
    }

    #[test]
    fn rename_safe_on_the_head_of_a_plain_field_access_produces_a_valid_program() {
        // End-to-end counterpart of the test above, via the studio-facing
        // `rename_safe` path: the folded `new_source` must keep both field
        // segments (`newname.x.y`), never collapse to a bare reference
        // (`newname`), and the rename must introduce no new diagnostics.
        let (s, id) = native_session(FIELD_ACCESS_SRC);
        let decl_pos =
            u32::try_from(FIELD_ACCESS_SRC.find("p = Guest").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "newname").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("let n = newname.x.y;"),
            "both field segments must survive the head rename: {new_source}"
        );
        assert!(
            !new_source.contains("let n = newname;"),
            "the head rename must not collapse into a bare reference, dropping the field \
             segments: {new_source}"
        );
        assert!(
            res.introduced.is_empty(),
            "a correct head rename introduces no new diagnostics, got {:?}",
            res.introduced
                .iter()
                .map(|d| (d.code.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn renaming_the_head_of_a_plain_field_access_in_ink_dialect_edits_only_the_head_segment() {
        // Review finding on #1560: the two tests above only exercise the
        // native `.brink` struct/`fn`/`let` surface (`native_session`), but
        // `brink-analyzer`'s own dotted-field-access-fallback tests
        // (`resolution_fallback_resolves_to_head_variable_when_no_static_path_matches`,
        // `resolution_fallback_resolves_to_head_param` in
        // `brink-analyzer/src/resolve.rs`) prove step 11 fires for plain
        // `.ink` source too (`VAR p = 0` / `~ y = p.x`) — the dialect where
        // the fallback (and thus the corrupting fixture) is already known
        // to occur. Same assertion as the native test above, on the `.ink`
        // dialect via the `session()` helper.
        let src = "VAR p = 0\n=== main ===\n~ y = p.x\n-> DONE\n";
        let (s, id) = session(src);
        let decl_pos = u32::try_from(src.find("p = 0").expect("decl")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(decl_pos), "newname").expect("rename");

        let ref_pos = u32::try_from(src.find("p.x").expect("ref")).expect("offset");
        let found = result
            .edits
            .iter()
            .find(|e| e.file == id && e.range.start() == TextSize::from(ref_pos));
        assert!(
            found.is_some(),
            "expected an edit at the field-access reference's head segment, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        let edit = found.expect("checked above");
        assert_eq!(edit.new_text, "newname");
        assert_eq!(
            usize::from(edit.range.end()) - usize::from(edit.range.start()),
            1,
            "the edit must span only the head's own `p` segment (1 byte), not the whole `p.x` \
             path — got {:?}",
            edit.range
        );
    }

    #[test]
    fn renaming_a_stitch_through_its_qualified_reference_is_not_narrowed_to_the_head() {
        // Review finding on #1560 (blocking): `field_access_head_range_at_path`'s
        // `matches!(target_kind, Variable | Constant | Param | Temp)` guard
        // is the only thing preventing this change from corrupting stitch
        // renames — nothing else exercised it. `~ y = hub.market` lowers to
        // a 2-segment `Expr::Path` with the whole-path range and resolves
        // (via `lookup_variable` step 8) to the stitch `market`, at the
        // SAME whole-path `ResolvedRef` shape the dotted-field-access
        // fallback (step 11) produces — an ungated narrowing would rewrite
        // `hub.market` down to just `hub`, corrupting the qualified
        // reference. Fixture mirrors
        // `resolve::resolution_fallback_static_dotted_path_wins_over_a_colliding_variable_name`.
        //
        // The head guard is asserted here; the *correct* rewrite of the same
        // reference (`hub.newname`) is issue #1571's tail narrowing, asserted
        // by `renaming_a_stitch_rewrites_only_the_tail_segment_of_a_qualified_reference`
        // below.
        let src = "=== hub ===\n= market\nHi.\n-> DONE\n=== main ===\n~ y = hub.market\n-> DONE\n";
        let (s, id) = session(src);
        let hir = s.hir(id).expect("hir");
        let decl_pos = declaration_offset(hir, "hub", Some("market")).expect("stitch decl");
        let analysis = s.analysis().expect("analysis");

        let result = rename(s.db(), analysis, id, decl_pos, "newname").expect("rename");

        let ref_pos = u32::try_from(src.find("hub.market").expect("ref")).expect("offset");
        if let Some(edit) = result
            .edits
            .iter()
            .find(|e| e.file == id && e.range.start() == TextSize::from(ref_pos))
        {
            assert_ne!(
                usize::from(edit.range.end()) - usize::from(edit.range.start()),
                3,
                "the qualified `hub.market` reference must not be narrowed to the 3-byte `hub` \
                 head segment — got {:?}",
                edit.range
            );
        }
    }

    // ── Issue #1571 variant 1: the TAIL half of the whole-path
    // `ResolvedRef` bug. When the resolved target is a stitch, a list item
    // or a label, the segment that names it is the path's *last* one —
    // rewriting the whole range collapses `-> hub.market` to `-> newname`
    // and `Colors.Red` to `Crimson`, dropping the qualifier ───────────────

    #[test]
    fn renaming_a_stitch_rewrites_only_the_tail_segment_of_a_qualified_reference() {
        // `~ y = hub.market` lowers to a 2-segment `Expr::Path` whose whole
        // range is the `ResolvedRef`'s range, targeting the stitch
        // `market`. Unnarrowed, the rename edit spans all of `hub.market`.
        let src = "=== hub ===\n= market\nHi.\n-> DONE\n=== main ===\n~ y = hub.market\n-> DONE\n";
        let (s, id) = session(src);
        let hir = s.hir(id).expect("hir");
        let decl_pos = declaration_offset(hir, "hub", Some("market")).expect("stitch decl");
        let analysis = s.analysis().expect("analysis");

        let result = rename(s.db(), analysis, id, decl_pos, "newname").expect("rename");

        let tail_pos =
            u32::try_from(src.find("hub.market").expect("ref") + "hub.".len()).expect("offset");
        let found = result
            .edits
            .iter()
            .find(|e| e.file == id && e.range.start() == TextSize::from(tail_pos));
        assert!(
            found.is_some(),
            "expected an edit at the qualified reference's tail segment, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        let edit = found.expect("checked above");
        assert_eq!(
            usize::from(edit.range.end()) - usize::from(edit.range.start()),
            "market".len(),
            "the edit must span only the `market` segment, not the whole `hub.market` path — \
             got {:?}",
            edit.range
        );
    }

    #[test]
    fn rename_safe_on_a_stitch_keeps_the_qualifier_of_a_divert_reference() {
        // The divert form of the same bug, end-to-end: `Projector::
        // walk_divert_target` records the whole `hub.market` path range for
        // a `-> hub.market` divert exactly as the expression form does, and
        // a divert lives on `Stmt::Divert` rather than `Expr::Path` — a
        // shape `find_field_access_ref` never looks at. Before #1571 this
        // rewrote the divert to `-> newname`, which resolves to nothing.
        let src = "=== hub ===\n= market\nHi.\n-> DONE\n=== main ===\n-> hub.market\n";
        let (s, id) = session(src);
        let hir = s.hir(id).expect("hir");
        let decl_pos = declaration_offset(hir, "hub", Some("market")).expect("stitch decl");

        let res = rename_safe(&s, id, decl_pos, "newname").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("-> hub.newname"),
            "the divert must keep its `hub.` qualifier: {new_source}"
        );
        assert!(
            !new_source.contains("-> newname"),
            "the divert must not collapse to the bare stitch name: {new_source}"
        );
        // Deliberately no `res.introduced.is_empty()` assertion here, unlike
        // the `.ink` knot rename above: a *stitch* declaration is also
        // recorded as a resolved reference at its own declaration range, so
        // `rename` emits two identical-range edits for it and `apply_edits`
        // splices both — mangling the declaration line. That duplicate-edit
        // bug is independent of this issue (it reproduces with no dotted
        // path in the fixture at all, and predates #1571); it is reported
        // separately rather than fixed here.
    }

    #[test]
    fn renaming_a_list_item_keeps_the_list_qualifier_at_every_reference() {
        // `Colors.Red` resolves (via `lookup_variable` step 4) to the list
        // item `Red`, whose declaration is the bare `Red` inside the `LIST`
        // line — so an unnarrowed rewrite of the whole-path reference range
        // collapsed `Colors.Red` to `Crimson`.
        //
        // The `VAR c = Colors.Red` reference is deliberately part of this
        // fixture: it is a declaration *initializer*, which
        // `project_manifest` walks but `hir::visit::visit` did not — so it
        // also pins issue #1571's `visit_with_decl_initializers` change
        // (variant 2) on the tail side.
        let src = "LIST Colors = Red, Green\nVAR c = Colors.Red\n=== main ===\n~ c = Colors.Red\n\
                   -> DONE\n";
        let (s, id) = session(src);
        let analysis = s.analysis().expect("analysis");
        let decl_pos = analysis
            .index
            .symbols
            .values()
            .find(|i| i.kind == brink_ir::SymbolKind::ListItem && i.name == "Colors.Red")
            .map(|i| i.range.start())
            .expect("the `Red` list-item declaration");

        let result = rename(s.db(), analysis, id, decl_pos, "Crimson").expect("rename");

        for occurrence in ["VAR c = Colors.Red", "~ c = Colors.Red"] {
            let tail_pos = u32::try_from(
                src.find(occurrence).expect("occurrence") + occurrence.len() - "Red".len(),
            )
            .expect("offset");
            let found = result
                .edits
                .iter()
                .find(|e| e.file == id && e.range.start() == TextSize::from(tail_pos));
            assert!(
                found.is_some(),
                "expected a tail-only edit for `{occurrence}`, got {:?}",
                result
                    .edits
                    .iter()
                    .map(|e| (e.range, e.new_text.as_str()))
                    .collect::<Vec<_>>()
            );
            let edit = found.expect("checked above");
            assert_eq!(
                usize::from(edit.range.end()) - usize::from(edit.range.start()),
                "Red".len(),
                "the edit for `{occurrence}` must span only `Red`, not the whole `Colors.Red` \
                 path — got {:?}",
                edit.range
            );
        }
    }

    #[test]
    fn renaming_a_knot_level_label_rewrites_only_the_tail_segment_of_a_qualified_reference() {
        // Review finding on this PR: `qualified_tail_range_at_path` gates on
        // `Stitch | ListItem | Label`, but only the Stitch and ListItem
        // variants had a regression test. `Label` is a live corruption path
        // of its own: `resolve::lookup_divert`'s dotted branch looks up
        // `&[SymbolKind::Stitch, SymbolKind::Label]`, and
        // `Projector::qualify_label` stores a knot-level label as
        // `hub.mylabel` — so `-> hub.mylabel` records the same whole-path
        // `ResolvedRef` shape a stitch reference does, and collapsed to
        // `-> newname` before this PR.
        let src = "=== hub ===\n- (mylabel) Hi.\n-> DONE\n=== main ===\n-> hub.mylabel\n";
        let (s, id) = session(src);
        let analysis = s.analysis().expect("analysis");
        let decl_pos = analysis
            .index
            .symbols
            .values()
            .find(|i| i.kind == brink_ir::SymbolKind::Label && i.name == "hub.mylabel")
            .map(|i| i.range.start())
            .expect("the `mylabel` label declaration");

        let result = rename(s.db(), analysis, id, decl_pos, "newname").expect("rename");

        let tail_pos =
            u32::try_from(src.find("hub.mylabel").expect("ref") + "hub.".len()).expect("offset");
        let found = result
            .edits
            .iter()
            .find(|e| e.file == id && e.range.start() == TextSize::from(tail_pos));
        assert!(
            found.is_some(),
            "expected an edit at the qualified reference's tail segment, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        let edit = found.expect("checked above");
        assert_eq!(
            usize::from(edit.range.end()) - usize::from(edit.range.start()),
            "mylabel".len(),
            "the edit must span only the `mylabel` segment, not the whole `hub.mylabel` path — \
             got {:?}",
            edit.range
        );
    }

    // ── Issue #1571 variant 2: `VAR`/`CONST` initializer expressions
    // bypassed the narrowing walker entirely. `symbols::project_manifest`
    // walks them and records whole-path `UnresolvedRef`s, but
    // `hir::visit::visit` covered only `root_content` + knot/stitch bodies,
    // so no narrowing helper could ever see the HIR path behind such a
    // reference ─────────────────────────────────────────────────────────

    #[test]
    fn renaming_a_variable_edits_only_the_head_segment_inside_decl_initializers() {
        // `VAR n = p.x` / `CONST k = p.y` both resolve through
        // `lookup_variable`'s step-11 field-access fallback to `p`, at the
        // whole `p.x` / `p.y` range. Before #1571 the narrowing found no
        // matching HIR path (the walker never entered a declaration
        // initializer), so both edits spanned the whole path and applying
        // the rename dropped the field segments.
        let src = "VAR p = 0\nVAR n = p.x\nCONST k = p.y\n=== main ===\n-> DONE\n";
        let (s, id) = session(src);
        let decl_pos = u32::try_from(src.find("p = 0").expect("decl")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(decl_pos), "newname").expect("rename");

        for occurrence in ["p.x", "p.y"] {
            let ref_pos = u32::try_from(src.find(occurrence).expect("ref")).expect("offset");
            let found = result
                .edits
                .iter()
                .find(|e| e.file == id && e.range.start() == TextSize::from(ref_pos));
            assert!(
                found.is_some(),
                "expected an edit at `{occurrence}`'s head segment, got {:?}",
                result
                    .edits
                    .iter()
                    .map(|e| (e.range, e.new_text.as_str()))
                    .collect::<Vec<_>>()
            );
            let edit = found.expect("checked above");
            assert_eq!(
                usize::from(edit.range.end()) - usize::from(edit.range.start()),
                1,
                "the edit inside the declaration initializer must span only the 1-byte `p` \
                 head, not the whole `{occurrence}` path — got {:?}",
                edit.range
            );
        }
    }

    // ── Issue #1571 variant 3: `prepare_rename` returned the whole
    // dotted-path range, so F2 on the head/receiver segment highlighted a
    // span wider than the one `rename` then edits ───────────────────────

    #[test]
    fn prepare_rename_on_a_field_access_head_offers_only_the_head_segment() {
        let src = "VAR p = 0\n=== main ===\n~ y = p.x\n-> DONE\n";
        let (s, id) = session(src);
        let ref_pos = u32::try_from(src.find("p.x").expect("ref")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let range = prepare_rename(s.db(), analysis, id, TextSize::from(ref_pos))
            .expect("the head segment is renameable");

        assert_eq!(
            (range.start(), range.end()),
            (TextSize::from(ref_pos), TextSize::from(ref_pos + 1)),
            "F2 on `p` must offer only the `p` segment, not the whole `p.x` path"
        );
    }

    #[test]
    fn prepare_rename_on_a_ufcs_receiver_offers_only_the_receiver_segment() {
        let (s, id) = native_session(UFCS_FREE_FN_SRC);
        let recv_pos =
            u32::try_from(UFCS_FREE_FN_SRC.find("g.greet(3)").expect("recv")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let range = prepare_rename(s.db(), analysis, id, TextSize::from(recv_pos))
            .expect("the receiver segment is renameable");

        assert_eq!(
            (range.start(), range.end()),
            (TextSize::from(recv_pos), TextSize::from(recv_pos + 1)),
            "F2 on `g` must offer only the receiver segment, not the whole `g.greet` path"
        );
    }

    // ── Issue #1672: rename stamps `#@was(old_name)` on the renamed
    // declaration (docs/modules-spec.md §5, RULED but never implemented
    // until now). Only under `Dialect::Brink` — `#@was` is itself a brink
    // extension (`dialect_gate.rs`), so stamping it into a strict-ink
    // project would introduce a fresh E051, making a "safe" rename unsafe.
    // ───────────────────────────────────────────────────────────────────

    fn brink_session(src: &str) -> (IdeSession, brink_ir::FileId) {
        let mut s = IdeSession::new();
        s.set_language_dialect(brink_analyzer::Dialect::Brink);
        let id = s.update_and_analyze("t.ink", src.to_string());
        (s, id)
    }

    #[test]
    fn renaming_a_knot_under_brink_dialect_stamps_was_directly_after_the_header() {
        let (s, id) = brink_session("=== hello ===\nHi.\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "hello", None).expect("offset");

        let res = rename_safe(&s, id, offset, "greeting").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("=== greeting ===\n#@was(hello)\nHi.\n-> END\n"),
            "expected #@was stamped as the first line of the knot body: {new_source}"
        );
        assert!(
            res.introduced.is_empty(),
            "stamping #@was under Dialect::Brink must introduce no new diagnostics, got {:?}",
            res.introduced
                .iter()
                .map(|d| (d.code.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn renaming_a_stitch_under_brink_dialect_computes_a_was_directive_edit() {
        // `rename_safe`'s apply-and-reanalyze round trip is deliberately
        // not used here: a stitch's own declaration is *also* recorded as a
        // resolved reference to itself — a pre-existing, independent bug
        // (issue #1571's scope note: "`rename` emits two identical-range
        // edits for a stitch declaration, and `apply_edits` splices both —
        // corrupting the source... Worth its own issue", never fixed).
        // That corrupts `apply_edits`' splice for *any* stitch rename,
        // #@was or not, so it's out of this issue's fence — flagged
        // separately rather than fixed here (see the PR description). This
        // test instead asserts directly against `rename`'s edit list, which
        // proves the #@was edit itself is computed correctly independent of
        // that unrelated bug.
        let src = "=== hub ===\n= market\nHi.\n-> DONE\n";
        let (s, id) = brink_session(src);
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "hub", Some("market")).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result = rename(s.db(), analysis, id, offset, "plaza").expect("rename");

        let insert_pos = u32::try_from(src.find("Hi.").expect("body")).expect("offset");
        let was_edit = result
            .edits
            .iter()
            .find(|e| e.range == rowan::TextRange::empty(TextSize::from(insert_pos)));
        assert!(
            was_edit.is_some(),
            "expected a #@was insertion right before the stitch body, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );
        assert_eq!(was_edit.expect("checked above").new_text, "#@was(market)\n");
    }

    #[test]
    fn renaming_a_variable_under_brink_dialect_stamps_was_on_the_line_above() {
        let src = "VAR hello = 0\n=== main ===\n-> DONE\n";
        let (s, id) = brink_session(src);
        let decl_pos = u32::try_from(src.find("hello").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "greeting").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("#@was(hello)\nVAR greeting = 0\n"),
            "expected #@was stamped on the line immediately above the VAR declaration: \
             {new_source}"
        );
        assert!(
            res.introduced.is_empty(),
            "stamping #@was under Dialect::Brink must introduce no new diagnostics, got {:?}",
            res.introduced
                .iter()
                .map(|d| (d.code.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn renaming_a_constant_under_brink_dialect_stamps_was_on_the_line_above() {
        let src = "CONST hello = 0\n=== main ===\n-> DONE\n";
        let (s, id) = brink_session(src);
        let decl_pos = u32::try_from(src.find("hello").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "greeting").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("#@was(hello)\nCONST greeting = 0\n"),
            "expected #@was stamped on the line immediately above the CONST declaration: \
             {new_source}"
        );
    }

    #[test]
    fn renaming_a_list_under_brink_dialect_stamps_was_on_the_line_above() {
        let src = "LIST Colors = Red, Green\n=== main ===\n-> DONE\n";
        let (s, id) = brink_session(src);
        let decl_pos = u32::try_from(src.find("Colors").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "Palette").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("#@was(Colors)\nLIST Palette = Red, Green\n"),
            "expected #@was stamped on the line immediately above the LIST declaration: \
             {new_source}"
        );
    }

    #[test]
    fn rename_does_not_duplicate_or_overwrite_an_existing_was_directive() {
        // A second rename of an already-migrated declaration keeps its
        // original `#@was` record rather than silently losing it (a fresh
        // stamp would only preserve the *most recent* rename, defeating the
        // "reads a chain back to the original name" purpose of the
        // directive).
        let (s, id) = brink_session("=== hello ===\n#@was(original)\nHi.\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "hello", None).expect("offset");

        let res = rename_safe(&s, id, offset, "greeting").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("=== greeting ===\n#@was(original)\n"),
            "the original #@was record must survive unchanged: {new_source}"
        );
        assert_eq!(
            new_source.matches("#@was").count(),
            1,
            "must not add a second #@was directive on top of an existing one: {new_source}"
        );
    }

    #[test]
    fn renaming_a_documented_variable_keeps_the_doc_block_above_the_was_directive() {
        // Review finding on #1672 (blocking): `insert_before_decl_line` used
        // to insert `#@was` at the start of the declaration's own line,
        // landing it *between* a `///` doc comment and the `VAR` it
        // documents — `collect_doc_lines` only walks a *contiguous* run of
        // doc lines back from the declaration, so the inserted directive
        // broke that contiguity and the doc block silently vanished from
        // `SymbolManifest::docs`, with no diagnostic.
        let src = "/// The player's gold.\nVAR hello = 0\n=== main ===\n-> DONE\n";
        let (s, id) = brink_session(src);
        let decl_pos = u32::try_from(src.find("hello").expect("decl")).expect("offset");

        let res = rename_safe(&s, id, TextSize::from(decl_pos), "greeting").expect("rename");

        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("#@was(hello)\n/// The player's gold.\nVAR greeting = 0\n"),
            "the #@was directive must land above the doc-comment run, not between it and the \
             declaration: {new_source}"
        );

        // The doc block itself must survive re-analysis, still attached to
        // the renamed declaration.
        let mut fresh = IdeSession::new();
        fresh.set_language_dialect(brink_analyzer::Dialect::Brink);
        let fresh_id = fresh.update_and_analyze("t.ink", new_source.to_owned());
        let manifest = fresh.manifest(fresh_id).expect("manifest");
        let doc = manifest
            .docs
            .get(&(brink_ir::SymbolKind::Variable, "greeting".to_owned()));
        assert_eq!(
            doc.and_then(|d| d.doc.clone()),
            Some("The player's gold.".to_owned()),
            "the doc block must survive the rename, attached to the new name: {manifest:?}"
        );
    }

    #[test]
    fn renaming_under_strict_ink_dialect_does_not_stamp_was() {
        // `#@was` is itself a brink extension (dialect_gate.rs) — under the
        // default `Dialect::StrictInk` (the oracle-conformance dialect),
        // stamping it would introduce a fresh E051 on every rename. This is
        // the regression test for the dialect gate in `rename` (issue
        // #1672 review): without it, `safe_rename_updates_refs_with_no_new_diagnostics`
        // above would fail because `rename_safe` reports the E051 it just
        // introduced.
        let (s, id) = session("=== hello ===\nHi.\n-> END\n");
        let hir = s.hir(id).expect("hir");
        let offset = declaration_offset(hir, "hello", None).expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result = rename(s.db(), analysis, id, offset, "greeting").expect("rename");

        assert!(
            result.edits.iter().all(|e| !e.new_text.contains("#@was")),
            "no edit may introduce #@was under Dialect::StrictInk, got {:?}",
            result
                .edits
                .iter()
                .map(|e| e.new_text.as_str())
                .collect::<Vec<_>>()
        );
    }

    // ── Issue #1838 review finding (blocking, correctness): a
    // natural-notation `@[convention(claims = "…", order = N)]` handler's
    // dispatch is a compiler-*synthesized* call whose `Path`/`Name` range is
    // the entire claimed prose line, not any real occurrence of the
    // handler's name. Unfiltered, `rename`/`prepare_rename` treated it like
    // any other reference and rewrote the claimed line's bytes — `brink ide
    // rename` on a claiming handler corrupted its own claimed prose
    // lines. ───────

    const CLAIMING_HANDLER_SRC: &str = "\
@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]
fn interior(place) {
  return place;
}

flow main() {
  INT. MARKET SQUARE
}
";

    #[test]
    fn renaming_a_claiming_handler_does_not_corrupt_the_claimed_prose_line() {
        // Regression test for the review finding: verified red without the
        // fix — before `is_synthesized_element_ref` gated this loop, an
        // edit at `heading_pos` was present in `result.edits`, rewriting
        // `INT. MARKET SQUARE` to `exterior`.
        let (s, id) = native_session(CLAIMING_HANDLER_SRC);
        let decl_pos = u32::try_from(CLAIMING_HANDLER_SRC.find("interior(place)").expect("decl"))
            .expect("offset");
        let analysis = s.analysis().expect("analysis");

        let result =
            rename(s.db(), analysis, id, TextSize::from(decl_pos), "exterior").expect("rename");

        let heading_pos = u32::try_from(
            CLAIMING_HANDLER_SRC
                .find("INT. MARKET SQUARE")
                .expect("heading"),
        )
        .expect("offset");
        assert!(
            result
                .edits
                .iter()
                .all(|e| e.range.start() != TextSize::from(heading_pos)),
            "the claimed prose line must never be an edit target, got {:?}",
            result
                .edits
                .iter()
                .map(|e| (e.range, e.new_text.as_str()))
                .collect::<Vec<_>>()
        );

        // End-to-end: applying every edit must leave the claimed line
        // byte-identical, and still rename the declaration.
        let res = rename_safe(&s, id, TextSize::from(decl_pos), "exterior").expect("rename_safe");
        let new_source = res.new_source.as_deref().expect("new_source");
        assert!(
            new_source.contains("INT. MARKET SQUARE"),
            "the claimed scene heading must survive byte-identical: {new_source}"
        );
        assert!(
            new_source.contains("fn exterior(place)"),
            "the handler declaration itself must still be renamed: {new_source}"
        );
    }

    #[test]
    fn prepare_rename_over_a_claimed_prose_line_is_not_renameable() {
        let (s, id) = native_session(CLAIMING_HANDLER_SRC);
        let heading_pos = u32::try_from(
            CLAIMING_HANDLER_SRC
                .find("INT. MARKET SQUARE")
                .expect("heading"),
        )
        .expect("offset");
        let analysis = s.analysis().expect("analysis");

        assert!(
            prepare_rename(s.db(), analysis, id, TextSize::from(heading_pos)).is_none(),
            "a compiler-synthesized claim reference is not a real identifier to rename"
        );
    }
}
