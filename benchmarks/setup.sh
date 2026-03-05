#!/usr/bin/env bash
# Benchmark setup — installs/verifies all implementations (idempotent).
set -euo pipefail

cd "$(dirname "$0")/.."

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
    cargo install binkplayer
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
npm install --silent
cd ../../..

echo "==> Checking hyperfine..."
if command -v hyperfine &>/dev/null; then
    echo "    hyperfine found: $(command -v hyperfine)"
else
    echo "    Installing hyperfine via brew..."
    brew install hyperfine
fi

echo "==> Setup complete."
