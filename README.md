# brink

**A Rust toolchain — compiler, runtime, and authoring studio — for [inkle's ink](https://github.com/inkle/ink) narrative scripting language.**

[![Crates.io](https://img.shields.io/crates/v/brink-runtime)](https://crates.io/crates/brink-runtime)
[![docs.rs](https://img.shields.io/docsrs/brink-runtime)](https://docs.rs/brink-runtime)
[![CI](https://github.com/syynth/brink/actions/workflows/ci.yml/badge.svg)](https://github.com/syynth/brink/actions/workflows/ci.yml)
[![Book](https://img.shields.io/badge/docs-book-blue)](https://syynth.github.io/brink/)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](./LICENSE)

brink compiles `.ink` stories to a compact bytecode and runs them in a small stack-based VM — usable as a **command-line tool**, an **embeddable Rust library**, a **Bevy plugin**, or a **WebAssembly module** behind a full browser-based authoring studio.

> ### ⚠️ An experiment in building software with LLMs
>
> **brink was built almost entirely by large language models.** It's the author's experiment in whether a large, real-world software project — a complete language toolchain, runtime, and IDE — can be designed and implemented primarily through LLM agents.
>
> Treat it as a **research artifact**: broad in scope, evolving fast, and *not yet production-hardened*. Much of the code, tests, and even these docs were written by AI under human direction. It's genuinely useful and extensively validated against a reference implementation (see [Status](#status)), but expect rough edges and breaking changes while it matures.

---

## Try it now — no install

- 🎮 **[Playground →](https://syynth.github.io/brink/integrations/web/playground.html)** — the full brink Studio running live in your browser. Pick a demo, edit ink on the left, play it on the right.
- 📖 **[Documentation & Book →](https://syynth.github.io/brink/)** — guides, concepts, and reference, from "your first story" to the bytecode format.

## What is ink, and what is brink?

[ink](https://github.com/inkle/ink) is inkle's scripting language for interactive narrative — branching choices, weaves, variables, threads, and more (it powers games like *Heaven's Vault* and *Sorcery!*). To learn the *language*, read inkle's [Writing with Ink](https://github.com/inkle/ink/blob/master/Documentation/WritingWithInk.md).

**brink** is an independent, from-scratch implementation of that language in Rust: a compiler, a runtime VM, and the tooling around them. It aims for behavioral parity with inkle's reference runtime while being fast, embeddable, memory-safe (no `unsafe`, no panics), and localization-ready.

## The pieces

brink is one workspace spanning a Rust toolchain and a TypeScript/React studio:

- **The CLI (`brink`)** — `compile` an `.ink` story to bytecode, `play` it in the terminal, or `convert` existing inklecate `.ink.json` output. Your entry point for everyday use.
- **The compiler** — a real pipeline: `.ink` source → parse → HIR → semantic analysis → LIR → bytecode codegen → a compact binary `StoryData` format. Multi-file (`INCLUDE`) aware.
- **The runtime (`brink-runtime`)** — a stack-based bytecode VM. One compiled program runs many independent story instances; output is a simple `Line` stream (text, choices, done, end). Embed it in any Rust program.
- **The converter** — a parallel, known-good pipeline that ingests inklecate's `.ink.json` and produces the same `StoryData`. It's the reference the native compiler is validated against, and a fallback for stories you already have as JSON.
- **Localization** — a translation-ready format with line templates, interpolation slots, and plural categories, plus an XLIFF round-trip workflow (`.ink` → compile → export → translate → relink).
- **[`bevy-brink`](https://syynth.github.io/brink/integrations/bevy/index.html)** — a [Bevy](https://bevyengine.org/) plugin: stories as assets, per-flow components, observer events, and a full external-function binding facility (ink ↔ engine).
- **Web & WASM (`@brink-lang/web`)** — the compiler + runtime compiled to WebAssembly, with an editor/LSP-style API for the browser.
- **[Studio](https://syynth.github.io/brink/integrations/studio/index.html) (`@brink-lang/studio`)** — a full browser-based authoring IDE: file binder, screenplay-style editor with live IDE intelligence, and an embedded player. It *is* the playground linked above.

## Quick start

### Command line

```sh
cargo install brink-cli                 # installs the `brink` command
brink compile story.ink -o story.inkb   # compile to bytecode
brink play story.ink                    # compile and play in the terminal
```

### Embed the runtime in Rust

```rust
// Load compiled StoryData, link it into a Program, and create a Story.
let story_data = brink_format::read_inkb(&bytes)?;
let (program, line_tables) = brink_runtime::link(&story_data)?;
let mut story = brink_runtime::Story::new(&program, line_tables);

use brink_runtime::Line;
loop {
    match story.continue_single()? {
        Line::Text { text, .. } => print!("{text}"),
        Line::Done { text, .. } => { print!("{text}"); break; }
        Line::Choices { text, choices, .. } => {
            print!("{text}");
            story.choose(pick_a_choice(&choices))?; // your choice-selection logic
        }
        Line::End { text, .. } => { print!("{text}"); break; }
    }
}
```

One `Program` can back many independent `Story` instances. See [Embedding the Runtime](https://syynth.github.io/brink/toolchain/embedding/index.html) for the full API.

### Web / npm

```sh
npm install @brink-lang/web      # compiler + runtime, compiled to WASM
npm install @brink-lang/studio   # the embeddable authoring studio
```

## Workspace layout

| Crate / package | Purpose |
|-----------------|---------|
| `brink-cli` | The CLI (installs the `brink` command) |
| `brink-runtime` | Stack-based bytecode VM |
| `brink-compiler` | Compiler pipeline driver |
| `brink-converter` | `.ink.json` → `StoryData` reference pipeline |
| `brink-format` | Binary interface between compiler and runtime |
| `bevy-brink` | Bevy integration: plugin, assets, external-function bindings |
| `@brink-lang/web` | WASM bindings (compiler + runtime + editor API) |
| `@brink-lang/studio` | Browser-based authoring IDE / playground |

Internal crates (`brink-syntax`, `brink-ir`, `brink-analyzer`, `brink-codegen-inkb`, `brink-ide`, …) implement the pipeline stages. The full map is in [Crate Layout](https://syynth.github.io/brink/contributing/crate-layout.html).

## Status

brink is under **active development**. Correctness is measured against a corpus of golden episodes generated by inkle's C# ink runtime (the "oracle"): a single compiled story is run through thousands of choice sequences and compared turn-by-turn against the reference.

Because the native compiler is still closing the last gap to full parity, there are **two ways to get a runnable story**:

- the **native compiler** (the normal path — reads `.ink`), validated against the converter; and
- the **converter** (reads inklecate's `.ink.json`), the known-good reference.

See [The Two Pipelines](https://syynth.github.io/brink/toolchain/concepts/two-pipelines.html) for which to use today.

## Contributing

Contributions and bug reports are welcome — keep the experimental nature in mind. Start with the [Development Workflow](https://syynth.github.io/brink/contributing/workflow.html) and [Test Corpus](https://syynth.github.io/brink/contributing/test-corpus.html) chapters.

```sh
cargo test --workspace                                   # Rust tests
cargo clippy --workspace --all-targets -- -D warnings    # lint (strict)
cargo fmt --all -- --check                               # format check
```

The Rust toolchain has no `unsafe`, and `unwrap`/`expect`/`panic`/`todo` are denied outside tests.

## License

[MIT](./LICENSE).

ink is a trademark of inkle Ltd. brink is an independent implementation and is not affiliated with or endorsed by inkle.
