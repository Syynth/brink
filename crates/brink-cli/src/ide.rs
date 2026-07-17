//! `brink ide` — scriptable brink-ide queries and refactors (epic #289).
//!
//! The project loader + name/`--at` addressing + output framework; the
//! read-queries `def`, `references`, `symbols`, `unused`, `check`, `hover`,
//! `signature`, `graph` (story flow → text/JSON/DOT), `lines`, and `actions`
//! (code actions at a cursor); and the mutations — `rename`, `move-file`
//! (with `INCLUDE` rewriting), and `refactor *` (sort / reorder / move-stitch /
//! promote / demote / convert-line). Every mutation shares the same modes:
//! preview (default) / `--patch [FILE]` / `--write`, safe-by-default against
//! newly-introduced diagnostics (`--unsafe` overrides). The CLI drives the same
//! `brink-ide` engine the LSP and studio use, via a `brink_driver::Driver` that
//! discovers the project from an entry `.ink` (following `INCLUDE`s) — identical
//! to `brink compile`. See `docs/cli-ide-inventory.md`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use brink_analyzer::AnalysisResult;
use brink_driver::Driver;
use brink_ide::LineIndex;
use brink_ide::code_actions::{CodeActionKind, code_actions};
use brink_ide::document::{DocumentSymbol, document_symbols, workspace_symbols};
use brink_ide::file_rename::rename_file;
use brink_ide::formatting::{format_region, sort_knots_in_source, sort_stitches_in_knot};
use brink_ide::hover::hover;
use brink_ide::line_context::line_contexts;
use brink_ide::line_convert::{ConvertTarget, convert_element};
use brink_ide::navigation::{find_def_at_offset, find_references};
use brink_ide::rename::{FileEdit, rename};
use brink_ide::session::IdeSession;
use brink_ide::signature::signature_help;
use brink_ide::story_graph::{StoryEdgeKind, StoryGraph, StoryNodeKind, story_graph};
use brink_ide::structural_move::{
    Direction, demote_knot_to_stitch, move_stitch, promote_stitch_to_knot, reorder_knot,
    reorder_knots, reorder_stitch, reorder_stitches,
};
use brink_ide::structural_result::StructuralResult;
use brink_ir::symbols::{SymbolInfo, SymbolKind};
use brink_ir::{Diagnostic, FileId, HirFile};
use clap::{Args, Subcommand, ValueEnum};
use rowan::TextRange;

// ── CLI surface ─────────────────────────────────────────────────────

