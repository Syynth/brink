# brink

An Ink compiler and runtime in Rust. Implements [inkle's ink](https://github.com/inkle/ink) narrative scripting language.

## Crate layout

| Crate | Path | Purpose |
|-------|------|---------|
| `brink` | `crates/brink/` | Public API — re-exports from compiler and runtime |
| `brink-compiler` | `crates/brink-compiler/` | Pipeline driver — orchestrates parsing, analysis, and codegen |
| `brink-runtime` | `crates/brink-runtime/` | Bytecode VM for executing compiled stories |
| `brink-cli` | `crates/brink-cli/` | CLI for compiling and running ink stories |
| `brink-lsp` | `crates/brink-lsp/` | Language server for ink files |
| `brink-intl` | `crates/brink-intl/` | Batteries-included plural resolution (ICU4X baked data) |
| `brink-syntax` | `crates/internal/brink-syntax/` | Lexer, parser, lossless CST, typed AST |
| `brink-ir` | `crates/internal/brink-ir/` | HIR types, LIR types, symbol tables, per-file lowering |
| `brink-analyzer` | `crates/internal/brink-analyzer/` | Cross-file semantic analysis (stateless pass) |
| `brink-db` | `crates/internal/brink-db/` | Incremental project database with per-file/per-knot caching |
| `brink-format` | `crates/internal/brink-format/` | Binary interface between compiler and runtime (the firewall) |
| `brink-json` | `crates/internal/brink-json/` | Parser for inklecate .ink.json output format |
| `brink-codegen-json` | `crates/internal/brink-codegen-json/` | JSON codegen backend: LIR → ink.json format |
| `brink-codegen-inkb` | `crates/internal/brink-codegen-inkb/` | Bytecode codegen backend: LIR → StoryData (inkb format) |
| `brink-converter` | `crates/internal/brink-converter/` | Converts .ink.json to .inkb (bootstraps runtime testing) |

Crates under `crates/internal/` are workspace-only and have `publish = false`. They are not published to crates.io. `brink-runtime` depends ONLY on `brink-format` — see `docs/spec.md` for the full dependency graph.

## Reference ink implementation

The reference inkle/ink C# implementation is available locally at `~/code/rs/s92-studio/reference/ink`. Use this for checking reference test expectations (e.g. `reference/ink/tests/Tests.cs`) instead of fetching from the web.

## Implementation policy

Spike implementations are v0 — they exist to validate crate interfaces, not to be final code. When working on a crate, **prefer rewriting over patching** if the existing implementation doesn't match the target design. Tests and public API signatures are the stable artifacts; everything behind them is disposable. See `docs/spec.md` for full implementation order and tier breakdown.

## Development philosophy

- **LSP as exercise, not exception.** When an LSP feature needs data the compiler infra doesn't yet provide, enhance the underlying subsystem (brink-ir, brink-analyzer, brink-db) rather than adding one-off workarounds in brink-lsp. The LSP is a consumer of the compiler pipeline — use its needs as opportunities to move the whole project forward.
- **Flag silent data drops.** If a lowering pass, transform, or conversion silently drops data (AST children, HIR nodes, content parts, etc.) without emitting a diagnostic, flag it to the user immediately. Silent drops are always bugs until proven otherwise — do not attempt to fix them without discussion first.

## Documentation conventions

- **Diagrams** should use Mermaid syntax, not plain ASCII art.

## Workspace conventions

- **Dependencies** are declared in `[workspace.dependencies]` in the root `Cargo.toml` and referenced via `dep.workspace = true` in each crate.
- **Lints** are configured in `[workspace.lints]` and inherited via `[lints] workspace = true`.
- **Edition, license, repository** are set in `[workspace.package]` and inherited with `field.workspace = true`.

## Key commands

```sh
cargo check --workspace                          # type-check
cargo test --workspace                            # run tests
cargo clippy --workspace --all-targets -- -D warnings  # lint
cargo fmt --all -- --check                        # format check
cargo fmt --all                                   # format fix
cargo build --release -p brink-lsp               # rebuild LSP for Zed

# JSON corpus conformance (summary + first failure diff)
cargo test -p brink-compiler --test json_corpus -- --nocapture 2>&1 | head -60
```

## Zed extension

`~/.cargo/bin/brink-lsp` is a symlink to `target/release/brink-lsp`. The `zed-brink` extension finds it via `PATH`, so `cargo build --release -p brink-lsp` is all that's needed to update the LSP that Zed uses. Restart the language server in Zed after rebuilding.

## Conformance work loop

When working on JSON corpus conformance (`tests/tier1/`, `tier2/`, `tier3/`), follow this loop:

1. **Run the corpus** — `cargo test -p brink-compiler --test json_corpus -- --nocapture 2>&1 | head -60` for summary + first failure diff.
2. **Root-cause the first failure** — read the `.ink` source, expected `.ink.json`, and brink's actual output. Determine which layer owns the bug: parsing (`brink-syntax`), HIR lowering (`brink-ir::hir`), analysis (`brink-analyzer`), LIR lowering (`brink-ir::lir`), or JSON emission (`brink-compiler::json`).
3. **Present the analysis** — show the root cause and proposed fix location to the user before implementing. Fixes often belong in a different layer than the symptom suggests.
4. **Implement, verify, commit** — fix, re-run corpus to confirm progress (pass count should increase), commit.
5. **Repeat from step 1.**

**Do not shop for test cases.** The corpus reports the first failure for a reason — fix THAT test, not a different one. Do not scan other categories, do not look for "simpler" or "quicker" wins, do not skip a hard test hoping to find low-hanging fruit elsewhere. Complex failures often have multiple constituent issues (e.g., wrong path format AND missing switch/case pattern AND extra whitespace). Break them down into individual sub-problems and tackle them one at a time — each sub-fix should compile and not regress the corpus. If a sub-fix requires implementing a missing feature (switch/case, new emit pattern, etc.), implement it.

The corpus test file is `crates/brink-compiler/tests/json_corpus.rs`. Test cases live in `tests/tier{1,2,3}/` — each has `story.ink` (source) and `story.ink.json` (expected inklecate output).

## Lint policy

- `unsafe_code`, `unwrap_used`, `expect_used`, `panic`, `todo`, `print_stdout`, `print_stderr` are **denied**.
- Clippy pedantic is on (with a few allows for noise).
- Tests are exempt from unwrap/expect/dbg/print restrictions (via `clippy.toml`).

