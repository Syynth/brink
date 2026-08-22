# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-intl-v0.0.11...brink-intl-v0.0.12) - 2026-08-22

### Added

- *(brink-runtime)* migrate output contract from Line to Step/OutputLine ([#1684](https://github.com/Syynth/brink/pull/1684))
- *(brink-syntax-native)* allow hyphens in span tag names ([#1996](https://github.com/Syynth/brink/pull/1996))
- *(brink-intl)* real XLIFF <pc>/<ph> inline-code mapping for markup spans ([#1734](https://github.com/Syynth/brink/pull/1734))
- *(brink-runtime,brink-intl)* resolve/translate LinePart::Span ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(format)* FS-3c FrameShapes section + invisible-container flag
- *(format)* T2-3 EffectRows emission — factored rows + DefinitionId→row table ([#862](https://github.com/Syynth/brink/pull/862))
- *(compiler,format,runtime)* M-3 renames — #@was, alias table, rehydration miss-path
- *(format,runtime)* TM-4 foundation — Value::Record, StructShapes section, field opcodes ([#620](https://github.com/Syynth/brink/pull/620))
- *(compiler,runtime)* T1b-2 — blocks, loops, collections, indexing go live ([#570](https://github.com/Syynth/brink/pull/570))

### Fixed

- *(xliff2)* coalesce spliced text across the catch-all boundary
- *(xliff2)* stop read_inline_content dropping text in unrecognized elements
- *(brink-intl)* export inverse for <cp>, correct foreign-<ph> disposition claim
- *(brink-intl)* close the remaining XLIFF-import silent drops in elements_to_parts
- *(brink-intl)* address #1806 review findings on CDATA decode PR
- *(brink-intl)* decode InlineElement::CData in elements_to_parts
- *(brink-intl)* reject <sc>/<ec>/<mrk> re-expression of brink spans on import
- clippy — disallowed_types allow for always-empty file_paths map, panic allow in test
- *(brink-analyzer)* #@was on a knot/stitch aliases every re-keyed descendant
- *(intl)* make compile-locale and regeneration alias-aware ([#1442](https://github.com/Syynth/brink/pull/1442))
- *(brink-intl)* address PR #1628 review findings
- *(brink-intl)* review fixes for #1594 — correct false rename-stability claim, fix name dup, log/warn migrate count, add CLI-path migration test
- *(brink-intl)* XLIFF unit ids keyed on DefinitionId, not display name ([#1442](https://github.com/Syynth/brink/pull/1442))

### Other

- Address review findings: legacy-slot back-compat fallback, spec truth, depth test
- gate elements_to_parts's pc/ph decode on brink-owned markers ([#1823](https://github.com/Syynth/brink/pull/1823))
- *(brink-compiler)* gate closure/path compile entry points behind test-util feature ([#2168](https://github.com/Syynth/brink/pull/2168))
- cargo fmt fixes for the sc/ec/mrk guard commit
- cargo fmt
- *(intl)* fmt + test-lint allowances for the alias rebinding tests
- *(intl)* record alias-aware rebinding rules and the transitive-#@was limit ([#1442](https://github.com/Syynth/brink/pull/1442))
- *(intl)* cover alias rebinding edges — direct-match precedence, anonymous scopes, ambiguity, XLIFF e2e
- *(intl)* allow clippy::panic in the rename characterization test
- *(intl)* characterize identity churn under a declared rename ([#1442](https://github.com/Syynth/brink/pull/1442))
- enable xliff2 metadata feature in workspace consumers
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge origin/main into train-pr
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- migrate fixture-building tests off the converter onto brink_compiler

## [0.0.10](https://github.com/Syynth/brink/compare/brink-intl-v0.0.9...brink-intl-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program
- *(runtime)* Program → Arc, delete <'p> lifetime (F1.1) ([#442](https://github.com/Syynth/brink/pull/442))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-intl-v0.0.6...brink-intl-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))
