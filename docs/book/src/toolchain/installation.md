# Installation

## CLI

Install the `brink` binary from crates.io:

```sh
cargo install brink-cli
```

The crate is named `brink-cli`; the command it installs is `brink`.

```sh
brink --help
```

Prebuilt binaries are not published yet. The release pipeline (cargo-dist,
targeting macOS on Apple Silicon and Intel, Linux x86-64, and Windows x86-64) is
configured but runs only on a manual dispatch, so for now every install builds
from source.

To track the development branch instead of a release:

```sh
cargo install --git https://github.com/Syynth/brink brink-cli
```

## Rust library

Add the runtime to your project:

```toml
[dependencies]
brink-runtime = "0.0.9"
```

`brink-runtime` is the primary library interface. It depends only on
`brink-format` — the binary interface between the two halves of the toolchain —
and pulls in no compiler code, which is what keeps embedded builds small. See
[Architecture & the Firewall](./concepts/architecture.md).

If you also need to compile `.ink` source (at build time, or at runtime for a
live-reloading editor), add the compiler alongside it:

```toml
[dependencies]
brink-compiler = "0.0.9"
brink-runtime = "0.0.9"
```

> The `brink` crate on crates.io is a name reservation and ships no code. Depend
> on `brink-runtime` and `brink-compiler` directly.

For the Bevy integration, see [Bevy](../integrations/bevy/index.md):

```toml
[dependencies]
bevy-brink = "0.0.9"
```

## JavaScript packages

The browser toolchain is published to npm. See
[Web & WASM](../integrations/web/index.md) for what each one exposes.

```sh
npm install @brink-lang/web       # compiler + runtime + IDE queries, via WASM
npm install @brink-lang/editor    # the CodeMirror 6 ink editor
```

`@brink-lang/studio` is the reference authoring app rather than a library — see
[Studio](../integrations/studio/index.md) for how to run it.
