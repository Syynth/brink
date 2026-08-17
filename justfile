# Every network-touching command in the recipes below is bounded by
# run_with_timeout, sourced from scripts/lib/run-with-timeout.sh — the same
# wedged-proxy hang class #2591/#2638/#2642 bounded in setup-dev.sh and #2667
# bounded in refresh-excluded-lockfiles.sh. These recipes were left BARE until
# #2677 because scripts/check-scripts.mjs only ever discovered
# scripts/**/*.sh: a justfile recipe body was a whole developer-facing surface
# outside the scan, and `just book-assets` is additionally a CI lane
# (.github/workflows/book.yml) that no workflow-YAML parser can see into.
#
#   Knob                                     Default  On timeout
#   ---------------------------------------------------------------------
#   BRINK_JUST_WASM_TIMEOUT                     900s   FAIL (exit 1). Covers a
#                                                      COLD release build of
#                                                      the brink-web crate tree
#                                                      plus, on a cache miss,
#                                                      the binaryen/wasm-opt
#                                                      tarball wasm-pack pulls
#                                                      from GitHub releases.
#   BRINK_JUST_NPM_INSTALL_TIMEOUT              600s   FAIL (exit 1). The
#                                                      book's ts-check project
#                                                      installs the PUBLISHED
#                                                      @brink-lang packages
#                                                      from the npm registry;
#                                                      it is a small tree, so
#                                                      600s is already generous
#                                                      for a cold fetch.
#   BRINK_JUST_NPM_CHECK_TIMEOUT                600s   FAIL (exit 1). `npm run
#                                                      check` is a local tsc
#                                                      run; it is bounded only
#                                                      because npm resolves
#                                                      through corepack's shim
#                                                      and so can itself fetch
#                                                      on a cache miss (#2642).
#   BRINK_JUST_PNPM_INSTALL_TIMEOUT             900s   FAIL (exit 1). The
#                                                      guarded workspace
#                                                      install; a truncated one
#                                                      is the half-written
#                                                      node_modules of
#                                                      #2479/#2593.
#   BRINK_JUST_STUDIO_BUILD_TIMEOUT             900s   FAIL (exit 1). The
#                                                      studio's Vite build,
#                                                      bounded for the same
#                                                      corepack-shim reason as
#                                                      the npm check above.
#
# EVERY row says FAIL, and none of them may warn-and-continue: unlike
# setup-dev.sh — where the prebuilt-tarball fetches WARN because a from-source
# build is a real fallback — each command here IS the recipe's entire output.
# A timeout that exited 0 would leave `just wasm` reporting success with a
# stale or missing pkg, which is the silent-stale-instrument failure
# packages/brink-desktop/scripts/ensure-wasm.mjs exists to prevent.

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
    #!/usr/bin/env bash
    set -euo pipefail
    . scripts/lib/run-with-timeout.sh
    BRINK_JUST_WASM_TIMEOUT="${BRINK_JUST_WASM_TIMEOUT:-900}"
    rc=0
    run_with_timeout "${BRINK_JUST_WASM_TIMEOUT}" wasm-pack build crates/brink-web --target web --out-dir www/pkg || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x wasm-pack build TIMED OUT after ${BRINK_JUST_WASM_TIMEOUT}s — the binaryen/wasm-opt download or the crates.io fetch behind it never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_JUST_WASM_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"

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
    . scripts/lib/run-with-timeout.sh
    BRINK_JUST_NPM_INSTALL_TIMEOUT="${BRINK_JUST_NPM_INSTALL_TIMEOUT:-600}"
    BRINK_JUST_NPM_CHECK_TIMEOUT="${BRINK_JUST_NPM_CHECK_TIMEOUT:-600}"
    cd docs/book/ts-check
    rc=0
    run_with_timeout "${BRINK_JUST_NPM_INSTALL_TIMEOUT}" npm install --no-audit --no-fund --silent || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x npm install TIMED OUT after ${BRINK_JUST_NPM_INSTALL_TIMEOUT}s in docs/book/ts-check — the npm-registry fetch of the published @brink-lang packages never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_JUST_NPM_INSTALL_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
    rc=0
    run_with_timeout "${BRINK_JUST_NPM_CHECK_TIMEOUT}" npm run check || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x npm run check TIMED OUT after ${BRINK_JUST_NPM_CHECK_TIMEOUT}s — tsc itself is local, so this is almost certainly npm resolving through corepack's shim against a stalled proxy. Retry when network is stable, or raise BRINK_JUST_NPM_CHECK_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"

