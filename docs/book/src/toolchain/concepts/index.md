# Concepts

These pages explain how brink works — the mental model behind the API, not a
how-to. Read them once and the [guides](../embedding/index.md) and
[reference](../reference/index.md) will make more sense; skip them and you can
still get a story running, you just won't know *why*.

- **[The Two Pipelines](./two-pipelines.md)** — why there's a native compiler
  *and* a converter, and which one you want.
- **[The Execution Model](./execution-model.md)** — how a compiled story runs:
  the `Program`/`Story` split, the step loop, `Line`, and choices. The shared
  foundation every client (raw Rust, Bevy, web) builds on.
- **[The State Model](./state-model.md)** — where a running story's state lives:
  the `World`/`FlowLocal` split, per-unit world/local scoping, and the sandbox
  primitive behind speculative evaluation.
- **[Architecture & the Firewall](./architecture.md)** — how the crates are
  split so the runtime never links the compiler.
- **[The Compilation Pipeline](./pipeline.md)** — the six phases that turn
  `.ink` source into bytecode.
