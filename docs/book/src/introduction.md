# Introduction

brink is a compiler and runtime for [inkle's ink](https://github.com/inkle/ink) narrative scripting language, written in Rust.

It compiles `.ink` source files to a compact bytecode format and executes them in a stack-based VM. You can use brink as a CLI tool to compile and play stories, or embed the runtime as a Rust library in games and applications.

## Features

- Full ink language support: choices, gathers, weave, variables, lists, sequences, tunnels, threads, external functions
- Bytecode compiler with multi-file support (`INCLUDE` resolution)
- Stack-based VM with multi-instance execution (one compiled program, many story instances)
- Localization-ready format with line templates, interpolation slots, and plural categories
- Language server (LSP) for editor integration
- No unsafe code, no panics -- strict lint policy

## How it works

brink has two pipelines:

1. **Native compiler**: `.ink` source -> parse -> HIR -> analyze -> LIR -> bytecode codegen -> `StoryData`
2. **Converter** (reference): `.ink.json` (inklecate output) -> convert -> `StoryData`

The converter pipeline processes output from inkle's reference C# compiler and serves as the known-good reference implementation. The native compiler is under active development and is validated against the converter's output using an episode-based test corpus.

Both pipelines produce the same `StoryData` structure, which is linked and executed by `brink-runtime`.

## Learning ink

brink implements the ink language as designed by inkle. To learn the ink language itself, see the [Writing with Ink](https://github.com/inkle/ink/blob/master/Documentation/WritingWithInk.md) documentation.

## Current status

The compiler is under active development. The episode-based test corpus tracks behavioral conformance between the native compiler and the converter reference. Not all ink features are fully supported by the native compiler yet -- the converter pipeline is available for production use in the meantime.
