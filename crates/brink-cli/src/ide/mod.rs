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
//!
//! Split into `commands` (the arg/command types + `run()` dispatch),
//! `handlers` (the per-command `run_xxx` functions and their mutation
//! pipeline), and `project` (the `Project`/`Loc` support types: loading,
//! resolving, diagnostics, edit application) — a behavior-preserving module
//! split (issue #682); every item kept its body verbatim, only crate-internal
//! visibility was bumped where a sibling module now needs it.

mod commands;
mod handlers;
pub(crate) mod project;

pub use commands::{IdeCommand, run};
