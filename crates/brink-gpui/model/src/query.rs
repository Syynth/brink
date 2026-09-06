//! Interactive queries — hover, completions, document symbols.
//!
//! These are request/response, which is what makes moving the session to a
//! worker cheap rather than invasive: `gpui-base`'s provider traits already
//! return a `Task`, so nothing on the UI side has to change shape. This is
//! how LSP works, and how Zed reaches its own analysis.
//!
//! A query is answered **after** the edits queued ahead of it, in the same
//! drain. The channel is FIFO and the editor sends its `Edit` before asking
//! (`document.rs`, `seed_edit`), so a query never sees text older than the
//! keystroke that prompted it. [`clamp_offset`] is the guard behind that
//! promise: an offset past the text is pulled back to its end rather than
//! allowed to panic the analysis thread.
//!
//! Results are plain data in **byte offsets**, like everything else crossing
//! the boundary. The mapping onto `lsp_types` lives with the editor that
//! consumes it, so it exists once.

use brink_ide::passage::PassageOrigin;
use brink_ir::SymbolKind;

/// What the UI wants to know.
#[derive(Debug, Clone)]
pub enum QueryKind {
    Hover {
        path: String,
        offset: u32,
    },
    Completions {
        path: String,
        offset: u32,
    },
    DocumentSymbols {
        path: String,
    },
    InlayHints {
        path: String,
    },
    /// Every knot and stitch in the project — the Conventions editor's
    /// passage picker (ruled 2026-09-02: sample lines come from a
    /// knot/stitch selector).
    PassageIndex,
    /// The content lines of `path` (`knot` or `knot.stitch`), as the
    /// author would mark them.
    Passage {
        path: String,
    },
    // ── Navigation (INVENTORY §0 item 1) ──────────────────────────────
    /// Where the symbol under `offset` is declared.
    Definition {
        path: String,
        offset: u32,
    },
    /// Every site that uses the symbol under `offset`, classified.
    References {
        path: String,
        offset: u32,
        include_declaration: bool,
    },
    /// Whether the symbol under `offset` can be renamed, and the range the
    /// editor should seed its prompt from.
    PrepareRename {
        path: String,
        offset: u32,
    },
    /// The full cross-file rename, gated: computed and re-analyzed, never
    /// applied here. Applying is the UI's act (ruled 2026-06-20, "safe-by-
    /// default with an in-place breakage report").
    Rename {
        path: String,
        offset: u32,
        new_name: String,
    },
    /// Structural fold candidates for `path`.
    FoldingRanges {
        path: String,
    },
    // ── Fixes (INVENTORY §0 item 3; `crate::fixes`) ───────────────────
    /// Every offered fix for the visible diagnostics under `offset`.
    FixesAt {
        path: String,
        offset: u32,
    },
    /// Every offered fix in the compilation, for the Problems panel.
    FixOffers,
    /// Run the safe batch to its fixpoint; the session is rolled back and
    /// the changed files answered for the host to write.
    FixAll {
        scope: crate::fixes::FixScope,
    },
    /// Whole-source refactors at `offset` (sort knots, format a knot…).
    Refactors {
        path: String,
        offset: u32,
    },
    /// The text a refactor produces.
    ResolveRefactor {
        path: String,
        data: String,
    },
    /// The file as `brink fmt` would write it (`[project] indent`
    /// honoured). Ink only: the formatter parses with the ink frontend, so
    /// a `.brink` file answers `None` rather than being fed to it.
    Format {
        path: String,
    },
    /// The compiled program, for the Program Explorer — see
    /// [`crate::program`]. Answered by the worker loop itself, which holds
    /// the entry and file list a compile needs.
    Program,
    /// The compiled program's `.inkt` dump, for Compiled Output — see
    /// [`crate::compiled`]. Answered by the worker loop for the same
    /// reason as [`Self::Program`], and off the same memoized compile.
    CompiledOutput,
}

