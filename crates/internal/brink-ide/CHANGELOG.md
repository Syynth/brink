# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.7](https://github.com/Syynth/brink/compare/brink-ide-v0.0.6...brink-ide-v0.0.7) - 2026-06-20

### Added

- *(cli)* brink ide move-file / refactor * / actions ([#293](https://github.com/Syynth/brink/pull/293)) ([#300](https://github.com/Syynth/brink/pull/300))

### Fixed

- *(brink-ide)* fold same-file ref edits into promote/demote/move new_source ([#302](https://github.com/Syynth/brink/pull/302))

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.6](https://github.com/Syynth/brink/compare/brink-ide-v0.0.5...brink-ide-v0.0.6) - 2026-06-19

### Added

- *(studio)* host functions panel categories + search ([#210](https://github.com/Syynth/brink/pull/210)) ([#270](https://github.com/Syynth/brink/pull/270))

### Fixed

- *(ide)* don't highlight prose words that match ink keywords ([#275](https://github.com/Syynth/brink/pull/275)) ([#277](https://github.com/Syynth/brink/pull/277))

## [0.0.5](https://github.com/Syynth/brink/compare/brink-ide-v0.0.4...brink-ide-v0.0.5) - 2026-06-17

### Added

- *(ide)* file rename/move core (#164 Stage 3, PR A) ([#252](https://github.com/Syynth/brink/pull/252))
- *(brink-ide,web)* host-sourced value-lists in the call Form ([#237](https://github.com/Syynth/brink/pull/237))
- *(brink-ide,studio)* drive the call Form from signature metadata ([#233](https://github.com/Syynth/brink/pull/233))
- *(studio)* typed argument widgets in the call Form + live inter-arg context ([#223](https://github.com/Syynth/brink/pull/223))
- *(studio)* argument widgets stage 5 — arg-groups + inter-arg context + modal ([#222](https://github.com/Syynth/brink/pull/222))
- *(studio)* argument widgets stage 3 — the call Form + launchers ([#220](https://github.com/Syynth/brink/pull/220))
- *(studio)* argument widgets stage 2 — argument_widgets query + Fill ([#219](https://github.com/Syynth/brink/pull/219))
- *(studio)* argument widgets stage 1 — registry + light color picker ([#218](https://github.com/Syynth/brink/pull/218))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-ide-v0.0.3...brink-ide-v0.0.4) - 2026-06-15

### Added

- *(ide,web)* host value push-cache transport for the argument picker ([#174](https://github.com/Syynth/brink/pull/174)) ([#205](https://github.com/Syynth/brink/pull/205))
- *(ide,studio)* arg-position value completion dropdown ([#175](https://github.com/Syynth/brink/pull/175)) ([#204](https://github.com/Syynth/brink/pull/204))
- *(manifest,ide)* static value-source + value-label inlay hints ([#174](https://github.com/Syynth/brink/pull/174)) ([#203](https://github.com/Syynth/brink/pull/203))

## [0.0.3](https://github.com/Syynth/brink/compare/brink-ide-v0.0.2...brink-ide-v0.0.3) - 2026-06-13

### Added

- *(ide,web)* story-graph extraction query, wasm-exposed ([#96](https://github.com/Syynth/brink/pull/96)) ([#139](https://github.com/Syynth/brink/pull/139))
- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))

### Fixed

- *(web)* attribute compile diagnostics to their own file (closes #43) ([#49](https://github.com/Syynth/brink/pull/49))

### Other

- Studio IDE: doc comments + type hints for all declarations (Track B integration) ([#101](https://github.com/Syynth/brink/pull/101))
