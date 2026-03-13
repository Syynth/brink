# Installation

## CLI

Install from source using Cargo:

```sh
cargo install --git https://github.com/user/brink brink-cli
```

This builds and installs the `brink` binary. No prebuilt binaries are available yet.

## Library

Add `brink-runtime` to your project. Since brink is not yet published to crates.io, use a git dependency:

```toml
[dependencies]
brink-runtime = { git = "https://github.com/user/brink" }
```

If you also need the compiler (to compile `.ink` source at build time or runtime):

```toml
[dependencies]
brink-compiler = { git = "https://github.com/user/brink" }
brink-runtime = { git = "https://github.com/user/brink" }
```

The `brink-runtime` crate is the primary library interface. It depends only on `brink-format` (the binary interface) and has no compiler dependencies.
