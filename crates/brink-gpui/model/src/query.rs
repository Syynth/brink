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
    /// Every `hex_color` literal in a file, for the editor's colour
    /// swatches. Cheap and per-file, like inlay hints.
    DocumentColors {
        path: String,
    },
    /// Turn the line at `offset` into another structural element — a
    /// choice, a gather, plain narrative, a choice body. The sigil
    /// arithmetic is `brink-ide`'s (`line_convert::convert_element`),
    /// which reads the line's real structural context rather than
    /// sniffing its text.
    ConvertLine {
        path: String,
        offset: u32,
        target: ConvertTarget,
    },
    /// Lift a stitch out of its knot and make it a knot of its own.
    Promote {
        path: String,
        knot: String,
        stitch: String,
    },
    /// Fold a knot into the knot ABOVE it as a stitch. The destination is
    /// the preceding knot in the file — the place a demoted knot lands
    /// when a file is read top to bottom — and the worker resolves it,
    /// since the panel has no reason to know the file's order.
    Demote {
        path: String,
        knot: String,
    },
    /// Lift the selected lines into a new knot (or function), replacing
    /// them with a call. `start`/`end` are byte offsets, snapped to whole
    /// lines by the op itself.
    Extract {
        path: String,
        start: u32,
        end: u32,
        name: String,
        /// A `=== function name() ===` rather than a knot.
        function: bool,
    },
    /// Spelling and light grammar over one file's prose. Answered in the
    /// worker loop, which holds the `[prose]` config the check needs.
    Prose {
        path: String,
    },
    /// The whole-project story graph — knots and stitches as nodes,
    /// diverts and choices as edges. Answered in the worker loop rather
    /// than in `answer`, since it needs the entry and the file list.
    StoryGraph,
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
    StoryGraph(Box<crate::graph::StoryGraphReport>),
    Prose(Vec<crate::prose::ProseLint>),
    /// `(start, end, "#RRGGBB")` per literal, in byte offsets.
    DocumentColors(Vec<(u32, u32, String)>),
    /// A structural move — promote or demote — as a plan the studio
    /// applies, or a refusal with the reason.
    Structural(StructuralOutcome),
    /// A single text edit, or `None` when the conversion makes no sense
    /// for the line asked about (a knot header, or the type it already
    /// is).
    LineEdit(Option<LineEdit>),
    CompiledOutput(Box<crate::compiled::CompiledOutput>),
    Unavailable,
}

/// What a structural move came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralOutcome {
    Plan(Box<StructuralPlan>),
    /// The op cannot be done, and why — a name collision, a knot with
    /// stitches of its own, nothing above it to demote into. Said rather
    /// than silently skipped: an author who asked deserves the reason.
    Refused(String),
}

/// A structural move, ready to apply. `new_source` replaces the primary
/// file wholesale; `edits` are the reference rewrites that land in OTHER
/// files. `introduced` empty means safe — anything else is the breakage
/// report's content, and applying it is the author's call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralPlan {
    /// What happened, for the notice: "Promoted `linger` to a knot".
    pub summary: String,
    pub path: String,
    pub new_source: String,
    pub edits: Vec<TextEdit>,
    pub introduced: Vec<Introduced>,
}

impl StructuralPlan {
    #[must_use]
    pub fn is_safe(&self) -> bool {
        self.introduced.is_empty()
    }
}

/// What a line is being turned into. A plain mirror of
/// `brink_ide::line_convert::ConvertTarget`, so nothing of the engine
/// crosses to the main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertTarget {
    Narrative,
    Choice { sticky: bool },
    Gather,
    ChoiceBody,
}

/// Replace `from..to` with `insert`, in bytes of one file. Distinct from
/// [`TextEdit`], which is a rename's per-file edit and carries the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    pub from: u32,
    pub to: u32,
    pub insert: String,
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
    /// The declaration's own name span, so a caller can reveal it rather
    /// than only open its file. Byte offsets, like everything else that
    /// crosses this boundary.
    pub span: std::ops::Range<usize>,
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

