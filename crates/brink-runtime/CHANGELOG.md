# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/brink-runtime-v0.0.11...brink-runtime-v0.0.12) - 2026-08-21

### Added

- *(brink-runtime,brink-web)* add Element type + OutputLine.element field (#1683, degenerate case)
- *(brink-runtime)* migrate output contract from Line to Step/OutputLine ([#1684](https://github.com/Syynth/brink/pull/1684))
- *(brink-ir,brink-analyzer,brink-runtime)* block capture for @[element(..., block)] ([#1839](https://github.com/Syynth/brink/pull/1839))
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(brink-runtime,brink-intl)* resolve/translate LinePart::Span ([#1716](https://github.com/Syynth/brink/pull/1716))
- *(stdlib)* fn-value verb layer slice 2 — filter_map, each, map_each ([#1679](https://github.com/Syynth/brink/pull/1679))
- *(stdlib)* the pure fn-value verb trio map/filter/fold (part of #1679)
- *(stdlib)* rename seq remove-by-index to `remove_at` ([#1484](https://github.com/Syynth/brink/pull/1484)) ([#1501](https://github.com/Syynth/brink/pull/1501))
- *(analyzer)* record `or`-coalescing types for LIR lowering ([#1492](https://github.com/Syynth/brink/pull/1492))
- *(compiler)* B1b the `as` binding — one construct, both condition positions ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(compiler)* B1 or-coalescing surface spelling on the native dialect ([#1460](https://github.com/Syynth/brink/pull/1460))
- *(NS-A9)* dialect-keyed type-policy default via resolve_type_policy seam
- *(NS-A8)* tower value kinds — glam-backed Value variants, wire, opcode, runtime ops (rebuild 1/3)
- *(NS-A5)* ranges as a real Value kind + the inhabited-range refinement ([#1111](https://github.com/Syynth/brink/pull/1111))
- *(NS-A6)* rng-as-cell — the RNG formalized, draws are writes, the rand verbs ([#1112](https://github.com/Syynth/brink/pull/1112))
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(NS-A1)* Option[T] as the third parameterized builtin ([#1107](https://github.com/Syynth/brink/pull/1107))
- *(bevy-brink)* enforce wake-condition purity at policy attach ([#995](https://github.com/Syynth/brink/pull/995))
- *(web)* FS-3w flow-addressed web surface — flow handles, Line::Suspended, wakeCheck ([#978](https://github.com/Syynth/brink/pull/978))
- *(brink-runtime)* expose Program::has_local_defaults for the host batch guard
- *(format)* FS-1 FlowFrame suspended-flow section in SaveState
- *(stdlib)* char_at(s, i) string-indexing primitive ([#857](https://github.com/Syynth/brink/pull/857))
- *(t1e-2)* real MakeProjection/ProjRead/ProjWrite lowering + persistence
- *(compiler,format,runtime)* M-3 renames — #@was, alias table, rehydration miss-path
- *(runtime,format)* T1d-1 Value::Handle spine — VAL_HANDLE, .inkt atom, equality/display, wasm marshal ([#757](https://github.com/Syynth/brink/pull/757))
- *(t1c-3)* bevy-brink host callback-invocation surface for function values
- *(t1c-3)* bind stdlib intrinsic, fn-value display form, structural equality
- *(t1c)* T1c-2 — lower/execute/persist #fn function values ([#700](https://github.com/Syynth/brink/pull/700))
- *(runtime,analyzer)* int()/float()/string() conversion intrinsics ([#659](https://github.com/Syynth/brink/pull/659))
- *(ir,codegen,runtime)* TM-4c structs LIR + codegen ([#666](https://github.com/Syynth/brink/pull/666))
- *(format,runtime)* TM-4 foundation — Value::Record, StructShapes section, field opcodes ([#620](https://github.com/Syynth/brink/pull/620))
- *(compiler,runtime)* T1b-3 — stdlib slice 1: len/keys/values/contains + push/insert/remove ([#571](https://github.com/Syynth/brink/pull/571))
- *(compiler,runtime)* T1b-2 — blocks, loops, collections, indexing go live ([#570](https://github.com/Syynth/brink/pull/570))
- *(format)* dedupe MAX_DECODE_DEPTH and cover VAL_MAP depth cap ([#561](https://github.com/Syynth/brink/pull/561))
- *(format)* add recursion-depth cap to VAL_ARRAY/VAL_MAP decode ([#524](https://github.com/Syynth/brink/pull/524))
- *(runtime)* format VERSION 4 — collection value tags + reserved surface ([#526](https://github.com/Syynth/brink/pull/526))
- *(runtime)* state plumbing for collection values — trees, wasm JSON, bindings ([#525](https://github.com/Syynth/brink/pull/525))
- *(runtime)* value core — Array/Map, COW mechanics, structural equality ([#524](https://github.com/Syynth/brink/pull/524))
- *(runtime)* no_std + alloc portability for brink-runtime/brink-format ([#434](https://github.com/Syynth/brink/pull/434))

### Fixed

- *(brink-runtime)* use expect() not panic!() in the new element.rs test
- *(brink-ir)* address #2344 review findings on heading slug/tag routing
- *(brink-format,brink-runtime)* persist block-run state for a parked flow ([#2108](https://github.com/Syynth/brink/pull/2108))
- *(brink-ir,brink-db,brink-analyzer)* conventions claiming reaches the whole project ([#2289](https://github.com/Syynth/brink/pull/2289))
- *(brink-runtime)* stop element-attach data leaking past its own run
- *(brink-runtime)* apply review findings for #2147 — accurate resolve_parts caller list, trailing-line parity, RULED doc extension
- *(brink-runtime)* suppress empty-content-capture blank line in the string-capture path ([#2147](https://github.com/Syynth/brink/pull/2147))
- *(brink-runtime)* correct scope claims + stale comment in #2091 empty-fragment suppression
- *(brink-runtime)* suppress the blank line from an empty content/Fragment capture ([#2091](https://github.com/Syynth/brink/pull/2091))
- *(brink-test-harness,brink-runtime)* apply review fixes for #2123 field-mutator COW fix
- *(brink-ir)* close the field-projection loop-append COW cliff ([#2123](https://github.com/Syynth/brink/pull/2123))
- *(brink-runtime,brink-web)* address #2109 review findings for Element degenerate field
- *(brink-runtime,brink-web,bevy-brink)* address #2102 review findings for the Step/OutputLine migration
- *(brink-runtime)* stop clobbering the pending RanOutOfContent cause across a call_function eval
- *(brink-runtime)* split RanOutOfContent into four call-stack-keyed causes
- *(brink-runtime)* drive_function_eval takes a caller-supplied step_limit
- *(brink-runtime)* sticky purity guard + wire SeqVerbOp::is_effectful (#1679 review)
- *(compiler,analyzer,runtime,docs,wasm)* PR #1708 review findings for E157/anonymous-state ([#1674](https://github.com/Syynth/brink/pull/1674))
- *(stdlib)* review findings for PR #1707 — CI clippy fix + comparator/callback role split
- *(brink-runtime,docs)* address #1701 review findings
- *(docs)* address review findings on brkt-trailing-section-findings.md ([#1519](https://github.com/Syynth/brink/pull/1519))
- clear clippy pedantic findings in the #937 view and bench axis
- *(bevy-brink)* row-directed wake dirtying ([#1146](https://github.com/Syynth/brink/pull/1146)) — un-quarantine the #1101 flake
- *(brink-format)* remove dead STARTS_WITH_WS/ENDS_WITH_WS LineFlags
- *(compiler)* admit struct construction literals as VAR/CONST defaults ([#1530](https://github.com/Syynth/brink/pull/1530))
- *(docs)* address review findings on yield-time terminal classifier writeup ([#1520](https://github.com/Syynth/brink/pull/1520))
- *(brink-analyzer,brink-runtime)* review findings for #1542 coverage bundle
- *(review)* merge main into B1b as-binding + apply reviewer findings ([#1475](https://github.com/Syynth/brink/pull/1475))
- *(analyzer)* surface coalescing mismatches as E066, fix eager-eval and precedence gaps (#1469 review)
- *(brink-runtime)* gate unused crate::vm import behind testing feature
- *(runtime,format)* restore no_std + alloc builds
- collapse doubled spaces in WeightedBadWeight message (review nit)
- *(web)* cap FlowHandle.continueMaximally + align StorySessionHandle.spawnFlow (#999, #1000)
- *(brink-runtime)* exhaustive cast_to_int/cast_to_float, fault on unruled variants ([#955](https://github.com/Syynth/brink/pull/955))
- *(brink-runtime)* total value equality — pointers, Projection, exact float ==
- *(brink-runtime)* Array/Array equality faults instead of comparing
- *(runtime)* spawn_flow/spawn_flow_shared by-id paths enforce #@private visibility ([#803](https://github.com/Syynth/brink/pull/803))
- *(brink-runtime)* M-3 rehydration miss-path gaps — global-name rename + Value::List deep-rebind
- *(brink-runtime)* link() rejects out-of-range NameId instead of panicking
- *(runtime)* contains(map, needle) totals on non-key-domain needle ([#580](https://github.com/Syynth/brink/pull/580))

### Other

- Fix review findings: past-tense #1684 landing citations
- Relabel oracle.rs / terminal_classification.rs allowance as ruled-permanent ([#1574](https://github.com/Syynth/brink/pull/1574))
- Fix #2903: index-operand postfix (a[0]++, m["k"]++) silently non-mutating
- bare-variable postfix x++/x-- inside a ~ { … } block never mutated
- Fix review findings on #2759 build-stamp freshness check
- Replace check-target-freshness.mjs's static heuristic with a real build stamp ([#2759](https://github.com/Syynth/brink/pull/2759))
- Fix reviewer findings: dead-link and wrong-obstacle in guard-limits doc
- name policy drift in benchmark_fixtures_compile.rs's guard-limits section
- address PR #2800 review findings
- *(brink-runtime)* guard benchmark fixtures against silent compile rot
- Merge origin/main into auto/issue-2077 (resolve #2079 compact-cue conflict)
- checkpoint before merging origin/main (issue #2108)
- *(brink-compiler)* gate closure/path compile entry points behind test-util feature ([#2168](https://github.com/Syynth/brink/pull/2168))
- Merge pull request #2159 from Syynth/auto/issue-2147
- extend Budget.steps docs to eval_function/resume_function_eval, note changeset tightening
- cargo fmt
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(stdlib)* record the shipped trio + changeset; E119 prose across specs and book
- *(bevy-brink)* borrow the frame-start world instead of cloning it per flow
- Merge origin/main into train-fix for PR #1579
- *(brink-runtime)* pin the terminal-classification seam (issue #1520)
- *(brink-analyzer,brink-runtime)* prop_oneof! exhaustiveness sweep for RefKind/MapKey
- *(brink-runtime)* pin the fragment-absent legacy .brkt decode shape
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge pull request #1523 from Syynth/auto/issue-1521
- Merge remote-tracking branch 'origin/main' into train-fix
- *(B1b)* correct the Option chapter's stale `as` spelling; changeset ([#1475](https://github.com/Syynth/brink/pull/1475))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(brink-runtime)* split story.rs into type-family submodules ([#681](https://github.com/Syynth/brink/pull/681))
- *(brink-runtime)* split output.rs into fragment/consume submodules ([#686](https://github.com/Syynth/brink/pull/686))
- bring train-fix up to date with origin/main
- *(brink-runtime)* cover Mat3/Mat4 scale in mat*scalar unit test ([#1145](https://github.com/Syynth/brink/pull/1145))
- Merge remote-tracking branch 'origin/main' into train-fix
- F34 comparator write-guard + F35 bevy default (incomplete)
- analyzer wiring — Ty::Weighted, typing arms, effect rows, F29 discharge
- Collect opcode family (0xFA) + Weighted value kind — wire, .inkt, VM ops
- ExecMode (dev/prod) + the ordering doctrine in the VM
- NS-A9 review fixes: LSP premise test rewrite, invalid-types-as-unset, two bench mounts
- Merge origin/main (post-#1133) — E116+E117 coexist; A5 rows absorbed into the shared intrinsics table
- Merge origin/main (NS-A3 registry) into ns-a6
- *(runtime)* correct note_effect_emit comment per review
- Merge origin/main into auto/issue-995
- *(brink-runtime)* update stale map-equality NOTE post #909 ruling
- Merge remote-tracking branch 'origin/main' into train-pr
- merge origin/main into train-fix
- *(brink-runtime)* transcript round-trip law over arbitrary parts/values ([#746](https://github.com/Syynth/brink/pull/746))
- Merge remote-tracking branch 'origin/main' into train-pr
- *(t2)* ground-truth effect-completeness harness — instrumented VM vs static rows ([#870](https://github.com/Syynth/brink/pull/870))
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge origin/main into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- path-projections tooling tail (docs/t1e-spec.md §8 item 3, #850)
- Merge pull request #816 from Syynth/auto/issue-803
- Merge pull request #797 from Syynth/auto/issue-785
- Merge origin/main into train-pr
- *(brink-runtime)* round-trip Value::Handle through the transcript codec
- *(brink-runtime)* new vm_no_panic fuzz target for malformed .inkb
- Merge origin/main into train-fix for PR #730
- *(comments)* fix stale value-model-spec citations and CLI help text ([#601](https://github.com/Syynth/brink/pull/601))
- Merge remote-tracking branch 'origin/main' into train-pr
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))
- retire the converter pipeline — remove brink-converter, brink-json, brink-codegen-json ([#544](https://github.com/Syynth/brink/pull/544))
- migrate fixture-building tests off the converter onto brink_compiler
- #@local implies VISITS counting ([#496](https://github.com/Syynth/brink/pull/496)) ([#507](https://github.com/Syynth/brink/pull/507))

## [0.0.11](https://github.com/Syynth/brink/compare/brink-runtime-v0.0.10...brink-runtime-v0.0.11) - 2026-07-11

### Other

- Merge pull request #495 from Syynth/bronch/compiler-local-var-keyword-0fdbbc
- seed ResolvedPolicy from compiled #@local scope bits ([#473](https://github.com/Syynth/brink/pull/473))

## [0.0.10](https://github.com/Syynth/brink/compare/brink-runtime-v0.0.9...brink-runtime-v0.0.10) - 2026-07-10

### Added

- *(bevy-brink)* thin onto scoped-flow-state core — shared World, per-flow FlowLocal (F6.2, #441) ([#488](https://github.com/Syynth/brink/pull/488))
- *(runtime)* subtree-inclusive knot scope in ResolvedPolicy (F6.1c, #441) ([#484](https://github.com/Syynth/brink/pull/484))
- *(runtime)* lift save_state/load_state off Story to any flow's context (F6.1b, #441) ([#481](https://github.com/Syynth/brink/pull/481))
- *(web)* speculation binding — composable verbs + evaluate + Value marshaling (F4.3) ([#456](https://github.com/Syynth/brink/pull/456))
- *(runtime)* kind-tiered externals handler + Speculation resume_function_eval (F4.2) ([#455](https://github.com/Syynth/brink/pull/455))
- *(runtime)* Speculation primitive — self-contained side-effect-proof speculative run (F4.1) ([#453](https://github.com/Syynth/brink/pull/453))
- *(runtime)* fork + sandbox mode + discard (F3.2) ([#451](https://github.com/Syynth/brink/pull/451))
- *(runtime)* flat FlowLocal storage + policy-aware ContextView routing (F2.2) ([#448](https://github.com/Syynth/brink/pull/448))
- *(runtime)* WorldPolicy + ResolvedPolicy + resolution (F2.1) ([#446](https://github.com/Syynth/brink/pull/446))

### Other

- Merge pull request #477 from Syynth/bronch/book-audit-updates-e2fc23
- *(runtime)* extract shared drive-to-terminal op onto FlowInstance (F6.1a) ([#475](https://github.com/Syynth/brink/pull/475))
- *(runtime)* CoW FlowLocal — frozen-base read-through chain + freeze (F3.1) ([#450](https://github.com/Syynth/brink/pull/450))
- *(runtime)* split Context into World + FlowLocal + routing view, all-World (F1.3) ([#445](https://github.com/Syynth/brink/pull/445))
- *(runtime)* collapse duplicate Context API, route Story accessors through the seam (F1.2) ([#444](https://github.com/Syynth/brink/pull/444))
- *(runtime)* Program → Arc, delete <'p> lifetime (F1.1) ([#442](https://github.com/Syynth/brink/pull/442))

## [0.0.9](https://github.com/Syynth/brink/compare/brink-runtime-v0.0.7...brink-runtime-v0.0.9) - 2026-07-06

### Added

- *(web)* expose StorySession as WebSession/StorySessionHandle ([#387](https://github.com/Syynth/brink/pull/387)) ([#389](https://github.com/Syynth/brink/pull/389))
- *(runtime)* Story Session core — journal, replay, snapshot/diff ([#385](https://github.com/Syynth/brink/pull/385))

### Other

- migrate LocalSessionProvider onto public StorySession ([#388](https://github.com/Syynth/brink/pull/388)) ([#401](https://github.com/Syynth/brink/pull/401))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-runtime-v0.0.6...brink-runtime-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-runtime-v0.0.3...brink-runtime-v0.0.4) - 2026-06-15

### Added

- shared-context flow sessions — "+ New flow" ([#200](https://github.com/Syynth/brink/pull/200)) ([#201](https://github.com/Syynth/brink/pull/201))
- *(runtime,web)* host-directed parameterized knot entry ([#178](https://github.com/Syynth/brink/pull/178)) ([#195](https://github.com/Syynth/brink/pull/195))
- *(replay)* shared external recording/replay primitive ([#189](https://github.com/Syynth/brink/pull/189)) ([#191](https://github.com/Syynth/brink/pull/191))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-runtime-v0.0.2...brink-runtime-v0.0.3) - 2026-06-13

### Added

- *(runtime,web)* choose_path_string / goToPath — host-directed story entry ([#165](https://github.com/Syynth/brink/pull/165)) ([#167](https://github.com/Syynth/brink/pull/167))
- *(brink-web)* external-function binding foundation (Track A) ([#73](https://github.com/Syynth/brink/pull/73))
- structured, name-resolved State View debugger ([#62](https://github.com/Syynth/brink/pull/62)) ([#70](https://github.com/Syynth/brink/pull/70))
