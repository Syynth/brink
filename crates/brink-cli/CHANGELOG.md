# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-cli-v0.0.11...brink-cli-v0.0.12) - 2026-08-17

### Added

- *(brink-runtime)* migrate output contract from Line to Step/OutputLine ([#1684](https://github.com/Syynth/brink/pull/1684))
- *(brink-ide,brink-cli)* IDE rename writes #@was on the renamed declaration (#1672 part 1)
- *(diagnostics)* add Info/Hint severity tier below Warning ([#1162](https://github.com/Syynth/brink/pull/1162))
- *(environment)* reify the compilation Environment + Project/SourceTree producer ([#1306](https://github.com/Syynth/brink/pull/1306))
- *(brink-driver,brink-compiler,brink-cli)* discover_native over SourceTree — RealFs walk + GitRev git-baseline
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- brink.toml project settings file for dialect + type policy ([#1005](https://github.com/Syynth/brink/pull/1005))
- *(web)* FS-3w flow-addressed web surface — flow handles, Line::Suspended, wakeCheck ([#978](https://github.com/Syynth/brink/pull/978))
- *(effects)* T2-4 tail — effects book chapter, hover row, `brink ide effects-diff`, corpus wing ([#863](https://github.com/Syynth/brink/pull/863))
- *(ide)* inferred-type hover + inlay hints via the per-def FG seam ([#621](https://github.com/Syynth/brink/pull/621))
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(analyzer,db)* TM-3 — types = strict policy, Unknown/Conflicted-escape, E063 wiring ([#619](https://github.com/Syynth/brink/pull/619))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(brink-ide,brink-web,brink-cli)* apply #2383 review findings for the AnalysisOptions seam
- *(brink-ide,brink-cli,brink-web,brink-lsp)* shared AnalysisOptions forwarding seam ([#2383](https://github.com/Syynth/brink/pull/2383))
- *(brink-analyzer,brink-ide,brink-lsp)* off-db conventions confinement (E169)
- *(brink-cli,docs)* address #2325 review findings on E169 reachability claims
- *(brink-cli)* forward [project] conventions in Project::ide_session
- *(brink-cli,brink-lsp)* mount stdlib into IDE loaders ([#2198](https://github.com/Syynth/brink/pull/2198))
- *(brink-cli)* correct false RUST_LOG-silent claim in #1957 regression test
- *(brink-cli)* drop unfulfilled clippy::unwrap_used expect on #[test] fns
- *(brink-cli)* render fatal compile diagnostics instead of a bare count ([#1957](https://github.com/Syynth/brink/pull/1957))
- *(brink-cli)* drop unfulfilled clippy::unwrap_used expectation in test
- *(brink-driver,brink-compiler)* PR #1712 review findings for #1610
- *(brink-driver)* route discovery warnings from native_source_root to stderr
- *(test)* avoid an incidental flow-name collision in prune fixtures ([#1673](https://github.com/Syynth/brink/pull/1673))
- *(brink-source-tree)* bound pruned-source scan by depth/budget, require file type
- *(brink-cli,docs)* address PR #1622 review findings
- *(brink-cli)* ide check renders the full four-tier Severity, not just error/warning
- *(cli,web)* apply PR #1600 review findings
- *(lsp,analyzer,cli,wasm)* review fixes for #1417 lint-override tier
- *(brink-intl)* review fixes for #1594 — correct false rename-stability claim, fix name dup, log/warn migrate count, add CLI-path migration test
- *(brink-intl)* XLIFF unit ids keyed on DefinitionId, not display name ([#1442](https://github.com/Syynth/brink/pull/1442))
- *(brink-ide)* close review findings on #1539's UFCS navigation trio
- *(brink-ide)* route def --at, find_references, and rename through the UFCS verdict table
- *(review #1480)* correct false test claims, cover NotATable, add warn-channel test
- *(brink-project-config)* thread brink.toml path/span into ConfigError itself ([#1384](https://github.com/Syynth/brink/pull/1384))
- *(brink-cli,brink-project-config,docs)* address PR #1432 review findings
- *(brink-project-config,brink-driver,brink-cli)* bound brink.toml walk-up at a workspace/git boundary
- *(brink-cli,brink-driver)* harden git-baseline guard against out-of-repo config root, fix stale docs
- *(brink-driver)* native_source_root walks past cwd for relative entries
- *(brink-cli,brink-driver)* apply review findings on #1403's SourceTree seam
- *(brink-driver)* delete dead ListScope::Project, collapse RealFs::project onto RealFs::new
- *(brink-compiler,brink-cli)* guard target/ discovery e2e; fix dropped doc separator
- *(brink-cli)* make ide_session type_policy test non-vacuous
- *(brink-cli)* Project::ide_session() applies resolved [lints]/dialect/types
- *(brink-source-tree,brink-driver)* drop SourceTree::list's contradictory root param, guard discover_native against non-.brink keys
- *(brink-environment)* thread config_key through the read half of #1369 too
- *(brink-analyzer)* validate [lints] codes against the real DiagnosticCode set
- *(brink-cli)* replace whole-tree drain with a lazy RealFs producer mount ([#1357](https://github.com/Syynth/brink/pull/1357))
- *(brink-cli)* read through to disk for INCLUDEs above the project root ([#1356](https://github.com/Syynth/brink/pull/1356))
- *(brink-cli)* avoid clippy::panic in the new #1295 fold-in test
- *(brink-cli)* guard load_git_baseline's repo_dir="." assumption (#1295 fold-in)
- *(brink-cli)* avoid clippy::panic in ide_cli test (fixup for #1295)
- *(brink-cli)* wire ide.rs discover to discover_native + fix native fs-write path bug ([#1295](https://github.com/Syynth/brink/pull/1295))
- *(brink-cli)* apply brink.toml consistently across all Driver-from-scratch code paths
- *(cli)* remove stray doc comment inside DialectArg match arms ([#524](https://github.com/Syynth/brink/pull/524))

### Other

- *(desktop)* guard run_cli's allowlist against brink-cli subcommand drift
- Merge pull request #1969 from Syynth/auto/issue-1949
- *(brink-cli)* assert #1957 regression test covers the path+byte-range prefix
- *(brink-cli)* cargo fmt log_diagnostic ([#1957](https://github.com/Syynth/brink/pull/1957))
- Merge pull request #1690 from Syynth/auto/issue-1442
- merge origin/main into train-fix for PR #1624
- merge origin/main + address review findings on #1615
- end-to-end coverage for [lints] deny-warnings across introduced_diagnostics and diagnostic_to_js ([#1383](https://github.com/Syynth/brink/pull/1383))
- Merge remote-tracking branch 'origin/main' into auto/issue-1417
- *(brink-cli,changeset)* CLI black-box coverage + web changeset for #1384
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-fix for PR #1401
- Merge remote-tracking branch 'origin/main' into train-pr
- *(brink-cli)* fix stale drain-mechanism doc in include-above-root test
- *(brink-cli)* drop the unfulfilled unwrap_used expectation on the test fn
- *(brink-cli)* split ide.rs into commands/handlers/project modules ([#682](https://github.com/Syynth/brink/pull/682))
- cargo fmt fixup for the #1295 fold-in guard test
- invert the brink-project-config → brink-analyzer edge ([#1234](https://github.com/Syynth/brink/pull/1234))
- *(comments)* fix stale value-model-spec citations and CLI help text ([#601](https://github.com/Syynth/brink/pull/601))
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- retire the converter pipeline — remove brink-converter, brink-json, brink-codegen-json ([#544](https://github.com/Syynth/brink/pull/544))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-cli-v0.0.9...brink-cli-v0.0.10) - 2026-07-10

### Other

- LineInfo on one shared projection: cache, option_path, standalone ([#480](https://github.com/Syynth/brink/pull/480)) ([#489](https://github.com/Syynth/brink/pull/489))
- Story::new takes Arc<Program>, not &Program
- *(runtime)* Program → Arc, delete <'p> lifetime (F1.1) ([#442](https://github.com/Syynth/brink/pull/442))

## [0.0.9](https://github.com/Syynth/brink/compare/brink-cli-v0.0.7...brink-cli-v0.0.9) - 2026-07-06

### Added

- *(ide,web)* unified StructuralResult + deleteSymbol + op-wide breakage gate ([#316](https://github.com/Syynth/brink/pull/316)) ([#336](https://github.com/Syynth/brink/pull/336))

### Other

- release v0.0.8 ([#307](https://github.com/Syynth/brink/pull/307))

## [0.0.8](https://github.com/Syynth/brink/compare/brink-cli-v0.0.7...brink-cli-v0.0.8) - 2026-07-01

### Added

- *(ide,web)* unified StructuralResult + deleteSymbol + op-wide breakage gate ([#316](https://github.com/Syynth/brink/pull/316)) ([#336](https://github.com/Syynth/brink/pull/336))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-cli-v0.0.6...brink-cli-v0.0.7) - 2026-06-20

### Added

- *(cli)* brink ide move-file / refactor * / actions ([#293](https://github.com/Syynth/brink/pull/293)) ([#300](https://github.com/Syynth/brink/pull/300))
- *(cli)* brink ide hover / signature / graph / lines ([#293](https://github.com/Syynth/brink/pull/293)) ([#299](https://github.com/Syynth/brink/pull/299))
- *(cli)* brink ide rename + mutation framework ([#292](https://github.com/Syynth/brink/pull/292)) ([#298](https://github.com/Syynth/brink/pull/298))
- *(cli)* brink ide symbols / unused / check ([#292](https://github.com/Syynth/brink/pull/292)) ([#297](https://github.com/Syynth/brink/pull/297))
- *(cli)* brink ide --at FILE:LINE:COL addressing ([#291](https://github.com/Syynth/brink/pull/291)) ([#296](https://github.com/Syynth/brink/pull/296))
- *(cli)* brink ide foundation + def/references ([#291](https://github.com/Syynth/brink/pull/291)) ([#295](https://github.com/Syynth/brink/pull/295))

### Fixed

- *(brink-ide)* fold same-file ref edits into promote/demote/move new_source ([#302](https://github.com/Syynth/brink/pull/302))

### Other

- *(book)* "brink ide" CLI chapter + JSON-schema stability ([#294](https://github.com/Syynth/brink/pull/294)) ([#301](https://github.com/Syynth/brink/pull/301))
- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-cli-v0.0.2...brink-cli-v0.0.3) - 2026-06-13

### Added

- *(cli)* accept raw .ink source for play/replay (closes #58) ([#60](https://github.com/Syynth/brink/pull/60))