# Build the full brink-studio as a standalone static app and stage it into
# docs/book/src/playground/ (the embedded book playground).
book-assets:
    #!/usr/bin/env bash
    set -euo pipefail
    . scripts/lib/run-with-timeout.sh
    BRINK_JUST_WASM_TIMEOUT="${BRINK_JUST_WASM_TIMEOUT:-900}"
    BRINK_JUST_PNPM_INSTALL_TIMEOUT="${BRINK_JUST_PNPM_INSTALL_TIMEOUT:-900}"
    BRINK_JUST_STUDIO_BUILD_TIMEOUT="${BRINK_JUST_STUDIO_BUILD_TIMEOUT:-900}"
    dest="docs/book/src/playground"
    # The studio's Vite build resolves `brink-web` against this wasm pkg, so it
    # must exist first (out-dir is relative to the crate -> crates/brink-web/www/pkg).
    rc=0
    run_with_timeout "${BRINK_JUST_WASM_TIMEOUT}" wasm-pack build crates/brink-web --target web --out-dir www/pkg || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x wasm-pack build TIMED OUT after ${BRINK_JUST_WASM_TIMEOUT}s — the binaryen/wasm-opt download or the crates.io fetch behind it never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_JUST_WASM_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
    # Install JS deps and build the studio as a self-contained static bundle.
    rc=0
    run_with_timeout "${BRINK_JUST_PNPM_INSTALL_TIMEOUT}" pnpm install:checked -- --frozen-lockfile || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x pnpm install:checked TIMED OUT after ${BRINK_JUST_PNPM_INSTALL_TIMEOUT}s — the npm-registry fetch never completed, likely a stalled proxy. node_modules is now HALF-WRITTEN (#2479/#2593); rerun after the network is stable, or raise BRINK_JUST_PNPM_INSTALL_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
    rc=0
    run_with_timeout "${BRINK_JUST_STUDIO_BUILD_TIMEOUT}" pnpm --filter @brink-lang/studio build:embed || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x studio build:embed TIMED OUT after ${BRINK_JUST_STUDIO_BUILD_TIMEOUT}s — the Vite build is local, so this is almost certainly pnpm resolving through corepack's shim against a stalled proxy. Retry when network is stable, or raise BRINK_JUST_STUDIO_BUILD_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
    # Stage the static build (index.html + assets/, wasm bundled) into the book.
    rm -rf "$dest"
    mkdir -p "$dest"
    cp -R packages/brink-studio/dist-embed/. "$dest/"

# Build the book with the embedded playground
book: book-assets
    mdbook build docs/book

# Run brink-studio dev server (builds wasm first)
studio-dev: wasm
    #!/usr/bin/env bash
    set -euo pipefail
    # check-scripts: allow-unbounded `pnpm dev` is a Vite dev server that runs until Ctrl-C, so a run_with_timeout bound would kill exactly the thing the recipe was asked to start. The one fetch it can do — corepack resolving the pinned pnpm tarball on a cache miss (#2642) — happens before the server starts, and the `wasm` dependency this recipe declares has already run that same shim under BRINK_JUST_WASM_TIMEOUT.
    cd packages/brink-studio && pnpm dev

# Build brink-studio for production (builds wasm first)
studio-build: wasm
    #!/usr/bin/env bash
    set -euo pipefail
    . scripts/lib/run-with-timeout.sh
    BRINK_JUST_STUDIO_BUILD_TIMEOUT="${BRINK_JUST_STUDIO_BUILD_TIMEOUT:-900}"
    cd packages/brink-studio
    rc=0
    run_with_timeout "${BRINK_JUST_STUDIO_BUILD_TIMEOUT}" pnpm build || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x studio build TIMED OUT after ${BRINK_JUST_STUDIO_BUILD_TIMEOUT}s — the Vite build is local, so this is almost certainly pnpm resolving through corepack's shim against a stalled proxy. Retry when network is stable, or raise BRINK_JUST_STUDIO_BUILD_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"

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
