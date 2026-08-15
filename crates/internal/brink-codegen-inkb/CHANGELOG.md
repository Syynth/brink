# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-codegen-inkb-v0.0.11...brink-codegen-inkb-v0.0.12) - 2026-08-15

### Added

- *(brink-ir,brink-analyzer,brink-runtime)* block capture for @[element(..., block)] ([#1839](https://github.com/Syynth/brink/pull/1839))
- *(stdlib)* fn-value verb layer slice 2 — filter_map, each, map_each ([#1679](https://github.com/Syynth/brink/pull/1679))
- *(stdlib)* the pure fn-value verb trio map/filter/fold (part of #1679)
- *(codegen)* assert no two containers share a DefinitionId ([#1673](https://github.com/Syynth/brink/pull/1673))
- *(stdlib)* rename seq remove-by-index to `remove_at` ([#1484](https://github.com/Syynth/brink/pull/1484)) ([#1501](https://github.com/Syynth/brink/pull/1501))
- *(compiler)* B1b the `as` binding — one construct, both condition positions ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(compiler)* B1 or-coalescing surface spelling on the native dialect ([#1460](https://github.com/Syynth/brink/pull/1460))
- *(NS-A8)* protocol fence (E118), analyzer typing, tests, tier1 case, changeset (rebuild 2/3)
- *(NS-A5)* ranges as a real Value kind + the inhabited-range refinement ([#1111](https://github.com/Syynth/brink/pull/1111))
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(NS-A1)* Option[T] as the third parameterized builtin ([#1107](https://github.com/Syynth/brink/pull/1107))
- *(format)* FS-3c FrameShapes section + invisible-container flag
- *(format)* T2-3 EffectRows emission — factored rows + DefinitionId→row table ([#862](https://github.com/Syynth/brink/pull/862))
- *(stdlib)* char_at(s, i) string-indexing primitive ([#857](https://github.com/Syynth/brink/pull/857))
- *(t1e-2)* real MakeProjection/ProjRead/ProjWrite lowering + persistence
- *(compiler,format,runtime)* M-3 renames — #@was, alias table, rehydration miss-path
- *(t1c-3)* bind stdlib intrinsic, fn-value display form, structural equality
- *(t1c)* T1c-2 — lower/execute/persist #fn function values ([#700](https://github.com/Syynth/brink/pull/700))
- *(runtime,analyzer)* int()/float()/string() conversion intrinsics ([#659](https://github.com/Syynth/brink/pull/659))
- *(ir,codegen,runtime)* TM-4c structs LIR + codegen ([#666](https://github.com/Syynth/brink/pull/666))
- *(format,runtime)* TM-4 foundation — Value::Record, StructShapes section, field opcodes ([#620](https://github.com/Syynth/brink/pull/620))
- *(compiler,runtime)* T1b-3 — stdlib slice 1: len/keys/values/contains + push/insert/remove ([#571](https://github.com/Syynth/brink/pull/571))
- *(compiler,runtime)* T1b-2 — blocks, loops, collections, indexing go live ([#570](https://github.com/Syynth/brink/pull/570))

### Fixed

- *(brink-codegen-inkb)* collapse whitespace recursively into Span children
- *(compiler)* admit struct construction literals as VAR/CONST defaults ([#1530](https://github.com/Syynth/brink/pull/1530))
- *(brink-ir,codegen-inkb)* construction-literal initializers evaluate in source order
- *(compiler,codegen)* real error paths for two debug_assert-guarded backstops (#585, #586)
- *(compiler)* break/continue outside a loop and mutator arity are E057/E058 compile errors (#577, #581)

### Other

- checkpoint before merging origin/main (issue #2108)
- cargo fmt
- Merge remote-tracking branch 'origin/main' into train-fix
- weighted/roll/heap lowering + codegen + E120 construction gate
- sort family in LIR/codegen/analyzer + E119 comparator-contract gate
- symbolic name-ref + relocation chunk representation (FG-4b, #808)
- Merge origin/main into train-pr
- Merge origin/main into train-fix for PR #730
- Merge remote-tracking branch 'origin/main' into train-pr
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))

## [0.0.11](https://github.com/Syynth/brink/compare/brink-codegen-inkb-v0.0.10...brink-codegen-inkb-v0.0.11) - 2026-07-11

### Other

- Merge pull request #495 from Syynth/bronch/compiler-local-var-keyword-0fdbbc
- #@local directive — flow-private scope through HIR/LIR/codegen ([#473](https://github.com/Syynth/brink/pull/473))
- scope bits on GlobalVarDef/ContainerDef, inkb VERSION 3 ([#473](https://github.com/Syynth/brink/pull/473))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-codegen-inkb-v0.0.9...brink-codegen-inkb-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program

## [0.0.7](https://github.com/Syynth/brink/compare/brink-codegen-inkb-v0.0.6...brink-codegen-inkb-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-codegen-inkb-v0.0.3...brink-codegen-inkb-v0.0.4) - 2026-06-15

### Added

- *(runtime,web)* host-directed parameterized knot entry ([#178](https://github.com/Syynth/brink/pull/178)) ([#195](https://github.com/Syynth/brink/pull/195))
