# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.17](https://github.com/Syynth/brink/compare/brink-environment-v0.0.16...brink-environment-v0.0.17) - 2026-08-29

### Added

- *(debug)* emit SectionKind::DebugInfo (0x11) — D6 bytecode offset -> source range map

### Fixed

- `[lints] allow` suppresses the diagnostic, and advisory codes are overridable ([#3175](https://github.com/Syynth/brink/pull/3175))

## [0.0.15](https://github.com/Syynth/brink/compare/brink-environment-v0.0.11...brink-environment-v0.0.15) - 2026-08-23

### Added

- *(brink-environment)* mount stdlib source into Environment's manifest
- *(diagnostics)* add Info/Hint severity tier below Warning ([#1162](https://github.com/Syynth/brink/pull/1162))
- *(diagnostics)* brink.toml [lints] control plane — per-code severity + deny-warnings ([#1160](https://github.com/Syynth/brink/pull/1160))

### Fixed

- *(brink-ir)* E184 backstop for the CONST/VAR/EXTERNAL silent-drop class ([#2262](https://github.com/Syynth/brink/pull/2262))
- *(brink-ir)* CUE/PARENTHETICAL tag extensions strip-then-match, uniformly
- *(brink-environment)* classify std preset's scene_entered extern as @kind effect
- *(brink-ir)* compact cue (@NAME: text) is now a claim candidate ([#2079](https://github.com/Syynth/brink/pull/2079))
- *(brink-db)* std:: mounts as a PEER ROOT of story::, not a child of it ([#2245](https://github.com/Syynth/brink/pull/2245))
- *(brink-cli,brink-lsp)* mount stdlib into IDE loaders ([#2198](https://github.com/Syynth/brink/pull/2198))
- *(brink-environment)* relocate stdlib source into package root
- *(brink-analyzer)* apply PR #2076 review findings for #1874
- *(brink-project-config)* thread brink.toml path/span into ConfigError itself ([#1384](https://github.com/Syynth/brink/pull/1384))
- *(brink-driver)* delete dead ListScope::Project, collapse RealFs::project onto RealFs::new
- *(brink-source-tree,brink-driver)* drop SourceTree::list's contradictory root param, guard discover_native against non-.brink keys
- *(brink-environment)* thread config_key through the read half of #1369 too
- *(brink-environment)* thread config_key path into LoadError::Config
- *(brink-analyzer)* validate [lints] codes against the real DiagnosticCode set
- *(brink-environment)* avoid clippy::panic in a test unreachable branch

### Other

- Merge branch 'main' into auto/issue-2179
- Merge remote-tracking branch 'origin/main' into auto/issue-2108
- checkpoint before merging origin/main (issue #2108)
- Merge pull request #2203 from Syynth/auto/issue-2166
- *(brink-environment)* prove mount reachability instead of asserting warnings-empty
- Merge remote-tracking branch 'origin/main' into auto/issue-1436
- Merge origin/main into train-fix for PR #1400
- *(brink-project-config)* drop dead root param, document SourceTree read/list asymmetry
- *(brink-source-tree,brink-project-config,brink-environment)* fix stale/incorrect PR #1378 review findings
