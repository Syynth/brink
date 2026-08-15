# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-fmt-v0.0.11...brink-fmt-v0.0.12) - 2026-08-15

### Added

- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(brink-ir)* per-branch source spans on CondBranch/SequenceBranch ([#404](https://github.com/Syynth/brink/pull/404))
- *(brink-ir)* widen hir::Stitch with return_type (issue #1509)
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(ir,analyzer)* await HIR lowering, strict-ink gate (E051), LIR fence (E052)
- *(brink-fmt)* canonical IMPORT statement spacing
- *(compiler)* M-2 visibility model + HIR imports + §7 diagnostics
- *(brink-fmt)* format STRUCT decl bodies like blocks — multiline + trailing comma
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(syntax,analyzer)* CONST declarations accept type annotations ([#641](https://github.com/Syynth/brink/pull/641))
- *(syntax)* TM-2 inline type annotation syntax — grammar/HIR/fmt/IDE, feeding signature() ([#618](https://github.com/Syynth/brink/pull/618))
- *(fmt)* indentation-aware formatting for ~ { } block internals ([#573](https://github.com/Syynth/brink/pull/573))
- *(compiler)* T1b-1 superset grammar + HIR + dialect gate ([#569](https://github.com/Syynth/brink/pull/569))

### Fixed

- *(brink-fmt)* delete unreachable character-based declaration fallback
- *(brink-fmt)* retokenize VAR/CONST/LIST declarations through join_token_text
- *(fmt)* preserve comments outside a ~ { … } block's body
- *(fmt)* don't classify construct lines as block-comment lines
- *(brink-fmt)* preserve comments in STRUCT decl bodies; drop dead branch
- *(fmt)* anchor verbatim spans to the physical line start ([#603](https://github.com/Syynth/brink/pull/603))
- *(fmt)* bail to verbatim pass-through on parse errors inside ~ { } blocks ([#603](https://github.com/Syynth/brink/pull/603))

### Other

- checkpoint before merging origin/main (issue #2108)
- *(brink-fmt)* split lib.rs into depth/classify/render modules
- canonicalize whitespace around type-annotation colons ([#642](https://github.com/Syynth/brink/pull/642))
- retokenize single-line `~ expr` logic lines ([#858](https://github.com/Syynth/brink/pull/858))
- path-projections tooling tail (docs/t1e-spec.md §8 item 3, #850)
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-fmt-v0.0.9...brink-fmt-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program

## [0.0.7](https://github.com/Syynth/brink/compare/brink-fmt-v0.0.6...brink-fmt-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))
