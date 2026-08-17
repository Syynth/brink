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

// ── Refusal-shape parity fixture (#2568) ─────────────────────────────

/// Machine-generated mirror of every *refusal* payload this module can emit,
/// checked in at `crates/brink-web/fixtures/refusal-shapes.json` and read from
/// the TypeScript side by
/// `packages/brink-studio/src/__tests__/structural-refusal-shape.test.ts`.
///
/// It exists because the studio's wasm mock understated these payloads: Rust
/// serializes the WHOLE struct on a refusal — only `path`/`new_source`/`edit`/
/// `error` carry `skip_serializing_if` — so an `ok: false` still ships
/// `safe: true` with empty `cross_file_edits`/`introduced_diagnostics`. The
/// mock answered `{ ok: false, error }` alone, so no studio test could see a
/// bug living in a field the mock never emitted — which is exactly how #2543
/// shipped past 1000+ green studio tests.
///
/// The fixture is DERIVED, never hand-written: it is produced by running the
/// real `error_json`/`dir_error_json` and by constructing the real
/// [`AutoImportJs`] struct, so adding or renaming a field either fails to
/// compile here or turns this test red. Regenerate with:
///
/// ```text
/// BRINK_BLESS_REFUSAL_SHAPES=1 cargo test -p brink-web --lib refusal_shape
/// ```
///
/// ## The enumeration itself is discovered, not trusted (#2577)
///
/// [`generated`] still *constructs* each shape by hand — Rust has no runtime
/// reflection, and this crate has no `inventory`-style registry or derive macro
/// to enumerate types with, so there is no way to build a payload for a struct
/// nobody named. What [`every_refusal_struct_is_in_the_fixture`] adds is that
/// nobody can *omit* one: it scans this crate's own sources for every
/// `Serialize` struct carrying both an `ok: bool` and an `error:
/// Option<String>` — the signature of a payload that can refuse — and fails if
/// one is missing from [`generated`]. Adding a fourth refusal struct is
/// therefore a red test, not a silent gap.
///
/// ## Shape is not vocabulary (#2603)
///
/// Everything above pins which *keys* a refusal ships. It says nothing about
/// the `error` *string* — the fixture deliberately carries a placeholder
/// (`REFUSAL`) there. That gap let the studio mock answer `"unknown handle"`
/// for both auto-import doc-handle ops while production answered
/// `"unknown document handle"`, with the mirroring test pinning the mock's
/// wording rather than production's — so the guard asserted the mock agreed
/// with itself. That is the fourth instance of the same drift class in three
/// waves (#2583, #2599, #2602).
///
/// [`driven_messages`] closes it for the doc-handle ops by *running* them:
/// every string in the fixture's `messages` map is read out of a real
/// [`EditorSession`] refusal payload, never typed. It is **per-site, not
/// automatic** — nothing in this crate can enumerate the (op, refusing-input)
/// pairs, so a site nobody drives stays unpinned.
/// [`doc_handle_refusal_vocabulary_is_uniform`] is the omission guard for
/// this one class: a new doc-handle refusal wording anywhere in the crate
/// turns it red instead of shipping unnoticed.
#[cfg(test)]
mod refusal_shape {
    use super::{AutoImportJs, dir_error_json, error_json};
    use crate::compile::CompileResult;
    use crate::editor::EditorSession;

    const FIXTURE: &str = include_str!("../fixtures/refusal-shapes.json");
    const FIXTURE_REL_PATH: &str = "fixtures/refusal-shapes.json";

    /// Arbitrary — the fixture pins the *shape* (which keys ship, with which
    /// defaults), not the message.
    const REFUSAL_MSG: &str = "REFUSAL";

    const COMMENT: &str = "GENERATED from the Rust refusal payloads — do not hand-edit. \
`shapes` are the serialized structs (message elided to the placeholder in `error`); \
`messages` are real refusal strings read back out of the production ops, not typed (#2603); \
`sources` are the exact inputs those drivers ran against, and `acceptance` is each \
(op, input) pair's own `ok` FLAG plus the `error` beside it — the half no wording-based \
guard can express, because a mock that never refuses has no string to compare (#2661); \
`outlines` are the symbol names/kinds/detail/OWNERSHIP-RANGE `file_symbols` reports, pinning \
the header recognizer itself rather than an op built on it (#2662, ranges added #2685 Gap 3); \
`regions` are the surviving `new_source` after a stitch region was deleted, the half neither \
acceptance nor the outline can see because the op succeeds either way (#2684); `payloads` are \
the same half for every `dispatchSymbolAction` op — `reorder_knots`/`reorder_stitches`/`move_stitch` \
(#2675 Gap C, #2685 Gap 3), plus `demote_knot`/`promote_stitch`/`reorder_knot`/`reorder_stitch` \
(#2706), plus the two extract editor commands `extract_to_function`/`extract_to_knot` (#2675 Gap C, \
#2685 Gap 3) — an indented-FIRST-knot header's end-to-end preamble behavior \
(`reorder_knots:indented-first-knot`, #2706), plus `rename_symbol`'s own answer on that same \
source and `move_stitch`/`promote_stitch`/`demote_knot` driven on ALT_STITCHES's indented \
`  = b` (#2721), plus `move_stitch`/`demote_knot` driven on ALT_FENCES's \
non-newline-terminated `three` boundary — the input shape that stresses their own \
`needs_newline_before`/`needs_nl` guards, which ALT_STITCHES's `  = b` does not (#2730); \
`call_forms` are the exact call-site LINE `extract_to_function` chooses — \
`{name()}` vs `~ name()` — the half `acceptance` cannot see because both forms answer \
`ok: true` (#2675 Gap A); `defaults` are session-seed values a fresh production session \
starts with (#2663); `diagnostics` are the introduced-diagnostic CODES a driven (op, input) \
pair reports, the half `acceptance`'s ok/error pair cannot see (review finding on #2662). \
Regenerate with `BRINK_BLESS_REFUSAL_SHAPES=1 cargo test -p brink-web --lib refusal_shape`. \
Mirrored by packages/brink-studio/src/__tests__/structural-refusal-shape.test.ts (#2568).";

    fn parse(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("refusal payload is valid JSON")
    }

    /// Build the fixture document from the real production payloads.
    fn generated() -> serde_json::Value {
        // A struct literal, so a new `AutoImportJs` field is a compile error
        // here rather than a silent shape drift.
        let auto_import = AutoImportJs {
            ok: false,
            already_reachable: false,
            edit: None,
            error: Some(REFUSAL_MSG.to_owned()),
        };
        let auto_import_json =
            serde_json::to_string(&auto_import).expect("AutoImportJs serializes");
        // Likewise a struct literal — `CompileResult` is the compile channel's
        // refusal (`{ ok: false, error }` out of `compile_project`), discovered
        // by `every_refusal_struct_is_in_the_fixture` rather than remembered.
        let compile = CompileResult {
            ok: false,
            story_bytes: None,
            warnings: Vec::new(),
            error: Some(REFUSAL_MSG.to_owned()),
        };
        let compile_json = serde_json::to_string(&compile).expect("CompileResult serializes");
        serde_json::json!({
            "$comment": COMMENT,
            "error": REFUSAL_MSG,
            "shapes": {
                "AutoImportJs": parse(&auto_import_json),
                "CompileResult": parse(&compile_json),
                "DirMoveResultJs": parse(&dir_error_json(REFUSAL_MSG)),
                "StructuralResultJs": parse(&error_json(REFUSAL_MSG)),
            },
            "messages": driven_messages(),
            "sources": source_fixtures(),
            "acceptance": driven_acceptance(),
            "headers": driven_header_rewrites(),
            "outlines": driven_outlines(),
            "regions": driven_stitch_regions(),
            "payloads": driven_payloads(),
            "call_forms": driven_call_forms(),
            "defaults": driven_defaults(),
            "diagnostics": driven_diagnostics(),
        })
    }

    // ── Vocabulary: the refusal STRING, driven out of production (#2603) ──

    /// An unknown document handle. `insert_doc` never hands out an id this
    /// large in these fixtures, and `0` is documented as never valid.
    const UNKNOWN_DOC: u32 = 999;

    /// The `error` string out of a refusal payload, with the payload's own
    /// `ok: false` asserted first — so a message can never be harvested from a
    /// *successful* answer and pinned as if it were a refusal's wording.
    fn refusal_message(site: &str, json: &str) -> String {
        let value = parse(json);
        assert!(
            value["ok"] == serde_json::json!(false),
            "`{site}` was expected to refuse, but answered ok: true — the driver no \
             longer reaches the refusal it is meant to pin: {value:#}"
        );
        let message = value["error"].as_str();
        assert!(
            message.is_some_and(|m| !m.is_empty()),
            "`{site}` refused without an `error` string, so there is no vocabulary \
             to pin: {value:#}"
        );
        message.expect("just asserted above").to_owned()
    }

    /// The two source fixtures the drivers refuse against, byte-identical to
    /// `structural-refusal-shape.test.ts`'s `MAIN` / `TWO_KNOTS` — the driven
    /// message and the mock call it pins must be produced from the same input,
    /// or the fixture pins production's answer to a *different* question.
    const MAIN: &str = "=== hello ===\nHi.\n-> END\n";
    const TWO_KNOTS: &str =
        "=== one ===\nFirst.\n= a\nA.\n= b\nB.\n\n=== two ===\nSecond.\n= a\nOther A.\n";

    /// A single `function` knot. `KnotHeader::name()` answers `greet` for it
    /// exactly as it would for a plain knot, so every op below treats it as an
    /// ordinary top-level knot — the fact a header-shaped regex has to skip a
    /// `function` segment to see is the whole reason this fixture exists.
    const FUNCTION_KNOT: &str = "=== function greet() ===\n~ return \"hi\"\n";

    /// A plain knot carrying one stitch, plus a function knot. The mixed file:
    /// production counts TWO top-level knots here.
    const KNOT_AND_FUNCTION: &str =
        "=== one ===\nFirst.\n= a\nA.\n\n=== function greet() ===\n~ return \"hi\"\n";

    /// Two knots, neither carrying a stitch — the input that separates
    /// `reorder_stitches`'s "no stitches, nothing to do" branch (production
    /// answers `Ok(source)` unchanged) from its `InvalidReorder` refusal.
    const STITCHLESS_KNOTS: &str = "=== one ===\nFirst.\n\n=== two ===\nSecond.\n";

    /// A stitch whose name is already taken by a top-level FUNCTION knot —
    /// `promote_stitch`'s collision check runs over every knot, function ones
    /// included.
    const STITCH_SHADOWS_FUNCTION: &str =
        "=== one ===\nFirst.\n= greet\nG.\n\n=== function greet() ===\n~ return \"hi\"\n";

    /// A `VAR` beside a knot — `extract_*` refuses a name that clashes with a
    /// declared variable/const/list, a check nothing in the outline model sees.
    const VAR_AND_KNOT: &str = "VAR score = 0\n\n=== one ===\nFirst.\nSecond.\n";

    /// A knot body with a blank line in it, so a selection can be non-empty in
    /// offsets yet whitespace-only in content.
    const BLANK_BODY: &str = "=== one ===\nFirst.\n\nLast.\n";

    /// A stitch body carrying one INDENTED line — the input that drives
    /// `extract_to_function`/`extract_to_knot`'s two indent-handling steps
    /// review found undriven (#2675 Gap C follow-up): `extract.rs`'s
    /// `plan.indent` (the selected line's own leading whitespace) prefixes
    /// the replacement call line (`extract.rs:121-125`), and `rebuild`'s
    /// `dedent(&plan.selected)` (`extract.rs:246`) strips that same
    /// indentation from the appended body. Both answered `ok: true` under the
    /// mock's un-indented call line and un-dedented raw-selection body, so
    /// `acceptance` never caught either — see [`driven_payloads`]'s
    /// `extract_to_function:indented` / `extract_to_knot:indented`.
    const INDENTED_LINE: &str = "=== one ===\nFirst.\n= a\n  Indented.\n";

    /// A stitch carrying parameters — the promote rewrite has to keep the
    /// `(n)` inside the new fences, not strand it after them.
    const PARAM_STITCH: &str = "=== one ===\nFirst.\n= deal(n)\nD.\n";

    /// A source whose very first character is a blank line (review finding
    /// on #2670). `snap_to_lines`'s line-start search runs `source[..lo]`,
    /// so an empty prefix answers `None` -> `0` for `lo == 0`; the mock's
    /// `source.lastIndexOf("\n", l - 1)` used to pass `-1`, which JS clamps
    /// to `0` and finds the LEADING newline itself instead of "no newline
    /// before here", answering `start = 1`.
    const LEADING_BLANK_LINE: &str = "\n=== a ===\nContent.\n";

