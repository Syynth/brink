# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-syntax-v0.0.11...brink-syntax-v0.0.12) - 2026-08-15

### Added

- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(brink-ir)* widen hir::Stitch with return_type (issue #1509)
- *(NS-A5)* ranges as a real Value kind + the inhabited-range refinement ([#1111](https://github.com/Syynth/brink/pull/1111))
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(syntax)* await statement grammar — CST/AST for FlowFrame suspension (FS-2)
- *(compiler)* T1e-1 path-projection grammar + HIR + analyzer ([#831](https://github.com/Syynth/brink/pull/831))
- *(syntax)* IMPORT grammar — KW_IMPORT keyword, both statement forms
- *(syntax)* parse #fn(name, args…) function-value literals in expression position ([#699](https://github.com/Syynth/brink/pull/699))
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(syntax,analyzer)* CONST declarations accept type annotations ([#641](https://github.com/Syynth/brink/pull/641))
- *(syntax)* TM-2 inline type annotation syntax — grammar/HIR/fmt/IDE, feeding signature() ([#618](https://github.com/Syynth/brink/pull/618))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(brink-syntax,brink-analyzer,brink-ir,brink-ide)* address PR #2271 review findings
- *(brink-syntax)* swap panic! for expect in the new stitch-header test
- *(brink-analyzer,brink-syntax)* review findings for #1509 stitch return-type
- *(compiler)* reject direct calls through a computed fn-value callee instead of silently dropping them ([#869](https://github.com/Syynth/brink/pull/869))
- *(brink-syntax)* bump_assert degrades to a parse error instead of panicking
- *(brink-syntax)* accept Index base in field-assignment target grammar ([#674](https://github.com/Syynth/brink/pull/674))

### Other

- promote 22 ignore'd doctests to no_run/text ([#1700](https://github.com/Syynth/brink/pull/1700))
- *(brink-syntax)* split inline/cst.rs test file by CST construct
- fmt, E052 fence doc, changeset, and exhaustive-match completion for await
- Merge remote-tracking branch 'origin/main' into train-pr
- origin/main into train-fix for PR #770
- M-2 clippy/fmt polish + @brink-lang/web changeset
- Merge origin/main into train-fix for PR #740
- *(brink-syntax)* seed the parser fuzz corpus with the T1b/T1c surface
- *(syntax)* fix stale at_struct_literal reference in at_struct_decl doc ([#665](https://github.com/Syynth/brink/pull/665))
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- salsa into brink-db — query-memoized pipeline stages (phase 0 slice B) ([#515](https://github.com/Syynth/brink/pull/515))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-syntax-v0.0.9...brink-syntax-v0.0.10) - 2026-07-10

### Other

- upgrade pinned Rust toolchain to 1.97.0
- Story::new takes Arc<Program>, not &Program

## [0.0.7](https://github.com/Syynth/brink/compare/brink-syntax-v0.0.6...brink-syntax-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-syntax-v0.0.2...brink-syntax-v0.0.3) - 2026-06-13

### Fixed

- *(syntax)* accept contextual keywords as EXTERNAL names and params ([#75](https://github.com/Syynth/brink/pull/75))
- *(compiler)* surface syntax errors + reject malformed inline conditionals (closes #44) ([#48](https://github.com/Syynth/brink/pull/48))

### Other

- *(release)* unblock release-plz — un-ignore committed files ([#169](https://github.com/Syynth/brink/pull/169))