/// Every `hex_color` argument literal in a file — the swatch the editor
/// draws beside it, and the value its picker edits.
///
/// Both surfaces, because both have the construct: `brink-ide` computes
/// the ink hints from the ink CST and the native ones from the native
/// CST, and a file is one or the other.
fn document_colors(
    session: &brink_ide::session::IdeSession,
    path: &str,
) -> Vec<(u32, u32, String)> {
    let Some(id) = session.file_id(path) else {
        return Vec::new();
    };
    let Some(analysis) = session.analysis() else {
        return Vec::new();
    };
    let hints = if let Some(root) = session.syntax_root(id) {
        let range = root.text_range();
        brink_ide::color::color_hints(&root, analysis, range)
    } else if let Some(root) = session.syntax_root_native(id) {
        let range = root.text_range();
        brink_ide::color::color_hints_native(&root, analysis, range)
    } else {
        Vec::new()
    };
    hints
        .into_iter()
        .map(|hint| (hint.start.into(), hint.end.into(), hint.value))
        .collect()
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
        // Answered in the worker loop, which holds the entry and the
        // file list; reaching here means something asked out of band.
        QueryKind::DocumentColors { path } => {
            QueryResult::DocumentColors(document_colors(session, path))
        }
        QueryKind::Extract {
            path,
            start,
            end,
            name,
            function,
        } => QueryResult::Structural(extract(session, path, *start, *end, name, *function)),
        QueryKind::Promote { path, knot, stitch } => {
            QueryResult::Structural(promote(session, path, knot, stitch))
        }
        QueryKind::Demote { path, knot } => QueryResult::Structural(demote(session, path, knot)),
        QueryKind::ConvertLine {
            path,
            offset,
            target,
        } => QueryResult::LineEdit(convert_line(session, path, *offset, *target)),
        QueryKind::Program
        | QueryKind::CompiledOutput
        | QueryKind::StoryGraph
        | QueryKind::Prose { .. } => QueryResult::Unavailable,
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

/// A HIR name's range as the plain byte range this boundary speaks in.
fn range_of(range: &brink_ir::TextRange) -> std::ops::Range<usize> {
    usize::from(range.start())..usize::from(range.end())
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
                span: range_of(&knot.name.range),
            });
            for stitch in &knot.stitches {
                out.push(PassageSymbol {
                    path: format!("{}.{}", knot.name.text, stitch.name.text),
                    is_stitch: true,
                    file: file.clone(),
                    span: range_of(&stitch.name.range),
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
    let source = session.source(id)?;
    let whole = rowan::TextRange::new(
        rowan::TextSize::from(0),
        rowan::TextSize::from(u32::try_from(source.len()).unwrap_or(u32::MAX)),
    );
    // The narrow symbol view, NEVER `session.analysis()`. The collector
    // reads the index and the metas only, but the analysis bundle also
    // carries a diagnostics half that re-runs every per-file check in the
    // project on any edit — so building the view from the bundle charges
    // this query for work it never looks at.
    //
    // Measured on TheIntercept (100 KB, 1686 lines), release, one mid-file
    // edit, identical 34 hints out, with `analysis()` ALREADY WARM in both
    // arms — the worker calls `analyze()` on every `Request::Edit` before it
    // serves any query, so charging the hints for that pull would overstate
    // the win: from the bundle 30.58 ms, from the index + metas 3.50 ms.
    // Same fix brink-web made for its own hints and argument widgets.
    //
    // This no longer early-returns when `session.analysis()` is None. It
    // does not need to: `symbol_index`/`symbol_meta` are self-sufficient
    // salsa queries that do not require `analyze()` to have run, and
    // `file_id`/`syntax_root` above still guard the file-not-loaded case.
    // brink-web's hints road makes the same trade.
    //
    // The on-demand queries above (hover, go-to-definition, rename) keep
    // taking `session.analysis()`: they genuinely need the bundle, and they
    // run on a keypress the author chose rather than on every character.
    let index = session.db().symbol_index();
    let symbol_meta = session.db().symbol_meta();
    let symbols = brink_ide::SymbolView {
        index: &index,
        symbol_meta: &symbol_meta,
    };
    // The native and ink frontends are distinct nominal trees, so the
    // dispatch is on the file's own language — feeding an ink-parsed root to
    // the native query would silently reproduce #2280.
    let hints = if session.is_native(id) {
        let root = session.syntax_root_native(id)?;
        brink_ide::inlay_hints::inlay_hints_native(&root, &symbols, session.db(), id, whole, None)
    } else {
        let root = session.syntax_root(id)?;
        brink_ide::inlay_hints::inlay_hints(&root, &symbols, session.db(), id, whole, None)
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

/// Lift the selected lines into a new knot or function.
fn extract(
    session: &brink_ide::session::IdeSession,
    path: &str,
    start: u32,
    end: u32,
    name: &str,
    function: bool,
) -> StructuralOutcome {
    let (start, end) = (start as usize, end as usize);
    let result = if function {
        brink_ide::extract::extract_to_function(session, path, start, end, name)
    } else {
        brink_ide::extract::extract_to_knot(session, path, start, end, name)
    };
    let what = if function { "function" } else { "knot" };
    match result {
        // Extraction gates ITSELF (`extract::gated`), so `plan` finds the
        // introduced list already filled and its own gate re-runs over the
        // same source — same answer, one extra analysis. Cheap enough at
        // author speed, and it keeps one packaging path.
        Ok(result) => plan(
            session,
            path,
            format!("Extracted `{name}` as a {what}"),
            result,
        ),
        Err(e) => StructuralOutcome::Refused(format!("{e:?}")),
    }
}

/// Lift `stitch` out of `knot` and make it a knot of its own.
fn promote(
    session: &brink_ide::session::IdeSession,
    path: &str,
    knot: &str,
    stitch: &str,
) -> StructuralOutcome {
    let Some((id, source, analysis)) = structural_parts(session, path) else {
        return StructuralOutcome::Refused(format!("{path} is not in this project."));
    };
    match brink_ide::structural_move::promote_stitch_to_knot(&source, analysis, id, knot, stitch) {
        Ok(result) => plan(
            session,
            path,
            format!("Promoted `{stitch}` to a knot"),
            result,
        ),
        Err(e) => StructuralOutcome::Refused(format!("{e:?}")),
    }
}

/// Fold `knot` into the knot above it. The destination is resolved here.
fn demote(session: &brink_ide::session::IdeSession, path: &str, knot: &str) -> StructuralOutcome {
    let Some((id, source, analysis)) = structural_parts(session, path) else {
        return StructuralOutcome::Refused(format!("{path} is not in this project."));
    };
    let Some(dest) = preceding_knot(&source, knot) else {
        return StructuralOutcome::Refused(format!(
            "`{knot}` is the first knot in {path} — there is nothing above it to demote into."
        ));
    };
    match brink_ide::structural_move::demote_knot_to_stitch(&source, analysis, id, knot, &dest) {
        Ok(result) => plan(
            session,
            path,
            format!("Demoted `{knot}` into `{dest}`"),
            result,
        ),
        Err(e) => StructuralOutcome::Refused(format!("{e:?}")),
    }
}

/// The knot declared immediately before `knot` in `source`, by the file's
/// own order.
fn preceding_knot(source: &str, knot: &str) -> Option<String> {
    let parse = brink_syntax::parse(source);
    let names: Vec<String> = parse
        .tree()
        .knots()
        .filter_map(|k| k.header().and_then(|h| h.name()))
        .collect();
    let at = names.iter().position(|n| n == knot)?;
    at.checked_sub(1).map(|before| names[before].clone())
}

fn structural_parts<'a>(
    session: &'a brink_ide::session::IdeSession,
    path: &str,
) -> Option<(brink_ir::FileId, String, &'a brink_analyzer::AnalysisResult)> {
    let id = session.file_id(path)?;
    let source = session.source(id)?.to_owned();
    let analysis = session.analysis()?;
    Some((id, source, analysis))
}

/// Run the safe-by-default gate over a structural result and package it.
fn plan(
    session: &brink_ide::session::IdeSession,
    path: &str,
    summary: String,
    result: brink_ide::structural_result::StructuralResult,
) -> StructuralOutcome {
    let Some(new_source) = result.new_source else {
        return StructuralOutcome::Refused("that move produced no text".to_owned());
    };
    let edits: Vec<TextEdit> = result
        .cross_file_edits
        .iter()
        .filter_map(|e| {
            Some(TextEdit {
                path: session.db().file_path(e.file)?.to_owned(),
                start: e.range.start().into(),
                end: e.range.end().into(),
                new_text: e.new_text.clone(),
            })
        })
        .collect();
    // The op does NOT gate itself — `move_result` returns `safe: true`
    // with nothing introduced, because the gate needs the session and the
    // op has only the text. So it is run here, on the whole-source shape
    // (`gate_with_source`), which is what a structural move produces.
    let introduced = brink_ide::structural_result::gate_with_source(
        session,
        path,
        &new_source,
        &result.cross_file_edits,
    )
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
    StructuralOutcome::Plan(Box::new(StructuralPlan {
        summary,
        path: path.to_owned(),
        new_source,
        edits,
        introduced,
    }))
}

/// One line's conversion, as a text edit. `None` for a line that cannot
/// be converted (a knot header, an `INCLUDE`) or is already the target.
fn convert_line(
    session: &brink_ide::session::IdeSession,
    path: &str,
    offset: u32,
    target: ConvertTarget,
) -> Option<LineEdit> {
    let id = session.file_id(path)?;
    let hir = session.hir(id)?;
    let source = session.source(id)?;
    let root = session.syntax_root(id)?;
    let target = match target {
        ConvertTarget::Narrative => brink_ide::line_convert::ConvertTarget::Narrative,
        ConvertTarget::Choice { sticky } => {
            brink_ide::line_convert::ConvertTarget::Choice { sticky }
        }
        ConvertTarget::Gather => brink_ide::line_convert::ConvertTarget::Gather,
        ConvertTarget::ChoiceBody => brink_ide::line_convert::ConvertTarget::ChoiceBody,
    };
    let edit = brink_ide::line_convert::convert_element(source, hir, &root, offset, target)?;
    Some(LineEdit {
        from: edit.from,
        to: edit.to,
        insert: edit.insert,
    })
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
    use super::{clamp_offset, document_colors};

    /// Whether a colour swatch can appear at all in this studio, and where
    /// from. The answer decides whether the provider is dead plumbing.
    #[test]
    fn a_colour_swatch_needs_a_host_manifest_to_declare_the_type() {
        use brink_ide::session::IdeSession;
        let mut session = IdeSession::new();
        session.update_source(
            "main.ink",
            "EXTERNAL tint(c)\n=== start ===\n~ tint(\"#ff0000\")\n-> DONE\n".to_owned(),
        );
        session.refresh_analysis();
        // With no manifest, nothing says `c` is a colour — so there is no
        // swatch to draw, however the literal is written.
        assert!(document_colors(&session, "main.ink").is_empty());

        // With one, the same call site carries a swatch. This is the whole
        // dependency: colours come from a HOST's vocabulary, and the
        // studio has no way to register one yet (`docs/studio-shell-spec.md`
        // §8) — the provider is ready for the day it does.
        session.set_host_manifest(brink_ir::host_manifest::HostManifest {
            externals: vec![brink_ir::host_manifest::ManifestExternal {
                name: "tint".to_owned(),
                params: vec![brink_ir::host_manifest::ManifestParam {
                    name: "c".to_owned(),
                    ty: brink_ir::host_manifest::TypeRef("hex_color".to_owned()),
                }],
                returns: brink_ir::host_manifest::TypeRef::default(),
                kind: brink_ir::host_manifest::ExternalKind::default(),
                doc: None,
                widgets: Vec::new(),
                path: Vec::new(),
            }],
            types: vec![brink_ir::host_manifest::SemanticTypeDef {
                name: "hex_color".to_owned(),
                base: brink_ir::host_manifest::BaseType::String,
                constraint: None,
                values: None,
                widget: Some(brink_ir::host_manifest::WidgetDecl {
                    kind: "color".to_owned(),
                }),
            }],
            ..Default::default()
        });
        let colours = document_colors(&session, "main.ink");
        assert_eq!(
            colours.len(),
            1,
            "the literal carries a swatch: {colours:?}"
        );
        assert_eq!(colours[0].2, "#ff0000");
    }

    #[test]
    fn a_selection_extracts_into_a_knot_and_leaves_a_tunnel_call_behind() {
        use super::{StructuralOutcome, extract};
        use brink_ide::session::IdeSession;
        let source = "=== shore ===\nThe tide.\nGulls argued.\n-> DONE\n";
        let mut session = IdeSession::new();
        session.update_source("main.ink", source.to_owned());
        session.refresh_analysis();
        // `Gulls argued.` — offsets inside the line; the op snaps to whole
        // lines itself, which is what makes a partial selection usable.
        let start = source.find("Gulls").expect("the line") as u32;
        let end = start + 5;
        let StructuralOutcome::Plan(plan) =
            extract(&session, "main.ink", start, end, "gulls", false)
        else {
            panic!("extract refused");
        };
        assert!(
            plan.new_source.contains("=== gulls ==="),
            "{}",
            plan.new_source
        );
        assert!(
            plan.new_source.contains("-> gulls ->"),
            "{}",
            plan.new_source
        );
        assert_eq!(plan.summary, "Extracted `gulls` as a knot");

        // A selection that crosses a knot header is refused rather than
        // relocating the declaration.
        let StructuralOutcome::Refused(_) = extract(&session, "main.ink", 0, end, "x", false)
        else {
            panic!("crossing a header must refuse");
        };
    }

    #[test]
    fn a_stitch_promotes_to_a_knot_and_a_knot_demotes_into_the_one_above_it() {
        use super::{StructuralOutcome, demote, promote};
        use brink_ide::session::IdeSession;
        let source = "=== shore ===\nThe tide.\n= linger\nGulls.\n\n\
                      === lighthouse ===\nThe door.\n-> DONE\n";
        let mut session = IdeSession::new();
        session.update_source("main.ink", source.to_owned());
        session.refresh_analysis();

        let StructuralOutcome::Plan(plan) = promote(&session, "main.ink", "shore", "linger") else {
            panic!("promote refused");
        };
        assert!(
            plan.new_source.contains("=== linger ==="),
            "{}",
            plan.new_source
        );
        assert_eq!(plan.summary, "Promoted `linger` to a knot");

        let StructuralOutcome::Plan(plan) = demote(&session, "main.ink", "lighthouse") else {
            panic!("demote refused");
        };
        assert!(
            plan.new_source.contains("= lighthouse"),
            "{}",
            plan.new_source
        );
        assert_eq!(plan.summary, "Demoted `lighthouse` into `shore`");

        // The FIRST knot has nothing above it, and is told so rather than
        // silently doing nothing.
        let StructuralOutcome::Refused(why) = demote(&session, "main.ink", "shore") else {
            panic!("the first knot must refuse");
        };
        assert!(why.contains("nothing above it"), "{why}");
    }

    #[test]
    fn a_line_converts_between_narrative_choice_and_gather() {
        use super::{ConvertTarget, convert_line};
        use brink_ide::session::IdeSession;
        let source = "=== shore ===\nThe tide was out.\n-> DONE\n";
        let mut session = IdeSession::new();
        session.update_source("main.ink", source.to_owned());
        session.refresh_analysis();
        // Byte 14 is the start of `The tide was out.`
        let at = 14;
        let edit = convert_line(
            &session,
            "main.ink",
            at,
            ConvertTarget::Choice { sticky: false },
        )
        .expect("narrative converts to a choice");
        let out = format!(
            "{}{}{}",
            &source[..edit.from as usize],
            edit.insert,
            &source[edit.to as usize..]
        );
        assert_eq!(out, "=== shore ===\n* The tide was out.\n-> DONE\n");

        let gather =
            convert_line(&session, "main.ink", at, ConvertTarget::Gather).expect("and to a gather");
        assert!(gather.insert.starts_with('-'), "{gather:?}");

        // A knot header is not a weave element and converts to nothing —
        // returned as `None` rather than as an edit that mangles it.
        assert!(convert_line(&session, "main.ink", 0, ConvertTarget::Gather).is_none());
        // Neither does a file the project does not hold.
        assert!(convert_line(&session, "nope.ink", 0, ConvertTarget::Gather).is_none());
    }

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