/// The answer. `Unavailable` is the honest result for a path the session
/// does not hold or a project that has not analyzed yet — distinct from an
/// empty answer, which means "asked, and there is nothing here".
#[derive(Debug, Clone)]
pub enum QueryResult {
    Hover(Option<HoverInfo>),
    Completions(Vec<Completion>),
    DocumentSymbols(Vec<Symbol>),
    InlayHints(Vec<InlayHint>),
    PassageIndex(Vec<PassageSymbol>),
    /// `None` when the path names nothing in the project.
    Passage(Option<Vec<PassageLine>>),
    /// `None` when nothing under the offset resolves.
    Definition(Option<Location>),
    References(Vec<Reference>),
    /// `None` when the symbol under the offset is not renameable.
    PrepareRename(Option<(u32, u32)>),
    /// `None` when the rename cannot be computed at all — distinct from a
    /// plan that is computed but unsafe, which comes back with its report.
    Rename(Option<RenamePlan>),
    FoldingRanges(Vec<Fold>),
    FixesAt(Vec<crate::fixes::FixPlan>),
    FixOffers(crate::fixes::FixOffers),
    FixAll(crate::fixes::FixAllReport),
    Refactors(Vec<crate::fixes::Refactor>),
    /// `None` when the refactor changes nothing.
    ResolvedRefactor(Option<String>),
    /// `None` when the file is native, unknown, or already formatted.
    Formatted(Option<String>),
    Program(Box<crate::program::ProgramReport>),
    CompiledOutput(Box<crate::compiled::CompiledOutput>),
    Unavailable,
}

/// A place in the project, in bytes of that file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Location {
    pub path: String,
    pub start: u32,
    pub end: u32,
}

/// How a reference site uses the symbol. Mirrors `brink_ide`'s
/// `ReferenceKind` as plain data so the app never imports the engine's
/// navigation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Decl,
    Call,
    Divert,
    Read,
    Write,
}

impl ReferenceKind {
    /// The Search card's badge text (docs/search-results-cards-spec.md).
    #[must_use]
    pub fn badge(self) -> &'static str {
        match self {
            Self::Decl => "decl",
            Self::Call => "call",
            Self::Divert => "divert",
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub location: Location,
    pub kind: ReferenceKind,
}

/// One edit of a rename, in bytes of `path` as it was when the plan was
/// computed. Ranges are disjoint per file; apply them last-to-first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextEdit {
    pub path: String,
    pub start: u32,
    pub end: u32,
    pub new_text: String,
}

/// A diagnostic the rename would introduce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Introduced {
    pub severity: brink_ir::Severity,
    pub code: String,
    pub message: String,
    pub path: String,
    /// 1-based.
    pub line: u32,
    /// 1-based.
    pub col: u32,
}

/// A computed rename and its safety report. `safe` is `introduced.is_empty()`
/// AND the symbol is not an external: an `EXTERNAL`'s name is the
/// story↔engine contract, so that rename is always unsafe (ruled 2026-08-24)
/// and applies only through Force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePlan {
    pub old_name: String,
    pub new_name: String,
    pub edits: Vec<TextEdit>,
    pub introduced: Vec<Introduced>,
    pub external: bool,
}

impl RenamePlan {
    #[must_use]
    pub fn is_safe(&self) -> bool {
        self.introduced.is_empty() && !self.external
    }

    /// Files touched, in edit order without repeats.
    #[must_use]
    pub fn files(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for e in &self.edits {
            if !out.contains(&e.path.as_str()) {
                out.push(&e.path);
            }
        }
        out
    }
}

/// A fold candidate, in 0-based lines — what the editor's gutter offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fold {
    pub start_line: u32,
    pub end_line: u32,
}

/// One entry of the passage picker: `knot` or `knot.stitch`, and the file
/// that declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassageSymbol {
    pub path: String,
    pub is_stitch: bool,
    pub file: String,
}

/// One content line of a passage, with the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassageLine {
    pub text: String,
    pub tags: Vec<String>,
    /// Zero-based source line.
    pub line: u32,
    pub origin: PassageOrigin,
    pub file: String,
}

/// A parameter-name hint, drawn inside the line although the buffer does
/// not contain it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    pub offset: u32,
    /// Already carries its own `:`; `padding_right` is folded in here so the
    /// editor does not have to know the convention.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverInfo {
    /// Markdown, with link refs already stripped.
    pub markdown: String,
    pub range: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
}

/// Kept as brink's own kind rather than an LSP one so the LSP mapping is
/// written once, next to the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Symbol(SymbolKind),
    StdlibFunction,
    /// `DONE` / `END`.
    Builtin,
}

