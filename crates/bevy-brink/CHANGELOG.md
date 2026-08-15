# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.12](https://github.com/Syynth/brink/compare/bevy-brink-v0.0.11...bevy-brink-v0.0.12) - 2026-08-15

### Added

- *(brink-runtime)* migrate output contract from Line to Step/OutputLine ([#1684](https://github.com/Syynth/brink/pull/1684))
- *(lang)* type-name conformance sweep — angle brackets + Uppercase non-primitives, Option<T>/Weighted<T> annotatable ([#1552](https://github.com/Syynth/brink/pull/1552))
- *(bevy-brink)* deferred/non-exclusive batch engine->ink call surface
- *(bevy-brink)* non-log surface for rejected with_config lint codes + plugin-level with_config tests
- *(diagnostics)* brink.toml [lints] control plane — per-code severity + deny-warnings ([#1160](https://github.com/Syynth/brink/pull/1160))
- *(bevy-brink)* brink.toml config discovery through the async AssetReader
- *(NS-A2)* effect-row extension — emits + tags + faults ([#1108](https://github.com/Syynth/brink/pull/1108))
- *(bevy-brink)* per-capability ECS change-tick wiring for wake conditions ([#996](https://github.com/Syynth/brink/pull/996))
- *(bevy-brink)* enforce wake-condition purity at policy attach ([#995](https://github.com/Syynth/brink/pull/995))
- *(bevy-brink)* FlowSleep + reactive-wake contract (closes #973)
- *(bevy-brink)* host-side ground-truth check for BH-1 capability access ([#938](https://github.com/Syynth/brink/pull/938))
- *(bevy-brink)* BH-3 parallel batch Step + Local-policy guard + determinism-law test
- *(bevy-brink)* batch-mode frame-start consistency core (BH-2, #914)
- *(bevy-brink)* scenario harness skeleton + SERIAL-driver baselines ([#900](https://github.com/Syynth/brink/pull/900))
- *(bevy-brink)* add dev-visibility-override plumbing to BrinkFlow
- *(t1c-3)* bevy-brink host callback-invocation surface for function values
- *(runtime)* state plumbing for collection values — trees, wasm JSON, bindings ([#525](https://github.com/Syynth/brink/pull/525))
- *(bevy-brink)* upgrade to Bevy 0.19 (official release)

### Fixed

- *(brink-runtime)* make pending_terminal invalidation unrepresentable ([#2104](https://github.com/Syynth/brink/pull/2104))
- *(brink-runtime,brink-web,bevy-brink)* address #2102 review findings for the Step/OutputLine migration
- *(bevy-brink)* repair tests that assumed terminals still carried text
- *(bevy-brink)* restore &keys/flow_context_view form in doctest example
- *(bevy-brink)* address #1645 review findings on brink_call_batch
- *(bevy-brink)* address #937 review findings — stale clone prose + unverified padding test
- clear clippy pedantic findings in the #937 view and bench axis
- *(bevy-brink)* correct stale serial-only Collect prose after #1633 fix
- *(bevy-brink)* advance_batch_parallel's Collect now consults FlowSleep
- *(bevy-brink)* parallel-driver same-tick missed wake + doc/ordering fixes (#1146 review)
- *(bevy-brink)* row-directed wake dirtying ([#1146](https://github.com/Syynth/brink/pull/1146)) — un-quarantine the #1101 flake
- *(brink-analyzer,bevy-brink)* correct review findings on #1620
- *(bevy-brink)* correct false query-binding claim + cover value-condition path (review, #1609)
- *(bevy-brink)* reject bind_brink_command bindings in FlowSleep wake-condition purity check
- *(bevy-brink)* address PR #1607 review findings on config-drop diagnostics
- *(bevy-brink)* address PR #1605 review findings on call_ink_function command bindings
- *(bevy-brink)* address PR #1604 review findings on compile_story_inline
- *(bevy-brink)* wire BrinkPlugin::with_config through compile_story_inline
- *(docs+tests)* scrub stale pre-#1530 E075 rationale flagged by review
- *(compiler)* admit struct construction literals as VAR/CONST defaults ([#1530](https://github.com/Syynth/brink/pull/1530))
- *(bevy-brink)* address #1437 review findings on brink.toml probe
- *(bevy-brink)* probe_brink_toml gathers every ancestor candidate so discover_from_entry_in_tree is the sole precedence decider ([#1406](https://github.com/Syynth/brink/pull/1406))
- *(bevy-brink)* address w52 review findings on #1430 with_config coverage
- *(bevy-brink)* address review findings on #1423 with_config lint-warning proof
- *(bevy-brink)* prove and document that with_config's invalid lint codes warn, not drop ([#1416](https://github.com/Syynth/brink/pull/1416))
- *(bevy-brink)* address review findings on #1394 lint-override PR
- *(bevy-brink)* forward [lints]/deny-warnings through the InkLoader override seam
- *(bevy-brink)* correct override-parity doc claim, add Load error guidance
- *(bevy-brink)* route compile_story_inline through the brink-environment producer
- *(bevy-brink)* correct probe_brink_toml doc + add multi-file INCLUDE regression test
- *(bevy-brink)* mark FlowSleep::condition_value #[reflect(ignore)]
- *(bevy-brink)* register ReflectComponent on FlowSleep for inspector visibility
- drop now-unused compile_test_story_brink import
- *(bevy-brink)* accept unregistered EXTERNALs in wake-condition purity check
- *(bevy-brink)* thread CapabilityManifest writes into wake-condition purity check
- *(bevy-brink)* purity tests avoid tripping the dev-feature replay reload; clippy
- *(bevy-brink)* gate the hot-reload/replay path with the per-marker capability check ([#997](https://github.com/Syynth/brink/pull/997))
- *(bevy-brink)* mark_wake_dirty must-polls capability-backed conditions
- *(bevy-brink)* DetectSummary::default() must be vacuously all-detect-capable
- *(bevy-brink)* AND/conservative detect-bit merge (closes #913)
- *(bevy-brink)* surface batch Step faults instead of laundering into stepped
- *(brink-runtime)* M-3 rehydration miss-path gaps — global-name rename + Value::List deep-rebind

### Other

- *(brink-compiler)* gate closure/path compile entry points behind test-util feature ([#2168](https://github.com/Syynth/brink/pull/2168))
- Merge remote-tracking branch 'origin/main' into train-fix
- promote 22 ignore'd doctests to no_run/text ([#1700](https://github.com/Syynth/brink/pull/1700))
- Merge remote-tracking branch 'origin/main' into train-fix
- *(bevy-brink)* retire the "per-flow frame-start clone" prose and record #937's measured effect
- *(bevy-brink)* add the brink-World-size axis (--story-globals) to the scenario harness
- *(bevy-brink)* borrow the frame-start world instead of cloning it per flow
- *(bevy-brink)* correct O(1) perf claim, stale struct doc, test docs post-#1439; add missing test
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into train-fix
- *(effects-spec)* record what a read row can and cannot say about a wake dependency ([#1146](https://github.com/Syynth/brink/pull/1146))
- Merge remote-tracking branch 'origin/main' into auto/issue-1436
- Merge remote-tracking branch 'origin/main' into train-fix
- *(bevy-brink)* reference filed follow-up issue #1609 in sleep.rs purity note
- Merge remote-tracking branch 'origin/main' into train-fix
- Merge remote-tracking branch 'origin/main' into auto/issue-1373
- *(bevy-brink,brink-cli)* document remaining [lints] scope gaps found in #1382 sweep
- *(bevy-brink)* migrate InkLoader onto the brink-environment producer ([#1360](https://github.com/Syynth/brink/pull/1360))
- *(bevy-brink)* split bindings.rs into registration/drive/tests modules
- Merge remote-tracking branch 'origin/main' into HEAD
- Merge remote-tracking branch 'origin/main' into auto/issue-1058
- add #[derive(Reflect)] to FlowSleep and related types (closes #998)
- Merge branch 'main' into auto/f34-f35-execmode
- bevy-brink profile-defaulted ExecMode + F34 changeset/docs
- NS-A9 review fixes: LSP premise test rewrite, invalid-types-as-unset, two bench mounts
- NS-A9 fallout: bevy-brink brink-dialect fixtures under the strict default
- Merge remote-tracking branch 'origin/main' into auto/issue-996
- origin/main into train-fix
- Merge origin/main into auto/issue-995
- Merge pull request #992 from Syynth/auto/issue-978
- Merge pull request #989 from Syynth/auto/issue-912
- *(bevy-brink)* fix pre-existing rustfmt drift in sleep/tests.rs
- *(bevy-brink)* give BH-3's determinism law its own pinned-seed CI lane
- *(bevy-brink)* canonical BH-3 parallel baselines (quiet-window)
- Merge pull request #933 from Syynth/auto/issue-923
- *(bevy-brink)* provisional BH-3 parallelism-curve data
- Merge remote-tracking branch 'origin/main' into train-pr
- *(host-capability-manifest)* fix ManifestParam example shape drift ([#924](https://github.com/Syynth/brink/pull/924))
- Merge remote-tracking branch 'origin/main' into train-pr
- Merge origin/main into train-fix for PR #920
- *(bevy-brink)* canonical serial baselines from a quiet-window solo run
- Merge branch 'main' into auto/issue-900
- path-projections tooling tail (docs/t1e-spec.md §8 item 3, #850)
- apply rustfmt after merge with main
- Merge remote-tracking branch 'origin/main' into train-pr
- sweep converter mentions after pipeline retirement ([#544](https://github.com/Syynth/brink/pull/544))

## [0.0.11](https://github.com/Syynth/brink/compare/bevy-brink-v0.0.10...bevy-brink-v0.0.11) - 2026-07-11

### Other

- Merge pull request #495 from Syynth/bronch/compiler-local-var-keyword-0fdbbc
- lint hygiene on flow_private_state example ([#473](https://github.com/Syynth/brink/pull/473))
- flow_private_state example — #@local end to end, zero host policy ([#473](https://github.com/Syynth/brink/pull/473))

## [0.0.10](https://github.com/Syynth/brink/compare/bevy-brink-v0.0.9...bevy-brink-v0.0.10) - 2026-07-10

### Added

- *(bevy-brink)* per-entity SaveState durability (F6.3, #441) ([#490](https://github.com/Syynth/brink/pull/490))
- *(bevy-brink)* thin onto scoped-flow-state core — shared World, per-flow FlowLocal (F6.2, #441) ([#488](https://github.com/Syynth/brink/pull/488))
- *(runtime)* WorldPolicy + ResolvedPolicy + resolution (F2.1) ([#446](https://github.com/Syynth/brink/pull/446))

### Other

- Merge origin/main into book-audit-updates
- re-export brink_runtime public-signature types + compile-check Bevy book examples ([#470](https://github.com/Syynth/brink/pull/470))
- *(runtime)* split Context into World + FlowLocal + routing view, all-World (F1.3) ([#445](https://github.com/Syynth/brink/pull/445))
- *(runtime)* Program → Arc, delete <'p> lifetime (F1.1) ([#442](https://github.com/Syynth/brink/pull/442))

## [0.0.7](https://github.com/Syynth/brink/compare/bevy-brink-v0.0.6...bevy-brink-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.5](https://github.com/Syynth/brink/compare/bevy-brink-v0.0.4...bevy-brink-v0.0.5) - 2026-06-17

### Added

- *(ide)* file rename/move core (#164 Stage 3, PR A) ([#252](https://github.com/Syynth/brink/pull/252))

## [0.0.4](https://github.com/Syynth/brink/compare/bevy-brink-v0.0.3...bevy-brink-v0.0.4) - 2026-06-15

### Added

- *(runtime,web)* host-directed parameterized knot entry ([#178](https://github.com/Syynth/brink/pull/178)) ([#195](https://github.com/Syynth/brink/pull/195))
- *(bevy-brink)* complete replay recording — non-exclusive path + async/task ([#173](https://github.com/Syynth/brink/pull/173)) ([#193](https://github.com/Syynth/brink/pull/193))
- *(bevy-brink)* Recorded-mode replay over the shared #189 primitive ([#173](https://github.com/Syynth/brink/pull/173)) ([#192](https://github.com/Syynth/brink/pull/192))

## [0.0.3](https://github.com/Syynth/brink/compare/bevy-brink-v0.0.2...bevy-brink-v0.0.3) - 2026-06-13

### Added

- *(brink-web)* external-function binding foundation (Track A) ([#73](https://github.com/Syynth/brink/pull/73))
