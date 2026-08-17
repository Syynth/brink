#!/usr/bin/env bash
# Benchmark setup — installs/verifies all implementations (idempotent).
#
# The three INSTALL steps below hit the network (crates.io, the npm registry,
# Homebrew's formulae + bottles) and are therefore bounded by run_with_timeout,
# the same wedged-proxy hang class #2591/#2638/#2642 bounded in setup-dev.sh
# and #2667 bounded in refresh-excluded-lockfiles.sh. They were left BARE until
# #2677 because scripts/check-scripts.mjs discovered only scripts/**/*.sh —
# this file sits one directory over, reachable from `just
# cross-language-benchmark`, and was outside the scan for exactly the reason
# refresh-excluded-lockfiles.sh had been. Knobs:
#
#   Knob                                     Default  On timeout
#   ---------------------------------------------------------------------
#   BRINK_BENCH_CARGO_INSTALL_TIMEOUT           900s   FAIL (exit 1). `cargo
#                                                      install binkplayer`
#                                                      fetches the crates.io
#                                                      index and then COMPILES
#                                                      from source, so the
#                                                      bound has to cover a
#                                                      cold build, not just a
#                                                      download.
#   BRINK_BENCH_NPM_INSTALL_TIMEOUT             600s   FAIL (exit 1). The inkjs
#                                                      driver's dependencies —
#                                                      a small tree, no
#                                                      compilation — so 600s is
#                                                      already generous for a
#                                                      cold registry fetch.
#   BRINK_BENCH_BREW_TIMEOUT                    900s   FAIL (exit 1). Homebrew
#                                                      may update its formulae
#                                                      before installing, which
#                                                      is itself a large fetch,
#                                                      and it falls back to a
#                                                      from-source build when
#                                                      no bottle matches.
#
# All three say FAIL because none of them has a fallback: this script's whole
# job is to leave every implementation runnable, and benchmarks/run.sh invokes
# binkplayer/hyperfine directly. A timeout that exited 0 would hand run.sh a
# missing binary — a confusing failure one step later instead of a named one
# here. (inklecate is the deliberate exception: it is DETECTED, never
# installed, and its absence already warns and skips.)
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=../scripts/lib/run-with-timeout.sh
. scripts/lib/run-with-timeout.sh

BRINK_BENCH_CARGO_INSTALL_TIMEOUT="${BRINK_BENCH_CARGO_INSTALL_TIMEOUT:-900}"
BRINK_BENCH_NPM_INSTALL_TIMEOUT="${BRINK_BENCH_NPM_INSTALL_TIMEOUT:-600}"
BRINK_BENCH_BREW_TIMEOUT="${BRINK_BENCH_BREW_TIMEOUT:-900}"

echo "==> Building brink-cli (release)..."
cargo build --release -p brink-cli

echo "==> Building brink-loop driver..."
cd benchmarks/drivers/brink-loop
cargo build --release
cd ../../..

echo "==> Checking binkplayer..."
if command -v binkplayer &>/dev/null; then
    echo "    binkplayer found: $(command -v binkplayer)"
else
    echo "    Installing binkplayer via cargo install..."
    rc=0
    run_with_timeout "${BRINK_BENCH_CARGO_INSTALL_TIMEOUT}" cargo install binkplayer || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x cargo install binkplayer TIMED OUT after ${BRINK_BENCH_CARGO_INSTALL_TIMEOUT}s — the crates.io fetch or the from-source build never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_BENCH_CARGO_INSTALL_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
fi

echo "==> Checking inklecate..."
INKLECATE="${INKLECATE:-}"
if [[ -n "$INKLECATE" ]]; then
    echo "    inklecate found (INKLECATE env): $INKLECATE"
elif command -v inklecate &>/dev/null; then
    echo "    inklecate found: $(command -v inklecate)"
elif [[ -x "$HOME/code/rs/s92-studio/reference/ink/inklecate/bin/Release/net8.0/inklecate" ]]; then
    echo "    inklecate found at reference location"
else
    echo "    WARNING: inklecate not found — will be skipped in benchmarks"
    echo "    Set INKLECATE=/path/to/inklecate or add to PATH"
fi

echo "==> Installing inkjs dependencies..."
cd benchmarks/drivers/inkjs
rc=0
run_with_timeout "${BRINK_BENCH_NPM_INSTALL_TIMEOUT}" npm install --silent || rc=$?
if [ "$rc" -eq 124 ]; then
    echo "==> x npm install TIMED OUT after ${BRINK_BENCH_NPM_INSTALL_TIMEOUT}s in benchmarks/drivers/inkjs — the npm-registry fetch never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_BENCH_NPM_INSTALL_TIMEOUT." >&2
    exit 1
fi
[ "$rc" -eq 0 ] || exit "$rc"
cd ../../..

echo "==> Checking hyperfine..."
if command -v hyperfine &>/dev/null; then
    echo "    hyperfine found: $(command -v hyperfine)"
else
    echo "    Installing hyperfine via brew..."
    rc=0
    run_with_timeout "${BRINK_BENCH_BREW_TIMEOUT}" brew install hyperfine || rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "==> x brew install hyperfine TIMED OUT after ${BRINK_BENCH_BREW_TIMEOUT}s — the formulae update or bottle download never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_BENCH_BREW_TIMEOUT." >&2
        exit 1
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
fi

echo "==> Setup complete."