/// One knot or stitch, for the Binder's structure view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// The name's own range — where "play from here" jumps to.
    pub start: u32,
    /// The whole declaration, header through body.
    pub full_start: u32,
    pub full_end: u32,
    pub is_function: bool,
    pub children: Vec<Symbol>,
}

pub(crate) fn answer(
    session: &mut brink_ide::session::IdeSession,
    kind: &QueryKind,
) -> QueryResult {
    match kind {
        QueryKind::FixesAt { path, offset } => match crate::fixes::fixes_at(session, path, *offset)
        {
            Some(found) => QueryResult::FixesAt(found),
            None => QueryResult::Unavailable,
        },
        QueryKind::FixOffers => QueryResult::FixOffers(crate::fixes::offers(session)),
        QueryKind::FixAll { scope } => match crate::fixes::fix_all(session, scope) {
            Some(report) => QueryResult::FixAll(report),
            None => QueryResult::Unavailable,
        },
        QueryKind::Refactors { path, offset } => {
            match crate::fixes::refactors(session, path, *offset) {
                Some(found) => QueryResult::Refactors(found),
                None => QueryResult::Unavailable,
            }
        }
        QueryKind::ResolveRefactor { path, data } => {
            QueryResult::ResolvedRefactor(crate::fixes::resolve_refactor(session, path, data))
        }
        QueryKind::Format { path } => QueryResult::Formatted(format(session, path)),
        // The worker loop answers these two before reaching here.
        QueryKind::Program | QueryKind::CompiledOutput => QueryResult::Unavailable,
        QueryKind::Hover { path, offset } => QueryResult::Hover(hover(session, path, *offset)),
        QueryKind::Completions { path, offset } => match completions(session, path, *offset) {
            Some(items) => QueryResult::Completions(items),
            None => QueryResult::Unavailable,
        },
        QueryKind::DocumentSymbols { path } => match symbols(session, path) {
            Some(found) => QueryResult::DocumentSymbols(found),
            None => QueryResult::Unavailable,
        },
        QueryKind::InlayHints { path } => match inlay_hints(session, path) {
            Some(found) => QueryResult::InlayHints(found),
            None => QueryResult::Unavailable,
        },
        QueryKind::PassageIndex => QueryResult::PassageIndex(passage_index(session)),
        QueryKind::Passage { path } => QueryResult::Passage(passage(session, path)),
        QueryKind::Definition { path, offset } => match definition(session, path, *offset) {
            Some(found) => QueryResult::Definition(found),
            None => QueryResult::Unavailable,
        },
        QueryKind::References {
            path,
            offset,
            include_declaration,
        } => match references(session, path, *offset, *include_declaration) {
            Some(found) => QueryResult::References(found),
            None => QueryResult::Unavailable,
        },
        QueryKind::PrepareRename { path, offset } => match prepare_rename(session, path, *offset) {
            Some(found) => QueryResult::PrepareRename(found),
            None => QueryResult::Unavailable,
        },
        QueryKind::Rename {
            path,
            offset,
            new_name,
        } => match rename(session, path, *offset, new_name) {
            Some(plan) => QueryResult::Rename(plan),
            None => QueryResult::Unavailable,
        },
        QueryKind::FoldingRanges { path } => match folding_ranges(session, path) {
            Some(found) => QueryResult::FoldingRanges(found),
            None => QueryResult::Unavailable,
        },
    }
}

/// `brink_fmt::format` over `path`, with the project's `[project] indent`.
/// `None` for a native file (the formatter is the ink formatter — gated
/// rather than relied on to no-op, the #2291 lesson), an unknown path, or
/// text the formatter leaves alone.
fn format(session: &brink_ide::session::IdeSession, path: &str) -> Option<String> {
    let id = session.file_id(path)?;
    if session.is_native(id) || session.is_mounted_std(id) {
        return None;
    }
    let source = session.source(id)?;
    let config = brink_project_config::ProjectConfig {
        indent: session.project_settings().indent,
        ..Default::default()
    };
    let formatted = brink_fmt::format(
        source,
        &brink_fmt::FormatConfig::from_project_config(&config),
    );
    (formatted != source).then_some(formatted)
}

// ── Navigation ───────────────────────────────────────────────────────

