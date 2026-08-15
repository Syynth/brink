# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-driver-v0.0.11...brink-driver-v0.0.12) - 2026-08-15

### Added

- *(diagnostics)* add Info/Hint severity tier below Warning ([#1162](https://github.com/Syynth/brink/pull/1162))
- *(diagnostics)* brink.toml [lints] control plane — per-code severity + deny-warnings ([#1160](https://github.com/Syynth/brink/pull/1160))
- *(brink-driver,brink-compiler,brink-cli)* discover_native over SourceTree — RealFs walk + GitRev git-baseline
- *(brink-db,brink-driver)* add SourceTree seam (RealFs/GitRev/InMemory), unwired
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- *(analyzer,db)* TM-3 — types = strict policy, Unknown/Conflicted-escape, E063 wiring ([#619](https://github.com/Syynth/brink/pull/619))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(brink-driver,brink-compiler)* PR #1712 review findings for #1610
- *(brink-driver)* route discovery warnings from native_source_root to stderr
- *(brink-db,brink-compiler,brink-lsp)* normalize #1504 root-content qualifier to a root-relative key ([#1696](https://github.com/Syynth/brink/pull/1696))
- *(review)* apply #1693 review findings for #1504 root-content identity
- *(docs)* address review findings on root-content-identity-findings.md ([#1504](https://github.com/Syynth/brink/pull/1504))
- *(discovery)* directory-prune escape hatch + silent-skip diagnostic ([#1407](https://github.com/Syynth/brink/pull/1407))
- *(brink-project-config)* apply review findings on #1435 bounded-walk fix
- *(review)* apply w?? merge-train findings on PR #1595 ([#1387](https://github.com/Syynth/brink/pull/1387))
- *(lsp)* mint compile-identical native module identity from absolute keys
- *(lsp)* unpin M-2d native-homonym diagnostics from declared dialect; fix stale prose
- *(brink-db)* native .brink files are one project, not one project per file
- *(ide,lsp,web)* propagate analysis options + module diagnostics to the IDE path ([#1553](https://github.com/Syynth/brink/pull/1553))
- *(brink-project-config,brink-driver,brink-cli)* bound brink.toml walk-up at a workspace/git boundary
- *(brink-cli,brink-driver)* harden git-baseline guard against out-of-repo config root, fix stale docs
- *(brink-driver)* native_source_root walks past cwd for relative entries
- *(brink-driver)* map an empty find_config parent to "." instead of absolutizing
- *(brink-cli,brink-driver)* apply review findings on #1403's SourceTree seam
- *(brink-driver)* delete dead ListScope::Project, collapse RealFs::project onto RealFs::new
- *(brink-source-tree,brink-driver)* resolve review findings on read/list asymmetry docs
- *(brink-driver)* prune ignored dirs from native compile walk; drop dead ListScope::Project brink.toml entry
- *(brink-source-tree,brink-driver)* drop SourceTree::list's contradictory root param, guard discover_native against non-.brink keys
- *(brink-db,brink-driver)* cite the full decision-log heading for the SourceTree seam
- *(brink-driver)* skip empty INCLUDE paths in discovery so E037 surfaces
- *(analyzer,db)* E063 error-severity under strict + void-assignment error E067 ([#619](https://github.com/Syynth/brink/pull/619))

### Other

- *(corpus)* tier-1 case for root weave in entry + INCLUDEd file ([#1504](https://github.com/Syynth/brink/pull/1504))
- merge origin/main into train-fix for PR #1662
- merge origin/main + address review findings on #1615
- close w48 gaps — native compile_fragment entry, RealFs brink.toml edge cases, ProjectSource.files contract
- cargo fmt
- *(brink-driver)* cover Driver::analyze_project's module-aware + E085-scoping contract
- *(test-harness)* route the t2 ground-truth corpus walk through Walk
- *(source-tree)* make directory pruning structural via a shared Walk
- Merge origin/main into train-fix for PR #1410
- *(brink-driver)* fix stale/false claims in ListScope-deletion doc rewrite
- *(brink-project-config)* find_config_in_tree probes ancestors directly instead of walking the tree
- merge origin/main into train-fix for PR #1364
- *(brink-db)* extract SourceTree seam into leaf crate brink-source-tree
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- salsa into brink-db — query-memoized pipeline stages (phase 0 slice B) ([#515](https://github.com/Syynth/brink/pull/515))
- extract the symbol service from brink-analyzer (phase 0 slice A) ([#509](https://github.com/Syynth/brink/pull/509))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-driver-v0.0.9...brink-driver-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program

## [0.0.7](https://github.com/Syynth/brink/compare/brink-driver-v0.0.6...brink-driver-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.5](https://github.com/Syynth/brink/compare/brink-driver-v0.0.4...brink-driver-v0.0.5) - 2026-06-17

### Added

- *(ide)* file rename/move core (#164 Stage 3, PR A) ([#252](https://github.com/Syynth/brink/pull/252))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-driver-v0.0.3...brink-driver-v0.0.4) - 2026-06-15

### Fixed

- *(#187)* tunnel calls aren't terminal (E033) + resolve diagnostics to paths (#190)

## [0.0.3](https://github.com/Syynth/brink/compare/brink-driver-v0.0.2...brink-driver-v0.0.3) - 2026-06-13

### Added

- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))

### Other

- Studio IDE: doc comments + type hints for all declarations (Track B integration) ([#101](https://github.com/Syynth/brink/pull/101))
