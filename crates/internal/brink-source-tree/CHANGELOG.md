# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-source-tree-v0.0.11...brink-source-tree-v0.0.12) - 2026-08-23

### Fixed

- *(brink-source-tree)* bound pruned-source scan by depth/budget, require file type
- *(discovery)* directory-prune escape hatch + silent-skip diagnostic ([#1407](https://github.com/Syynth/brink/pull/1407))
- *(brink-lsp)* address PR #1603 review findings on ignored-dir pruning
- *(source-tree)* stop leaking the wrapper temp dir in the missing-root walk test
- *(brink-lsp)* reuse #1381's ignored-dir pruning in walk_and_load
- *(brink-source-tree,brink-driver)* resolve review findings on read/list asymmetry docs
- *(brink-source-tree,brink-driver)* drop SourceTree::list's contradictory root param, guard discover_native against non-.brink keys

### Other

- Merge origin/main into train-fix
- *(source-tree)* cover the symlink behavior delta Walk's doc comments claim
- *(source-tree)* make directory pruning structural via a shared Walk
- *(brink-lsp)* fix reviewer-flagged inaccuracies in admission policy docs
- *(brink-lsp,brink-source-tree)* decide and document the ignored-dir admission policy once
- Merge origin/main into train-fix for PR #1410
- *(brink-project-config)* drop dead root param, document SourceTree read/list asymmetry
- *(brink-source-tree,brink-project-config,brink-environment)* fix stale/incorrect PR #1378 review findings
