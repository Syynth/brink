# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-lsp-v0.0.11...brink-lsp-v0.0.12) - 2026-08-21

### Added

- *(brink-analyzer,brink-db,brink-ide,brink-lsp,brink-web)* wire the harvest index into cue-name completion ([#2134](https://github.com/Syynth/brink/pull/2134))
- *(brink-ide,brink-lsp,docs)* detect undeclared renames at authoring time (#1672 part 2)
- *(brink-lsp)* tag unnecessary-class diagnostics with DiagnosticTag::UNNECESSARY
- *(diagnostics)* add Info/Hint severity tier below Warning ([#1162](https://github.com/Syynth/brink/pull/1162))
- *(ide,lsp,web)* extend the [lints]/deny-warnings override tier to brink ide, brink-lsp, and EditorSession
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- *(brink-lsp)* surface malformed brink.toml diagnostic + live reload
- *(brink-ide,web,lsp)* auto-import IMPORT quick-fix for out-of-scope refs
- *(ide)* inferred-type hover + inlay hints via the per-def FG seam ([#621](https://github.com/Syynth/brink/pull/621))
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(lsp)* thread client-declared dialect into background analysis_loop ([#599](https://github.com/Syynth/brink/pull/599))
- *(brink-ide,brink-lsp)* T1b-4 IDE polish — stdlib completion, signature help, block folding, hover ([#589](https://github.com/Syynth/brink/pull/589))

### Fixed

- *(brink-lsp)* decline formatting for native documents (#2360 review)
- *(brink-lsp)* route .brink documents through the native CST for semantic tokens, inlay hints, and code actions
- *(brink-ide,brink-cli,brink-web,brink-lsp)* shared AnalysisOptions forwarding seam ([#2383](https://github.com/Syynth/brink/pull/2383))
- *(brink-analyzer,brink-ide,brink-lsp)* off-db conventions confinement (E169)
- *(brink-ide,brink-lsp)* correct misattributed E169 causal claims (#2316 review F1)
- *(brink-ide,brink-web,brink-lsp)* thread [project] conventions into the editor/LSP live db ([#1880](https://github.com/Syynth/brink/pull/1880))
- *(brink-lsp)* add @ as a completion trigger and fall back for ink files
- *(brink-ide,brink-lsp)* stop offering multi-word cue completions past the first word
- *(brink-lsp)* stop leaking stdlib mounts across root moves and file wins
- *(brink-cli,brink-lsp)* mount stdlib into IDE loaders ([#2198](https://github.com/Syynth/brink/pull/2198))
- *(brink-lsp)* address PR #2202 review findings on doc drift, ID overflow, and lock contention
- *(brink-lsp)* one native ProjectDb per governing brink.toml ([#1580](https://github.com/Syynth/brink/pull/1580))
- *(brink-ide,brink-lsp)* PR #1711 review findings for #1672 rename/#@was
- *(brink-db,brink-compiler,brink-lsp)* normalize #1504 root-content qualifier to a root-relative key ([#1696](https://github.com/Syynth/brink/pull/1696))
- *(brink-ide,brink-lsp)* address PR #1626 review findings
- *(brink-lsp)* drop misanchored E092 from UNNECESSARY tag, document tag in book
- *(brink-lsp)* address PR #1603 review findings on ignored-dir pruning
- *(brink-lsp)* path_under_ignored_dir declines to prune with empty workspace_roots
- *(lsp,analyzer,cli,wasm)* review fixes for #1417 lint-override tier
- *(brink-lsp)* fix vacuous native_source_root config-directory test
- *(lsp)* mint compile-identical native module identity from absolute keys
- *(lsp)* unpin M-2d native-homonym diagnostics from declared dialect; fix stale prose
- *(brink-lsp)* give native .brink workspaces real cross-file scope
- *(lsp,ide)* keep analysis_loop under the line cap; test helper lint-clean
- *(ide,lsp,web)* propagate analysis options + module diagnostics to the IDE path ([#1553](https://github.com/Syynth/brink/pull/1553))
- *(brink-ide)* route def --at, find_references, and rename through the UFCS verdict table
- *(brink-project-config)* thread brink.toml path/span into ConfigError itself ([#1384](https://github.com/Syynth/brink/pull/1384))
- *(brink-lsp)* address #1419 review findings on ignored-dir file-watcher pruning
- *(brink-lsp)* prune ignored dirs in did_change_watched_files (4th walk path)
- *(brink-lsp)* reuse #1381's ignored-dir pruning in walk_and_load
- *(brink-ide,brink-lsp)* review fixes for #1367 diagnostic-severity PR
- *(brink-ide,brink-lsp,brink-web)* diagnostic display sites use effective severity, not raw default
- *(brink-analyzer)* validate [lints] codes against the real DiagnosticCode set
- *(brink-lsp)* route config_error_diagnostic severity through convert.rs
- *(brink-lsp)* reconcile initializationOptions.dialect/.types with brink.toml ([#1030](https://github.com/Syynth/brink/pull/1030))
- *(lsp)* eradicate the #615 diagnostics-publisher flake — test wait + production race ([#759](https://github.com/Syynth/brink/pull/759))
- *(brink-lsp)* condition-driven wait for background_analysis_* tests, replacing fixed 0.4s poll deadline

### Other

- Fix #2320: resolve relative [project] conventions pointer against native_root, not cwd
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-ide)* UFCS hover + go-to-def wiring ([#1507](https://github.com/Syynth/brink/pull/1507)) — UNVERIFIED, see issue
- *(source-tree)* make directory pruning structural via a shared Walk
- *(brink-lsp)* fix reviewer-flagged inaccuracies in admission policy docs
- *(brink-lsp,brink-source-tree)* decide and document the ignored-dir admission policy once
- *(brink-lsp)* extract LSP adapter helpers into backend/adapters.rs
- invert the brink-project-config → brink-analyzer edge ([#1234](https://github.com/Syynth/brink/pull/1234))
- NS-A9 review fixes: LSP premise test rewrite, invalid-types-as-unset, two bench mounts
- path-projections tooling tail (docs/t1e-spec.md §8 item 3, #850)
- Merge pull request #691 from Syynth/auto/issue-621
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-lsp-v0.0.9...brink-lsp-v0.0.10) - 2026-07-10

### Added

- *(ide)* weave folding from projection container extents ([#476](https://github.com/Syynth/brink/pull/476))

### Other

- Merge pull request #483 from Syynth/worktree-476-weave-folding

## [0.0.9](https://github.com/Syynth/brink/compare/brink-lsp-v0.0.8...brink-lsp-v0.0.9) - 2026-07-06

### Other

- update Cargo.lock dependencies

## [0.0.7](https://github.com/Syynth/brink/compare/brink-lsp-v0.0.6...brink-lsp-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-lsp-v0.0.3...brink-lsp-v0.0.4) - 2026-06-15

### Added

- *(ide,web)* host value push-cache transport for the argument picker ([#174](https://github.com/Syynth/brink/pull/174)) ([#205](https://github.com/Syynth/brink/pull/205))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-lsp-v0.0.2...brink-lsp-v0.0.3) - 2026-06-13

### Other

- Studio IDE: doc comments + type hints for all declarations (Track B integration) ([#101](https://github.com/Syynth/brink/pull/101))