    /// Five knots whose `=` fences are all legal and none of them the
    /// `=== name ===` shape every other fixture uses (#2662).
    ///
    /// `brink_syntax`'s `parser/knot.rs` used to document the `knot_header`
    /// rule as `"==" ~ "="* ~ INLINE_WS* ~ ("function" ~ INLINE_WS+)? ~
    /// identifier … ~ INLINE_WS* ~ ("==" ~ "="*)?` (the comment now says
    /// `INLINE_WS*` after `function` too, mismatch fixed separately by
    /// #2707), but the **code**'s `p.bump()` + `p.skip_ws()` after `function`
    /// has always matched **zero or more**, same as every other whitespace
    /// step in the rule — only the doc comment changed. So production's real
    /// vocabulary is **two or more** `=`, **zero or more** spaces,
    /// **tolerated leading indent** (`skip_ws` runs before the fence is even
    /// looked for), and an **optional** trailing fence of **any width** —
    /// `== one ==`, `===two===`, `==== three ====`, `  ==== four ====` and
    /// `=== five` are all ordinary top-level knots, and `one` still carries
    /// its stitch `a`.
    ///
    /// The studio mock had two narrower answers to that question, and which
    /// one applied depended on which op a test happened to call: `parseOutline`
    /// wanted exactly three `=` and a REQUIRED space (so all five knots here
    /// were invisible to the outline and to the seven ops built on it), while
    /// `delete_symbol`/`rename_symbol` matched two-or-three `=` inline (so the
    /// first two resolved and the rest did not). One source exercises both
    /// halves, which is the point: a fixture that only used `== one ==` would
    /// leave the inline family green and read as though the split were
    /// one-sided.
    ///
    /// `four` and `five` are the two widenings `KNOT_HEADER_PREFIX`'s `^\s*`
    /// and `KNOT_HEADER_RE`'s trailing `(?:={2,})?` claim and nothing before
    /// them drove (review finding on #2662): every other source here uses a
    /// flush-left header with a closing fence, so an indent regression or a
    /// required-trailing-fence regression in either recognizer would have gone
    /// green against all ten sources in this fixture.
    const ALT_FENCES: &str = "== one ==\nFirst.\n= a\nA.\n\n===two===\nSecond.\n\n==== three ====\nThird.\n\n  ==== four ====\nFourth.\n\n=== five\nFifth.\n";

    /// Three stitches, only one of them the `= name` shape every other fixture
    /// uses, plus two `=`-leading lines that are NOT headers (#2684).
    ///
    /// The stitch level had #2662's split one rung down. `brink_syntax`'s
    /// `parser/knot.rs` used to document `stitch_header` as
    ///
    /// ```text
    /// stitch_header = { "=" ~ !("=" | ">") ~ INLINE_WS+ ~ identifier ~ … }
    /// ```
    ///
    /// but the **code** is `at_stitch` (`current() == EQ && nth(1) != EQ &&
    /// nth(1) != GT`) followed by `p.skip_ws()`, and `skip_ws` matches
    /// **zero** or more. So production's real vocabulary is: a **tolerated
    /// leading indent** (`current()` skips trivia), **exactly one** `=`, a
    /// negative lookahead excluding `=` and `>`, and **optional** whitespace —
    /// the `INLINE_WS+` that used to be in the doc comment was never what the
    /// parser does (the comment now says `INLINE_WS*`, mismatch fixed
    /// separately by #2695).
    ///
    /// Driven, not read (the whole lesson of #2662): `file_symbols` reports a
    /// stitch for each of `= a`, `  = b`, `=c`, `   =d`, `= e(n)` and
    /// `\t= h`, and reports **none** for `=> f`, `  => g`, `= > j`, `= = k`
    /// or a bare `=`. The last three are the reason the mock's exclusion is
    /// `=(?!\s*[=>])` rather than `=(?![=>])`: `nth(1)` skips trivia, so a
    /// space between the `=` and the `>` does not make a stitch of a divert.
    ///
    /// The mock had the same two-answer split #2662 fixed for knots:
    ///
    /// | consumer | pattern before | `= a` | `  = b` | `=c` | `=> x` / `= > y` |
    /// |---|---|---|---|---|---|
    /// | `parseOutline` / `selectionCrossesHeader` | `^=\s+(\w+)` | stitch | invisible | invisible | not a header |
    /// | `delete_symbol` / `rename_symbol` guards | `^\s*=\s+` | stitch | stitch | invisible | not a header |
    /// | `opensHeader` (region end) | `^\s*=` | ends region | ends region | ends region | **ends region** |
    ///
    /// — so an indented `  = b` was a stitch to the ops and invisible to the
    /// outline, a tight `=c` was invisible to both yet still ENDED a region,
    /// and `=> x` ended a region production keeps running through. `a` is the
    /// positive control: it is the one shape every family already resolved,
    /// so a widening bought by making everything match is red here.
    /// The indented `  = b` is deliberately LAST. Production's regions are
    /// CST node ranges, not lines: deleting `b` leaves its `  ` indent behind
    /// (`  ` + the next header), and deleting the stitch *before* an indented
    /// one consumes that indent. The mock's region model is line-based, so
    /// only a boundary where a line start and a node start coincide can be
    /// pinned byte-for-byte — see [`driven_stitch_regions`], which drives the
    /// one flush-left boundary. The indent/node-range divergence is real and
    /// out of #2684's fence; it is recorded on the issue rather than papered
    /// over here.
    const ALT_STITCHES: &str = "=== one ===\nFirst.\n= a\nA.\n=> x\nStill a.\n= > y\nStill a too.\n=c\nC.\n  = b\nB.\n\n=== two ===\nSecond.\n";

    /// A file whose very FIRST knot header is itself indented (#2706) — the
    /// case #2703's `full_start` fix left undriven on both sides.
    ///
    /// #2703 found that an indented header's leading whitespace is glued to
    /// the PRECEDING symbol's trailing trivia: `knot_body`'s loop calls
    /// `p.skip_ws()` before checking `at_knot`, so the still-open predecessor
    /// swallows the indent before the parser notices a new header started.
    /// A FIRST knot has no preceding symbol, so that mechanism cannot apply
    /// to it — but `source_file`'s own loop has the identical shape: it also
    /// calls `p.skip_ws()` before dispatching to `knot_definition`, and that
    /// call happens while `SOURCE_FILE` (not `KNOT_DEF`) is the open node.
    /// So the indent is consumed as `SOURCE_FILE`'s own leading trivia,
    /// never reaching `KNOT_DEF`'s range at all — the general rule ("an
    /// indent belongs to whatever precedes the header, in the CST") holds
    /// even with no preceding symbol, it is simply the root node doing the
    /// swallowing instead of another knot's body.
    ///
    /// Driven here rather than assumed, per the general #2703/#2685 Gap 3
    /// lesson that a symbol's ownership boundary can be WRONG in a way no
    /// `ok`/`error` flag sees: [`driven_outlines`] pins `full_start` for
    /// `one` against `document_symbols`'s `doc_extended_start`, and
    /// [`driven_payloads`]'s `reorder_knots:indented-first-knot` pins the
    /// end-to-end consequence — whether the leading indent stays behind as
    /// untouched file preamble (like `structural_move.rs`'s
    /// `decl_region_start`) when knot `one` is moved out of first place.
    const INDENTED_FIRST_KNOT: &str = "  === one ===\nFirst.\n= a\nA.\n\n=== two ===\nSecond.\n";

    /// Every named source the drivers run against, shipped INTO the fixture.
    ///
    /// The mirroring TypeScript test asserts its own constants are
    /// byte-identical to these. Before #2661 that identity was a comment
    /// ("byte-identical to `structural-refusal-shape.test.ts`'s `MAIN` /
    /// `TWO_KNOTS`") and nothing checked it — yet every parity claim in the
    /// fixture depends on it, since a driven answer is only evidence about the
    /// mock if the mock was asked the same question.
    fn source_fixtures() -> serde_json::Value {
        serde_json::json!({
            "ALT_FENCES": ALT_FENCES,
            "ALT_STITCHES": ALT_STITCHES,
            "BLANK_BODY": BLANK_BODY,
            "FUNCTION_KNOT": FUNCTION_KNOT,
            "INDENTED_FIRST_KNOT": INDENTED_FIRST_KNOT,
            "INDENTED_LINE": INDENTED_LINE,
            "KNOT_AND_FUNCTION": KNOT_AND_FUNCTION,
            "MAIN": MAIN,
            "PARAM_STITCH": PARAM_STITCH,
            "LEADING_BLANK_LINE": LEADING_BLANK_LINE,
            "STITCHLESS_KNOTS": STITCHLESS_KNOTS,
            "STITCH_SHADOWS_FUNCTION": STITCH_SHADOWS_FUNCTION,
            "TWO_KNOTS": TWO_KNOTS,
            "VAR_AND_KNOT": VAR_AND_KNOT,
        })
    }

    fn session_with(files: &[(&str, &str)]) -> EditorSession {
        let mut session = EditorSession::new();
        for (path, source) in files {
            session.update_file(path, source);
        }
        session
    }

    /// The refusal *vocabulary* of the document-handle ops, obtained by calling
    /// the real production methods on a real [`EditorSession`] and reading the
    /// `error` field back out of the JSON they answer with.
    ///
    /// Kept separate from [`driven_op_messages`] because
    /// [`doc_handle_refusal_vocabulary_is_uniform`] cross-checks *these* against
    /// the crate's handle literals: an op refusing for some other reason has no
    /// business being measured against the handle vocabulary.
    ///
    /// Nothing here types a message. Change
    /// `crates/brink-web/src/editor/refactor.rs`'s or `code_actions.rs`'s
    /// wording and this map moves with it, turning the checked-in fixture
    /// stale — which is what forces the studio mock
    /// (`packages/brink-studio/src/__mocks__/brink-web.ts`) to be updated too,
    /// since `structural-refusal-shape.test.ts` reads its expectations from
    /// here rather than repeating them (#2603).
    ///
    /// Keys are `<op>:<refusing-input>`. Every key must have a consumer on the
    /// TS side; adding one without a mock counterpart just parks a string.
    fn driven_doc_handle_messages() -> serde_json::Value {
        let mut session = session_with(&[("main.ink", MAIN)]);

        let include_doc = refusal_message(
            "auto_import_include_doc",
            &session.auto_import_include_doc(UNKNOWN_DOC, "other.ink"),
        );
        let apply_include_doc = refusal_message(
            "auto_import_apply_include_doc",
            &session.auto_import_apply_include_doc(UNKNOWN_DOC, "other.ink"),
        );
        let code_action_doc = refusal_message(
            "resolve_code_action_doc",
            &session.resolve_code_action_doc(
                UNKNOWN_DOC,
                &serde_json::json!({ "action": "SortKnots" }).to_string(),
                0,
            ),
        );

        // The read-only-mount fence (#2621). `EditorSession::new()` mounts the
        // stdlib, so its first key is read-only in a session nobody shadowed —
        // the same route `is_read_only_true_for_mount_...` uses.
        let mut mounted = session_with(&[("main.ink", MAIN)]);
        let std_key = brink_environment::stdlib_sources()[0].0;
        let std_doc = mounted.open_document(std_key);
        assert!(
            std_doc != 0,
            "the mounted stdlib key `{std_key}` has no document handle, so the \
             read-only fence cannot be driven"
        );
        let apply_read_only = refusal_message(
            "auto_import_apply_include_doc (read-only mount)",
            &mounted.auto_import_apply_include_doc(std_doc, "main.ink"),
        );

        serde_json::json!({
            "auto_import_include_doc:unknown-handle": include_doc,
            "auto_import_apply_include_doc:unknown-handle": apply_include_doc,
            "auto_import_apply_include_doc:read-only-mount": apply_read_only,
            "resolve_code_action_doc:unknown-handle": code_action_doc,
        })
    }

    /// The rest of the refusal vocabulary, driven the same way (#2620).
    ///
    /// Before this, ~28 of ~30 `error:` strings in
    /// `structural-refusal-shape.test.ts` were hand-transcribed from a reading
    /// of the production call path — and three of them were transcribed
    /// *wrongly*, so the mock had been lying and the mirroring test had been
    /// asserting the lie (see that file's header table). Every site below is
    /// now produced by running the op, so no reading is involved.
    ///
    /// This is still **per-site, not automatic**: nothing enumerates the
    /// (op, refusing-input) pairs, so a site nobody writes a driver for stays
    /// unpinned. What changed is that the pinned set is now nearly all of them
    /// rather than three.
    fn driven_op_messages() -> serde_json::Value {
        merge_driven(&[
            driven_file_and_symbol_messages(),
            driven_outline_messages(),
            driven_mock_fidelity_messages(),
        ])
    }

