# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.10](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.9...brink-compiler-v0.0.10) - 2026-07-10

### Other

- Story::new takes Arc<Program>, not &Program
- *(runtime)* Program → Arc, delete <'p> lifetime (F1.1) ([#442](https://github.com/Syynth/brink/pull/442))

## [0.0.7](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.6...brink-compiler-v0.0.7) - 2026-06-20

### Other

- welcoming README (playground/book links, overview, LLM-experiment disclaimer) ([#285](https://github.com/Syynth/brink/pull/285))

## [0.0.4](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.3...brink-compiler-v0.0.4) - 2026-06-15

### Fixed

- *(#187)* tunnel calls aren't terminal (E033) + resolve diagnostics to paths (#190)

## [0.0.3](https://github.com/Syynth/brink/compare/brink-compiler-v0.0.2...brink-compiler-v0.0.3) - 2026-06-13

### Added

- host capability manifest — Tier 1 + closed Tier 2 (Track B MVP) ([#74](https://github.com/Syynth/brink/pull/74))

### Fixed

- *(compiler)* surface syntax errors + reject malformed inline conditionals (closes #44) ([#48](https://github.com/Syynth/brink/pull/48))
