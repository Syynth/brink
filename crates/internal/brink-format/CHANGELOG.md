# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-format-v0.0.11...brink-format-v0.0.12) - 2026-08-22

### Added

- *(brink-ir,brink-format,brink-web)* transport succession rows through the conventions projection ([#2115](https://github.com/Syynth/brink/pull/2115))
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(compiler)* manifest-validated markup vocabulary (§4.2 second half)
- *(brink-runtime,brink-intl)* resolve/translate LinePart::Span ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(brink-format,brink-ir)* LinePart::Span wire encoding, hash-transparent recognition ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(stdlib)* fn-value verb layer slice 2 — filter_map, each, map_each ([#1679](https://github.com/Syynth/brink/pull/1679))
- *(runtime)* report dropped anonymous visit/turn state via LoadReport
- *(stdlib)* rename seq remove-by-index to `remove_at` ([#1484](https://github.com/Syynth/brink/pull/1484)) ([#1501](https://github.com/Syynth/brink/pull/1501))
- *(analyzer)* record `or`-coalescing types for LIR lowering ([#1492](https://github.com/Syynth/brink/pull/1492))
- *(compiler)* B1 or-coalescing surface spelling on the native dialect ([#1460](https://github.com/Syynth/brink/pull/1460))
- *(NS-A8)* protocol fence (E118), analyzer typing, tests, tier1 case, changeset (rebuild 2/3)
- *(NS-A8)* tower value kinds — glam-backed Value variants, wire, opcode, runtime ops (rebuild 1/3)
- *(NS-A5)* ranges as a real Value kind + the inhabited-range refinement ([#1111](https://github.com/Syynth/brink/pull/1111))
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(NS-A1)* Option[T] as the third parameterized builtin ([#1107](https://github.com/Syynth/brink/pull/1107))
- *(format)* FS-3c FrameShapes section + invisible-container flag
- *(format)* FS-1 FlowFrame suspended-flow section in SaveState
- *(format)* T2-3 EffectRows emission — factored rows + DefinitionId→row table ([#862](https://github.com/Syynth/brink/pull/862))
- *(stdlib)* char_at(s, i) string-indexing primitive ([#857](https://github.com/Syynth/brink/pull/857))
- *(t1e-2)* real MakeProjection/ProjRead/ProjWrite lowering + persistence
- *(compiler,format,runtime)* M-3 renames — #@was, alias table, rehydration miss-path
- *(runtime,format)* T1d-1 Value::Handle spine — VAL_HANDLE, .inkt atom, equality/display, wasm marshal ([#757](https://github.com/Syynth/brink/pull/757))
- *(t1c-3)* bind stdlib intrinsic, fn-value display form, structural equality
- *(t1c)* T1c-2 — lower/execute/persist #fn function values ([#700](https://github.com/Syynth/brink/pull/700))
- *(runtime,analyzer)* int()/float()/string() conversion intrinsics ([#659](https://github.com/Syynth/brink/pull/659))
- *(ir,codegen,runtime)* TM-4c structs LIR + codegen ([#666](https://github.com/Syynth/brink/pull/666))
- *(syntax,ir,analyzer)* TM-4b structs grammar + HIR + analyzer, diagnostics-only ([#665](https://github.com/Syynth/brink/pull/665))
- *(format,runtime)* TM-4 foundation — Value::Record, StructShapes section, field opcodes ([#620](https://github.com/Syynth/brink/pull/620))
- *(runtime,compiler)* T1b-4 — TakeGlobal/TakeTemp close the indexed-write COW cliff ([#576](https://github.com/Syynth/brink/pull/576))
- *(compiler,runtime)* T1b-2 — blocks, loops, collections, indexing go live ([#570](https://github.com/Syynth/brink/pull/570))
- *(format)* dedupe MAX_DECODE_DEPTH and cover VAL_MAP depth cap ([#561](https://github.com/Syynth/brink/pull/561))
- *(runtime)* reserve sharing-discipline opcode block ([#558](https://github.com/Syynth/brink/pull/558))
- *(runtime)* numerically reserve LiteralPool/StructShapes/EffectRows sections + collection-opcode block ([#554](https://github.com/Syynth/brink/pull/554))
- *(runtime)* format VERSION 4 — collection value tags + reserved surface ([#526](https://github.com/Syynth/brink/pull/526))
- *(runtime)* state plumbing for collection values — trees, wasm JSON, bindings ([#525](https://github.com/Syynth/brink/pull/525))
- *(runtime)* value core — Array/Map, COW mechanics, structural equality ([#524](https://github.com/Syynth/brink/pull/524))
- *(runtime)* no_std + alloc portability for brink-runtime/brink-format ([#434](https://github.com/Syynth/brink/pull/434))

### Fixed

- *(wasm-types,brink-format)* address #2314 review findings on SaveState TS mirror
- *(wasm-types,brink-format)* mirror global_ids/suspended in SaveState TS + add drift tripwire
- *(brink-format,brink-runtime)* persist block-run state for a parked flow ([#2108](https://github.com/Syynth/brink/pull/2108))
- *(brink-format,brink-ir)* strip transitions/templates from ConventionsProjectionDef
- *(brink-db,brink-format,brink-ir)* close #2212 review findings
- *(brink-ir,brink-db,brink-format)* close #2111's three review findings
- *(brink-analyzer)* register writes the conventions registry cell (#1840 Q4)
- *(brink-format)* PART_SPAN is its own VERSION bump, not a no-bump ride
- *(brink-runtime)* sticky purity guard + wire SeqVerbOp::is_effectful (#1679 review)
- *(compiler,analyzer,runtime,docs,wasm)* PR #1708 review findings for E157/anonymous-state ([#1674](https://github.com/Syynth/brink/pull/1674))
- *(brink-format)* review fixes for PR #1602 (LineFlags bit stability)
- *(brink-format)* remove dead STARTS_WITH_WS/ENDS_WITH_WS LineFlags
- *(brink-format)* LineFlags::from_template whitespace checks skip empty-string edge literals
- *(review)* merge main into B1b as-binding + apply reviewer findings ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(brink-format)* review fixes for #1476 sweep — real citation + accurate limitation
- *(analyzer)* surface coalescing mismatches as E066, fix eager-eval and precedence gaps (#1469 review)
- *(runtime,format)* restore no_std + alloc builds
- *(brink-format,brink-runtime)* reject duplicate map keys at every OrderedMap deserialization boundary ([#985](https://github.com/Syynth/brink/pull/985))
- *(brink-format)* OrderedMap equality is content-based, not order-sensitive
- *(brink-format)* reject param_count/params-metadata mismatch in .inkb reader ([#954](https://github.com/Syynth/brink/pull/954))
- *(brink-format)* allow clippy::panic on FS-1 frame-drift test
- *(brink-format)* round-trip struct_shapes through .inkt ([#883](https://github.com/Syynth/brink/pull/883))
- *(format,db)* wire #@private freeze semantics into T2-3 EffectRows emission ([#882](https://github.com/Syynth/brink/pull/882))
- add missing effect_rows field after merge with main
- *(brink-format)* correct CHAR_AT opcode comment's error variant names
- *(brink-runtime)* M-3 rehydration miss-path gaps — global-name rename + Value::List deep-rebind

### Other

- Merge remote-tracking branch 'origin/main' into auto/issue-2108
- checkpoint before merging origin/main (issue #2108)
- *(brink-compiler)* gate closure/path compile entry points behind test-util feature ([#2168](https://github.com/Syynth/brink/pull/2168))
- *(brink-format)* point struct-field mutator Value-layer test doc at the landed #1495 fix
- *(brink-format)* scope closure val-capture test's doc claim ([#1508](https://github.com/Syynth/brink/pull/1508))
- *(brink-format)* aliasing regression tests for Closure/OptionVal/Weighted val-capture ([#1508](https://github.com/Syynth/brink/pull/1508))
- cargo fmt
- *(brink-format)* update stale SeqVerb comment for all six kinds (#1679 review)
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- COW no-aliasing invariant sweep — regression tests + value-model doc statement ([#1476](https://github.com/Syynth/brink/pull/1476))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-format)* split .inkt reader into grammar-rule clusters ([#685](https://github.com/Syynth/brink/pull/685))
- Collect opcode family (0xFA) + Weighted value kind — wire, .inkt, VM ops
- ExecMode (dev/prod) + the ordering doctrine in the VM
- SeqSorted/SeqSortedBy opcodes (0xF8/0xF9) — wire + .inkt + proptest
- Merge pull request #1118 from Syynth/auto/ns-a1
- Merge remote-tracking branch 'origin/main' into train-pr
- merge origin/main into train-fix
- *(brink-format)* dedicated deep-equality law suite ([#746](https://github.com/Syynth/brink/pull/746))
- *(brink-format)* close the Value::List arb_value generator gap ([#746](https://github.com/Syynth/brink/pull/746))
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-pr
- Fix stale version/tag assertions and docs after M-2b/M-3 tag merge
- Merge origin/main into train-pr
- Merge remote-tracking branch 'origin/main' into train-fix
- property-based law suites for the value model (issue #672 workstream B)
- Merge origin/main into train-fix for PR #730
- Merge remote-tracking branch 'origin/main' into HEAD
- Merge remote-tracking branch 'origin/main' into train-pr
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- retire the converter pipeline — remove brink-converter, brink-json, brink-codegen-json ([#544](https://github.com/Syynth/brink/pull/544))
- migrate fixture-building tests off the converter onto brink_compiler

## [0.0.11](https://github.com/Syynth/brink/compare/brink-format-v0.0.10...brink-format-v0.0.11) - 2026-07-11

### Other

- Merge pull request #495 from Syynth/bronch/compiler-local-var-keyword-0fdbbc
- #@local directive — flow-private scope through HIR/LIR/codegen ([#473](https://github.com/Syynth/brink/pull/473))
- scope bits on GlobalVarDef/ContainerDef, inkb VERSION 3 ([#473](https://github.com/Syynth/brink/pull/473))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-format-v0.0.9...brink-format-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program

## [0.0.7](https://github.com/Syynth/brink/compare/brink-format-v0.0.6...brink-format-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-format-v0.0.3...brink-format-v0.0.4) - 2026-06-15

### Added

- *(runtime,web)* host-directed parameterized knot entry ([#178](https://github.com/Syynth/brink/pull/178)) ([#195](https://github.com/Syynth/brink/pull/195))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-format-v0.0.2...brink-format-v0.0.3) - 2026-06-13

### Added

- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))
- *(brink-web)* external-function binding foundation (Track A) ([#73](https://github.com/Syynth/brink/pull/73))
- *(studio)* Program Explorer — structured compiled-program inspector ([#68](https://github.com/Syynth/brink/pull/68)) ([#71](https://github.com/Syynth/brink/pull/71))
