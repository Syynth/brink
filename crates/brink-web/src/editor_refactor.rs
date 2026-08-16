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
`messages` are real refusal strings read back out of the production ops, not typed (#2603). \
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

    /// The refusal *vocabulary* of the document-handle ops, obtained by calling
    /// the real production methods on a real [`EditorSession`] and reading the
    /// `error` field back out of the JSON they answer with.
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
    fn driven_messages() -> serde_json::Value {
        let mut session = EditorSession::new();
        session.update_file("main.ink", "=== hello ===\nHi.\n-> END\n");

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

        serde_json::json!({
            "auto_import_include_doc:unknown-handle": include_doc,
            "auto_import_apply_include_doc:unknown-handle": apply_include_doc,
            "resolve_code_action_doc:unknown-handle": code_action_doc,
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
    /// — so `"unknown handle"`, `"bad doc handle"` or any other coinage is red
    /// at the source, before it can reach a mock.
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
        // picked up elsewhere.
        let driven = driven_messages();
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
