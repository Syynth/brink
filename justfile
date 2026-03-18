# Setup git hooks
setup:
    git config core.hooksPath .githooks

# Type-check the workspace
check:
    cargo check --workspace

# Run all tests
test:
    cargo test --workspace

# Run all lints (fmt check + clippy)
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Auto-fix what can be fixed
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty

# Run internal divan benchmarks
bench:
    cargo bench -p brink-runtime

# Run cross-implementation benchmark comparison
cross-language-benchmark:
    #!/usr/bin/env bash
    set -euo pipefail
    bash benchmarks/setup.sh
    bash benchmarks/run.sh

# Build brink-web wasm package
wasm:
    wasm-pack build crates/brink-web --target web --out-dir www/pkg

# Run brink-studio dev server (builds wasm first)
studio-dev: wasm
    cd packages/brink-studio && pnpm dev

# Build brink-studio for production (builds wasm first)
studio-build: wasm
    cd packages/brink-studio && pnpm build

# Duration per fuzz target in seconds (default: 5 minutes)
fuzz_duration := "300"

# Run all 8 fuzz targets in parallel (~5 minutes at default duration)
fuzz:
    #!/usr/bin/env bash
    set -euo pipefail
    pids=()
    targets=(
        "brink-syntax:parse_no_panic"
        "brink-syntax:parse_lossless"
        "brink-format:read_no_panic"
        "brink-format:read_index_no_panic"
        "brink-format:write_read_roundtrip"
        "brink-format:section_reads_no_panic"
        "brink-format:read_inkt_no_panic"
        "brink-format:inkt_write_read_roundtrip"
    )
    for entry in "${targets[@]}"; do
        crate="${entry%%:*}"
        target="${entry##*:}"
        echo "Starting fuzz: $crate/$target ({{fuzz_duration}}s)"
        (cd "crates/internal/$crate/fuzz" && cargo +nightly fuzz run "$target" -- -timeout=1 -max_total_time={{fuzz_duration}}) &
        pids+=($!)
    done
    failed=0
    for i in "${!pids[@]}"; do
        if ! wait "${pids[$i]}"; then
            echo "FAILED: ${targets[$i]}"
            failed=1
        fi
    done
    if [ "$failed" -ne 0 ]; then exit 1; fi
    echo "All fuzz targets passed."

# Run all 8 fuzz targets sequentially (30s each, stops on first failure)
fuzz-serial:
    cd crates/internal/brink-syntax/fuzz && cargo +nightly fuzz run parse_no_panic -- -timeout=1 -max_total_time=30
    cd crates/internal/brink-syntax/fuzz && cargo +nightly fuzz run parse_lossless -- -timeout=1 -max_total_time=30
    cd crates/internal/brink-format/fuzz && cargo +nightly fuzz run read_no_panic -- -timeout=1 -max_total_time=30
    cd crates/internal/brink-format/fuzz && cargo +nightly fuzz run read_index_no_panic -- -timeout=1 -max_total_time=30
    cd crates/internal/brink-format/fuzz && cargo +nightly fuzz run write_read_roundtrip -- -timeout=1 -max_total_time=30
    cd crates/internal/brink-format/fuzz && cargo +nightly fuzz run section_reads_no_panic -- -timeout=1 -max_total_time=30
    cd crates/internal/brink-format/fuzz && cargo +nightly fuzz run read_inkt_no_panic -- -timeout=1 -max_total_time=30
    cd crates/internal/brink-format/fuzz && cargo +nightly fuzz run inkt_write_read_roundtrip -- -timeout=1 -max_total_time=30
