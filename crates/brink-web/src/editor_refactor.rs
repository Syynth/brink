use brink_ide::session::IdeSession;
use serde::Serialize;

// ── Auto-import helper ───────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct AutoImportJs {
    pub(crate) ok: bool,
    pub(crate) already_reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) edit: Option<brink_ide::line_convert::TextEdit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

// ── Structural result helpers (#316) ─────────────────────────────────

/// The unified JSON payload for every mutating structural op (rename, move,
/// promote, demote, reorder, file-rename, delete). `new_source` is the rewritten
/// primary file; `cross_file_edits` carry the referencing files' rewrites
/// (resolved to full source). `safe` + `introduced_diagnostics` are the
/// safe-by-default breakage gate — empty/`true` for reorders and clean ops.
#[derive(Serialize)]
struct StructuralResultJs {
    ok: bool,
    /// The file path this result applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_source: Option<String>,
    cross_file_edits: Vec<CrossFileEditJs>,
    /// Diagnostics present after the op but not before. Empty ⇒ `safe`.
    introduced_diagnostics: Vec<RenameDiagJs>,
    /// True when the op introduces no new diagnostics.
    safe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// A cross-file reference edit, resolved to the full new source of the file.
///
/// brink-ide reports cross-file edits as `(FileId, byte range, new_text)`, but
/// the editor works in paths and UTF-16 offsets. We resolve each affected file
/// here — applying its byte-range edits against its source — so the consumer
/// just replaces the file's content by path.
#[derive(Serialize)]
struct CrossFileEditJs {
    path: String,
    new_source: String,
}

/// Apply non-overlapping byte-range edits to `src`. Edits are applied from the
/// highest start offset down so earlier offsets stay valid; out-of-bounds or
/// non-char-boundary edits are skipped (defensive — they should never occur).
pub(crate) fn apply_edits(src: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = src.to_owned();
    for (start, end, text) in edits {
        if start <= end
            && end <= out.len()
            && out.is_char_boundary(start)
            && out.is_char_boundary(end)
        {
            out.replace_range(start..end, &text);
        }
    }
    out
}

/// One entry in a structural op's breakage report.
#[derive(Serialize)]
struct RenameDiagJs {
    severity: String,
    code: String,
    message: String,
    path: String,
    /// 1-based line of the diagnostic's start.
    line: u32,
    /// 1-based column of the diagnostic's start.
    col: u32,
}

/// Map a brink-ide [`IntroducedDiagnostic`](brink_ide::structural_result::IntroducedDiagnostic)
/// to its JSON shape.
fn diag_js(d: &brink_ide::structural_result::IntroducedDiagnostic) -> RenameDiagJs {
    RenameDiagJs {
        severity: match d.severity {
            brink_ir::Severity::Error => "error",
            brink_ir::Severity::Warning => "warning",
            brink_ir::Severity::Info => "info",
            brink_ir::Severity::Hint => "hint",
        }
        .to_owned(),
        code: d.code.as_str().to_owned(),
        message: d.message.clone(),
        path: d.path.clone(),
        line: d.line,
        col: d.col,
    }
}

/// Resolve a [`StructuralResult`](brink_ide::structural_result::StructuralResult)'s
/// cross-file `FileEdit`s to full new file sources (applying each file's
/// byte-range edits against its current source), excluding the primary `path`
/// (already covered by `new_source`). Deterministic (BTreeMap-grouped).
fn resolve_cross_file_edits(
    session: &IdeSession,
    result: &brink_ide::structural_result::StructuralResult,
    path: &str,
) -> Vec<CrossFileEditJs> {
    let mut by_file: std::collections::BTreeMap<u32, Vec<(usize, usize, String)>> =
        std::collections::BTreeMap::new();
    for e in &result.cross_file_edits {
        by_file.entry(e.file.0).or_default().push((
            usize::from(e.range.start()),
            usize::from(e.range.end()),
            e.new_text.clone(),
        ));
    }

    let mut edits: Vec<CrossFileEditJs> = Vec::new();
    for (file_raw, file_edits) in by_file {
        let file_id = brink_ir::FileId(file_raw);
        let (Some(src), Some(fpath)) = (session.source(file_id), session.file_path(file_id)) else {
            continue;
        };
        if fpath == path {
            continue;
        }
        edits.push(CrossFileEditJs {
            path: fpath.to_owned(),
            new_source: apply_edits(src, file_edits),
        });
    }
    edits
}

/// Serialize a fully-formed [`StructuralResult`](brink_ide::structural_result::StructuralResult)
/// (already carrying `safe` / `introduced`) to the unified `StructuralResultJs`
/// JSON. Used by rename, file-rename, and delete — ops that gate themselves.
pub(crate) fn structural_result_json(
    session: &IdeSession,
    result: &brink_ide::structural_result::StructuralResult,
    path: &str,
) -> String {
    let cross_file_edits = resolve_cross_file_edits(session, result, path);
    let introduced_diagnostics: Vec<RenameDiagJs> = result.introduced.iter().map(diag_js).collect();
    let resp = StructuralResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: result.new_source.clone(),
        cross_file_edits,
        safe: result.safe,
        introduced_diagnostics,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

/// Run the op-agnostic breakage gate over a structural *move*'s result (which
/// arrives un-gated from the pure ops), then serialize. The move's primary
/// source is a full-file rewrite, so the gate overlays it wholesale and the
/// cross-file edits onto their own files.
pub(crate) fn gated_move_json(
    session: &IdeSession,
    mut result: brink_ide::structural_result::StructuralResult,
    path: &str,
) -> String {
    if let Some(new_source) = result.new_source.as_deref() {
        let introduced = brink_ide::structural_result::gate_with_source(
            session,
            path,
            new_source,
            &result.cross_file_edits,
        );
        result.safe = introduced.is_empty();
        result.introduced = introduced;
    }
    structural_result_json(session, &result, path)
}

// ── Directory-move result helpers (#314) ─────────────────────────────

/// The JSON payload for an atomic directory rename/move (#314) — the multi-file
/// analog of [`StructuralResultJs`]. `moved_files` are the relocated files (each
/// carrying its new path + rewritten source); `cross_file_edits` carry the
/// outside referrers' rewrites. `safe` + `introduced_diagnostics` are the shared
/// safe-by-default breakage gate.
#[derive(Serialize)]
struct DirMoveResultJs {
    ok: bool,
    /// Every file relocated by the move.
    moved_files: Vec<MovedFileJs>,
    /// Reference edits in files outside the moved directory, resolved to full
    /// new source.
    cross_file_edits: Vec<CrossFileEditJs>,
    /// Diagnostics present after the move but not before. Empty ⇒ `safe`.
    introduced_diagnostics: Vec<RenameDiagJs>,
    /// True when the move introduces no new diagnostics.
    safe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// One relocated file: written at `new_path` (with `new_source`), removed from
/// `old_path`.
#[derive(Serialize)]
struct MovedFileJs {
    old_path: String,
    new_path: String,
    new_source: String,
}

/// Serialize a [`DirMoveResult`](brink_ide::dir_rename::DirMoveResult) to its
/// JSON shape. Cross-file (inbound) edits are resolved to full new source
/// deterministically (BTreeMap-grouped); moved files are already full sources.
pub(crate) fn dir_move_result_json(
    session: &IdeSession,
    result: &brink_ide::dir_rename::DirMoveResult,
) -> String {
    let moved_files: Vec<MovedFileJs> = result
        .moved_files
        .iter()
        .map(|m| MovedFileJs {
            old_path: m.old_path.clone(),
            new_path: m.new_path.clone(),
            new_source: m.new_source.clone(),
        })
        .collect();

    // Inbound edits land in files outside the folder — resolve each to full
    // source. Group by file id for determinism (BTreeMap), splice from the end.
    let mut by_file: std::collections::BTreeMap<u32, Vec<(usize, usize, String)>> =
        std::collections::BTreeMap::new();
    for e in &result.cross_file_edits {
        by_file.entry(e.file.0).or_default().push((
            usize::from(e.range.start()),
            usize::from(e.range.end()),
            e.new_text.clone(),
        ));
    }
    let mut cross_file_edits: Vec<CrossFileEditJs> = Vec::new();
    for (file_raw, file_edits) in by_file {
        let file_id = brink_ir::FileId(file_raw);
        let (Some(src), Some(fpath)) = (session.source(file_id), session.file_path(file_id)) else {
            continue;
        };
        cross_file_edits.push(CrossFileEditJs {
            path: fpath.to_owned(),
            new_source: apply_edits(src, file_edits),
        });
    }

    let introduced_diagnostics: Vec<RenameDiagJs> = result.introduced.iter().map(diag_js).collect();
    let resp = DirMoveResultJs {
        ok: true,
        moved_files,
        cross_file_edits,
        safe: result.safe,
        introduced_diagnostics,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

/// A `DirMoveResult`-shaped error payload (`ok: false`).
pub(crate) fn dir_error_json(msg: &str) -> String {
    let resp = DirMoveResultJs {
        ok: false,
        moved_files: Vec::new(),
        cross_file_edits: Vec::new(),
        introduced_diagnostics: Vec::new(),
        safe: true,
        error: Some(msg.to_owned()),
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

/// A trivially-safe single-file rewrite (reorders): no gate, empty breakage.
pub(crate) fn move_result_json_simple(new_source: String, path: &str) -> String {
    let resp = StructuralResultJs {
        ok: true,
        path: Some(path.to_owned()),
        new_source: Some(new_source),
        cross_file_edits: Vec::new(),
        introduced_diagnostics: Vec::new(),
        safe: true,
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

pub(crate) fn error_json(msg: &str) -> String {
    let resp = StructuralResultJs {
        ok: false,
        path: None,
        new_source: None,
        cross_file_edits: Vec::new(),
        introduced_diagnostics: Vec::new(),
        safe: true,
        error: Some(msg.to_owned()),
    };
    serde_json::to_string(&resp).unwrap_or_default()
}