/// A `FileId`'s path and the `(start, end)` of a range in it, as one
/// `Location`. `None` for the mounted stdlib or a retired file: neither is
/// somewhere the author can be taken.
fn location(
    session: &brink_ide::session::IdeSession,
    file: brink_ir::FileId,
    range: rowan::TextRange,
) -> Option<Location> {
    if session.is_mounted_std(file) {
        return None;
    }
    Some(Location {
        path: session.db().file_path(file)?.to_owned(),
        start: range.start().into(),
        end: range.end().into(),
    })
}

/// Outer `None`: the file is not in the session. Inner `None`: nothing
/// under the offset resolves — an ordinary answer, not an error.
fn definition(
    session: &brink_ide::session::IdeSession,
    path: &str,
    offset: u32,
) -> Option<Option<Location>> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = rowan::TextSize::from(clamp_offset(source, offset));
    let found = brink_ide::navigation::goto_definition(session.db(), analysis, id, offset);
    Some(found.and_then(|loc| location(session, loc.file, loc.range)))
}

fn references(
    session: &brink_ide::session::IdeSession,
    path: &str,
    offset: u32,
    include_declaration: bool,
) -> Option<Vec<Reference>> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = rowan::TextSize::from(clamp_offset(source, offset));
    let found = brink_ide::navigation::find_references_with_kinds(
        session.db(),
        analysis,
        id,
        offset,
        include_declaration,
    );
    let mut out: Vec<Reference> = found
        .into_iter()
        .filter_map(|r| {
            Some(Reference {
                location: location(session, r.file, r.range)?,
                kind: match r.kind {
                    brink_ide::navigation::ReferenceKind::Decl => ReferenceKind::Decl,
                    brink_ide::navigation::ReferenceKind::Call => ReferenceKind::Call,
                    brink_ide::navigation::ReferenceKind::Divert => ReferenceKind::Divert,
                    brink_ide::navigation::ReferenceKind::Read => ReferenceKind::Read,
                    brink_ide::navigation::ReferenceKind::Write => ReferenceKind::Write,
                },
            })
        })
        .collect();
    // File order then offset — the order a reader expects a list of places
    // to be in, and stable across analyses that changed nothing.
    out.sort_by(|a, b| {
        (&a.location.path, a.location.start).cmp(&(&b.location.path, b.location.start))
    });
    Some(out)
}

fn prepare_rename(
    session: &brink_ide::session::IdeSession,
    path: &str,
    offset: u32,
) -> Option<Option<(u32, u32)>> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = rowan::TextSize::from(clamp_offset(source, offset));
    Some(
        brink_ide::rename::prepare_rename(session.db(), analysis, id, offset)
            .map(|r| (r.start().into(), r.end().into())),
    )
}

/// The rename, gated. `brink_ide::rename::rename` computes every edit or
/// refuses outright (a missed correlation is a refusal, never a partial edit
/// set — #1539); `structural_result::gate` then overlays the edits and
/// re-analyzes without touching the session. Both halves are the same ones
/// `brink ide rename` and the web studio use, so there is exactly one rename
/// pipeline and one safety guarantee (ruled 2026-06-20).
fn rename(
    session: &brink_ide::session::IdeSession,
    path: &str,
    offset: u32,
    new_name: &str,
) -> Option<Option<RenamePlan>> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = rowan::TextSize::from(clamp_offset(source, offset));

    let Some(range) = brink_ide::rename::prepare_rename(session.db(), analysis, id, offset) else {
        return Some(None);
    };
    let old_name = source
        .get(usize::from(range.start())..usize::from(range.end()))
        .unwrap_or_default()
        .to_owned();
    let Some(result) = brink_ide::rename::rename(session.db(), analysis, id, offset, new_name)
    else {
        return Some(None);
    };

    let introduced = brink_ide::structural_result::gate(session, &result.edits)
        .into_iter()
        .map(|d| Introduced {
            severity: d.severity,
            code: d.code.as_str().to_owned(),
            message: d.message,
            path: d.path,
            line: d.line,
            col: d.col,
        })
        .collect();

    let mut edits: Vec<TextEdit> = result
        .edits
        .iter()
        .filter_map(|e| {
            let at = location(session, e.file, e.range)?;
            Some(TextEdit {
                path: at.path,
                start: at.start,
                end: at.end,
                new_text: e.new_text.clone(),
            })
        })
        .collect();
    edits.sort_by(|a, b| (&a.path, a.start).cmp(&(&b.path, b.start)));

    Some(Some(RenamePlan {
        old_name,
        new_name: new_name.to_owned(),
        edits,
        introduced,
        external: result.external_binding.is_some(),
    }))
}

