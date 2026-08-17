# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-project-config-v0.0.11...brink-project-config-v0.0.12) - 2026-08-17

### Added

- *(brink-project-config,brink-web,ink-editor,brink-studio,brink-desktop)* [project] entry precedence
- *(brink-ir)* split @[element(claims=…)] into @[convention(claims=…, order=N)]
- *(brink-analyzer)* validate `[project] elements` preset names against a closed set ([#1874](https://github.com/Syynth/brink/pull/1874))
- *(diagnostics)* add Info/Hint severity tier below Warning ([#1162](https://github.com/Syynth/brink/pull/1162))
- *(diagnostics)* brink.toml [lints] control plane — per-code severity + deny-warnings ([#1160](https://github.com/Syynth/brink/pull/1160))
- *(environment)* reify the compilation Environment + Project/SourceTree producer ([#1306](https://github.com/Syynth/brink/pull/1306))
- *(brink-project-config)* add SourceTree-based config discovery ([#1312](https://github.com/Syynth/brink/pull/1312))

### Fixed

- *(brink-project-config)* address PR review findings on #2180's rename sweep
- *(discovery)* directory-prune escape hatch + silent-skip diagnostic ([#1407](https://github.com/Syynth/brink/pull/1407))
- *(brink-project-config)* apply review findings on #1435 bounded-walk fix
- *(brink-project-config)* bound find_config for VCS-less trees, warn on skipped config, unify .git marker ([#1435](https://github.com/Syynth/brink/pull/1435))
- *(review #1480)* correct false test claims, cover NotATable, add warn-channel test
- *(brink-project-config)* thread brink.toml path/span into ConfigError itself ([#1384](https://github.com/Syynth/brink/pull/1384))
- *(brink-cli,brink-project-config,docs)* address PR #1432 review findings
- *(brink-project-config,brink-driver,brink-cli)* bound brink.toml walk-up at a workspace/git boundary
- *(brink-source-tree,brink-driver)* drop SourceTree::list's contradictory root param, guard discover_native against non-.brink keys
- *(brink-project-config)* find_config_in_tree must not propagate a probe read's non-NotFound error
- *(brink-analyzer)* validate [lints] codes against the real DiagnosticCode set

### Other

- rename [project] elements to [project] conventions with deprecated alias ([#2180](https://github.com/Syynth/brink/pull/2180))
- *(brink-ir/brink-analyzer/brink-db)* confine pattern-claiming handlers to the brink.toml-named conventions module ([#1844](https://github.com/Syynth/brink/pull/1844))
- *(brink-project-config)* document the #1425 workspace/git boundary
- *(brink-project-config)* drop dead root param, document SourceTree read/list asymmetry
- *(brink-source-tree,brink-project-config,brink-environment)* fix stale/incorrect PR #1378 review findings
- *(brink-project-config)* find_config_in_tree probes ancestors directly instead of walking the tree
