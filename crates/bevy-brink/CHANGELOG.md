# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