/// Structural folds only (ruled #479, 2026-07-10): the machinery/narrative
/// run folds are opt-in view modes the native studio has not wired, and
/// offering them in the gutter is exactly the noise that ruling removed.
fn folding_ranges(session: &brink_ide::session::IdeSession, path: &str) -> Option<Vec<Fold>> {
    let id = session.file_id(path)?;
    let hir = session.hir(id)?;
    let source = session.source(id)?;
    let projection = session.projection(id)?;
    let mut ranges = brink_ide::folding::folding_ranges(hir, source, &projection);
    // `~ { … }` blocks and nested control bodies are a separate pass, as
    // brink-lsp's `folding_range` also does.
    ranges.extend(brink_ide::folding::block_folds(hir, source));
    let mut out: Vec<Fold> = ranges
        .into_iter()
        .filter(|r| r.end_line > r.start_line)
        .map(|r| Fold {
            start_line: r.start_line,
            end_line: r.end_line,
        })
        .collect();
    out.sort_by_key(|f| (f.start_line, f.end_line));
    out.dedup();
    Some(out)
}

/// Every knot and stitch of the author's files, in file order then
/// declaration order — the mounted stdlib is not the author's to mark.
fn passage_index(session: &brink_ide::session::IdeSession) -> Vec<PassageSymbol> {
    let mut files: Vec<(String, brink_db::FileId)> = session
        .db()
        .file_ids()
        .filter(|id| !session.is_mounted_std(*id))
        .filter_map(|id| Some((session.db().file_path(id)?.to_owned(), id)))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for (file, id) in files {
        let Some(hir) = session.hir(id) else {
            continue;
        };
        for knot in &hir.knots {
            out.push(PassageSymbol {
                path: knot.name.text.clone(),
                is_stitch: false,
                file: file.clone(),
            });
            for stitch in &knot.stitches {
                out.push(PassageSymbol {
                    path: format!("{}.{}", knot.name.text, stitch.name.text),
                    is_stitch: true,
                    file: file.clone(),
                });
            }
        }
    }
    out
}

/// The passage at `path`, found in whichever file declares it.
fn passage(session: &brink_ide::session::IdeSession, path: &str) -> Option<Vec<PassageLine>> {
    for id in session.db().file_ids() {
        if session.is_mounted_std(id) {
            continue;
        }
        let Some(hir) = session.hir(id) else {
            continue;
        };
        let source = session.source(id).unwrap_or("");
        let Some(contexts) = session.line_contexts(id) else {
            continue;
        };
        let Some(lines) = brink_ide::passage::passage_lines(hir, source, &contexts, path) else {
            continue;
        };
        let file = session.db().file_path(id).unwrap_or("").to_owned();
        return Some(
            lines
                .into_iter()
                .map(|l| PassageLine {
                    text: l.text,
                    tags: l.tags,
                    line: l.line,
                    origin: l.origin,
                    file: file.clone(),
                })
                .collect(),
        );
    }
    None
}

fn inlay_hints(session: &brink_ide::session::IdeSession, path: &str) -> Option<Vec<InlayHint>> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let whole = rowan::TextRange::new(
        rowan::TextSize::from(0),
        rowan::TextSize::from(u32::try_from(source.len()).unwrap_or(u32::MAX)),
    );
    // The native and ink frontends are distinct nominal trees, so the
    // dispatch is on the file's own language — feeding an ink-parsed root to
    // the native query would silently reproduce #2280.
    let hints = if session.is_native(id) {
        let root = session.syntax_root_native(id)?;
        brink_ide::inlay_hints::inlay_hints_native(&root, analysis, session.db(), id, whole, None)
    } else {
        let root = session.syntax_root(id)?;
        brink_ide::inlay_hints::inlay_hints(&root, analysis, session.db(), id, whole, None)
    };
    Some(
        hints
            .into_iter()
            .map(|h| InlayHint {
                offset: u32::from(h.offset),
                label: if h.padding_right {
                    format!("{} ", h.label)
                } else {
                    h.label
                },
            })
            .collect(),
    )
}

