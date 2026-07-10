# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
