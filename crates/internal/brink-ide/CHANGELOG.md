# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.9](https://github.com/Syynth/brink/compare/brink-ide-v0.0.7...brink-ide-v0.0.9) - 2026-07-06

### Added

- *(ide,editor,web)* fold kinds — structural/machinery/narrative + summary pills ([#365](https://github.com/Syynth/brink/pull/365)) ([#400](https://github.com/Syynth/brink/pull/400))
- *(ir,ide,web)* dialogue-dialect schema + Rust classification ([#368](https://github.com/Syynth/brink/pull/368)) ([#386](https://github.com/Syynth/brink/pull/386))
- *(ide,web)* story-graph edges carry source-span occurrences ([#371](https://github.com/Syynth/brink/pull/371)) ([#378](https://github.com/Syynth/brink/pull/378))
- *(ide,web)* extract selection to knot/function ops (#315 H) ([#341](https://github.com/Syynth/brink/pull/341))
- *(ide,web)* atomic reference-aware rename_dir ([#314](https://github.com/Syynth/brink/pull/314)) ([#342](https://github.com/Syynth/brink/pull/342))
- *(ide,web)* unified StructuralResult + deleteSymbol + op-wide breakage gate ([#316](https://github.com/Syynth/brink/pull/316)) ([#336](https://github.com/Syynth/brink/pull/336))
- *(brink-web)* wasm resolve_code_action op with self-describing action data ([#321](https://github.com/Syynth/brink/pull/321)) ([#328](https://github.com/Syynth/brink/pull/328))
- *(studio)* knot/stitch Rename — safe-by-default + breakage report ([#305](https://github.com/Syynth/brink/pull/305)) ([#306](https://github.com/Syynth/brink/pull/306))

### Fixed

- *(release)* path-only dev-deps in brink-ide — unblock stuck 0.0.8 publish ([#419](https://github.com/Syynth/brink/pull/419))
- *(ide,editor)* sigil-wins-chain + conditional scaffold classification ([#413](https://github.com/Syynth/brink/pull/413)) ([#425](https://github.com/Syynth/brink/pull/425))

### Other

- release v0.0.8 ([#307](https://github.com/Syynth/brink/pull/307))
- #312 + #313 (Track N core): shared INCLUDE-block detector + fold/auto-import cores ([#331](https://github.com/Syynth/brink/pull/331))
- *(brink-ide,brink-db)* regression coverage for shallower file-move outbound INCLUDE rewrite ([#325](https://github.com/Syynth/brink/pull/325))

## [0.0.8](https://github.com/Syynth/brink/compare/brink-ide-v0.0.7...brink-ide-v0.0.8) - 2026-07-01

### Added

- *(ide,web)* extract selection to knot/function ops (#315 H) ([#341](https://github.com/Syynth/brink/pull/341))
- *(ide,web)* atomic reference-aware rename_dir ([#314](https://github.com/Syynth/brink/pull/314)) ([#342](https://github.com/Syynth/brink/pull/342))
- *(ide,web)* unified StructuralResult + deleteSymbol + op-wide breakage gate ([#316](https://github.com/Syynth/brink/pull/316)) ([#336](https://github.com/Syynth/brink/pull/336))
- *(brink-web)* wasm resolve_code_action op with self-describing action data ([#321](https://github.com/Syynth/brink/pull/321)) ([#328](https://github.com/Syynth/brink/pull/328))
- *(studio)* knot/stitch Rename — safe-by-default + breakage report ([#305](https://github.com/Syynth/brink/pull/305)) ([#306](https://github.com/Syynth/brink/pull/306))

### Other

- #312 + #313 (Track N core): shared INCLUDE-block detector + fold/auto-import cores ([#331](https://github.com/Syynth/brink/pull/331))
- *(brink-ide,brink-db)* regression coverage for shallower file-move outbound INCLUDE rewrite ([#325](https://github.com/Syynth/brink/pull/325))

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
