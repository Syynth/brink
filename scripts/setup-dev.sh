#!/usr/bin/env bash
# Idempotent fresh-environment setup for Brink development (Linux/macOS).
#
# Installs/verifies:
#   - rustup + the toolchain pinned in rust-toolchain.toml (rustup honors
#     that file automatically on first `cargo`/`rustc` invocation in this
#     repo — `rustup show` here just triggers that resolution up front).
#   - wasm-pack, pinned to the version CI uses (.github/workflows/ci.yml,
#     jetli/wasm-pack-action version v0.14.0) — needed to build the
#     crates/brink-web wasm bundle that @brink-lang/web depends on.
#   - pnpm, via corepack, pinned to the major version CI uses
#     (pnpm/action-setup version: 10 in .github/workflows/ci.yml).
#
# Safe to re-run: every step checks current state before acting.

set -euo pipefail

echo "==> Brink dev environment setup"

# --- rustup + toolchain -----------------------------------------------------

if ! command -v rustup >/dev/null 2>&1; then
  echo "==> Installing rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
else
  echo "==> rustup already installed ($(rustup --version | head -n1))"
fi

# rustup reads rust-toolchain.toml from the repo root and installs/selects
# the pinned toolchain (channel, components, wasm32 target) on first use.
echo "==> Resolving pinned toolchain from rust-toolchain.toml"
rustup show

# --- wasm-pack ---------------------------------------------------------------

WASM_PACK_VERSION="v0.14.0" # keep in sync with .github/workflows/ci.yml

if command -v wasm-pack >/dev/null 2>&1; then
  echo "==> wasm-pack already installed ($(wasm-pack --version))"
else
  echo "==> Installing wasm-pack ${WASM_PACK_VERSION}"
  cargo install wasm-pack --version "${WASM_PACK_VERSION#v}" --locked
fi

# --- pnpm (via corepack) -----------------------------------------------------

PNPM_MAJOR="10" # keep in sync with pnpm/action-setup in .github/workflows/ci.yml

if ! command -v corepack >/dev/null 2>&1; then
  echo "==> corepack not found; it ships with Node.js >= 16.9 — install Node first"
  exit 1
fi

echo "==> Enabling corepack + pinning pnpm@${PNPM_MAJOR}"
corepack enable
corepack prepare "pnpm@${PNPM_MAJOR}" --activate

echo "==> pnpm ready ($(pnpm --version))"

echo "==> Done. Next steps:"
echo "    cargo check --workspace"
echo "    pnpm install --frozen-lockfile"