/// `offset` pulled inside `source`: at most its length, and never inside a
/// multi-byte character.
pub(crate) fn clamp_offset(source: &str, offset: u32) -> u32 {
    let mut at = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(source.len());
    while at > 0 && !source.is_char_boundary(at) {
        at -= 1;
    }
    u32::try_from(at).unwrap_or(u32::MAX)
}

fn hover(session: &brink_ide::session::IdeSession, path: &str, offset: u32) -> Option<HoverInfo> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = clamp_offset(source, offset);
    let info = brink_ide::hover::hover(
        analysis,
        session.db(),
        id,
        source,
        offset.into(),
        &session.db().file_metadata(),
    )?;
    Some(HoverInfo {
        markdown: brink_ide::hover::strip_link_refs(&info.content),
        range: info.range.map(|r| (r.start().into(), r.end().into())),
    })
}

fn completions(
    session: &brink_ide::session::IdeSession,
    path: &str,
    offset: u32,
) -> Option<Vec<Completion>> {
    use brink_ide::{
        CompletionContext, cursor_scope, detect_completion_context, is_visible_in_context,
        ref_arg_root_prefix, stdlib_completions,
    };

    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = clamp_offset(source, offset) as usize;

    let ctx = detect_completion_context(source, offset);
    let scope = cursor_scope(source, offset);
    let ref_root = ref_arg_root_prefix(source, offset);
    let mut items = Vec::new();

    // A dotted path is exhaustive: only that knot's members can complete,
    // so this returns rather than falling through to the general sweep.
    if let CompletionContext::DottedPath { ref knot } = ctx {
        let prefix = format!("{knot}.");
        for (name, ids) in &analysis.index.by_name {
            let Some(suffix) = name.strip_prefix(&*prefix) else {
                continue;
            };
            for def_id in ids {
                if let Some(info) = analysis.index.symbols.get(def_id) {
                    items.push(Completion {
                        label: suffix.to_owned(),
                        kind: CompletionKind::Symbol(info.kind),
                    });
                }
            }
        }
        return Some(items);
    }

    for info in analysis.index.symbols.values() {
        if !is_visible_in_context(&ctx, info, &scope) {
            continue;
        }
        // A `ref` argument can only take a variable, so nothing else is a
        // legal completion there however visible it is.
        if ref_root.is_some() && info.kind != SymbolKind::Variable {
            continue;
        }
        items.push(Completion {
            label: info.name.clone(),
            kind: CompletionKind::Symbol(info.kind),
        });
    }
    for f in stdlib_completions(&ctx, session.language_dialect()) {
        items.push(Completion {
            label: f.name.to_owned(),
            kind: CompletionKind::StdlibFunction,
        });
    }
    if matches!(
        ctx,
        CompletionContext::Divert | CompletionContext::InlineExpr
    ) {
        for label in ["DONE", "END"] {
            items.push(Completion {
                label: label.to_owned(),
                kind: CompletionKind::Builtin,
            });
        }
    }
    Some(items)
}

fn symbols(session: &brink_ide::session::IdeSession, path: &str) -> Option<Vec<Symbol>> {
    let id = session.file_id(path)?;
    let hir = session.hir(id)?;
    let manifest = session.manifest(id)?;
    let source = session.source(id)?;
    Some(
        brink_ide::document::document_symbols(hir, manifest, source)
            .iter()
            .map(convert)
            .collect(),
    )
}

fn convert(symbol: &brink_ide::document::DocumentSymbol) -> Symbol {
    Symbol {
        name: symbol.name.clone(),
        start: symbol.range.start().into(),
        full_start: symbol.full_range.start().into(),
        full_end: symbol.full_range.end().into(),
        is_function: symbol
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("function")),
        children: symbol.children.iter().map(convert).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_offset;

    #[test]
    fn an_offset_past_the_text_lands_on_its_end_at_a_char_boundary() {
        assert_eq!(clamp_offset("hello", 3), 3);
        assert_eq!(clamp_offset("hello", 5), 5);
        assert_eq!(clamp_offset("hello", 199), 5);
        // "é" is two bytes; an offset inside it steps back to its start.
        assert_eq!(clamp_offset("caf\u{e9}", 4), 3);
        assert_eq!(clamp_offset("", 7), 0);
    }
}