    /// The three gaps #2620's sweep left behind (#2634 / #2635 / #2641).
    ///
    /// Its own function rather than more lines in
    /// [`driven_file_and_symbol_messages`] because that one is already at the
    /// crate's line limit, and because these sites share a reason: each is a
    /// production refusal the studio mock could not answer *at all*, rather
    /// than one it answered with the wrong words.
    ///
    /// - `rename_symbol:missing-symbol` (#2634) — `declaration_offset` finds
    ///   no declaration. The mock had no such branch: renaming a knot that had
    ///   been edited away *succeeded*.
    /// - `resolve_code_action:missing-file` (#2635) — the active path is not a
    ///   loaded file. The mock's wording was already right; the site was simply
    ///   undriven, which is #2621 gap 2 made concrete. `EditorSession`'s
    ///   `active_path` starts as `main.ink`, so a session that loaded only
    ///   `other.ink` reaches the guard without touching `set_active_file`.
    /// - `delete_symbol:stitch-under-wrong-knot` /
    ///   `delete_symbol:stitch-under-missing-knot` (#2641) — production
    ///   resolves the knot first and looks the stitch up inside its body only.
    ///   The mock scanned the whole file, so both of these DELETED instead of
    ///   refusing. Two keys, not one, because production answers them with
    ///   different `MoveError` variants.
    fn driven_mock_fidelity_messages() -> serde_json::Value {
        let main = session_with(&[("main.ink", MAIN)]);
        let two = session_with(&[("main.ink", TWO_KNOTS)]);
        // `active_path` defaults to `main.ink` and `set_active_file` refuses a
        // path it cannot resolve, so loading only `other.ink` leaves the active
        // path pointing at a file the session does not have.
        let inactive = session_with(&[("other.ink", MAIN)]);

        serde_json::json!({
            "rename_symbol:missing-symbol": refusal_message(
                "rename_symbol (no declaration for the named symbol)",
                &main.rename_symbol("main.ink", "nowhere", "", "hi"),
            ),
            "resolve_code_action:missing-file": refusal_message(
                "resolve_code_action (active path not loaded)",
                &inactive.resolve_code_action(
                    &serde_json::json!({ "action": "SortKnots" }).to_string(),
                    0,
                ),
            ),
            // `TWO_KNOTS` puts stitches `a`/`b` under knot `one` and a second
            // `a` under knot `two`. `b` therefore exists in the file but NOT
            // under `two` — the exact input a whole-file scan gets wrong.
            "delete_symbol:stitch-under-wrong-knot": refusal_message(
                "delete_symbol (stitch exists, but under another knot)",
                &two.delete_symbol("main.ink", "two", "b"),
            ),
            // And with no such knot at all, production refuses at the knot
            // lookup before the stitch is ever considered.
            "delete_symbol:stitch-under-missing-knot": refusal_message(
                "delete_symbol (named knot does not exist)",
                &two.delete_symbol("main.ink", "ghost", "a"),
            ),
        })
    }

    /// Concatenate driven-message maps, refusing a key claimed twice — two
    /// drivers for the same site would let one silently win, pinning whichever
    /// input happened to be listed last.
    fn merge_driven(parts: &[serde_json::Value]) -> serde_json::Value {
        let mut merged = serde_json::Map::new();
        for part in parts {
            for (key, value) in part.as_object().cloned().unwrap_or_default() {
                assert!(
                    merged.insert(key.clone(), value).is_none(),
                    "duplicate driven-message key `{key}` — two drivers claim the \
                     same site, so one silently wins"
                );
            }
        }
        serde_json::Value::Object(merged)
    }

    /// File-level and symbol-level ops, plus the two non-`StructuralResultJs`
    /// channels (`rename_dir`, `compile_project`) and the code-action resolver.
    /// Split from [`driven_outline_messages`] only to keep either function
    /// under the crate's line limit.
    fn driven_file_and_symbol_messages() -> serde_json::Value {
        let main = session_with(&[("main.ink", MAIN)]);

        // `rename_file`'s read-only fence, driven off the real stdlib mount
        // rather than a test seam.
        let mounted = session_with(&[("main.ink", MAIN)]);
        let std_key = brink_environment::stdlib_sources()[0].0;

        let mut active = session_with(&[("main.ink", TWO_KNOTS)]);
        assert!(
            active.set_active_file("main.ink"),
            "`main.ink` must be settable as the active file for the \
             `resolve_code_action` drivers"
        );
        let mut single = session_with(&[("main.ink", MAIN)]);
        assert!(
            single.set_active_file("main.ink"),
            "`main.ink` must be settable as the active file for the \
             `resolve_code_action` no-change driver"
        );

        let mut compile = session_with(&[("main.ink", MAIN)]);

        serde_json::json!({
            "rename_file:read-only-mount": refusal_message(
                "rename_file (read-only mount)",
                &mounted.rename_file(std_key, "mine.ink"),
            ),
            "rename_file:missing-file": refusal_message(
                "rename_file (missing file)",
                &main.rename_file("ghost.ink", "other.ink"),
            ),
            "rename_file:target-exists": refusal_message(
                "rename_file (target exists)",
                &session_with(&[("main.ink", MAIN), ("other.ink", MAIN)])
                    .rename_file("main.ink", "other.ink"),
            ),
            "delete_symbol:missing-file": refusal_message(
                "delete_symbol (missing file)",
                &main.delete_symbol("ghost.ink", "hello", ""),
            ),
            "delete_symbol:missing-symbol": refusal_message(
                "delete_symbol (missing symbol)",
                &main.delete_symbol("main.ink", "nowhere", ""),
            ),
            // #2627 review: the missing-KNOT case above and a missing STITCH
            // inside a knot that DOES exist are different `MoveError`
            // variants in production (`SourceNotFound` vs `StitchNotFound`),
            // so they need their own driven input — `active` is already
            // loaded with `TWO_KNOTS`, whose knot `one` is real but has no
            // stitch `nowhere`.
            "delete_symbol:missing-stitch-in-knot": refusal_message(
                "delete_symbol (missing stitch in existing knot)",
                &active.delete_symbol("main.ink", "one", "nowhere"),
            ),
            "extract_to_knot:missing-file": refusal_message(
                "extract_to_knot (missing file)",
                &main.extract_to_knot("ghost.ink", 0, 4, "lifted"),
            ),
            "extract_to_knot:empty-selection": refusal_message(
                "extract_to_knot (empty selection)",
                &main.extract_to_knot("main.ink", 4, 4, "lifted"),
            ),
            "extract_to_function:missing-file": refusal_message(
                "extract_to_function (missing file)",
                &main.extract_to_function("ghost.ink", 0, 4, "lifted"),
            ),
            "extract_to_function:empty-selection": refusal_message(
                "extract_to_function (empty selection)",
                &main.extract_to_function("main.ink", 4, 4, "lifted"),
            ),
            "rename_symbol:missing-file": refusal_message(
                "rename_symbol (missing file)",
                &main.rename_symbol("ghost.ink", "hello", "", "hi"),
            ),
            "rename_symbol_at:unrenameable": refusal_message(
                "rename_symbol_at (unrenameable offset)",
                &main.rename_symbol_at("main.ink", 0, "hi"),
            ),
            "resolve_code_action:unknown-variant": refusal_message(
                "resolve_code_action (unknown variant)",
                &active.resolve_code_action(
                    &serde_json::json!({ "action": "Nonsense" }).to_string(),
                    0,
                ),
            ),
            // `SortKnots` over a single-knot file: the rewrite is a genuine
            // no-op, so the pure resolver answers `None`. The previously
            // hand-copied site used `FormatKnot` on `TWO_KNOTS`, which
            // production *accepts* (it reindents) — see #2620's table.
            "resolve_code_action:no-change": refusal_message(
                "resolve_code_action (no change)",
                &single.resolve_code_action(
                    &serde_json::json!({ "action": "SortKnots" }).to_string(),
                    0,
                ),
            ),
            "rename_dir:missing-dir": refusal_message(
                "rename_dir (missing directory)",
                &main.rename_dir("ghost", "other"),
            ),
            "rename_dir:destination-occupied": refusal_message(
                "rename_dir (destination occupied)",
                &session_with(&[("src/a.ink", MAIN), ("dst/a.ink", MAIN)])
                    .rename_dir("src", "dst"),
            ),
            "compile_project:missing-entry": refusal_message(
                "compile_project (missing entry)",
                &compile.compile_project("ghost.ink"),
            ),
        })
    }

    /// The outline-reshaping ops (`reorder_*`, `move_stitch`,
    /// `promote_stitch`, `demote_knot`) — the seven `dispatchSymbolAction`
    /// sites the studio's symbol menu drives.
    fn driven_outline_messages() -> serde_json::Value {
        let main = session_with(&[("main.ink", MAIN)]);
        let two = session_with(&[("main.ink", TWO_KNOTS)]);

        serde_json::json!({
            "reorder_stitch:missing-file": refusal_message(
                "reorder_stitch (missing file)",
                &main.reorder_stitch("ghost.ink", "hello", "a", 1),
            ),
            "reorder_stitch:missing-stitch": refusal_message(
                "reorder_stitch (missing stitch)",
                &two.reorder_stitch("main.ink", "one", "nowhere", 1),
            ),
            "reorder_knot:missing-knot": refusal_message(
                "reorder_knot (missing knot)",
                &two.reorder_knot("main.ink", "nowhere", 1),
            ),
            "reorder_stitches:invalid-order": refusal_message(
                "reorder_stitches (invalid order)",
                &two.reorder_stitches("main.ink", "one", vec!["a".to_owned()]),
            ),
            "reorder_knots:invalid-order": refusal_message(
                "reorder_knots (invalid order)",
                &two.reorder_knots("main.ink", vec!["one".to_owned(), "one".to_owned()]),
            ),
            "move_stitch:missing-dest-knot": refusal_message(
                "move_stitch (missing destination knot)",
                &two.move_stitch("main.ink", "one", "a", "nope"),
            ),
            "move_stitch:name-collision": refusal_message(
                "move_stitch (name collision)",
                &two.move_stitch("main.ink", "one", "a", "two"),
            ),
            "promote_stitch:name-collision": refusal_message(
                "promote_stitch (name collision)",
                &two.promote_stitch("main.ink", "one", "two"),
            ),
            "promote_stitch:missing-stitch": refusal_message(
                "promote_stitch (missing stitch)",
                &two.promote_stitch("main.ink", "one", "nowhere"),
            ),
            "demote_knot:illegal-nesting": refusal_message(
                "demote_knot (illegal nesting)",
                &two.demote_knot("main.ink", "one", "two"),
            ),
            "demote_knot:missing-dest-knot": refusal_message(
                "demote_knot (missing destination knot)",
                &two.demote_knot("main.ink", "two", "nope"),
            ),
        })
    }

    /// The whole driven map: doc-handle vocabulary plus every other site.
    fn driven_messages() -> serde_json::Value {
        merge_driven(&[driven_doc_handle_messages(), driven_op_messages()])
    }

    // ── Acceptance: the `ok` FLAG, driven out of production (#2661) ──

    /// An op's answer reduced to the two things a mock can get wrong about
    /// *whether* it happened: the `ok` flag, and the `error` beside it (`null`
    /// on success).
    ///
    /// Deliberately NOT [`refusal_message`]: that helper asserts `ok: false`
    /// up front, because a message harvested from a success is not a refusal's
    /// wording. This one records whichever answer production gave, which is
    /// the point — a case that production ACCEPTS is evidence too, and the
    /// class #2661 is about is a mock answering `ok: true` where production
    /// refuses (and its mirror image, a mock refusing what production accepts).
    fn outcome(site: &str, json: &str) -> serde_json::Value {
        let value = parse(json);
        let ok = value["ok"].as_bool();
        assert!(
            ok.is_some(),
            "`{site}` answered no `ok` flag at all, so there is no acceptance to \
             pin: {value:#}"
        );
        let error = value
            .get("error")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        assert!(
            ok != Some(false) || error.is_string(),
            "`{site}` refused without an `error` string: {value:#}"
        );
        serde_json::json!({ "ok": ok, "error": error })
    }

    /// The UTF-16 offset of `marker` in `source`. The fixtures are ASCII, so
    /// this is also the byte offset; both sides derive their offsets from the
    /// (byte-identical) source text rather than typing a number twice.
    fn at(source: &str, marker: &str) -> u32 {
        let idx = source.find(marker);
        assert!(
            idx.is_some(),
            "marker {marker:?} is not in the source fixture — the driver is \
             pointing at text that no longer exists"
        );
        u32::try_from(idx.expect("just asserted above")).expect("fixtures are tiny")
    }

