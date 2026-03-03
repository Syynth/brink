# Development Workflow

## Building

```sh
cargo check --workspace                                # type-check
cargo build --workspace                                # full build
```

## Testing

```sh
cargo test --workspace                                 # run all tests
cargo test -p brink-runtime corpus_tier1 -- --nocapture  # runtime corpus tests
```

## Linting

```sh
cargo clippy --workspace --all-targets -- -D warnings  # lint
cargo fmt --all -- --check                             # format check
cargo fmt --all                                        # format fix
```

## Lint policy

- `unsafe_code`, `unwrap_used`, `expect_used`, `panic`, `todo`, `print_stdout`, `print_stderr` are **denied**
- Clippy pedantic is on (with a few allows for noise)
- Tests are exempt from unwrap/expect/dbg/print restrictions (via `clippy.toml`)

## Implementation policy

Spike implementations are v0 — they exist to validate crate interfaces, not to be final code. When working on a crate, prefer rewriting over patching if the existing implementation doesn't match the target design. Tests and public API signatures are the stable artifacts; everything behind them is disposable.

<!-- TODO: expand on contribution guidelines once the project matures:
  - PR process
  - Commit message conventions
  - How to add a new opcode
  - How to add a new test case
-->
