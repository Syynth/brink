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

# Compile-time benchmark baseline (#498): cold corpus compile, studio-scale
# synthetic project (cold + warm one-line-edit recompile), per-stage breakdown.
# Extra args pass through, e.g. `just bench-compile --runs 9`.
bench-compile *ARGS:
    cargo run --release -p brink-test-harness --bin compile_bench -- {{ARGS}}

# Editor-session memory profiling harness (#529): drives a long, deterministic
# editor-style session through a persistent ProjectDb and reports salsa
# memo-table/heap growth per query kind. Measurement only — does not set any
# salsa LRU capacities (that's a data-driven follow-up ruling, spec §8).
# Extra args pass through, e.g. `just bench-editor-session --edits 2000`.
bench-editor-session *ARGS:
    cargo run --release -p brink-test-harness --bin editor_session_bench -- {{ARGS}}

# Run cross-implementation benchmark comparison
cross-language-benchmark:
    #!/usr/bin/env bash
    set -euo pipefail
    bash benchmarks/setup.sh
    bash benchmarks/run.sh

# Build brink-web wasm package
wasm:
    wasm-pack build crates/brink-web --target web --out-dir www/pkg

# Compile-check the book's Rust examples. Two mechanisms, by chapter:
#
#  - The Bevy "flows" and "saves" pages `{{#include}}` their snippets from
#    compiled examples (crates/bevy-brink/examples/book_flows.rs,
#    book_saves.rs); `cargo build --example` is what checks that code, so
#    those fences are `rust,ignore` for mdbook test.
#  - Every other Rust example is a real doctest, run by `mdbook test`.
#
# mdbook test builds into a dedicated target dir. rustdoc resolves the examples'
# `extern crate` declarations off the -L search path, and that path must hold
# exactly one build of each crate: two artifacts with different hashes (a
# `cargo check` .rmeta beside a `cargo build` .rlib, or two different feature
# unifications) make rustdoc bail with E0464 "multiple candidates". So: always
# build the whole package set in ONE cargo invocation, and wipe the dir if a
# stale artifact from some other invocation snuck in.
book-test:
    #!/usr/bin/env bash
    set -euo pipefail

    # The Bevy flows/saves pages are checked by building their backing examples.
    cargo build -p bevy-brink --example book_flows
    cargo build -p bevy-brink --example book_saves

    export CARGO_TARGET_DIR=target/book-doctest
    deps="$CARGO_TARGET_DIR/debug/deps"
    pkgs="-p brink-runtime -p brink-compiler -p brink-format -p brink-intl -p bevy-brink -p brink-project-config"

    # Self-heal: if any crate has more than one hash, the dir is polluted.
    if [ -d "$deps" ]; then
        for c in brink_format brink_runtime brink_compiler brink_intl bevy_brink brink_project_config; do
            n=$(ls "$deps" 2>/dev/null | sed -nE "s/^lib${c}-([0-9a-f]+)\.(rlib|rmeta)$/\1/p" | sort -u | wc -l | tr -d ' ')
            if [ "$n" -gt 1 ]; then
                echo "book-test: stale artifacts for ${c} (${n} hashes) — rebuilding clean"
                rm -rf "$CARGO_TARGET_DIR"
                break
            fi
        done
    fi

    cargo build $pkgs

    # Bevy's derives (Component/Event/Resource) resolve the `bevy` vs `bevy_ecs`
    # crate path by reading CARGO_MANIFEST_DIR's Cargo.toml. rustdoc runs outside
    # a cargo context, so without this they panic with "CARGO_MANIFEST_DIR is not
    # defined". Point them at bevy-brink, which depends on the bevy_* subcrates.
    export CARGO_MANIFEST_DIR="$PWD/crates/bevy-brink"

    # Run mdbook under the pinned toolchain so its rustdoc matches the rustc
    # that built the deps above — otherwise a differing default toolchain makes
    # rustdoc reject them (E0514).
    channel=$(grep -oE 'channel = "[^"]+"' rust-toolchain.toml | cut -d'"' -f2)
    rustup run "$channel" mdbook test docs/book -L "$deps"

# Type-check the book's TypeScript examples against the published @brink-lang
# packages. The TS counterpart of `book-test`: mdbook only runs Rust doctests,
# so this extracts the `ts`/`typescript` blocks and runs `tsc` over them. See
# docs/book/ts-check/README.md.
book-ts-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cd docs/book/ts-check
    npm install --no-audit --no-fund --silent
    npm run check

# Build the full brink-studio as a standalone static app and stage it into
# docs/book/src/playground/ (the embedded book playground).
book-assets:
    #!/usr/bin/env bash
    set -euo pipefail
    dest="docs/book/src/playground"
    # The studio's Vite build resolves `brink-web` against this wasm pkg, so it
    # must exist first (out-dir is relative to the crate -> crates/brink-web/www/pkg).
    wasm-pack build crates/brink-web --target web --out-dir www/pkg
    # Install JS deps and build the studio as a self-contained static bundle.
    pnpm install:checked -- --frozen-lockfile
    pnpm --filter @brink-lang/studio build:embed
    # Stage the static build (index.html + assets/, wasm bundled) into the book.
    rm -rf "$dest"
    mkdir -p "$dest"
    cp -R packages/brink-studio/dist-embed/. "$dest/"

# Build the book with the embedded playground
book: book-assets
    mdbook build docs/book

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