    /// The outline-reshaping ops' ACCEPTANCE, driven the same way the messages
    /// are (#2661).
    ///
    /// Every case here is an (op, input) pair where the studio mock's own
    /// resolution could disagree with production's about whether the op runs
    /// at all. Most of them turn on the same thing: production resolves knots
    /// through `brink_syntax`'s `tree.knots()`, which yields a `function` knot
    /// like any other, while the mock resolves them with a header regex.
    fn driven_outline_acceptance() -> serde_json::Value {
        let function_only = session_with(&[("main.ink", FUNCTION_KNOT)]);
        let mixed = session_with(&[("main.ink", KNOT_AND_FUNCTION)]);
        let stitchless = session_with(&[("main.ink", STITCHLESS_KNOTS)]);
        let shadowed = session_with(&[("main.ink", STITCH_SHADOWS_FUNCTION)]);
        let two = session_with(&[("main.ink", TWO_KNOTS)]);

        serde_json::json!({
            // A file whose only knot is a function knot: production finds it,
            // and a "move the last knot down" is a no-op it ACCEPTS.
            "reorder_knot:function-knot": outcome(
                "reorder_knot (the only knot is a function knot)",
                &function_only.reorder_knot("main.ink", "greet", 1),
            ),
            // Two knots exist here, so a one-name order is not a permutation.
            "reorder_knots:function-knot-counted": outcome(
                "reorder_knots (a function knot counts toward the permutation)",
                &mixed.reorder_knots("main.ink", vec!["one".to_owned()]),
            ),
            // ...and the full permutation is accepted.
            "reorder_knots:function-knot-permuted": outcome(
                "reorder_knots (permuting a plain knot with a function knot)",
                &mixed.reorder_knots("main.ink", vec!["greet".to_owned(), "one".to_owned()]),
            ),
            // A knot with a body but no stitches: production short-circuits to
            // `Ok(source)` BEFORE the permutation is resolved, so even a
            // nonsense order is accepted rather than refused.
            "reorder_stitches:stitchless-knot": outcome(
                "reorder_stitches (knot carries no stitches)",
                &stitchless.reorder_stitches("main.ink", "one", vec!["nope".to_owned()]),
            ),
            // The function knot resolves, so this refuses on the STITCH, not
            // on the knot — a different `MoveError` from "source knot not found".
            "reorder_stitch:function-knot": outcome(
                "reorder_stitch (stitch inside a function knot)",
                &mixed.reorder_stitch("main.ink", "greet", "a", 1),
            ),
            "move_stitch:into-function-knot": outcome(
                "move_stitch (destination is a function knot)",
                &mixed.move_stitch("main.ink", "one", "a", "greet"),
            ),
            // The collision check runs over every top-level knot, so the
            // function knot `greet` blocks promoting a stitch of that name.
            "promote_stitch:collides-with-function-knot": outcome(
                "promote_stitch (name taken by a function knot)",
                &shadowed.promote_stitch("main.ink", "one", "greet"),
            ),
            "demote_knot:function-knot-source": outcome(
                "demote_knot (demoting a function knot)",
                &mixed.demote_knot("main.ink", "greet", "one"),
            ),
            // Both knots resolve, so production reaches the nesting check —
            // `one` has a stitch — instead of refusing on the destination.
            "demote_knot:function-knot-dest": outcome(
                "demote_knot (destination is a function knot)",
                &mixed.demote_knot("main.ink", "one", "greet"),
            ),
            // Positive controls: the guards above must not have been bought by
            // refusing (or accepting) everything.
            "reorder_stitch:accepted": outcome(
                "reorder_stitch (ordinary success)",
                &two.reorder_stitch("main.ink", "one", "a", 1),
            ),
            "move_stitch:accepted": outcome(
                "move_stitch (ordinary success)",
                &two.move_stitch("main.ink", "one", "b", "two"),
            ),
        })
    }

    /// The ACCEPTANCE half of #2721's two indent-bearing coverage gaps —
    /// whether each op even RUNS on these inputs, driven the same way
    /// [`driven_outline_acceptance`] is (#2661). The `new_source` payload
    /// half lives in [`driven_payloads`] (and, for `delete_symbol`,
    /// deliberately does not — see [`driven_stitch_regions`]'s doc).
    ///
    /// Gap 1: `delete_symbol`/`rename_symbol` were never driven on
    /// [`INDENTED_FIRST_KNOT`] at all. #2703 established that neither op
    /// reads `parseOutline`'s ranges — `delete_symbol` runs its own
    /// line-based scan and `rename_symbol` substitutes the declared name
    /// token in place — so #2713's `full_start`/`payloads` fix for the
    /// seven `dispatchSymbolAction` ops says nothing about whether either of
    /// these two agrees with production here.
    ///
    /// Gap 2: `move_stitch`/`promote_stitch`/`demote_knot` were never driven
    /// on [`ALT_STITCHES`]'s indented `  = b`, the input shape #2703's fix
    /// was made for, though the other four `dispatchSymbolAction` ops were.
    ///
    /// Gap 3 (#2730, follow-up from #2725's review): `ALT_STITCHES`'s `  = b`
    /// lands `move_stitch`/`demote_knot`'s insertion point right after a byte
    /// that is already `\n` (see [`driven_payloads`]'s doc), so Gap 2's two
    /// cases above cannot exercise either op's `needs_newline_before`/
    /// `needs_nl` guard (`structural_move.rs`'s `move_stitch` and
    /// `demote_knot_to_stitch`) at all — only `promote_stitch_to_knot`'s
    /// analogous guard got driven somewhere that mattered, by
    /// `promote_stitch:alt-stitch-indented` on the SAME source. [`ALT_FENCES`]'s
    /// indented `  ==== four ====` is the input that does stress it: it glues
    /// its own leading whitespace onto knot `three`'s trailing trivia (same
    /// mechanism, #2703), leaving `three`'s region ending in a bare `"  "`
    /// with no trailing newline — so inserting INTO `three` is where a missing
    /// guard would show. This is acceptance only (`ok`/`error`, both sides
    /// agree here); [`driven_payloads`]'s pair of the same name carries the
    /// `new_source` byte-level answer the guard actually changes.
    fn driven_indent_acceptance() -> serde_json::Value {
        let delete_indented_first = session_with(&[("main.ink", INDENTED_FIRST_KNOT)]);
        let rename_indented_first = session_with(&[("main.ink", INDENTED_FIRST_KNOT)]);
        let move_indented_stitch = session_with(&[("main.ink", ALT_STITCHES)]);
        let promote_indented_stitch = session_with(&[("main.ink", ALT_STITCHES)]);
        let demote_indented_stitch = session_with(&[("main.ink", ALT_STITCHES)]);
        let move_into_alt_fence_boundary = session_with(&[("main.ink", ALT_FENCES)]);
        let demote_into_alt_fence_boundary = session_with(&[("main.ink", ALT_FENCES)]);

        serde_json::json!({
            "delete_symbol:indented-first-knot": outcome(
                "delete_symbol (source's FIRST knot header is itself indented)",
                &delete_indented_first.delete_symbol("main.ink", "one", ""),
            ),
            "rename_symbol:indented-first-knot": outcome(
                "rename_symbol (source's FIRST knot header is itself indented)",
                &rename_indented_first.rename_symbol("main.ink", "one", "", "renamed"),
            ),
            "move_stitch:alt-stitch-indented": outcome(
                "move_stitch (moving ALT_STITCHES's indented `  = b` itself)",
                &move_indented_stitch.move_stitch("main.ink", "one", "b", "two"),
            ),
            "promote_stitch:alt-stitch-indented": outcome(
                "promote_stitch (promoting ALT_STITCHES's indented `  = b`)",
                &promote_indented_stitch.promote_stitch("main.ink", "one", "b"),
            ),
            "demote_knot:alt-stitch-indented": outcome(
                "demote_knot (demoting `two` into `one`, landing after the indented `  = b`)",
                &demote_indented_stitch.demote_knot("main.ink", "two", "one"),
            ),
            "move_stitch:alt-fence-three-boundary": outcome(
                "move_stitch (moving `one`'s stitch `a` into `three`, whose region ends \
                 in a bare two-space indent glued from `four`'s header)",
                &move_into_alt_fence_boundary.move_stitch("main.ink", "one", "a", "three"),
            ),
            "demote_knot:alt-fence-three-boundary": outcome(
                "demote_knot (demoting `two` into `three`, whose region ends in a bare \
                 two-space indent glued from `four`'s header)",
                &demote_into_alt_fence_boundary.demote_knot("main.ink", "two", "three"),
            ),
        })
    }

    /// `extract_to_knot` / `extract_to_function` acceptance (#2661).
    ///
    /// `brink_ide::extract::ExtractError` has EIGHT variants; the studio mock
    /// modelled three of them, so five production refusals answered `ok: true`
    /// under test. Offsets are derived from the (fixture-shipped) source text
    /// via [`at`] so the TypeScript mirror can compute the same window without
    /// a hand-typed number.
    fn driven_extract_acceptance() -> serde_json::Value {
        let main = session_with(&[("main.ink", MAIN)]);
        let two = session_with(&[("main.ink", TWO_KNOTS)]);
        let vars = session_with(&[("main.ink", VAR_AND_KNOT)]);
        let blank = session_with(&[("main.ink", BLANK_BODY)]);
        let leading_blank = session_with(&[("main.ink", LEADING_BLANK_LINE)]);

        let hi = at(MAIN, "Hi.");
        let end_divert = at(MAIN, "-> END");
        let first = at(VAR_AND_KNOT, "First.");
        // The blank line sits immediately after the first `\n\n`.
        let blank_line = at(BLANK_BODY, "\n\n") + 1;

        serde_json::json!({
            // The snapped window swallows knot `two`'s header line.
            "extract_to_knot:crosses-header": outcome(
                "extract_to_knot (selection crosses a knot header)",
                &two.extract_to_knot("main.ink", at(TWO_KNOTS, "B."), at(TWO_KNOTS, "Second."), "lifted"),
            ),
            "extract_to_knot:knot-name-collision": outcome(
                "extract_to_knot (name already a top-level knot)",
                &main.extract_to_knot("main.ink", hi, hi + 3, "hello"),
            ),
            "extract_to_knot:var-collision": outcome(
                "extract_to_knot (name already a VAR)",
                &vars.extract_to_knot("main.ink", first, first + 6, "score"),
            ),
            "extract_to_knot:invalid-name": outcome(
                "extract_to_knot (name is not an identifier)",
                &main.extract_to_knot("main.ink", hi, hi + 3, "1bad"),
            ),
            // Non-empty in offsets, whitespace-only in content.
            "extract_to_knot:blank-selection": outcome(
                "extract_to_knot (selection is a blank line)",
                &blank.extract_to_knot("main.ink", blank_line, blank_line + 1, "lifted"),
            ),
            "extract_to_knot:accepted": outcome(
                "extract_to_knot (ordinary success)",
                &main.extract_to_knot("main.ink", hi, hi + 3, "lifted"),
            ),
            // Review finding on #2670: a source whose FIRST character is a
            // blank line, selected at [0, 1). `snap_to_lines`'s `lo == 0`
            // never finds a preceding newline (start stays 0), so the
            // snapped window is just that leading blank line — empty
            // content, not a real extraction.
            "extract_to_knot:leading-blank-line": outcome(
                "extract_to_knot (selection at offset 0, source starts with a blank line)",
                &leading_blank.extract_to_knot("main.ink", 0, 1, "lifted"),
            ),
            // Functions cannot divert; `-> END` is flow control.
            "extract_to_function:flow-control": outcome(
                "extract_to_function (selection contains a divert)",
                &main.extract_to_function("main.ink", end_divert, end_divert + 6, "lifted"),
            ),
            "extract_to_function:invalid-name": outcome(
                "extract_to_function (name is not an identifier)",
                &main.extract_to_function("main.ink", hi, hi + 3, "1bad"),
            ),
            "extract_to_function:var-collision": outcome(
                "extract_to_function (name already a VAR)",
                &vars.extract_to_function("main.ink", first, first + 6, "score"),
            ),
            "extract_to_function:accepted": outcome(
                "extract_to_function (ordinary success)",
                &main.extract_to_function("main.ink", hi, hi + 3, "lifted"),
            ),
        })
    }

    /// `extract_to_function`'s call-FORM choice, the half `acceptance` cannot
    /// see (#2675 Gap A).
    ///
    /// Both cases below answer `ok: true` on both sides — `acceptance`'s
    /// `outcome()` is structurally blind to them. `extract.rs::
    /// is_value_expression` picks `{name()}` for a single inline value
    /// expression and `~ name()` for anything else (a statement, a multi-line
    /// selection, ...), but the studio mock ALWAYS emitted `~ name()`. The
    /// value-expression case reuses `extract_to_function:accepted`'s own
    /// input (`MAIN`, selecting `"Hi."`, name `lifted`) rather than a new
    /// source — that selection already IS a single-line value expression, so
    /// the divergence was sitting in an already-driven case the whole time,
    /// invisible only because nothing looked past `ok`.
    fn driven_call_forms() -> serde_json::Value {
        let main = session_with(&[("main.ink", MAIN)]);
        let mixed = session_with(&[("main.ink", KNOT_AND_FUNCTION)]);
        let hi = at(MAIN, "Hi.");
        let stmt = at(KNOT_AND_FUNCTION, "~ return");
        let stmt_end = stmt + u32::try_from("~ return \"hi\"".len()).expect("fixture is tiny");

        serde_json::json!({
            "extract_to_function:value-expression": call_line(
                "extract_to_function (single-line value-expression selection)",
                &main.extract_to_function("main.ink", hi, hi + 3, "lifted"),
                "lifted",
            ),
            "extract_to_function:statement": call_line(
                "extract_to_function (`~`-statement selection)",
                &mixed.extract_to_function("main.ink", stmt, stmt_end, "lifted2"),
                "lifted2",
            ),
        })
    }

    /// The single line of a successful extraction's `new_source` that calls
    /// `name` — `{name()}` or `~ name()` — the call-form half
    /// [`driven_call_forms`] pins.
    fn call_line(site: &str, json: &str, name: &str) -> String {
        let value = parse(json);
        assert!(
            value["ok"] == serde_json::json!(true),
            "`{site}` refused, so there is no call line to read: {value:#}"
        );
        let source = value["new_source"].as_str().unwrap_or_default();
        // The call site is `{name()}` or `~ name()` — never `=== function
        // name() ===`, which also contains the substring `name(` and would
        // otherwise double-match the appended declaration.
        let value_form = format!("{{{name}()}}");
        let stmt_form = format!("~ {name}()");
        let lines: Vec<&str> = source
            .lines()
            .map(str::trim)
            .filter(|l| *l == value_form || *l == stmt_form)
            .collect();
        assert!(
            lines.len() == 1,
            "`{site}` produced {} line(s) matching the call form of `{name}`, so \
             there is no single call line to pin: {lines:?}",
            lines.len()
        );
        (*lines.first().expect("just asserted above")).to_owned()
    }