/// Scriptable IDE queries over an ink project (epic #289).
///
/// Address a symbol by its qualified name (`intro`, `intro.evidence`,
/// `Colors.Red`) — the same dotted paths ink uses. Output is human-readable by
/// default, or `--format json` for scripting. Exit codes: 0 ok, 1 query-false,
/// 2 usage error.
#[derive(Subcommand)]
pub enum IdeCommand {
    /// Print where a symbol is defined.
    #[command(after_help = "\
Examples:
  brink ide def intro --entry main.ink
  brink ide def intro.evidence -e main.ink --format json
  brink ide def --at main.ink:7:5 -e main.ink   # the symbol under that cursor")]
    Def {
        #[command(flatten)]
        addr: Address,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// List the references to a symbol across the project.
    #[command(after_help = "\
Examples:
  brink ide references gold -e main.ink
  brink ide references intro --exists -e main.ink   # exit 0 if used, 1 if not
  brink ide references gold --count -e main.ink --format json")]
    References {
        #[command(flatten)]
        addr: Address,
        /// Include the declaration site in the results.
        #[arg(long)]
        include_decl: bool,
        /// Print nothing; exit 0 if the symbol is referenced, 1 if not.
        #[arg(long)]
        exists: bool,
        /// Print only the number of references.
        #[arg(long)]
        count: bool,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// List a file's outline, or search symbols across the project.
    #[command(after_help = "\
Examples:
  brink ide symbols -e main.ink                  # outline of the entry file
  brink ide symbols --file scenes/intro.ink -e main.ink
  brink ide symbols --search gold -e main.ink    # project-wide name search
  brink ide symbols --kind knot -e main.ink")]
    Symbols {
        /// Outline this file instead of the entry file.
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        /// Project-wide substring search by name instead of an outline.
        #[arg(long, value_name = "QUERY")]
        search: Option<String>,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// List declared symbols that have no references (dead-code lint).
    ///
    /// Exit 1 if any are found. Note: this is reference-based, not reachability
    /// — an entry knot reached implicitly (no `->`) can show up here.
    #[command(after_help = "\
Examples:
  brink ide unused -e main.ink
  brink ide unused --kind variable -e main.ink
  brink ide unused -e main.ink --format json")]
    Unused {
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Report project diagnostics (exit 1 if any error).
    #[command(after_help = "\
Examples:
  brink ide check -e main.ink
  brink ide check -e main.ink --format json")]
    Check {
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Rename a symbol and all its references across the project.
    ///
    /// Default output is a preview (the edits + any diagnostics the rename would
    /// introduce). `--patch`/`--write` are safe-by-default: they refuse to
    /// produce output if the rename introduces a new error, unless `--unsafe`.
    #[command(after_help = "\
Examples:
  brink ide rename gold --to coins -e main.ink            # preview the edits
  brink ide rename gold --to coins --patch -e main.ink    # git-applyable diff to stdout
  brink ide rename gold --to coins --patch out.diff -e main.ink
  brink ide rename --at main.ink:5:5 --to newname --write -e main.ink")]
    Rename {
        #[command(flatten)]
        addr: Address,
        /// The new name for the symbol.
        #[arg(long = "to", value_name = "NEW_NAME")]
        new_name: String,
        /// Emit a `git apply`-able patch to stdout, or to FILE if given.
        #[arg(long, value_name = "FILE", num_args = 0..=1, default_missing_value = "-", conflicts_with = "write")]
        patch: Option<String>,
        /// Apply the edits to the project files in place.
        #[arg(long)]
        write: bool,
        /// Produce the patch / write even if the rename introduces new errors.
        #[arg(long = "unsafe", visible_alias = "force")]
        unsafe_mode: bool,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Show hover info (kind, signature, docs) for a symbol.
    #[command(after_help = "\
Examples:
  brink ide hover gold -e main.ink
  brink ide hover --at main.ink:5:5 -e main.ink")]
    Hover {
        #[command(flatten)]
        addr: Address,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Show the signature of the call at a cursor position.
    #[command(after_help = "\
Example:
  brink ide signature --at main.ink:8:14 -e main.ink")]
    Signature {
        /// Cursor inside the call: `FILE:LINE:COL` (1-based).
        #[arg(long, value_name = "FILE:LINE:COL")]
        at: String,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Print the story flow graph (knots/stitches and their diverts/choices).
    #[command(after_help = "\
Examples:
  brink ide graph -e main.ink
  brink ide graph --dot -e main.ink | dot -Tsvg -o story.svg")]
    Graph {
        /// Emit Graphviz DOT instead of text/JSON.
        #[arg(long)]
        dot: bool,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Print a file's per-line structural classification.
    #[command(after_help = "\
Examples:
  brink ide lines -e main.ink
  brink ide lines --file scenes/intro.ink -e main.ink --format json")]
    Lines {
        /// Classify this file instead of the entry file.
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Rename or move a file, rewriting every `INCLUDE` that points at it.
    ///
    /// Paths are project-relative (as they appear in `INCLUDE`s). Like every
    /// mutation: preview by default, `--patch`/`--write` are safe-by-default.
    #[command(after_help = "\
Examples:
  brink ide move-file scenes/intro.ink scenes/act1/intro.ink -e main.ink
  brink ide move-file old.ink new.ink --patch -e main.ink
  brink ide move-file old.ink new.ink --write -e main.ink")]
    MoveFile {
        /// Current project-relative path of the file.
        #[arg(value_name = "OLD")]
        old: String,
        /// New project-relative path for the file.
        #[arg(value_name = "NEW")]
        new: String,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Apply a structural refactor (sort / reorder / move / promote / convert).
    ///
    /// Each operation reuses the shared mutation modes (preview / `--patch` /
    /// `--write`) and the safe-by-default diagnostic gate.
    Refactor {
        #[command(subcommand)]
        op: RefactorOp,
    },
    /// List the code actions (refactors) available at a cursor position.
    #[command(after_help = "\
Examples:
  brink ide actions --at main.ink:8:3 -e main.ink
  brink ide actions --at main.ink:8:3 -e main.ink --format json")]
    Actions {
        /// Cursor position: `FILE:LINE:COL` (1-based).
        #[arg(long, value_name = "FILE:LINE:COL")]
        at: String,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Diff inferred effect rows between two revisions (or a revision and
    /// the working tree) — drift *visibility*, not a gate (docs/effects-spec.md
    /// §10, sitting 2: "there is no drift policy — Drift visibility is
    /// tooling"). CI-comment-friendly: exits `0` with no output when nothing
    /// drifted, `1` with a unified-diff-shaped report otherwise.
    #[command(after_help = "\
Examples:
  brink ide effects-diff -e main.ink --base HEAD~1             # working tree vs. last commit
  brink ide effects-diff -e main.ink --base origin/main        # working tree vs. a branch
  brink ide effects-diff -e main.ink --base main --head feature/foo  # rev vs. rev
  brink ide effects-diff -e main.ink --base HEAD~1 --format json    # CI-comment-friendly")]
    EffectsDiff {
        /// The revision to diff against (any git commit-ish: SHA, branch, tag, `HEAD~N`).
        #[arg(long, value_name = "REV")]
        base: String,
        /// Diff against this revision instead of the working tree.
        #[arg(long, value_name = "REV")]
        head: Option<String>,
        #[command(flatten)]
        opts: EffectsDiffOpts,
    },
}

/// Options for `effects-diff`: entry + format, no `--kind` (it addresses
/// every knot/stitch in the project, never a single symbol).
#[derive(Args)]
pub struct EffectsDiffOpts {
    /// Entry-point `.ink` file; `INCLUDE`s are followed to build the project.
    #[arg(long, short = 'e', value_name = "FILE")]
    entry: PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

/// A structural refactor operation. Each addresses a knot/stitch by its
/// qualified name and routes through the shared mutation modes.
#[derive(Subcommand)]
pub enum RefactorOp {
    /// Alphabetize the top-level knots in a file (preamble preserved).
    SortKnots {
        /// Operate on this file instead of the entry file.
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Alphabetize the stitches within a knot.
    SortStitches {
        /// The knot whose stitches to sort.
        #[arg(value_name = "KNOT")]
        knot: String,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Reformat just one knot (or one stitch with `KNOT.STITCH`).
    Format {
        /// `KNOT` or `KNOT.STITCH`.
        #[arg(value_name = "KNOT[.STITCH]")]
        target: String,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Move a knot up or down in the file (pure text, no reference changes).
    ReorderKnot {
        #[arg(value_name = "KNOT")]
        knot: String,
        #[arg(value_name = "DIRECTION")]
        direction: Dir,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Move a stitch up or down within its knot (`KNOT.STITCH`).
    ReorderStitch {
        #[arg(value_name = "KNOT.STITCH")]
        target: String,
        #[arg(value_name = "DIRECTION")]
        direction: Dir,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Reorder all top-level knots to an explicit permutation.
    ReorderKnots {
        /// Comma-separated knot names, a permutation of the existing set.
        #[arg(value_name = "A,B,C")]
        order: String,
        /// Operate on this file instead of the entry file.
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Reorder a knot's stitches to an explicit permutation.
    ReorderStitches {
        #[arg(value_name = "KNOT")]
        knot: String,
        /// Comma-separated stitch names, a permutation of the existing set.
        #[arg(value_name = "A,B,C")]
        order: String,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Move a stitch into another knot, re-qualifying references.
    MoveStitch {
        /// The stitch to move: `KNOT.STITCH`.
        #[arg(value_name = "KNOT.STITCH")]
        target: String,
        /// The destination knot.
        #[arg(long = "to", value_name = "DEST_KNOT")]
        dest: String,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Promote a stitch to a top-level knot (`KNOT.STITCH`).
    PromoteStitch {
        #[arg(value_name = "KNOT.STITCH")]
        target: String,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Demote a knot to a stitch under another knot.
    DemoteKnot {
        #[arg(value_name = "KNOT")]
        knot: String,
        /// The destination knot to nest under.
        #[arg(long = "to", value_name = "DEST_KNOT")]
        dest: String,
        #[command(flatten)]
        mode: MutOpts,
    },
    /// Convert a line's structural type at a cursor, preserving weave depth.
    #[command(after_help = "\
Example:
  brink ide refactor convert-line --at main.ink:9:1 choice -e main.ink")]
    ConvertLine {
        /// The line to convert: `FILE:LINE:COL` (1-based).
        #[arg(long, value_name = "FILE:LINE:COL")]
        at: String,
        /// What to convert the line into.
        #[arg(value_name = "TARGET")]
        target: ConvertTo,
        #[command(flatten)]
        mode: MutOpts,
    },
}

/// Options shared by every `brink ide` query.
#[derive(Args)]
pub struct CommonOpts {
    /// Entry-point `.ink` file; `INCLUDE`s are followed to build the project.
    #[arg(long, short = 'e', value_name = "FILE")]
    entry: PathBuf,
    /// Disambiguate when a name matches multiple symbol kinds.
    #[arg(long, value_enum, value_name = "KIND")]
    kind: Option<KindFilter>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

/// How a query addresses its target — by qualified name or by cursor position.
#[derive(Args)]
pub struct Address {
    /// Qualified symbol name (knot / knot.stitch / List.Item / var / …).
    symbol: Option<String>,
    /// Address by cursor position instead: `FILE:LINE:COL` (1-based line & column).
    #[arg(long, value_name = "FILE:LINE:COL", conflicts_with = "symbol")]
    at: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

/// Options shared by every mutating command (move-file, refactor *). Unlike
/// `CommonOpts` there is no `--kind`: these address by knot/stitch name or
/// cursor, never by a bare ambiguous symbol.
#[derive(Args)]
pub struct MutOpts {
    /// Entry-point `.ink` file; `INCLUDE`s are followed to build the project.
    #[arg(long, short = 'e', value_name = "FILE")]
    entry: PathBuf,
    /// Output format (preview / JSON).
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    #[command(flatten)]
    flags: ModeFlags,
}

/// The mutually-exclusive mutation output mode (default: preview).
#[derive(Args)]
pub struct ModeFlags {
    /// Emit a `git apply`-able patch to stdout, or to FILE if given.
    #[arg(long, value_name = "FILE", num_args = 0..=1, default_missing_value = "-", conflicts_with = "write")]
    patch: Option<String>,
    /// Apply the edits to the project files in place.
    #[arg(long)]
    write: bool,
    /// Produce the patch / write even if the change introduces new diagnostics.
    #[arg(long = "unsafe", visible_alias = "force")]
    unsafe_mode: bool,
}

impl ModeFlags {
    fn mode(&self) -> Mode<'_> {
        match (&self.patch, self.write) {
            (Some(dest), _) => Mode::Patch(dest),
            (None, true) => Mode::Write,
            (None, false) => Mode::Preview,
        }
    }
}

/// Direction for the `reorder-knot` / `reorder-stitch` refactors.
#[derive(Clone, Copy, ValueEnum)]
pub enum Dir {
    Up,
    Down,
}

impl From<Dir> for Direction {
    fn from(d: Dir) -> Self {
        match d {
            Dir::Up => Direction::Up,
            Dir::Down => Direction::Down,
        }
    }
}

/// Target structural type for `refactor convert-line`.
#[derive(Clone, Copy, ValueEnum)]
pub enum ConvertTo {
    /// Plain narrative text (strip sigils).
    Narrative,
    /// A choice line (`*`).
    Choice,
    /// A sticky choice line (`+`).
    StickyChoice,
    /// A gather line (`-`).
    Gather,
    /// Indented choice body (strip sigils, keep depth).
    ChoiceBody,
}

impl From<ConvertTo> for ConvertTarget {
    fn from(t: ConvertTo) -> Self {
        match t {
            ConvertTo::Narrative => ConvertTarget::Narrative,
            ConvertTo::Choice => ConvertTarget::Choice { sticky: false },
            ConvertTo::StickyChoice => ConvertTarget::Choice { sticky: true },
            ConvertTo::Gather => ConvertTarget::Gather,
            ConvertTo::ChoiceBody => ConvertTarget::ChoiceBody,
        }
    }
}

/// The symbol kinds addressable from the CLI (mirrors `SymbolKind`).
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KindFilter {
    Knot,
    Stitch,
    Variable,
    Constant,
    List,
    ListItem,
    External,
    Label,
    Param,
    Temp,
}

impl KindFilter {
    fn matches(self, k: SymbolKind) -> bool {
        matches!(
            (self, k),
            (Self::Knot, SymbolKind::Knot)
                | (Self::Stitch, SymbolKind::Stitch)
                | (Self::Variable, SymbolKind::Variable)
                | (Self::Constant, SymbolKind::Constant)
                | (Self::List, SymbolKind::List)
                | (Self::ListItem, SymbolKind::ListItem)
                | (Self::External, SymbolKind::External)
                | (Self::Label, SymbolKind::Label)
                | (Self::Param, SymbolKind::Param)
                | (Self::Temp, SymbolKind::Temp)
        )
    }
}

fn kind_name(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Knot => "knot",
        SymbolKind::Stitch => "stitch",
        SymbolKind::Variable => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::List => "list",
        SymbolKind::ListItem => "list-item",
        SymbolKind::External => "external",
        SymbolKind::Label => "label",
        SymbolKind::Param => "param",
        SymbolKind::Temp => "temp",
        SymbolKind::Struct => "struct",
    }
}

// ── Entry point ─────────────────────────────────────────────────────

/// Run a `brink ide` subcommand. Returns the process exit code.
pub fn run(cmd: &IdeCommand) -> ExitCode {
    let result = match cmd {
        IdeCommand::Def { addr, opts } => run_def(addr, opts),
        IdeCommand::References {
            addr,
            include_decl,
            exists,
            count,
            opts,
        } => run_references(addr, *include_decl, *exists, *count, opts),
        IdeCommand::Symbols { file, search, opts } => {
            run_symbols(file.as_deref(), search.as_deref(), opts)
        }
        IdeCommand::Unused { opts } => run_unused(opts),
        IdeCommand::Check { opts } => run_check(opts),
        IdeCommand::Rename {
            addr,
            new_name,
            patch,
            write,
            unsafe_mode,
            opts,
        } => run_rename(addr, new_name, patch.as_deref(), *write, *unsafe_mode, opts),
        IdeCommand::Hover { addr, opts } => run_hover(addr, opts),
        IdeCommand::Signature { at, opts } => run_signature(at, opts),
        IdeCommand::Graph { dot, opts } => run_graph(*dot, opts),
        IdeCommand::Lines { file, opts } => run_lines(file.as_deref(), opts),
        IdeCommand::MoveFile { old, new, mode } => run_move_file(old, new, mode),
        IdeCommand::Refactor { op } => run_refactor(op),
        IdeCommand::Actions { at, opts } => run_actions(at, opts),
        IdeCommand::EffectsDiff { base, head, opts } => {
            run_effects_diff(base, head.as_deref(), opts)
        }
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            let _ = writeln!(io::stderr(), "error: {msg}");
            ExitCode::from(2)
        }
    }
}

// ── Commands ────────────────────────────────────────────────────────

fn run_def(addr: &Address, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve(addr, opts.kind)?;
    let loc = project.location_of(sym.file, sym.range);

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Text => writeln!(out, "{} {}", kind_name(sym.kind), loc.display()),
        Format::Json => writeln!(
            out,
            "{}",
            serde_json::json!({ "name": sym.name, "kind": kind_name(sym.kind), "location": loc })
        ),
    }
    .map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

fn run_references(
    addr: &Address,
    include_decl: bool,
    exists: bool,
    count: bool,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve(addr, opts.kind)?;
    // The definition offset is a valid query position: find_references resolves
    // the symbol there and collects every use (optionally including the decl).
    let refs = find_references(&project.analysis, sym.file, sym.range.start(), include_decl);

    if exists {
        return Ok(if refs.is_empty() {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }

    let locs: Vec<Loc> = refs
        .iter()
        .map(|r| project.location_of(r.file, r.range))
        .collect();

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Text => {
            if count {
                writeln!(out, "{}", locs.len()).map_err(|e| e.to_string())?;
            } else {
                for loc in &locs {
                    writeln!(out, "{}", loc.display()).map_err(|e| e.to_string())?;
                }
            }
        }
        Format::Json => writeln!(
            out,
            "{}",
            serde_json::json!({
                "name": sym.name,
                "kind": kind_name(sym.kind),
                "count": locs.len(),
                "references": locs,
            })
        )
        .map_err(|e| e.to_string())?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_symbols(
    file: Option<&str>,
    search: Option<&str>,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let mut out = io::stdout().lock();

    let entries: Vec<SymEntry> = if let Some(query) = search {
        // Project-wide name search (flat list).
        workspace_symbols(std::iter::once(&project.analysis), query)
            .into_iter()
            .filter(|s| opts.kind.is_none_or(|k| k.matches(s.kind)))
            .map(|s| SymEntry {
                name: s.name,
                kind: kind_name(s.kind).into(),
                detail: None,
                location: project.location_of(s.file, s.range),
                children: Vec::new(),
            })
            .collect()
    } else {
        // Outline of one file (default: the entry file). The full tree is kept;
        // `--kind` applies to search/unused, not the hierarchical outline.
        let db = project.driver.db();
        let file_id = match file {
            Some(f) => db
                .file_id(f)
                .ok_or_else(|| format!("file not in project: {f}"))?,
            None => project.entry_id,
        };
        let hir = db.hir(file_id).ok_or("no HIR for that file")?;
        let manifest = db.manifest(file_id).ok_or("no manifest for that file")?;
        let source = db.source(file_id).unwrap_or_default();
        document_symbols(hir, manifest, source)
            .iter()
            .map(|d| doc_to_entry(&project, file_id, d))
            .collect()
    };

    match opts.format {
        Format::Json => {
            writeln!(out, "{}", to_json(&entries)?).map_err(|e| e.to_string())?;
        }
        Format::Text => print_tree(&mut out, &entries, 0)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_unused(opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let mut unused: Vec<SymEntry> = project
        .analysis
        .index
        .symbols
        .values()
        .filter(|info| opts.kind.is_none_or(|k| k.matches(info.kind)))
        .filter(|info| {
            find_references(&project.analysis, info.file, info.range.start(), false).is_empty()
        })
        .map(|info| SymEntry {
            name: info.name.clone(),
            kind: kind_name(info.kind).into(),
            detail: None,
            location: project.location_of(info.file, info.range),
            children: Vec::new(),
        })
        .collect();
    unused.sort_by(|a, b| {
        (&a.location.path, a.location.byte_start).cmp(&(&b.location.path, b.location.byte_start))
    });

    let any = !unused.is_empty();
    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => writeln!(out, "{}", to_json(&unused)?).map_err(|e| e.to_string())?,
        Format::Text => {
            for e in &unused {
                writeln!(out, "{} {} {}", e.kind, e.name, e.location.display())
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(if any {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn run_check(opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let report = project
        .driver
        .collect_diagnostics(&project.analysis, Some(project.entry_id));

    let mut diags: Vec<DiagEntry> = report
        .errors
        .iter()
        .map(|d| project.diag_entry(d, "error"))
        .chain(
            report
                .warnings
                .iter()
                .map(|d| project.diag_entry(d, "warning")),
        )
        .collect();
    diags.sort_by(|a, b| {
        (&a.location.path, a.location.byte_start).cmp(&(&b.location.path, b.location.byte_start))
    });

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => writeln!(out, "{}", to_json(&diags)?).map_err(|e| e.to_string())?,
        Format::Text => {
            for d in &diags {
                writeln!(
                    out,
                    "{}[{}] {} {}",
                    d.severity,
                    d.code,
                    d.location.display(),
                    d.message
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(if report.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// What a mutation does with its computed edits.
#[derive(Clone, Copy)]
enum Mode<'a> {
    Preview,
    /// Emit a `git apply`-able patch — to stdout (`"-"`) or to a file path.
    Patch(&'a str),
    Write,
}

fn run_rename(
    addr: &Address,
    new_name: &str,
    patch: Option<&str>,
    write: bool,
    unsafe_mode: bool,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve(addr, opts.kind)?;
    let result =
        rename(&project.analysis, sym.file, sym.range.start(), new_name).ok_or_else(|| {
            format!(
                "'{}' cannot be renamed (a built-in or unresolved symbol)",
                sym.name
            )
        })?;
    if result.edits.is_empty() {
        return Err("rename produced no edits".to_string());
    }

    // Apply the edits in-memory; `emit_mutation` re-analyzes to gate on any new
    // diagnostic. Rename carries its fine-grained edits for a per-edit preview.
    let edited = project.apply_edits(&result.edits)?;
    let mode = match (patch, write) {
        (Some(dest), _) => Mode::Patch(dest),
        (None, true) => Mode::Write,
        (None, false) => Mode::Preview,
    };
    let mutation = Mutation {
        edited,
        edits: Some(result.edits),
    };
    emit_mutation(
        &project,
        &opts.entry,
        &mutation,
        &mode,
        opts.format,
        unsafe_mode,
    )
}

// ── Mutation pipeline (rename / move-file / refactor *) ──────────────

/// A computed mutation ready to emit: the new full source for every file it
/// touches, plus optional fine-grained edits for a richer preview.
struct Mutation {
    /// path → new full source, for every file the operation changes.
    edited: BTreeMap<String, String>,
    /// Fine-grained edits (rename) for a per-edit preview; `None` → diff preview.
    edits: Option<Vec<FileEdit>>,
}

/// Emit a mutation through the requested mode, applying the safe-by-default
/// diagnostic gate. `preview` always informs (prints edits + introduced
/// diagnostics, exit 0); `--patch`/`--write` refuse on any newly-introduced
/// diagnostic unless `unsafe_mode`. Returns the process exit code.
fn emit_mutation(
    project: &Project,
    entry: &Path,
    mutation: &Mutation,
    mode: &Mode,
    format: Format,
    unsafe_mode: bool,
) -> Result<ExitCode, String> {
    let introduced = project.introduced_diagnostics(entry, &mutation.edited, None)?;

    if !matches!(mode, Mode::Preview) && !introduced.is_empty() && !unsafe_mode {
        let mut err = io::stderr().lock();
        writeln!(
            err,
            "refusing: change introduces {} new diagnostic(s) (re-run with --unsafe to override):",
            introduced.len()
        )
        .map_err(|e| e.to_string())?;
        for d in &introduced {
            writeln!(
                err,
                "  {}[{}] {} {}",
                d.severity,
                d.code,
                d.location.display(),
                d.message
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(ExitCode::from(1));
    }

    let mut out = io::stdout().lock();
    match mode {
        Mode::Preview => {
            if let Some(edits) = &mutation.edits {
                let entries = project.edit_entries(edits);
                emit_rename_preview(&mut out, format, &entries, &introduced)?;
            } else {
                emit_diff_preview(&mut out, project, &mutation.edited, format, &introduced)?;
            }
        }
        Mode::Patch(dest) => {
            let diff = project.unified_diff(&mutation.edited)?;
            if *dest == "-" {
                write!(out, "{diff}").map_err(|e| e.to_string())?;
            } else {
                std::fs::write(dest, diff).map_err(|e| format!("{dest}: {e}"))?;
            }
        }
        Mode::Write => {
            for (path, src) in &mutation.edited {
                std::fs::write(path, src).map_err(|e| format!("{path}: {e}"))?;
            }
            writeln!(out, "wrote {} file(s)", mutation.edited.len()).map_err(|e| e.to_string())?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Preview a whole-file mutation as a unified diff (text) or
/// `{ diff, files, introducedDiagnostics, safe }` (JSON), plus the diagnostics
/// it would introduce.
fn emit_diff_preview(
    out: &mut impl Write,
    project: &Project,
    edited: &BTreeMap<String, String>,
    format: Format,
    introduced: &[DiagEntry],
) -> Result<(), String> {
    let diff = project.unified_diff(edited)?;
    match format {
        Format::Json => {
            let files: Vec<&String> = edited.keys().collect();
            let v = serde_json::json!({
                "diff": diff,
                "files": files,
                "introducedDiagnostics": introduced,
                "safe": introduced.is_empty(),
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            write!(out, "{diff}").map_err(|e| e.to_string())?;
            if !introduced.is_empty() {
                writeln!(
                    out,
                    "would introduce {} new diagnostic(s):",
                    introduced.len()
                )
                .map_err(|e| e.to_string())?;
                for d in introduced {
                    writeln!(
                        out,
                        "  {}[{}] {} {}",
                        d.severity,
                        d.code,
                        d.location.display(),
                        d.message
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}

/// Render a rename preview — the edits plus any diagnostics it would introduce.
fn emit_rename_preview(
    out: &mut impl Write,
    format: Format,
    entries: &[EditEntry],
    introduced: &[DiagEntry],
) -> Result<(), String> {
    match format {
        Format::Json => {
            let v = serde_json::json!({
                "edits": entries,
                "introducedDiagnostics": introduced,
                "safe": introduced.is_empty(),
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            for e in entries {
                writeln!(out, "{}  {} -> {}", e.location.display(), e.old, e.new)
                    .map_err(|x| x.to_string())?;
            }
            if !introduced.is_empty() {
                writeln!(
                    out,
                    "would introduce {} new diagnostic(s):",
                    introduced.len()
                )
                .map_err(|x| x.to_string())?;
                for d in introduced {
                    writeln!(
                        out,
                        "  {}[{}] {} {}",
                        d.severity,
                        d.code,
                        d.location.display(),
                        d.message
                    )
                    .map_err(|x| x.to_string())?;
                }
            }
        }
    }
    Ok(())
}

fn run_hover(addr: &Address, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve(addr, opts.kind)?;
    let db = project.driver.db();
    let source = db.source(sym.file).unwrap_or_default();
    let ids: Vec<FileId> = db.file_ids().collect();
    let project_files: Vec<(FileId, String, String)> = ids
        .iter()
        .filter_map(|&id| {
            Some((
                id,
                db.file_path(id)?.to_string(),
                db.source(id)?.to_string(),
            ))
        })
        .collect();
    let info = hover(
        &project.analysis,
        db,
        sym.file,
        source,
        sym.range.start(),
        &project_files,
    )
    .ok_or("no hover information for that symbol")?;

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let v = serde_json::json!({
                "content": info.content,
                "location": project.location_of(sym.file, sym.range),
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => writeln!(out, "{}", info.content.trim_end()).map_err(|e| e.to_string())?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_signature(at: &str, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let (file, line, col) = parse_at(at)?;
    let db = project.driver.db();
    let file_id = db
        .file_id(&file)
        .ok_or_else(|| format!("file not in project: {file}"))?;
    let source = db.source(file_id).unwrap_or_default();
    let offset = LineIndex::new(source).offset(line.saturating_sub(1), col.saturating_sub(1));
    let sig = signature_help(&project.analysis, source, u32::from(offset) as usize)
        .ok_or("no call signature at that position")?;

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let params: Vec<&str> = sig.parameters.iter().map(|p| p.label.as_str()).collect();
            let v = serde_json::json!({
                "label": sig.label,
                "documentation": sig.documentation,
                "parameters": params,
                "activeParameter": sig.active_parameter,
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            writeln!(out, "{}", sig.label).map_err(|e| e.to_string())?;
            if let Some(doc) = &sig.documentation {
                writeln!(out, "{doc}").map_err(|e| e.to_string())?;
            }
            if let Some(p) = sig.parameters.get(sig.active_parameter as usize) {
                writeln!(out, "active parameter: {}", p.label).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_graph(dot: bool, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let db = project.driver.db();
    let ids: Vec<FileId> = db.file_ids().collect();
    let files: Vec<(FileId, &HirFile)> = ids
        .iter()
        .filter_map(|&id| db.hir(id).map(|h| (id, h)))
        .collect();
    let graph = story_graph(&project.analysis, &files);

    let mut out = io::stdout().lock();
    if dot {
        write_graph_dot(&mut out, &graph)?;
    } else {
        match opts.format {
            Format::Json => {
                writeln!(out, "{}", to_json(&graph_json(&graph))?).map_err(|e| e.to_string())?;
            }
            Format::Text => {
                for n in &graph.nodes {
                    writeln!(out, "{} {}", node_kind_name(n.kind), n.id)
                        .map_err(|e| e.to_string())?;
                }
                for e in &graph.edges {
                    writeln!(out, "{} --{}-> {}", e.from, edge_kind_name(e.kind), e.to)
                        .map_err(|x| x.to_string())?;
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_lines(file: Option<&str>, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let db = project.driver.db();
    let file_id = match file {
        Some(f) => db
            .file_id(f)
            .ok_or_else(|| format!("file not in project: {f}"))?,
        None => project.entry_id,
    };
    let hir = db.hir(file_id).ok_or("no HIR for that file")?;
    let source = db.source(file_id).unwrap_or_default();
    let root = db
        .parse(file_id)
        .ok_or("no parse tree for that file")?
        .syntax();
    let projection = brink_ide::hir_projection::project_hir_structural(hir, source);
    let ctxs = line_contexts(source, &root, &projection);

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let arr: Vec<_> = ctxs
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    serde_json::json!({
                        "line": i + 1,
                        "element": format!("{:?}", c.element),
                        "depth": c.weave.depth,
                    })
                })
                .collect();
            writeln!(out, "{}", to_json(&arr)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            for (i, c) in ctxs.iter().enumerate() {
                writeln!(out, "{}: {:?} depth={}", i + 1, c.element, c.weave.depth)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ── Mutating commands: move-file / refactor / actions ───────────────

fn run_move_file(old: &str, new: &str, mode: &MutOpts) -> Result<ExitCode, String> {
    let project = Project::load(&mode.entry)?;
    let session = project.ide_session();
    let result = rename_file(&session, old, new).map_err(|e| e.to_string())?;

    // A file move changes the file *set*: the old path is removed and `new`
    // appears with `result.new_source`. Inbound `INCLUDE` rewrites land on other
    // files; the moved file itself is covered by `new_source`.
    let new_source = result
        .new_source
        .ok_or("file rename produced no primary source")?;
    let mut edited = project.apply_edits(&result.cross_file_edits)?;
    edited.remove(old);
    edited.insert(new.to_string(), new_source);

    // The destination is a brand-new path, so the whole-project re-analysis in
    // the safety gate must read it. `introduced_diagnostics` already overlays the
    // edited map onto the on-disk read closure, so `new` resolves to its content.
    let mutation = Mutation {
        edited,
        edits: None,
    };
    emit_move_mutation(&project, &mode.entry, old, new, &mutation, mode)
}

/// Emit a file move. Like `emit_mutation`, but the diff/write must account for
/// the path change (delete `old`, create `new`) rather than an in-place edit.
fn emit_move_mutation(
    project: &Project,
    entry: &Path,
    old: &str,
    new: &str,
    mutation: &Mutation,
    mode: &MutOpts,
) -> Result<ExitCode, String> {
    let m = mode.flags.mode();
    let introduced = project.introduced_diagnostics(entry, &mutation.edited, Some(old))?;

    if !matches!(m, Mode::Preview) && !introduced.is_empty() && !mode.flags.unsafe_mode {
        let mut err = io::stderr().lock();
        writeln!(
            err,
            "refusing: move introduces {} new diagnostic(s) (re-run with --unsafe to override):",
            introduced.len()
        )
        .map_err(|e| e.to_string())?;
        for d in &introduced {
            writeln!(
                err,
                "  {}[{}] {} {}",
                d.severity,
                d.code,
                d.location.display(),
                d.message
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(ExitCode::from(1));
    }

    // Build the diff: a rename hunk for old→new, plus in-place hunks for the
    // inbound-include files.
    let db = project.driver.db();
    let old_src = db
        .file_id(old)
        .and_then(|id| db.source(id))
        .unwrap_or_default();
    let new_src = mutation
        .edited
        .get(new)
        .map(String::as_str)
        .unwrap_or_default();

    let mut out = io::stdout().lock();

    if let Mode::Write = m {
        if let Some(parent) = Path::new(new)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::rename(old, new).map_err(|e| format!("{old} -> {new}: {e}"))?;
        // The moved file's new content (outbound INCLUDE rewrites) is in `edited`
        // under `new`; the rename above just relocated the old bytes.
        for (path, src) in &mutation.edited {
            std::fs::write(path, src).map_err(|e| format!("{path}: {e}"))?;
        }
        writeln!(
            out,
            "moved {old} -> {new} ({} file(s) updated)",
            mutation.edited.len()
        )
        .map_err(|e| e.to_string())?;
        return Ok(ExitCode::SUCCESS);
    }

    // Preview / Patch: build the diff (rename hunk + inbound-include hunks).
    let mut diff = String::new();
    rename_diff(&mut diff, old, new, old_src, new_src);
    for (path, src) in &mutation.edited {
        if path == new {
            continue;
        }
        let old = db
            .file_id(path)
            .and_then(|id| db.source(id))
            .unwrap_or_default();
        file_diff(&mut diff, path, old, src);
    }
    match m {
        // A patch is always a raw diff, regardless of `--format`.
        Mode::Patch(dest) if dest != "-" => {
            std::fs::write(dest, diff).map_err(|e| format!("{dest}: {e}"))?;
        }
        Mode::Patch(_) => write!(out, "{diff}").map_err(|e| e.to_string())?,
        // Preview honors `--format`, matching `refactor` / `rename`.
        _ => match mode.format {
            Format::Json => {
                let files: Vec<&String> = mutation.edited.keys().collect();
                let v = serde_json::json!({
                    "diff": diff,
                    "files": files,
                    "introducedDiagnostics": introduced,
                    "safe": introduced.is_empty(),
                });
                writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
            }
            Format::Text => {
                write!(out, "{diff}").map_err(|e| e.to_string())?;
                emit_introduced(&mut out, &introduced)?;
            }
        },
    }
    Ok(ExitCode::SUCCESS)
}

fn run_refactor(op: &RefactorOp) -> Result<ExitCode, String> {
    match op {
        RefactorOp::SortKnots { file, mode } => {
            let project = Project::load(&mode.entry)?;
            let (id, source) = project.file_or_entry(file.as_deref())?;
            let new = sort_knots_in_source(&source);
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::SortStitches { knot, mode } => {
            let project = Project::load(&mode.entry)?;
            let (id, source) = project.knot_file(knot)?;
            let new = sort_stitches_in_knot(&source, knot);
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::Format { target, mode } => {
            let project = Project::load(&mode.entry)?;
            let (knot, stitch) = split_dotted(target);
            let (id, source) = project.knot_file(knot)?;
            let new = format_region(&source, knot, stitch);
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderKnot {
            knot,
            direction,
            mode,
        } => {
            let project = Project::load(&mode.entry)?;
            let (id, source) = project.knot_file(knot)?;
            let new =
                reorder_knot(&source, knot, (*direction).into()).map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderStitch {
            target,
            direction,
            mode,
        } => {
            let project = Project::load(&mode.entry)?;
            let (knot, stitch) = split_dotted(target);
            let stitch = stitch.ok_or("reorder-stitch needs KNOT.STITCH")?;
            let (id, source) = project.knot_file(knot)?;
            let new = reorder_stitch(&source, knot, stitch, (*direction).into())
                .map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderKnots { order, file, mode } => {
            let project = Project::load(&mode.entry)?;
            let (id, source) = project.file_or_entry(file.as_deref())?;
            let names = parse_order(order);
            let new = reorder_knots(&source, &names).map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderStitches { knot, order, mode } => {
            let project = Project::load(&mode.entry)?;
            let (id, source) = project.knot_file(knot)?;
            let names = parse_order(order);
            let new = reorder_stitches(&source, knot, &names).map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::MoveStitch { target, dest, mode } => {
            let project = Project::load(&mode.entry)?;
            let (knot, stitch) = split_dotted(target);
            let stitch = stitch.ok_or("move-stitch needs KNOT.STITCH")?;
            let (id, source) = project.knot_file(knot)?;
            let result = move_stitch(&source, &project.analysis, id, knot, stitch, dest)
                .map_err(|e| e.to_string())?;
            project.emit_move_result(id, result, mode)
        }
        RefactorOp::PromoteStitch { target, mode } => {
            let project = Project::load(&mode.entry)?;
            let (knot, stitch) = split_dotted(target);
            let stitch = stitch.ok_or("promote-stitch needs KNOT.STITCH")?;
            let (id, source) = project.knot_file(knot)?;
            let result = promote_stitch_to_knot(&source, &project.analysis, id, knot, stitch)
                .map_err(|e| e.to_string())?;
            project.emit_move_result(id, result, mode)
        }
        RefactorOp::DemoteKnot { knot, dest, mode } => {
            let project = Project::load(&mode.entry)?;
            let (id, source) = project.knot_file(knot)?;
            let result = demote_knot_to_stitch(&source, &project.analysis, id, knot, dest)
                .map_err(|e| e.to_string())?;
            project.emit_move_result(id, result, mode)
        }
        RefactorOp::ConvertLine { at, target, mode } => run_convert_line(at, *target, mode),
    }
}

fn run_convert_line(at: &str, target: ConvertTo, mode: &MutOpts) -> Result<ExitCode, String> {
    let project = Project::load(&mode.entry)?;
    let (file, line, col) = parse_at(at)?;
    let db = project.driver.db();
    let id = db
        .file_id(&file)
        .ok_or_else(|| format!("file not in project: {file}"))?;
    let source = db.source(id).unwrap_or_default().to_string();
    let hir = db.hir(id).ok_or("no HIR for that file")?;
    let root = db.parse(id).ok_or("no parse tree for that file")?.syntax();
    let offset = LineIndex::new(&source).offset(line.saturating_sub(1), col.saturating_sub(1));
    let edit = convert_element(&source, hir, &root, u32::from(offset), target.into())
        .ok_or("that line cannot be converted to the requested type")?;
    let mut new = source.clone();
    new.replace_range(edit.from as usize..edit.to as usize, &edit.insert);
    project.emit_single(id, &source, new, mode)
}

fn run_actions(at: &str, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let (file, line, col) = parse_at(at)?;
    let db = project.driver.db();
    let id = db
        .file_id(&file)
        .ok_or_else(|| format!("file not in project: {file}"))?;
    let source = db.source(id).unwrap_or_default();
    let offset = LineIndex::new(source).offset(line.saturating_sub(1), col.saturating_sub(1));
    let actions = code_actions(source, u32::from(offset) as usize);

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let arr: Vec<_> = actions
                .iter()
                .map(|a| serde_json::json!({ "title": a.title, "kind": action_kind_name(&a.kind) }))
                .collect();
            writeln!(out, "{}", to_json(&arr)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            for a in &actions {
                writeln!(out, "{}", a.title).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn action_kind_name(k: &CodeActionKind) -> &'static str {
    match k {
        CodeActionKind::QuickFix => "quickfix",
        CodeActionKind::Refactor => "refactor",
        CodeActionKind::Source => "source",
    }
}

/// Split `KNOT` / `KNOT.STITCH` into its parts (only the first dot is honored).
fn split_dotted(s: &str) -> (&str, Option<&str>) {
    match s.split_once('.') {
        Some((knot, stitch)) => (knot, Some(stitch)),
        None => (s, None),
    }
}

/// Parse a comma-separated permutation list, trimming whitespace.
fn parse_order(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Append a `git`-style file-rename diff (delete `old`, create `new`).
fn rename_diff(out: &mut String, old: &str, new: &str, old_src: &str, new_src: &str) {
    let old_lines: Vec<&str> = old_src.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new_src.split_inclusive('\n').collect();
    let _ = write!(
        out,
        "diff --git a/{old} b/{new}\nrename from {old}\nrename to {new}\n--- a/{old}\n+++ b/{new}\n@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for l in &old_lines {
        push_diff_line(out, '-', l);
    }
    for l in &new_lines {
        push_diff_line(out, '+', l);
    }
}

fn emit_introduced(out: &mut impl Write, introduced: &[DiagEntry]) -> Result<(), String> {
    if !introduced.is_empty() {
        writeln!(
            out,
            "would introduce {} new diagnostic(s):",
            introduced.len()
        )
        .map_err(|e| e.to_string())?;
        for d in introduced {
            writeln!(
                out,
                "  {}[{}] {} {}",
                d.severity,
                d.code,
                d.location.display(),
                d.message
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ── Story-graph rendering ───────────────────────────────────────────

fn node_kind_name(k: StoryNodeKind) -> &'static str {
    match k {
        StoryNodeKind::Knot => "knot",
        StoryNodeKind::Stitch => "stitch",
        StoryNodeKind::End => "end",
        StoryNodeKind::Done => "done",
    }
}

fn edge_kind_name(k: StoryEdgeKind) -> &'static str {
    match k {
        StoryEdgeKind::Divert => "divert",
        StoryEdgeKind::Choice => "choice",
        StoryEdgeKind::Tunnel => "tunnel",
        StoryEdgeKind::Thread => "thread",
    }
}

fn graph_json(graph: &StoryGraph) -> serde_json::Value {
    let nodes: Vec<_> = graph
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id,
                "name": n.name,
                "kind": node_kind_name(n.kind),
                "parent": n.parent,
            })
        })
        .collect();
    let edges: Vec<_> = graph
        .edges
        .iter()
        .map(|e| serde_json::json!({ "from": e.from, "to": e.to, "kind": edge_kind_name(e.kind) }))
        .collect();
    serde_json::json!({ "nodes": nodes, "edges": edges })
}

fn write_graph_dot(out: &mut impl Write, graph: &StoryGraph) -> Result<(), String> {
    writeln!(out, "digraph story {{").map_err(|e| e.to_string())?;
    for n in &graph.nodes {
        writeln!(
            out,
            "  {:?} [label={:?}];",
            n.id,
            format!("{} ({})", n.name, node_kind_name(n.kind))
        )
        .map_err(|e| e.to_string())?;
    }
    for e in &graph.edges {
        writeln!(
            out,
            "  {:?} -> {:?} [label={:?}];",
            e.from,
            e.to,
            edge_kind_name(e.kind)
        )
        .map_err(|x| x.to_string())?;
    }
    writeln!(out, "}}").map_err(|e| e.to_string())?;
    Ok(())
}

// ── Project loader ──────────────────────────────────────────────────

struct Project {
    driver: Driver,
    analysis: AnalysisResult,
    entry_id: FileId,
}

impl Project {
    /// Discover + analyze the project rooted at `entry` (follows `INCLUDE`s),
    /// exactly like `brink compile`.
    fn load(entry: &Path) -> Result<Self, String> {
        let entry = entry.to_string_lossy().into_owned();
        let mut driver = Driver::new();
        driver
            .discover(&entry, |p| {
                std::fs::read_to_string(p)
                    .map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
            })
            .map_err(|e| format!("{e}"))?;
        let analysis = driver.analyze().clone();
        let entry_id = driver
            .db()
            .file_id(&entry)
            .ok_or_else(|| format!("entry file not found after discovery: {entry}"))?;
        Ok(Self {
            driver,
            analysis,
            entry_id,
        })
    }

    /// Discover + analyze the project as it existed at a git revision
    /// (`rev: None` means the working tree — plain [`Project::load`]).
    /// `entry` is still a filesystem path (relative to the current
    /// directory, or absolute); it's resolved to a path relative to the
    /// repo root so the same string addresses the blob at `rev` via `git
    /// show rev:path`, and every `INCLUDE` resolved from it (always
    /// relative-to-entry's-directory, never repo-root-relative on its own)
    /// stays consistent between the two reads.
    fn load_at_rev(entry: &Path, rev: Option<&str>) -> Result<Self, String> {
        let Some(rev) = rev else {
            return Self::load(entry);
        };
        let (repo_root, rel_entry) = repo_relative_path(entry)?;
        let mut driver = Driver::new();
        driver
            .discover(&rel_entry, |p: &str| read_git_blob(&repo_root, rev, p))
            .map_err(|e| format!("{e}"))?;
        let analysis = driver.analyze().clone();
        let entry_id = driver
            .db()
            .file_id(&rel_entry)
            .ok_or_else(|| format!("entry file not found in {rev}: {rel_entry}"))?;
        Ok(Self {
            driver,
            analysis,
            entry_id,
        })
    }

    /// Resolve a query's target to a single symbol — by `--at FILE:LINE:COL`
    /// (cursor → the symbol there, resolving a reference to its definition) or
    /// by qualified name.
    fn resolve(&self, addr: &Address, kind: Option<KindFilter>) -> Result<&SymbolInfo, String> {
        if let Some(at) = &addr.at {
            let (file, line, col) = parse_at(at)?;
            let db = self.driver.db();
            let file_id = db
                .file_id(&file)
                .ok_or_else(|| format!("file not in project: {file}"))?;
            let src = db.source(file_id).unwrap_or_default();
            // `--at` is 1-based; LineIndex (like line_col) is 0-based.
            let offset = LineIndex::new(src).offset(line.saturating_sub(1), col.saturating_sub(1));
            find_def_at_offset(&self.analysis, file_id, offset)
                .ok_or_else(|| format!("no symbol at {at}"))
        } else if let Some(name) = &addr.symbol {
            self.resolve_unique(name, kind)
        } else {
            Err("provide a symbol name or --at FILE:LINE:COL".to_string())
        }
    }

    /// Resolve a qualified name to exactly one symbol, honoring `--kind`.
    fn resolve_unique(&self, name: &str, kind: Option<KindFilter>) -> Result<&SymbolInfo, String> {
        let ids = self.analysis.index.by_name.get(name);
        let mut hits: Vec<&SymbolInfo> = ids
            .into_iter()
            .flatten()
            .filter_map(|id| self.analysis.index.symbols.get(id))
            .filter(|s| kind.is_none_or(|k| k.matches(s.kind)))
            .collect();
        let db = self.driver.db();
        hits.sort_by_key(|s| {
            (
                db.file_path(s.file).unwrap_or_default().to_string(),
                s.range.start(),
            )
        });

        match hits.as_slice() {
            [] => Err(format!("no symbol named '{name}' in the project")),
            [one] => Ok(one),
            many => {
                let kinds: Vec<&str> = many.iter().map(|s| kind_name(s.kind)).collect();
                Err(format!(
                    "'{name}' is ambiguous (matches: {}); disambiguate with --kind",
                    kinds.join(", ")
                ))
            }
        }
    }

    fn location_of(&self, file: FileId, range: TextRange) -> Loc {
        let db = self.driver.db();
        let path = db.file_path(file).unwrap_or_default().to_string();
        let src = db.source(file).unwrap_or_default();
        let idx = LineIndex::new(src);
        let (line, col) = idx.line_col(range.start());
        Loc {
            path,
            line: line + 1,
            col: col + 1,
            byte_start: u32::from(range.start()),
            byte_end: u32::from(range.end()),
        }
    }

    fn diag_entry(&self, d: &Diagnostic, severity: &str) -> DiagEntry {
        DiagEntry {
            severity: severity.to_string(),
            code: d.code.as_str().to_string(),
            message: d.message.clone(),
            location: self.location_of(d.file, d.range),
        }
    }

    /// Apply rename edits in-memory, returning the new source per touched file.
    fn apply_edits(&self, edits: &[FileEdit]) -> Result<BTreeMap<String, String>, String> {
        let db = self.driver.db();
        let mut by_file: HashMap<FileId, Vec<&FileEdit>> = HashMap::new();
        for e in edits {
            by_file.entry(e.file).or_default().push(e);
        }
        let mut out = BTreeMap::new();
        for (file, mut es) in by_file {
            let path = db
                .file_path(file)
                .ok_or("edit targets an unknown file")?
                .to_string();
            let mut src = db.source(file).unwrap_or_default().to_string();
            // Splice from the end so earlier offsets stay valid.
            es.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
            for e in es {
                src.replace_range(
                    usize::from(e.range.start())..usize::from(e.range.end()),
                    &e.new_text,
                );
            }
            out.insert(path, src);
        }
        Ok(out)
    }

    /// Re-analyze the project with the edited sources and return the diagnostics
    /// the edit *introduced* — any error or warning present now but not in the
    /// baseline (matched by code + message). A rename that creates a collision or
    /// shadow surfaces as a warning, so warnings count.
    fn introduced_diagnostics(
        &self,
        entry: &Path,
        edited: &BTreeMap<String, String>,
        removed: Option<&str>,
    ) -> Result<Vec<DiagEntry>, String> {
        let entry_s = entry.to_string_lossy().into_owned();
        let mut driver = Driver::new();
        driver
            .discover(&entry_s, |p| {
                // A moved file no longer exists at its old path: surface any
                // stale reference as a diagnostic instead of reading the disk copy.
                if Some(p) == removed {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{p}: file moved"),
                    ));
                }
                if let Some(s) = edited.get(p) {
                    Ok(s.clone())
                } else {
                    std::fs::read_to_string(p)
                        .map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
                }
            })
            .map_err(|e| format!("{e}"))?;
        let new_analysis = driver.analyze().clone();
        let new_entry = driver
            .db()
            .file_id(&entry_s)
            .ok_or("entry file vanished during re-analysis")?;
        let new_report = driver.collect_diagnostics(&new_analysis, Some(new_entry));
        let base_report = self
            .driver
            .collect_diagnostics(&self.analysis, Some(self.entry_id));

        // Baseline diagnostic multiset (errors + warnings) keyed by (code, message).
        let mut baseline: HashMap<(String, String), i32> = HashMap::new();
        for d in base_report.errors.iter().chain(base_report.warnings.iter()) {
            *baseline
                .entry((d.code.as_str().to_string(), d.message.clone()))
                .or_default() += 1;
        }

        let new_diags = new_report
            .errors
            .iter()
            .map(|d| ("error", d))
            .chain(new_report.warnings.iter().map(|d| ("warning", d)));

        let mut introduced = Vec::new();
        for (severity, d) in new_diags {
            let key = (d.code.as_str().to_string(), d.message.clone());
            let count = baseline.entry(key).or_default();
            if *count > 0 {
                *count -= 1;
            } else {
                // Location lives in the *new* driver's db.
                let path = driver
                    .db()
                    .file_path(d.file)
                    .unwrap_or_default()
                    .to_string();
                let src = driver.db().source(d.file).unwrap_or_default();
                let (line, col) = LineIndex::new(src).line_col(d.range.start());
                introduced.push(DiagEntry {
                    severity: severity.into(),
                    code: d.code.as_str().into(),
                    message: d.message.clone(),
                    location: Loc {
                        path,
                        line: line + 1,
                        col: col + 1,
                        byte_start: u32::from(d.range.start()),
                        byte_end: u32::from(d.range.end()),
                    },
                });
            }
        }
        Ok(introduced)
    }

    /// One preview entry per edit: where, and the old → new text.
    fn edit_entries(&self, edits: &[FileEdit]) -> Vec<EditEntry> {
        let db = self.driver.db();
        let mut v: Vec<EditEntry> = edits
            .iter()
            .map(|e| {
                let src = db.source(e.file).unwrap_or_default();
                let old = src
                    .get(usize::from(e.range.start())..usize::from(e.range.end()))
                    .unwrap_or_default()
                    .to_string();
                EditEntry {
                    location: self.location_of(e.file, e.range),
                    old,
                    new: e.new_text.clone(),
                }
            })
            .collect();
        v.sort_by(|a, b| {
            (&a.location.path, a.location.byte_start)
                .cmp(&(&b.location.path, b.location.byte_start))
        });
        v
    }

    /// A `git apply`-able patch for the edited files (whole-file hunks).
    fn unified_diff(&self, edited: &BTreeMap<String, String>) -> Result<String, String> {
        let db = self.driver.db();
        let mut out = String::new();
        for (path, new_src) in edited {
            let file = db.file_id(path).ok_or("diff targets an unknown file")?;
            let old = db.source(file).unwrap_or_default();
            file_diff(&mut out, path, old, new_src);
        }
        Ok(out)
    }

    /// Build an `IdeSession` seeded with every project file (db-level only — no
    /// analysis), for the `brink-ide` ops (`file_rename`) that take a session.
    fn ide_session(&self) -> IdeSession {
        let db = self.driver.db();
        let mut session = IdeSession::new();
        let ids: Vec<FileId> = db.file_ids().collect();
        for id in ids {
            if let (Some(path), Some(src)) = (db.file_path(id), db.source(id)) {
                session.update_source(path, src.to_string());
            }
        }
        session
    }

    /// The `(id, source)` for `file` (project-relative) or, if `None`, the entry.
    fn file_or_entry(&self, file: Option<&str>) -> Result<(FileId, String), String> {
        let db = self.driver.db();
        let id = match file {
            Some(f) => db
                .file_id(f)
                .ok_or_else(|| format!("file not in project: {f}"))?,
            None => self.entry_id,
        };
        let src = db.source(id).unwrap_or_default().to_string();
        Ok((id, src))
    }

    /// Resolve a knot name to the file that declares it (and that file's source).
    fn knot_file(&self, knot: &str) -> Result<(FileId, String), String> {
        let sym = self.resolve_unique(knot, Some(KindFilter::Knot))?;
        let file = sym.file;
        let src = self
            .driver
            .db()
            .source(file)
            .unwrap_or_default()
            .to_string();
        Ok((file, src))
    }

    /// Emit a single-file refactor result (`old_source` → `new_source`) through
    /// the requested mode. Reports "no change" if the refactor is a no-op.
    fn emit_single(
        &self,
        id: FileId,
        old_source: &str,
        new_source: String,
        mode: &MutOpts,
    ) -> Result<ExitCode, String> {
        if new_source == old_source {
            let mut out = io::stdout().lock();
            match mode.format {
                Format::Text => writeln!(out, "no change").map_err(|e| e.to_string())?,
                Format::Json => writeln!(
                    out,
                    "{}",
                    to_json(&serde_json::json!({
                        "changed": false,
                        "diff": "",
                        "files": Vec::<String>::new(),
                        "introducedDiagnostics": Vec::<DiagEntry>::new(),
                        "safe": true,
                    }))?
                )
                .map_err(|e| e.to_string())?,
            }
            return Ok(ExitCode::SUCCESS);
        }
        let path = self
            .driver
            .db()
            .file_path(id)
            .ok_or("refactor targets an unknown file")?
            .to_string();
        let mut edited = BTreeMap::new();
        edited.insert(path, new_source);
        let mutation = Mutation {
            edited,
            edits: None,
        };
        let m = mode.flags.mode();
        emit_mutation(
            self,
            &mode.entry,
            &mutation,
            &m,
            mode.format,
            mode.flags.unsafe_mode,
        )
    }

    /// Emit a cross-file [`StructuralResult`] (primary `new_source` + reference
    /// edits in other files) through the requested mode. The primary file is
    /// covered by `new_source`, so any cross-file edit landing on it is overridden.
    fn emit_move_result(
        &self,
        primary: FileId,
        result: StructuralResult,
        mode: &MutOpts,
    ) -> Result<ExitCode, String> {
        let primary_path = self
            .driver
            .db()
            .file_path(primary)
            .ok_or("move targets an unknown file")?
            .to_string();
        let new_source = result
            .new_source
            .ok_or("structural move produced no primary source")?;
        let mut edited = self.apply_edits(&result.cross_file_edits)?;
        edited.insert(primary_path, new_source);
        let mutation = Mutation {
            edited,
            edits: None,
        };
        let m = mode.flags.mode();
        emit_mutation(
            self,
            &mode.entry,
            &mutation,
            &m,
            mode.format,
            mode.flags.unsafe_mode,
        )
    }
}

/// Append a whole-file unified-diff hunk for `path` (old → new) to `out`.
fn file_diff(out: &mut String, path: &str, old: &str, new: &str) {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let _ = write!(
        out,
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for l in &old_lines {
        push_diff_line(out, '-', l);
    }
    for l in &new_lines {
        push_diff_line(out, '+', l);
    }
}

fn push_diff_line(out: &mut String, sign: char, line: &str) {
    out.push(sign);
    out.push_str(line.strip_suffix('\n').unwrap_or(line));
    out.push('\n');
    if !line.ends_with('\n') {
        out.push_str("\\ No newline at end of file\n");
    }
}

/// Recursively convert a `brink-ide` outline node into an output entry.
fn doc_to_entry(project: &Project, file: FileId, d: &DocumentSymbol) -> SymEntry {
    SymEntry {
        name: d.name.clone(),
        kind: kind_name(d.kind).into(),
        detail: d.detail.clone(),
        location: project.location_of(file, d.range),
        children: d
            .children
            .iter()
            .map(|c| doc_to_entry(project, file, c))
            .collect(),
    }
}

fn print_tree(out: &mut impl Write, entries: &[SymEntry], depth: usize) -> Result<(), String> {
    for e in entries {
        let indent = "  ".repeat(depth);
        let detail = e
            .detail
            .as_deref()
            .map(|d| format!(" [{d}]"))
            .unwrap_or_default();
        writeln!(
            out,
            "{indent}{} {}{}  {}",
            e.kind,
            e.name,
            detail,
            e.location.display()
        )
        .map_err(|x| x.to_string())?;
        print_tree(out, &e.children, depth + 1)?;
    }
    Ok(())
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string(v).map_err(|e| e.to_string())
}

/// Parse `FILE:LINE:COL` (line/col 1-based). The file may itself contain `:`,
/// so split the two numeric fields off the right.
fn parse_at(s: &str) -> Result<(String, u32, u32), String> {
    let mut parts = s.rsplitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(col), Some(line), Some(file)) if !file.is_empty() => {
            let line = line
                .parse::<u32>()
                .map_err(|_| format!("bad line in --at '{s}'"))?;
            let col = col
                .parse::<u32>()
                .map_err(|_| format!("bad column in --at '{s}'"))?;
            Ok((file.to_string(), line, col))
        }
        _ => Err(format!("--at must be FILE:LINE:COL, got '{s}'")),
    }
}

// ── Git-revision project loading (`effects-diff`) ──────────────────────

/// Resolve `entry` to `(repo root, path relative to repo root)` via `git
/// rev-parse --show-toplevel`, so `git show REV:path` addresses the same
/// file regardless of the caller's current directory.
fn repo_relative_path(entry: &Path) -> Result<(String, String), String> {
    let abs = std::fs::canonicalize(entry).map_err(|e| format!("{}: {e}", entry.display()))?;
    // Scoped with `-C <entry's directory>` — plain `git rev-parse` resolves
    // against the *process's* cwd, which is not necessarily where `entry`
    // lives (a script can pass `-e ../other-project/main.ink`).
    let entry_dir = abs.parent().unwrap_or(&abs);
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(entry_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("git rev-parse --show-toplevel: {e}"))?;
    if !out.status.success() {
        return Err(format!("not inside a git repository: {}", entry.display()));
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let root_canon = std::fs::canonicalize(&root).map_err(|e| format!("{root}: {e}"))?;
    let rel = abs.strip_prefix(&root_canon).map_err(|_| {
        format!(
            "{} is outside the git repo rooted at {root}",
            entry.display()
        )
    })?;
    Ok((root, rel.to_string_lossy().replace('\\', "/")))
}

/// Read `path` (repo-root-relative) as it existed at `rev`, via `git show`.
/// Surfaced as an `io::Error` so it composes with `Driver::discover`'s
/// `read_file` callback contract exactly like the filesystem reader does.
fn read_git_blob(repo_root: &str, rev: &str, path: &str) -> Result<String, io::Error> {
    let out = std::process::Command::new("git")
        .args(["-C", repo_root, "show", &format!("{rev}:{path}")])
        .output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("git show {rev}:{path}: {stderr}"),
        ));
    }
    String::from_utf8(out.stdout)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// One knot/stitch's effect row, names already resolved (so it compares
/// stably across two separately-analyzed revisions where `DefinitionId`s
/// aren't the same values). Sets are `BTreeSet`s throughout for
/// deterministic diff/serialization order.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
struct RowView {
    reads: std::collections::BTreeSet<String>,
    writes: std::collections::BTreeSet<String>,
    calls: std::collections::BTreeSet<String>,
    opaque: bool,
}

impl RowView {
    fn from_row(row: &brink_db::EffectRow, index: &brink_ir::SymbolIndex) -> Self {
        let name_of = |id: &brink_format::DefinitionId| {
            index
                .symbols
                .get(id)
                .map_or_else(|| format!("{id:?}"), |info| info.name.clone())
        };
        Self {
            reads: row.reads.iter().map(name_of).collect(),
            writes: row.writes.iter().map(name_of).collect(),
            calls: row.calls.iter().cloned().collect(),
            opaque: row.opaque,
        }
    }
}

/// Every knot/stitch's [`RowView`] in `project`, keyed by qualified name —
/// the join key across two separately-compiled revisions (spec §10: "every
/// knot/stitch ships its row").
fn effect_rows_by_name(project: &Project) -> BTreeMap<String, RowView> {
    let db = project.driver.db();
    project
        .analysis
        .index
        .symbols
        .values()
        .filter(|info| matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch))
        .filter_map(|info| {
            let row = db.effects(info.id)?;
            Some((
                info.name.clone(),
                RowView::from_row(&row, &project.analysis.index),
            ))
        })
        .collect()
}

/// One def's drift between two revisions — omitted entirely when nothing
/// changed. `added`/`removed` cover the def not existing on one side (a new
/// or deleted knot/stitch); `changed` covers a row present on both sides
/// with different atoms.
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RowDrift {
    Added { row: RowView },
    Removed { row: RowView },
    Changed { base: RowView, head: RowView },
}

fn diff_effect_rows(
    base: &BTreeMap<String, RowView>,
    head: &BTreeMap<String, RowView>,
) -> BTreeMap<String, RowDrift> {
    let mut out = BTreeMap::new();
    for (name, head_row) in head {
        match base.get(name) {
            None => {
                out.insert(
                    name.clone(),
                    RowDrift::Added {
                        row: head_row.clone(),
                    },
                );
            }
            Some(base_row) if base_row != head_row => {
                out.insert(
                    name.clone(),
                    RowDrift::Changed {
                        base: base_row.clone(),
                        head: head_row.clone(),
                    },
                );
            }
            Some(_) => {}
        }
    }
    for (name, base_row) in base {
        if !head.contains_key(name) {
            out.insert(
                name.clone(),
                RowDrift::Removed {
                    row: base_row.clone(),
                },
            );
        }
    }
    out
}

/// Render one atom-set delta line (`+reads a, b` / `-writes c`), omitted
/// when the two sides agree on that category. `opaque` renders as its own
/// pseudo-atom since it isn't a member of any set.
fn render_row_delta(out: &mut String, base: &RowView, head: &RowView) {
    let delta = |label: &str,
                 before: &std::collections::BTreeSet<String>,
                 after: &std::collections::BTreeSet<String>,
                 out: &mut String| {
        let added: Vec<&str> = after.difference(before).map(String::as_str).collect();
        let removed: Vec<&str> = before.difference(after).map(String::as_str).collect();
        if !added.is_empty() {
            let _ = writeln!(out, "    + {label} {}", added.join(", "));
        }
        if !removed.is_empty() {
            let _ = writeln!(out, "    - {label} {}", removed.join(", "));
        }
    };
    delta("reads", &base.reads, &head.reads, out);
    delta("writes", &base.writes, &head.writes, out);
    delta("calls", &base.calls, &head.calls, out);
    if base.opaque != head.opaque {
        let _ = writeln!(out, "    {} opaque", if head.opaque { "+" } else { "-" });
    }
}

fn run_effects_diff(
    base: &str,
    head: Option<&str>,
    opts: &EffectsDiffOpts,
) -> Result<ExitCode, String> {
    let base_project = Project::load_at_rev(&opts.entry, Some(base))?;
    let head_project = Project::load_at_rev(&opts.entry, head)?;
    let base_rows = effect_rows_by_name(&base_project);
    let head_rows = effect_rows_by_name(&head_project);
    let drift = diff_effect_rows(&base_rows, &head_rows);

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            writeln!(out, "{}", to_json(&drift)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            let head_label = head.unwrap_or("working tree");
            for (name, d) in &drift {
                match d {
                    RowDrift::Added { row } => {
                        let mut line = format!("+ {name}\n");
                        render_row_delta(&mut line, &RowView::default(), row);
                        write!(out, "{line}").map_err(|e| e.to_string())?;
                    }
                    RowDrift::Removed { row } => {
                        let mut line = format!("- {name}\n");
                        render_row_delta(&mut line, row, &RowView::default());
                        write!(out, "{line}").map_err(|e| e.to_string())?;
                    }
                    RowDrift::Changed { base: b, head: h } => {
                        let mut line = format!("~ {name}\n");
                        render_row_delta(&mut line, b, h);
                        write!(out, "{line}").map_err(|e| e.to_string())?;
                    }
                }
            }
            if drift.is_empty() {
                writeln!(out, "no effect-row drift: {base} vs. {head_label}")
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(if drift.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

// ── Output location ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct Loc {
    path: String,
    line: u32,
    col: u32,
    byte_start: u32,
    byte_end: u32,
}

impl Loc {
    fn display(&self) -> String {
        format!("{}:{}:{}", self.path, self.line, self.col)
    }
}

/// An outline / search / unused entry (a symbol with an optional child list).
#[derive(serde::Serialize)]
struct SymEntry {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    location: Loc,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<SymEntry>,
}

/// A diagnostic entry for `brink ide check`.
#[derive(serde::Serialize)]
struct DiagEntry {
    severity: String,
    code: String,
    message: String,
    location: Loc,
}

/// One edit in a rename preview: where, and the old → new text.
#[derive(serde::Serialize)]
struct EditEntry {
    location: Loc,
    old: String,
    new: String,
}
