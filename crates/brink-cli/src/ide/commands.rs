//! The `brink ide` CLI surface: command/arg types (clap derives) and the
//! top-level `run()` dispatch that routes each [`IdeCommand`] to its handler
//! in [`super::handlers`].

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use brink_ide::line_convert::ConvertTarget;
use brink_ide::structural_move::Direction;
use brink_ir::symbols::SymbolKind;
use clap::{Args, Subcommand, ValueEnum};

use super::handlers::{
    run_actions, run_check, run_def, run_effects_diff, run_graph, run_hover, run_lines,
    run_move_file, run_refactor, run_references, run_rename, run_signature, run_symbols,
    run_unused,
};

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
    /// Diff every knot/stitch's inferred effect row against a baseline.
    ///
    /// The ruled drift-*visibility* tool (docs/effects-spec.md §10): effect
    /// rows are inference output, not a checked-in artifact, so there is no
    /// lockfile — this surfaces what an author's change did to the shipped
    /// rows. Output is a CI-comment-friendly Markdown summary (or `--format
    /// json`). Baseline is either another entry file (`--base`) or a git
    /// revision of the same project (`--rev`, e.g. `--rev HEAD` for
    /// working-tree-vs-HEAD).
    #[command(after_help = "\
Examples:
  brink ide effects-diff --rev HEAD -e main.ink       # working tree vs HEAD
  brink ide effects-diff --rev main -e main.ink --format json
  brink ide effects-diff --base ../old/main.ink -e main.ink
  brink ide effects-diff --rev HEAD -e main.ink --exit-code   # exit 1 if rows moved")]
    EffectsDiff {
        #[command(flatten)]
        opts: EffectsDiffOpts,
    },
}

/// Options for `brink ide effects-diff`. A dedicated struct (not `CommonOpts`)
/// — it has no `--kind`/symbol addressing, and it owns the baseline selectors.
#[derive(Args)]
pub struct EffectsDiffOpts {
    /// Entry-point `.ink` file; `INCLUDE`s are followed to build the project.
    #[arg(long, short = 'e', value_name = "FILE")]
    pub(super) entry: PathBuf,
    /// Baseline: a git revision of *this* project (read via `git show`).
    #[arg(long, value_name = "REV", conflicts_with = "base")]
    pub(super) rev: Option<String>,
    /// Baseline: a second entry file (e.g. an older checkout) to diff against.
    #[arg(long, value_name = "FILE", conflicts_with = "rev")]
    pub(super) base: Option<PathBuf>,
    /// Exit 1 (not 0) when any effect row changed — for CI gating.
    #[arg(long)]
    pub(super) exit_code: bool,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub(super) format: Format,
    #[command(flatten)]
    pub(super) lints: LintOverrideArgs,
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
    pub(super) entry: PathBuf,
    /// Disambiguate when a name matches multiple symbol kinds.
    #[arg(long, value_enum, value_name = "KIND")]
    pub(super) kind: Option<KindFilter>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub(super) format: Format,
    #[command(flatten)]
    pub(super) lints: LintOverrideArgs,
}

/// `--deny`/`--warn`/`--allow <CODE>` (`-D warnings`) — `brink ide`'s
/// counterpart of `brink compile`'s CLI/API lint-override tier (issue
/// #1373), extended here to `brink ide` (issue #1417) so an embedder that
/// scripts `brink ide` sees the same denied-warning-as-error policy a real
/// `brink compile` of the same project would enforce. Flattened into every
/// query/mutation options struct (`CommonOpts`, `MutOpts`,
/// `EffectsDiffOpts`) so every `brink ide` subcommand carries the flags —
/// [`super::project::resolve_analysis_options`] is the one place they're
/// applied, via [`Self::resolve`] and
/// `AnalysisOptions::apply_lint_overrides`, always winning over a
/// discovered `brink.toml`'s `[lints]` table for the same code (#1005
/// `CLI/API > file > default` precedence).
#[derive(Args, Clone, Default)]
pub struct LintOverrideArgs {
    /// Deny a diagnostic code, promoting it to a hard error in this
    /// session's diagnostics (issue #1373/#1417). Repeatable. Only codes
    /// whose *default* severity is `Warning` are overridable (#1160) — an
    /// unrecognized or non-overridable code is ignored with a warning
    /// through the usual channel, never silently. The special code
    /// `warnings` (`-D warnings`, mirroring rustc) is `deny-warnings`:
    /// promote every diagnostic that would otherwise resolve to `Warning`
    /// up to `Error`, the CLI equivalent of `[lints] deny-warnings = true`.
    #[arg(short = 'D', long = "deny", value_name = "CODE")]
    pub(super) deny: Vec<String>,
    /// Force a diagnostic code to `Warning`, promotable back to `Error` by
    /// `-D warnings`/`deny-warnings` like any unconfigured warning (issue
    /// #1373/#1417). Repeatable; same overridability rules and precedence
    /// as `--deny`.
    #[arg(long = "warn", value_name = "CODE")]
    pub(super) warn: Vec<String>,
    /// Never escalate a diagnostic code past `Warning`, even under `-D
    /// warnings`/`deny-warnings` (issue #1373/#1417). Repeatable; same
    /// overridability rules and precedence as `--deny`.
    #[arg(long = "allow", value_name = "CODE")]
    pub(super) allow: Vec<String>,
}

impl LintOverrideArgs {
    /// Resolve into the `(lints, deny_warnings)` pair
    /// [`super::project::LintOverrides`] carries, sharing
    /// [`crate::lint_overrides::resolve_lint_overrides`] with `brink
    /// compile` so the two CLI surfaces can never silently drift on flag
    /// semantics (issue #1417).
    pub(super) fn resolve(&self) -> super::project::LintOverrides {
        let (lints, deny_warnings) =
            crate::lint_overrides::resolve_lint_overrides(&self.deny, &self.warn, &self.allow);
        super::project::LintOverrides {
            lints,
            deny_warnings,
        }
    }
}

/// How a query addresses its target — by qualified name or by cursor position.
#[derive(Args)]
pub struct Address {
    /// Qualified symbol name (knot / knot.stitch / List.Item / var / …).
    pub(super) symbol: Option<String>,
    /// Address by cursor position instead: `FILE:LINE:COL` (1-based line & column).
    #[arg(long, value_name = "FILE:LINE:COL", conflicts_with = "symbol")]
    pub(super) at: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum Format {
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
    pub(super) entry: PathBuf,
    /// Output format (preview / JSON).
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub(super) format: Format,
    #[command(flatten)]
    pub(super) flags: ModeFlags,
    #[command(flatten)]
    pub(super) lints: LintOverrideArgs,
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
    pub(super) unsafe_mode: bool,
}

impl ModeFlags {
    pub(super) fn mode(&self) -> super::handlers::Mode<'_> {
        match (&self.patch, self.write) {
            (Some(dest), _) => super::handlers::Mode::Patch(dest),
            (None, true) => super::handlers::Mode::Write,
            (None, false) => super::handlers::Mode::Preview,
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
pub(super) enum KindFilter {
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
    pub(super) fn matches(self, k: SymbolKind) -> bool {
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

pub(super) fn kind_name(k: SymbolKind) -> &'static str {
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
        IdeCommand::EffectsDiff { opts } => run_effects_diff(opts),
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            let _ = writeln!(io::stderr(), "error: {msg}");
            ExitCode::from(2)
        }
    }
}