    /// `new_source` payload fidelity for the outline-reshaping ops, beyond the
    /// `ok`/`error` flag `acceptance` pins (#2675 Gap C, #2685 Gap 3).
    ///
    /// `acceptance` cannot see a wrong-but-successful rewrite — the same shape
    /// `driven_stitch_regions` closed for `delete_symbol`'s ONE case. This is
    /// the same mechanism for `reorder_knots`/`reorder_stitches`/`move_stitch`,
    /// run against the SAME alt-fenced/alt-stitched sources
    /// `driven_fence_acceptance`/`driven_stitch_acceptance` already exercise
    /// for `ok`/`error` alone: `ALT_FENCES`'s indented `  ==== four ====` and
    /// `ALT_STITCHES`'s indented `  = b` are exactly where a symbol's
    /// OWNERSHIP RANGE can disagree with production without any acceptance
    /// case noticing, because production's parser attaches a header's leading
    /// indent to the PRECEDING symbol's trailing trivia (`knot_body`'s
    /// `skip_ws()` runs before the at-knot/at-stitch check that ends the
    /// loop), not to the indented header's own leading edge — driven and
    /// confirmed against `document_symbols`, not read off the grammar.
    ///
    /// #2706 extends the same fidelity to the four `dispatchSymbolAction` ops
    /// #2703 did not reach — `demote_knot`, `promote_stitch`, `reorder_knot`
    /// (singular), `reorder_stitch` (singular) — including a full
    /// `new_source` pin for `demote_knot:alt-fence-knot` (previously pinned
    /// only on its header line, in [`driven_header_rewrites`]). It also adds
    /// `reorder_knots:indented-first-knot`, driving [`INDENTED_FIRST_KNOT`]'s
    /// leading-indent-on-the-first-knot question end-to-end: whether that
    /// indent stays behind as untouched preamble when the knot it visually
    /// belongs to moves out of first place.
    ///
    /// #2721 closes two coverage gaps #2713 left: `rename_symbol:indented-
    /// first-knot` (the sibling of `reorder_knots:indented-first-knot`, on
    /// the same [`INDENTED_FIRST_KNOT`] source) and three cases run on
    /// [`ALT_STITCHES`]'s indented `  = b` — `move_stitch:alt-stitch-
    /// indented`, `promote_stitch:alt-stitch-indented`,
    /// `demote_knot:alt-stitch-indented` — exercising the exact input shape
    /// #2703's `full_start` fix was made for, on the three
    /// `dispatchSymbolAction` ops #2713 had not yet driven there.
    /// `delete_symbol:indented-first-knot` is deliberately NOT among them —
    /// see [`driven_stitch_regions`]'s doc for why.
    ///
    /// #2730 (follow-up from #2725's review) adds
    /// `move_stitch:alt-fence-three-boundary` / `demote_knot:alt-fence-three-
    /// boundary`: #2725 found and fixed `promote_stitch_to_knot`'s missing
    /// `needs_newline_before`-style guard, and review found the SAME gap
    /// still open in `structural_move.rs`'s `move_stitch`
    /// (`needs_newline_before`) and `demote_knot_to_stitch` (`needs_nl`) —
    /// but the two `ALT_STITCHES` cases above happen to land right after a
    /// byte that is already `\n`, so neither exercises either guard. Moving
    /// INTO `three` does: `three`'s region ends in the bare `"  "` left
    /// behind by `four`'s indented header (same #2703 mechanism), so the
    /// byte immediately before the insertion point is a space, not `\n`.
    #[expect(
        clippy::too_many_lines,
        reason = "driven fixture table — one block per (op, input) pair; splitting it would scatter cases that are read together"
    )]
    fn driven_payloads() -> serde_json::Value {
        let alt_fences = session_with(&[("main.ink", ALT_FENCES)]);
        let alt_stitches = session_with(&[("main.ink", ALT_STITCHES)]);
        let two = session_with(&[("main.ink", TWO_KNOTS)]);
        let mixed = session_with(&[("main.ink", KNOT_AND_FUNCTION)]);
        let function_only = session_with(&[("main.ink", FUNCTION_KNOT)]);
        let indented_first = session_with(&[("main.ink", INDENTED_FIRST_KNOT)]);
        let indented_start = at(INDENTED_LINE, "Indented.");
        let indented_end =
            indented_start + u32::try_from("Indented.".len()).expect("fixture is tiny");

        serde_json::json!({
            "reorder_knots:alt-fences": payload_source(
                "reorder_knots (knots fenced ==, ===tight, ====, indented ====, bare ===)",
                &alt_fences.reorder_knots(
                    "main.ink",
                    vec![
                        "five".to_owned(),
                        "four".to_owned(),
                        "three".to_owned(),
                        "two".to_owned(),
                        "one".to_owned(),
                    ],
                ),
            ),
            "reorder_stitches:alt-stitches": payload_source(
                "reorder_stitches (stitches `= a`, indented `  = b`, tight `=c`)",
                &alt_stitches.reorder_stitches(
                    "main.ink",
                    "one",
                    vec!["b".to_owned(), "c".to_owned(), "a".to_owned()],
                ),
            ),
            "move_stitch:accepted": payload_source(
                "move_stitch (ordinary success)",
                &two.move_stitch("main.ink", "one", "b", "two"),
            ),
            // #2706: the four ops #2703 did not reach. `alt-fence-knot` reuses
            // the same (op, input) pair `driven_header_rewrites` already
            // pins the header line for — this is the SAME fidelity gap
            // #2703 fixed for the other three ops, present here in a fixture
            // #2703 itself introduced.
            "demote_knot:alt-fence-knot": payload_source(
                "demote_knot (source knot fenced with four `=`, into `one`)",
                &alt_fences.demote_knot("main.ink", "three", "one"),
            ),
            "demote_knot:function-knot-source": payload_source(
                "demote_knot (demoting a function knot)",
                &mixed.demote_knot("main.ink", "greet", "one"),
            ),
            "promote_stitch:alt-fence-terse": payload_source(
                "promote_stitch (stitch under a `==`-fenced knot)",
                &alt_fences.promote_stitch("main.ink", "one", "a"),
            ),
            // Review finding on #2706: the `function-knot` case below pins the
            // OUT-OF-RANGE CLAMP (the single-knot source has no `target` to
            // move to, so `structuralOk(path, source)` returns it unchanged)
            // — it never exercises `planKnots`/`renderKnots`'s actual swap.
            // This case drives the real move: `one` past `two` on `TWO_KNOTS`,
            // which carries stitches on both sides, so the head+stitches
            // reassembly the payloads map exists to pin is inside this
            // singular op's pin too.
            "reorder_knot:two-knots": payload_source(
                "reorder_knot (ordinary success — the only other knot is past it)",
                &two.reorder_knot("main.ink", "one", 1),
            ),
            "reorder_knot:function-knot": payload_source(
                "reorder_knot (the only knot is a function knot — out-of-range clamp)",
                &function_only.reorder_knot("main.ink", "greet", 1),
            ),
            "reorder_stitch:accepted": payload_source(
                "reorder_stitch (ordinary success)",
                &two.reorder_stitch("main.ink", "one", "a", 1),
            ),
            // #2706: the indented-FIRST-knot question, driven end-to-end.
            // `one`'s two-space indent has no preceding SYMBOL to glue to
            // (unlike `ALT_FENCES`'s `four`/`ALT_STITCHES`'s `b`) — moving
            // `one` out of first place is the only way to see whether the
            // indent stayed behind as untouched file preamble (matching
            // `structural_move.rs`'s `decl_region_start`) or moved with the
            // knot.
            "reorder_knots:indented-first-knot": payload_source(
                "reorder_knots (source's FIRST knot header is itself indented)",
                &indented_first
                    .reorder_knots("main.ink", vec!["two".to_owned(), "one".to_owned()]),
            ),
            // #2721 Gap 1: `rename_symbol` never rewrites the header's leading
            // trivia at all — it substitutes the declared NAME token in place
            // (`brink_ide::rename::rename_safe`), so the first-knot indent
            // question `reorder_knots:indented-first-knot` answers for a
            // region MOVE has no analogue here to diverge on. Driven rather
            // than assumed, per the same #2703/#2685 Gap 3 lesson every other
            // entry in this map follows.
            "rename_symbol:indented-first-knot": payload_source(
                "rename_symbol (source's FIRST knot header is itself indented)",
                &indented_first.rename_symbol("main.ink", "one", "", "renamed"),
            ),
            // #2721 Gap 2: `move_stitch`/`promote_stitch`/`demote_knot` driven
            // on ALT_STITCHES's indented `  = b` — precisely the input shape
            // #2703's `full_start` fix matters for, on the three
            // `dispatchSymbolAction` ops #2713's payload coverage did not
            // reach with it. All three resolve `b`'s ownership region through
            // `decl_region_start`/`full_start` exactly as `reorder_stitches:
            // alt-stitches` above already does, so this end-to-end pin is
            // confirmation the fix generalizes to these three ops too, not a
            // new mechanism.
            "move_stitch:alt-stitch-indented": payload_source(
                "move_stitch (moving ALT_STITCHES's indented `  = b` itself)",
                &alt_stitches.move_stitch("main.ink", "one", "b", "two"),
            ),
            "promote_stitch:alt-stitch-indented": payload_source(
                "promote_stitch (promoting ALT_STITCHES's indented `  = b`)",
                &alt_stitches.promote_stitch("main.ink", "one", "b"),
            ),
            // `two` has no stitches of its own, so demoting it into `one`
            // lands the new stitch immediately AFTER `one`'s existing last
            // stitch — the indented `b` — rather than colliding with the
            // nesting/collision checks another source would trip first.
            "demote_knot:alt-stitch-indented": payload_source(
                "demote_knot (demoting `two` into `one`, landing after the indented `  = b`)",
                &alt_stitches.demote_knot("main.ink", "two", "one"),
            ),
            // #2730 (follow-up from #2725's review): `move_stitch`/
            // `demote_knot` on ALT_STITCHES's `  = b` above both land right
            // after a byte that is already `\n`, so neither case can exercise
            // `structural_move.rs`'s `needs_newline_before` (`move_stitch`) /
            // `needs_nl` (`demote_knot_to_stitch`) guard — the exact defect
            // class #2725 found and fixed for `promote_stitch_to_knot`'s own
            // guard. `ALT_FENCES`'s indented `  ==== four ====` glues its
            // leading whitespace onto knot `three`'s trailing trivia (#2703),
            // leaving `three`'s region ending in a bare `"  "` with no
            // trailing newline — inserting INTO `three` is where a missing
            // guard would actually show up in `new_source`. Hand-verified
            // against `structural_move.rs`: both guards fire here (the byte
            // before the insertion point is a space, not `\n`), so production
            // inserts a separating `\n` that a mock without the matching
            // guard would drop.
            "move_stitch:alt-fence-three-boundary": payload_source(
                "move_stitch (moving `one`'s stitch `a` into `three`'s non-newline-\
                 terminated region)",
                &alt_fences.move_stitch("main.ink", "one", "a", "three"),
            ),
            "demote_knot:alt-fence-three-boundary": payload_source(
                "demote_knot (demoting `two` into `three`'s non-newline-terminated \
                 region)",
                &alt_fences.demote_knot("main.ink", "two", "three"),
            ),
            // Review finding on #2675 Gap C: extract.rs's call-line indent
            // (`extract.rs:121-125`, `plan.indent` prefixing the call) and
            // body dedent (`extract.rs:246`, `dedent(&plan.selected)`) were
            // both structurally invisible to `call_forms` — its own
            // `call_line` helper `.map(str::trim)`s before matching, so an
            // indent bug on the call line cannot show up there, and neither
            // helper looks at the appended body at all. `new_source` payload
            // fidelity is the only place either is visible.
            "extract_to_function:indented": payload_source(
                "extract_to_function (indented selected line — call-line indent + body dedent)",
                &session_with(&[("main.ink", INDENTED_LINE)]).extract_to_function(
                    "main.ink",
                    indented_start,
                    indented_end,
                    "lifted3",
                ),
            ),
            "extract_to_knot:indented": payload_source(
                "extract_to_knot (indented selected line — call-line indent + body dedent)",
                &session_with(&[("main.ink", INDENTED_LINE)]).extract_to_knot(
                    "main.ink",
                    indented_start,
                    indented_end,
                    "lifted4",
                ),
            ),
        })
    }

    /// Alias for [`deleted_source`] (defined further below, alongside
    /// `driven_stitch_regions`) — the same "must have succeeded" contract,
    /// used by [`driven_payloads`]'s non-delete sites. Named separately
    /// because "deleted" would misdescribe a reorder/move's `new_source`.
    fn payload_source(site: &str, json: &str) -> String {
        deleted_source(site, json)
    }

    /// The knot-header VOCABULARY, driven through both mock families (#2662).
    ///
    /// Every case runs against [`ALT_FENCES`], whose five knots are fenced
    /// `==`, `===` (no spaces), `====`, an indented `====`, and a bare `===`
    /// with no closing fence. Production resolves all five through
    /// `tree.knots()`; the mock had two narrower recognizers, so the same
    /// source answered differently depending on which op was called:
    ///
    /// | op family | recognizer | `== one ==` | `===two===` | `==== three ====` | `  ==== four ====` | `=== five` |
    /// |---|---|---|---|---|---|---|
    /// | the seven outline ops (`parseOutline`) | `^===\s+` | invisible | invisible | invisible | invisible | invisible |
    /// | `delete_symbol` / `rename_symbol` (inline) | `^\s*={2,3}\s*` | resolves | resolves | invisible | invisible | resolves |
    ///
    /// Both families are driven here on purpose. A fixture that only pinned
    /// `== one ==` would be green on the inline half and read as though the
    /// split had one victim, which is how the two recognizers stayed out of
    /// step across #2658 and #2670. `four` (indented) and `five` (bare
    /// trailing fence) are the two widenings nothing before them drove
    /// (review finding on #2662): every other fixture uses a flush-left
    /// header with a closing fence, so `KNOT_HEADER_PREFIX`'s `^\s*` and
    /// `KNOT_HEADER_RE`'s trailing `(?:={2,})?` could both regress to
    /// something narrower without any case here going red.
    fn driven_fence_acceptance() -> serde_json::Value {
        let outline_family = session_with(&[("main.ink", ALT_FENCES)]);
        let promote = session_with(&[("main.ink", ALT_FENCES)]);
        let delete_wide = session_with(&[("main.ink", ALT_FENCES)]);
        let delete_terse = session_with(&[("main.ink", ALT_FENCES)]);
        let rename_wide = session_with(&[("main.ink", ALT_FENCES)]);

        serde_json::json!({
            // The `parseOutline` family. Production counts FIVE knots, so
            // this order is a permutation; a mock that sees fewer than five
            // of them cannot answer `ok: true` here however it words its
            // refusal — the indented and bare-fence knots included.
            "reorder_knots:alt-fences": outcome(
                "reorder_knots (knots fenced ==, ===tight, ====, indented ====, and bare ===)",
                &outline_family.reorder_knots(
                    "main.ink",
                    vec![
                        "five".to_owned(),
                        "four".to_owned(),
                        "three".to_owned(),
                        "two".to_owned(),
                        "one".to_owned(),
                    ],
                ),
            ),
            // A second op on that family, so the case is not carried by
            // `reorder_knots`'s permutation check alone: `one` really does own
            // stitch `a`, and promoting it is an ordinary success.
            "promote_stitch:alt-fence-terse": outcome(
                "promote_stitch (stitch under a `==`-fenced knot)",
                &promote.promote_stitch("main.ink", "one", "a"),
            ),
            // The inline family. `====` is where `={2,3}` runs out.
            "delete_symbol:alt-fence-wide": outcome(
                "delete_symbol (knot fenced with four `=`)",
                &delete_wide.delete_symbol("main.ink", "three", ""),
            ),
            "rename_symbol:alt-fence-wide": outcome(
                "rename_symbol (knot fenced with four `=`)",
                &rename_wide.rename_symbol("main.ink", "three", "", "renamed"),
            ),
            // Positive control for the inline family: the two-`=` fence it
            // already resolved must keep resolving, so the widening is not
            // bought by making everything match.
            "delete_symbol:alt-fence-terse": outcome(
                "delete_symbol (knot fenced with two `=`)",
                &delete_terse.delete_symbol("main.ink", "one", ""),
            ),
        })
    }

    /// The stitch-header VOCABULARY, driven through both mock families
    /// (#2684) — the sibling of [`driven_fence_acceptance`] one rung down.
    ///
    /// Every case runs against [`ALT_STITCHES`]. `c` (`=c`, no whitespace) is
    /// the shape BOTH mock families missed: `parseOutline`'s `^=\s+` and the
    /// inline guards' `^\s*=\s+` each required a space production does not.
    /// `b` (`  = b`, indented) is the shape only the outline family missed,
    /// which is the point of driving both — a case list covering one family
    /// would read as though the split had a single victim, exactly the way
    /// #2662's did before its review.
    ///
    /// `a` is the positive control: the one flush-left `= name` shape every
    /// family already resolved. It has to stay green, so the widening cannot
    /// be bought by making everything match.
    fn driven_stitch_acceptance() -> serde_json::Value {
        let outline_family = session_with(&[("main.ink", ALT_STITCHES)]);
        let delete_tight = session_with(&[("main.ink", ALT_STITCHES)]);
        let rename_tight = session_with(&[("main.ink", ALT_STITCHES)]);
        let delete_indented = session_with(&[("main.ink", ALT_STITCHES)]);
        let delete_plain = session_with(&[("main.ink", ALT_STITCHES)]);

        serde_json::json!({
            // The `parseOutline` family. Production counts THREE stitches
            // under `one`, so this order is a permutation; a mock that sees
            // fewer than three cannot answer `ok: true` here however it words
            // its refusal.
            "reorder_stitches:alt-stitches": outcome(
                "reorder_stitches (stitches `= a`, indented `  = b`, tight `=c`)",
                &outline_family.reorder_stitches(
                    "main.ink",
                    "one",
                    vec!["b".to_owned(), "c".to_owned(), "a".to_owned()],
                ),
            ),
            // The inline family. `=c` is where the required `\s+` runs out.
            "delete_symbol:alt-stitch-tight": outcome(
                "delete_symbol (stitch declared `=c`, no whitespace)",
                &delete_tight.delete_symbol("main.ink", "one", "c"),
            ),
            "rename_symbol:alt-stitch-tight": outcome(
                "rename_symbol (stitch declared `=c`, no whitespace)",
                &rename_tight.rename_symbol("main.ink", "one", "c", "renamed"),
            ),
            // Indented: resolved by the inline family before #2684, invisible
            // to the outline family. Driving it here keeps the asymmetry on
            // the record rather than letting one family's green stand in.
            "delete_symbol:alt-stitch-indented": outcome(
                "delete_symbol (stitch declared `  = b`, indented)",
                &delete_indented.delete_symbol("main.ink", "one", "b"),
            ),
            // Positive control for both families: the plain `= a` shape.
            "delete_symbol:alt-stitch-plain": outcome(
                "delete_symbol (stitch declared `= a`)",
                &delete_plain.delete_symbol("main.ink", "one", "a"),
            ),
        })
    }

    /// The whole driven acceptance map.
    fn driven_acceptance() -> serde_json::Value {
        merge_driven(&[
            driven_outline_acceptance(),
            driven_extract_acceptance(),
            driven_fence_acceptance(),
            driven_stitch_acceptance(),
            driven_indent_acceptance(),
        ])
    }

    /// The stitch OWNERSHIP REGION, read out of production's own `new_source`
    /// (#2684) — the half neither `acceptance` nor `outlines` can see.
    ///
    /// A stitch's region runs to the next header of any level, and `opensHeader`
    /// is the mock's answer to "is this line one". Acceptance cannot see a
    /// wrong answer: `delete_symbol` reports `ok: true` either way and simply
    /// removes a shorter span, so the op succeeds with the wrong content.
    ///
    /// The cases that matter are `=> x` and `= > y`. Production's
    /// `stitch_body` breaks on `at_knot(p) || at_stitch(p)`, and `at_stitch`
    /// excludes a following `>` via the trivia-SKIPPING `nth(1) != GT` — so
    /// neither line ends a stitch, and deleting `a` takes all four lines with
    /// it. The mock's `opensHeader` was a bare `^\s*=` with no such lookahead,
    /// so it stopped at `=> x` and left four orphaned lines behind.
    ///
    /// The single case carries BOTH directions, which is why one is enough:
    /// stitch `a`'s body holds `=> x` and `= > y` (must NOT end the region)
    /// and is followed by the tight header `=c` (must end it). An
    /// `opensHeader` that answered `true` too often stops at `=> x` and leaves
    /// orphan lines; one that answered `false` too often runs past `=c` and
    /// swallows the next stitch; one whose lookahead forgot that `nth(1)`
    /// skips trivia stops at `= > y`. Only the correct vocabulary produces the
    /// pinned string, so the widening cannot be bought by making everything
    /// (or nothing) match.
    ///
    /// ⚠ Only a boundary where a LINE start and a CST NODE start coincide can
    /// be pinned here — see [`ALT_STITCHES`]'s note on the indented `  = b`.
    fn driven_stitch_regions() -> serde_json::Value {
        let spans_arrow = session_with(&[("main.ink", ALT_STITCHES)]);

        serde_json::json!({
            "delete_symbol:alt-stitch-plain": deleted_source(
                "delete_symbol (stitch `a`: body carries `=> x`, boundary is `=c`)",
                &spans_arrow.delete_symbol("main.ink", "one", "a"),
            ),
            // NOT `delete_symbol:alt-stitch-indented` (review finding on
            // #2685 Gap 3, checked rather than assumed): `parseOutline`'s
            // `full_start` fix reattributes an indented header's leading
            // whitespace to the PRECEDING symbol's trailing trivia — which
            // fixes every op built ON `parseOutline` (the seven
            // `dispatchSymbolAction` ops, pinned in `outlines`/`payloads`) —
            // but the mock's `delete_symbol` does NOT go through
            // `parseOutline`'s ranges at all; it is its own independent
            // line-based scan (`lines.findIndex` + `opensHeader`) that
            // deletes the WHOLE physical line `  = b` sits on, indent
            // included. Driven and confirmed, not assumed: production's
            // answer for this exact call keeps the two-space indent behind,
            // glued onto the following line — `...C.\n  === two ===\n...` —
            // while the mock answers `...C.\n=== two ===\n...`. Pinning this
            // here would pin a mismatch as a match. The divergence stays
            // open — see the spec's "Not covered" note.
            //
            // NOT `delete_symbol:indented-first-knot` either (#2721 Gap 1,
            // same check rather than assumption): on `INDENTED_FIRST_KNOT`
            // (`"  === one ===\n..."`), `decl_region_start` — the same
            // function `full_start` is built from — places knot `one`'s
            // region at [2, 29), so production's `delete_symbol("one", "")`
            // keeps the two-space indent behind, glued onto the FOLLOWING
            // knot's header: `"  === two ===\nSecond.\n"`. The mock's
            // `delete_symbol` still deletes whole physical LINES — `lines
            // .findIndex` matches line 0 (`"  === one ==="`, `KNOT_HEADER_RE`
            // tolerates the indent), so the entire line, indent included, is
            // spliced out: `"=== two ===\nSecond.\n"`, no leading indent at
            // all. Same divergence class as the `  = b` case above, a
            // SECOND instance of it rather than a new one — driven and
            // recorded per #2721's ask, not decided; #2694 is still the open
            // ruling for both.
        })
    }

    /// The `new_source` of an op that must have SUCCEEDED — the surviving text
    /// after a region was removed.
    fn deleted_source(site: &str, json: &str) -> String {
        let value = parse(json);
        assert!(
            value["ok"] == serde_json::json!(true),
            "`{site}` refused, so there is no surviving source to pin: {value:#}"
        );
        let source = value["new_source"].as_str();
        assert!(
            source.is_some(),
            "`{site}` succeeded without a `new_source`: {value:#}"
        );
        source.expect("just asserted above").to_owned()
    }

    /// Production's own INITIAL `active_file()` (#2663).
    ///
    /// `EditorSession::new` seeds `active_path` with `"main.ink"`
    /// (`crates/brink-web/src/editor/mod.rs`), while the studio mock seeded it
    /// with `""`. Both reach `file not loaded` for a session that has loaded
    /// nothing, which is why #2635's driven `resolve_code_action` site stayed
    /// green over the divergence — but `update_source` writes into
    /// `files[activePath]`, so a mock session that never calls
    /// `set_active_file` wrote to key `""` where production writes to
    /// `"main.ink"`. Driven rather than typed, so a change to production's
    /// seed moves this fixture instead of silently un-aligning the mock again.
    fn driven_defaults() -> serde_json::Value {
        serde_json::json!({ "active_file": EditorSession::new().active_file() })
    }

    /// The OUTLINE production reports for a source: each symbol's `name`,
    /// `kind`, `detail`, and OWNERSHIP RANGE (`full_start`/`full_end`), nested
    /// (#2662, ranges added #2685 Gap 3).
    ///
    /// Acceptance pins whether an op runs; this pins the recognizer's own
    /// answer — which symbols `file_symbols` reports at all, and the exact
    /// span each one owns — for the studio's Binder, symbol menu and story
    /// graph, all of which read the mock's `parseOutline` through this entry
    /// point. The NAME range (`start`/`end`) is deliberately still left out —
    /// that one is pinned elsewhere, by the `#2670` offset guards.
    fn outline_of(source: &str) -> serde_json::Value {
        let session = session_with(&[("main.ink", source)]);
        let symbols = parse(&session.file_symbols("main.ink"));
        outline_shape(&symbols)
    }

    /// `name`/`kind`/`detail`/`full_start`/`full_end`/`children`, recursively.
    /// The NAME range (`start`/`end`) is the only thing left out — it is
    /// pinned elsewhere (the `#2670` offset guards).
    ///
    /// `detail` is NOT a range: it is `DocumentSymbolJs.detail`
    /// (`crates/brink-web/src/editor_dto.rs`), set to `Some("function")` for a
    /// function knot (`crates/internal/brink-ide/src/document.rs`). It is the
    /// field `KNOT_AND_FUNCTION` exists to control — `Binder.tsx`'s function
    /// marker renders off exactly `knot.detail === "function"` — so dropping
    /// it here would leave that control asserting nothing (review finding on
    /// #2662).
    ///
    /// `full_start`/`full_end` are the OWNERSHIP range every one of the seven
    /// structural ops slices by (#2685 Gap 3) — the #2670 offset guards cover
    /// only the NAME span, so a mock whose ownership boundary disagreed with
    /// production passed both that guard and #2682's fixture. `ALT_FENCES`'s
    /// indented `  ==== four ====` and `ALT_STITCHES`'s indented `  = b` are
    /// exactly where that boundary is non-obvious: production's parser
    /// attaches a header's leading indent to the PRECEDING symbol's trailing
    /// trivia, not to the indented header's own leading edge (driven and
    /// confirmed against `document_symbols`, not read off the grammar — see
    /// [`driven_payloads`] for the end-to-end consequence on a reorder).
    fn outline_shape(symbols: &serde_json::Value) -> serde_json::Value {
        serde_json::Value::Array(
            symbols
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|sym| {
                    serde_json::json!({
                        "name": sym["name"],
                        "kind": sym["kind"],
                        "detail": sym["detail"],
                        "full_start": sym["full_start"],
                        "full_end": sym["full_end"],
                        "children": outline_shape(&sym["children"]),
                    })
                })
                .collect(),
        )
    }

    /// The outlines the mock's `parseOutline` has to reproduce.
    ///
    /// [`ALT_FENCES`] is the point — every knot in it uses a legal fence the
    /// mock's outline regex rejected. [`KNOT_AND_FUNCTION`]/[`TWO_KNOTS`] are
    /// controls: the ordinary `=== name ===` shape and a function knot, both
    /// of which already worked, so a widening that broke them is red here.
    ///
    /// [`VAR_AND_KNOT`] is a different question (#2685 Gap 2):
    /// `document_symbols` (`crates/internal/brink-ide/src/document.rs`) also
    /// reports top-level `Variable`/`Constant`/`List`/`Struct`/`External`
    /// declarations from the manifest, appended AFTER every knot regardless of
    /// where they sit in the source — `score` is declared before knot `one`
    /// textually, but the driven answer puts `one` first. The studio mock's
    /// `parseOutline` had no concept of a non-knot/non-stitch top-level
    /// symbol at all, so `file_symbols` on a file with a `VAR` never reported
    /// it.
    fn driven_outlines() -> serde_json::Value {
        serde_json::json!({
            "ALT_FENCES": outline_of(ALT_FENCES),
            "ALT_STITCHES": outline_of(ALT_STITCHES),
            "INDENTED_FIRST_KNOT": outline_of(INDENTED_FIRST_KNOT),
            "KNOT_AND_FUNCTION": outline_of(KNOT_AND_FUNCTION),
            "TWO_KNOTS": outline_of(TWO_KNOTS),
            "VAR_AND_KNOT": outline_of(VAR_AND_KNOT),
        })
    }

    /// The header line `promote_stitch` / `demote_knot` REWRITE, read out of
    /// production's own `new_source` (#2661).
    ///
    /// Acceptance alone cannot see this: both ops answer `ok: true` here, so a
    /// rewrite that silently left the header alone (or mangled it) is a
    /// successful answer with wrong content. Production's two rewrites are
    /// name-agnostic — they strip the `=` fences and keep whatever is between
    /// them — which is why a function knot keeps its `function` segment and a
    /// parameterised stitch keeps its `(n)` inside the new fences.
    ///
    /// The mock interpolated the declared name into a regex instead, so
    /// `=== function greet() ===` matched nothing (header left untouched) and
    /// `= deal(n)` promoted to `=== deal ===(n)`.
    fn driven_header_rewrites() -> serde_json::Value {
        let mixed = session_with(&[("main.ink", KNOT_AND_FUNCTION)]);
        let params = session_with(&[("main.ink", PARAM_STITCH)]);
        let alt_fences = session_with(&[("main.ink", ALT_FENCES)]);

        serde_json::json!({
            "demote_knot:function-knot": rewritten_header(
                "demote_knot (function knot)",
                &mixed.demote_knot("main.ink", "greet", "one"),
                "greet",
            ),
            "promote_stitch:parameterised": rewritten_header(
                "promote_stitch (stitch with parameters)",
                &params.promote_stitch("main.ink", "one", "deal"),
                "deal",
            ),
            // #2685 Gap 1: `knotHeaderToStitch`'s `^=+`/`=+$` strip LOOKED
            // fence-width-agnostic, but #2682 drove no case past a plain
            // `===` knot. `three` is fenced `====` (four `=`) and carries no
            // stitches of its own, so demoting it into `one` reaches the
            // rewrite rather than refusing on `IllegalNesting` first.
            "demote_knot:alt-fence-knot": rewritten_header(
                "demote_knot (source knot fenced with four `=`)",
                &alt_fences.demote_knot("main.ink", "three", "one"),
                "three",
            ),
        })
    }

    /// The single `=`-leading line of a successful op's `new_source` that
    /// mentions `name` — i.e. the header the rewrite produced.
    fn rewritten_header(site: &str, json: &str, name: &str) -> String {
        let value = parse(json);
        assert!(
            value["ok"] == serde_json::json!(true),
            "`{site}` refused, so there is no rewritten header to read: {value:#}"
        );
        let source = value["new_source"].as_str().unwrap_or_default();
        let headers: Vec<&str> = source
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('=') && l.contains(name))
            .collect();
        assert!(
            headers.len() == 1,
            "`{site}` produced {} header lines mentioning `{name}`, so there is no \
             single rewrite to pin: {headers:?}",
            headers.len()
        );
        (*headers.first().expect("just asserted above")).to_owned()
    }

    // ── Diagnostics: the half `acceptance` cannot see (review finding on #2662) ──

    /// Site nobody drove: `rename_symbol`'s collision check counts a
    /// function knot as an ordinary duplicate (E022), via `knotHeaderFor` —
    /// which now carries the `(?:function\s+)?` segment `KNOT_FENCE` does.
    /// `acceptance`'s `outcome()` only records `ok`/`error`, and this call
    /// answers `ok: true` on both sides regardless of whether the collision
    /// fires — the rename itself succeeds either way, so no acceptance case
    /// can see whether `introduced_diagnostics` actually carries the E022.
    /// This drives it out of production instead.
    fn driven_diagnostics() -> serde_json::Value {
        let mixed = session_with(&[("main.ink", KNOT_AND_FUNCTION)]);
        // Renaming knot `one` to `greet` collides with the existing function
        // knot `=== function greet() ===` — production's collision check runs
        // over every top-level knot, function ones included.
        let renamed = mixed.rename_symbol("main.ink", "one", "", "greet");
        let value = parse(&renamed);
        assert!(
            value["ok"] == serde_json::json!(true),
            "`rename_symbol (collides with function knot)` was expected to \
             succeed with an introduced diagnostic, not refuse: {value:#}"
        );
        let codes: Vec<serde_json::Value> = value["introduced_diagnostics"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|d| d["code"].clone())
            .collect();
        assert!(
            !codes.is_empty(),
            "`rename_symbol (collides with function knot)` introduced no \
             diagnostics — the collision this fixture exists to pin did not \
             fire: {value:#}"
        );
        serde_json::json!({
            "rename_symbol:collides-with-function-knot": codes,
        })
    }

    // ── Discovery: which structs can refuse at all (#2577) ───────────

    /// Every `.rs` file under this crate's `src/`, sorted for determinism.
    fn crate_sources() -> Vec<std::path::PathBuf> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        out.sort();
        out
    }

    /// `struct Name {` (with any visibility) → `Name`.
    fn struct_name(line: &str) -> Option<&str> {
        let rest = line
            .strip_prefix("pub(crate) ")
            .or_else(|| line.strip_prefix("pub "))
            .unwrap_or(line);
        let rest = rest.strip_prefix("struct ")?;
        let name = rest.strip_suffix(" {")?;
        name.chars()
            .all(|c| c.is_alphanumeric() || c == '_')
            .then_some(name)
    }

    /// Does `body` declare a field `name` of exactly type `ty`?
    fn has_field(body: &str, name: &str, ty: &str) -> bool {
        body.lines().any(|line| {
            let line = line.trim();
            let line = line
                .strip_prefix("pub(crate) ")
                .or_else(|| line.strip_prefix("pub "))
                .unwrap_or(line);
            line == format!("{name}: {ty},")
        })
    }

    /// The names of every `Serialize` struct in `text` that carries BOTH an
    /// `ok: bool` and an `error: Option<String>` — i.e. a wire payload that can
    /// express a refusal. Line-oriented rather than a real parser (this crate
    /// has no `syn` dependency and does not want one for a test): a `#[derive]`
    /// attribute mentioning `Serialize` immediately above a `struct X {` line,
    /// whose body runs to the first `}` at the struct's own indentation. Every
    /// struct in this crate is written that way; a hand-implemented `Serialize`
    /// or a macro-generated struct would slip past, which is why the guard is
    /// documented as "no new struct is silently omitted", not "no shape can
    /// possibly exist elsewhere".
    fn refusal_structs_in(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut found = Vec::new();
        let mut derives_serialize = false;
        let mut i = 0;
        while i < lines.len() {
            let raw = lines[i];
            let line = raw.trim_start();
            if line.starts_with("#[derive(") {
                derives_serialize = line.contains("Serialize");
            } else if let Some(name) = struct_name(line) {
                if derives_serialize {
                    let indent = &raw[..raw.len() - line.len()];
                    let close = format!("{indent}}}");
                    let mut body = String::new();
                    let mut j = i + 1;
                    while j < lines.len() && lines[j] != close {
                        body.push_str(lines[j]);
                        body.push('\n');
                        j += 1;
                    }
                    if has_field(&body, "ok", "bool") && has_field(&body, "error", "Option<String>")
                    {
                        found.push(name.to_owned());
                    }
                    i = j;
                }
                derives_serialize = false;
            } else if !line.is_empty() && !line.starts_with("//") && !line.starts_with('#') {
                // Any other code between the derive and a struct ends its reach.
                derives_serialize = false;
            }
            i += 1;
        }
        found
    }

    /// The *set* of refusal-message literals in `text` (sorted, deduplicated —
    /// `dir_error_json("…")` also contains `error_json("…")`, and two sites may
    /// legitimately share a message; only the vocabulary matters here).
    ///
    /// Every refusal-message *literal* in `text`: the string argument of an
    /// `error_json("…")` / `dir_error_json("…")` call, and the literal in an
    /// `error: Some("…".to_owned())` struct field. Deliberately literal-only —
    /// a message built from a lower-layer error (`Some(e.to_string())`, e.g.
    /// `brink-ide`'s `entry file not found in session: {0}`) has no string in
    /// this crate to find, and pretending otherwise is how a scan starts
    /// looking more complete than it is.
    fn refusal_message_literals_in(text: &str) -> Vec<String> {
        const PREFIXES: [&str; 3] = ["error_json(\"", "dir_error_json(\"", "error: Some(\""];
        let mut found = Vec::new();
        for line in text.lines() {
            for prefix in PREFIXES {
                let mut rest = line;
                while let Some(at) = rest.find(prefix) {
                    let after = &rest[at + prefix.len()..];
                    // Messages never contain an escaped quote today; a literal
                    // that did would simply be truncated here, not missed.
                    if let Some(end) = after.find('"') {
                        found.push(after[..end].to_owned());
                        rest = &after[end..];
                    } else {
                        break;
                    }
                }
            }
        }
        found.sort();
        found.dedup();
        found
    }

    /// The omission guard for the vocabulary [`driven_messages`] pins (#2603).
    ///
    /// Driving is per-site, so a *fourth* doc-handle op could refuse with its
    /// own invented wording and nothing above would notice. This scans the
    /// crate for every refusal-message literal that talks about a handle and
    /// asserts the vocabulary is exactly the two strings production uses today
    /// — so any coinage that still contains the word `"handle"` (`"unknown
    /// handle"`, `"bad doc handle"`, ...) is red at the source, before it can
    /// reach a mock. The filter is the literal substring `"handle"`
    /// (`refusal_message_literals_in`'s callers filter on `m.contains
    /// ("handle")`): a handle refusal worded without that word — `"unknown
    /// document id"`, `"no such document"` — has no literal for the scan to
    /// catch and is invisible to it.
    #[test]
    fn doc_handle_refusal_vocabulary_is_uniform() {
        let mut handle_words: Vec<String> = crate_sources()
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .flat_map(|text| refusal_message_literals_in(&text))
            .filter(|m| m.contains("handle"))
            .collect();
        handle_words.sort();
        handle_words.dedup();

        let expected = vec![
            "document handle is read-only (mounted stdlib file)".to_owned(),
            "unknown document handle".to_owned(),
        ];
        assert!(
            handle_words == expected,
            "the doc-handle refusal vocabulary changed.\n\
             Every op that refuses an unhandled document handle must say \
             `unknown document handle` — one wording, so the studio mock and \
             `structural-refusal-shape.test.ts` cannot drift into a private \
             dialect (#2603, the fourth instance of that class after \
             #2583/#2599/#2602).\n\
             If the wording legitimately changed, drive the new site in \
             `driven_messages`, regenerate the fixture, and update this list.\n\
             found in source: {handle_words:?}\n\
             expected:        {expected:?}"
        );

        // The driven map is the other half: the strings the fixture ships must
        // be members of the vocabulary just scanned, not something a driver
        // picked up elsewhere. Only the DOC-HANDLE half is measured against the
        // handle vocabulary — `driven_op_messages` (#2620) drives refusals that
        // have nothing to do with handles, and asserting those are handle
        // literals would be nonsense.
        let driven = driven_doc_handle_messages();
        let entries = driven.as_object().expect("driven messages is an object");
        assert!(
            !entries.is_empty(),
            "no doc-handle message is driven at all"
        );
        for (key, value) in entries {
            let message = value.as_str().unwrap_or_default();
            assert!(
                handle_words.iter().any(|w| w == message),
                "driven message for `{key}` is {message:?}, which is not one of the \
                 doc-handle refusal literals found in this crate's source: {handle_words:?}"
            );
        }
    }

    /// `"current file source unavailable"` (`editor/refactor.rs`) is a
    /// DEFENSIVE branch with no reaching input, so it gets no driver and no
    /// mock counterpart (#2621).
    ///
    /// #2621 recorded it as "reachable in real Rust usage (`open_document` +
    /// `remove_file`)". It is not. `auto_import_apply_include_doc` only reaches
    /// that `let ... else` after `ensure_include` has already answered `Ok`, and
    /// `ensure_include` itself resolves `session.source(current_id)` — mapping a
    /// missing one to `AutoImportError::CurrentNotFound`. So removing the file
    /// out from under an open handle refuses one layer earlier, with
    /// `brink-ide`'s wording, and the `source_of` guard below it can only fire
    /// if those two disagree about the same session.
    ///
    /// This test pins the *actual* refusal for that route. If a refactor ever
    /// makes the defensive branch reachable, this goes red and the branch earns
    /// a driver plus a mock counterpart — mirroring it into the mock today
    /// would model a branch production cannot produce (#2577's lesson: a mock
    /// method nothing can reach closes nothing).
    #[test]
    fn removing_a_file_under_an_open_handle_refuses_before_the_source_guard() {
        let mut session = session_with(&[("main.ink", MAIN), ("other.ink", MAIN)]);
        let doc = session.open_document("main.ink");
        assert!(doc != 0, "`main.ink` must have a document handle");
        assert!(
            session.remove_file("main.ink"),
            "`main.ink` must be removable"
        );

        let message = refusal_message(
            "auto_import_apply_include_doc (file removed under the handle)",
            &session.auto_import_apply_include_doc(doc, "other.ink"),
        );
        assert!(
            message != "current file source unavailable",
            "the defensive `source_of` guard is now reachable — give it a driver \
             in `driven_op_messages` and a mock counterpart (#2621)"
        );
        assert!(
            message.contains("main.ink"),
            "expected `brink-ide`'s CurrentNotFound wording naming the removed \
             file, got {message:?}"
        );
    }

    /// `rename_symbol`'s `"no analysis"` branch gets NO mock counterpart, and
    /// this is the input that reaches it (#2634).
    ///
    /// #2634 asked for a per-string decision on the two refusals its Ask did
    /// not name. This is the first: `session.hir(file_id)` is `None` only when
    /// `file_id` resolved but `brink_db`'s `is_source_file` excludes the path
    /// — i.e. an extension that is neither `.ink` nor `.brink` (`db.rs:304`).
    /// A `.ink` file that is loaded always has HIR, so the branch is
    /// unreachable for every path the studio can hand it: `performSymbolRename`
    /// (`packages/studio-ui/src/symbolMenuActions.ts`) takes `req.path` from
    /// the outline, and only source files have an outline to open a symbol menu
    /// on. Mirroring it would model a production answer no studio path can
    /// produce (#2577's lesson, the same call `current file source
    /// unavailable` got above).
    ///
    /// The half that keeps that reasoning honest is the second assertion: a
    /// loaded `.ink` file gets *past* the guard. If HIR ever became optional
    /// for source files, this goes red and the branch earns a mock branch.
    #[test]
    fn rename_symbol_says_no_analysis_only_for_a_non_source_extension() {
        let mut session = session_with(&[("main.ink", MAIN)]);
        session.update_file("notes.md", MAIN);

        let message = refusal_message(
            "rename_symbol (non-source extension)",
            &session.rename_symbol("notes.md", "hello", "", "hi"),
        );
        assert!(
            message == "no analysis",
            "expected the `no analysis` guard for a non-source path, got {message:?}"
        );

        let ink: serde_json::Value = parse(&session.rename_symbol("main.ink", "hello", "", "hi"));
        assert!(
            ink["ok"] == serde_json::json!(true),
            "a loaded `.ink` file must get past the `no analysis` guard, or the \
             branch is studio-reachable and needs a mock counterpart (#2634): {ink:#}"
        );
    }

    /// `rename_symbol`'s `"cannot rename this symbol"` branch gets no SECOND
    /// mock counterpart either — the vocabulary is already mirrored, by the
    /// sibling op that can actually produce it (#2634).
    ///
    /// The branch sits below `declaration_offset`, so it is only reached when
    /// a declaration WAS resolved and `rename_safe` then declined it. Every
    /// declining case `rename` has — an `External` symbol, a UFCS field call, a
    /// prelude intrinsic — is a symbol `declaration_offset` cannot name in the
    /// first place: it walks `hir.knots` and their stitches only. So the
    /// name-based op reaches its own last line and answers, as the two
    /// assertions below pin for a knot and for a stitch.
    ///
    /// This is deliberately weaker than a proof of unreachability — no input
    /// we can construct reaches it. What makes it safe to leave unmirrored is
    /// that the wording is NOT unpinned: `rename_symbol_at` (the F2 road) does
    /// reach it with an offset that resolves nothing, the mock answers it
    /// there, and `rename_symbol_at:unrenameable` drives it. So the studio's
    /// notification path for this string is exercised; only a second entrance
    /// to it would be modelled, and nothing can walk through that entrance.
    #[test]
    fn rename_symbol_answers_once_a_declaration_resolves() {
        let two = session_with(&[("main.ink", TWO_KNOTS)]);

        for (knot, stitch) in [("one", ""), ("one", "a")] {
            let value: serde_json::Value =
                parse(&two.rename_symbol("main.ink", knot, stitch, "zz"));
            assert!(
                value["error"] != serde_json::json!("cannot rename this symbol"),
                "`rename_symbol({knot:?}, {stitch:?})` reached the post-`rename_safe` \
                 refusal — it now has an input the name-based road can produce, so give \
                 it a driver and a mock counterpart (#2634): {value:#}"
            );
        }
    }

    /// The scanner above is load-bearing, so it is exercised on inputs it must
    /// accept and reject rather than only on the live tree.
    #[test]
    fn the_message_scanner_reads_both_refusal_literal_forms() {
        let both = "        return error_json(\"file not loaded\");\n\
             \x20               error: Some(\"unknown document handle\".to_owned()),\n\
             \x20   return dir_error_json(\"no files found\");\n";
        let found = refusal_message_literals_in(both);
        assert!(
            found
                == vec![
                    "file not loaded".to_owned(),
                    "no files found".to_owned(),
                    "unknown document handle".to_owned(),
                ],
            "{found:?}"
        );

        // A message composed from a lower-layer error carries no literal here,
        // and the scan must not invent one for it.
        let composed = "            error: Some(e.to_string()),\n";
        assert!(refusal_message_literals_in(composed).is_empty());

        // An unrelated string literal on the same line is not a refusal message.
        let unrelated = "    let label = \"unknown document handle\";\n";
        assert!(refusal_message_literals_in(unrelated).is_empty());
    }

    /// The guard the hand-written enumeration in [`generated`] needed: a NEW
    /// refusal-producing struct anywhere in this crate turns this red instead
    /// of shipping with every gate green and no mock counterpart (#2577).
    #[test]
    fn every_refusal_struct_is_in_the_fixture() {
        let mut discovered: Vec<String> = crate_sources()
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .flat_map(|text| refusal_structs_in(&text))
            .collect();
        discovered.sort();
        discovered.dedup();

        assert!(
            discovered.len() >= 3,
            "the source scan found {} refusal-producing struct(s) — it has stopped \
             seeing this crate's payloads, so it can no longer catch a new one. \
             Check `refusal_structs_in`'s assumptions against the current source: {discovered:?}",
            discovered.len()
        );

        let generated = generated();
        let mut enumerated: Vec<String> = generated["shapes"]
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        enumerated.sort();

        assert!(
            discovered == enumerated,
            "the refusal-shape fixture no longer covers every refusal-producing struct \
             in this crate.\n\
             A `Serialize` struct with both `ok: bool` and `error: Option<String>` can \
             refuse, so its shape must be pinned here AND mirrored by the studio mock \
             (`packages/brink-studio/src/__mocks__/brink-web.ts`) — otherwise the studio \
             suite is blind to bugs living in the fields it omits (#2543/#2568/#2577).\n\
             Add it to `generated()`, regenerate with \
             `BRINK_BLESS_REFUSAL_SHAPES=1 cargo test -p brink-web --lib refusal_shape`, \
             then update the mock and \
             `packages/brink-studio/src/__tests__/structural-refusal-shape.test.ts`.\n\
             found in source: {discovered:?}\n\
             enumerated here: {enumerated:?}"
        );
    }

    /// The scanner is the load-bearing half of the guard above, so it is
    /// exercised on inputs it must accept and reject rather than only on the
    /// live tree (where a silently-broken scanner would still find the three
    /// structs that happen to be there).
    #[test]
    fn the_scanner_keys_on_serialize_plus_ok_plus_error() {
        let accepted = "#[derive(Serialize)]\n\
             pub(crate) struct Refuses {\n\
             \x20   pub(crate) ok: bool,\n\
             \x20   #[serde(skip_serializing_if = \"Option::is_none\")]\n\
             \x20   pub(crate) error: Option<String>,\n\
             }\n";
        assert!(
            refusal_structs_in(accepted) == vec!["Refuses".to_owned()],
            "{:?}",
            refusal_structs_in(accepted)
        );

        // No `Serialize` — an internal type, not a wire payload.
        let no_derive = accepted.replace("#[derive(Serialize)]", "#[derive(Debug)]");
        assert!(refusal_structs_in(&no_derive).is_empty());

        // `ok` without `error` is a report, not a refusal channel.
        let no_error = "#[derive(Serialize)]\n\
             struct Reports {\n\
             \x20   ok: bool,\n\
             }\n";
        assert!(refusal_structs_in(no_error).is_empty());

        // A struct with an unrelated `error` field but no `ok` flag.
        let no_ok = "#[derive(Serialize)]\n\
             struct Diagnostic {\n\
             \x20   error: Option<String>,\n\
             }\n";
        assert!(refusal_structs_in(no_ok).is_empty());

        // The struct's own body ends at its own indentation — a nested struct
        // literal inside a later fn must not be swallowed into it.
        let two = format!("{accepted}\n{}", accepted.replace("Refuses", "AlsoRefuses"));
        let mut names = refusal_structs_in(&two);
        names.sort();
        assert!(
            names == vec!["AlsoRefuses".to_owned(), "Refuses".to_owned()],
            "{names:?}"
        );
    }

    #[test]
    fn refusal_shape_fixture_matches_the_rust_payloads() {
        let generated = generated();

        if std::env::var_os("BRINK_BLESS_REFUSAL_SHAPES").is_some() {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_REL_PATH);
            let parent = path.parent().expect("fixture path has a parent");
            std::fs::create_dir_all(parent).expect("create fixture directory");
            let mut text = serde_json::to_string_pretty(&generated).expect("fixture serializes");
            text.push('\n');
            std::fs::write(&path, text).expect("write fixture");
            return;
        }

        let checked_in: serde_json::Value =
            serde_json::from_str(FIXTURE).expect("fixture is valid JSON");
        assert!(
            checked_in == generated,
            "`{FIXTURE_REL_PATH}` is stale — the Rust refusal payloads changed shape.\n\
             Regenerate with `BRINK_BLESS_REFUSAL_SHAPES=1 cargo test -p brink-web --lib refusal_shape`,\n\
             then update `packages/brink-studio/src/__mocks__/brink-web.ts` to match (#2568).\n\
             checked in: {checked_in:#}\n\
             generated:  {generated:#}"
        );
    }

    /// The property the studio mock has to reproduce, stated once here so the
    /// fixture's point survives even for a reader who only sees the Rust side.
    #[test]
    fn a_refusal_still_ships_the_full_gate_fields() {
        let refusal = parse(&error_json("boom"));
        assert!(refusal["ok"] == serde_json::json!(false), "{refusal:#}");
        assert!(refusal["safe"] == serde_json::json!(true), "{refusal:#}");
        assert!(
            refusal["cross_file_edits"] == serde_json::json!([]),
            "{refusal:#}"
        );
        assert!(
            refusal["introduced_diagnostics"] == serde_json::json!([]),
            "{refusal:#}"
        );
        // Only these are omitted on a refusal.
        assert!(refusal.get("path").is_none(), "{refusal:#}");
        assert!(refusal.get("new_source").is_none(), "{refusal:#}");
        assert!(refusal["error"] == serde_json::json!("boom"), "{refusal:#}");
    }
}
