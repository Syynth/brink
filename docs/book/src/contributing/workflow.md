# Development Workflow

## Building

```sh
cargo check --workspace                                # type-check
cargo build --workspace                                # full build
```

## Testing

```sh
cargo test --workspace                                 # run all tests
```

### Episode corpus

The episode corpus is the primary correctness tool. It compares the native compiler's output against the converter reference.

```sh
# Corpus report -- per-category pass/fail breakdown
cargo test -p brink-test-harness --test corpus_report -- --nocapture

# Run all episodes
cargo test -p brink-test-harness --test brink_native_episodes -- --nocapture

# Single test case (filter by name substring)
BRINK_CASE=I002 cargo test -p brink-test-harness --test brink_native_episodes -- --nocapture
```

The harness prints three things for each failure: the `.ink` source, the compiler's `.inkt` dump, and the converter's `.inkt` dump. This enables side-by-side comparison to root-cause divergences.

## Linting

```sh
cargo clippy --workspace --all-targets -- -D warnings  # lint
cargo fmt --all -- --check                             # format check
cargo fmt --all                                        # format fix
```

## Lint policy

- `unsafe_code`, `unwrap_used`, `expect_used`, `panic`, `todo`, `print_stdout`, `print_stderr` are **denied** in library crates
- Clippy pedantic is enabled (with targeted allows for noise)
- Tests are exempt from unwrap/expect/dbg/print restrictions (via `clippy.toml`)

## Determinism

Never iterate `HashMap` keys/values where order affects output. Sort or use `BTreeMap`. This applies to all output-producing code paths -- bytecode emission, line table construction, name table serialization, and test output.
