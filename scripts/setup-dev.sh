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
#   - cargo-nextest — the autonomous pump's per-round GATE runs
#     `cargo nextest run --workspace`, measured at 35s vs `cargo test`'s
#     2m52s for identical results (issue #1695). Without it every pump agent
#     falls back to the 5x-slower path or fails outright.
#   - cargo-deny — the `cargo-deny` CI job (.github/workflows/ci.yml) gates
#     advisories/licenses; installing it locally lets a gate run catch what
#     would otherwise only fail in CI.
#
# Prebuilt binaries are preferred over `cargo install` wherever upstream
# publishes them: building nextest + wasm-pack from source costs minutes each,
# which matters when this runs as a cloud-session setup script on every start.
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

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"

if command -v wasm-pack >/dev/null 2>&1; then
  echo "==> wasm-pack already installed ($(wasm-pack --version))"
else
  echo "==> Installing wasm-pack ${WASM_PACK_VERSION}"
  # Prefer the upstream prebuilt-binary installer; fall back to building from
  # source if it can't be reached (sandboxed networks sometimes block the
  # GitHub release assets it fetches).
  if ! curl --proto '=https' --tlsv1.2 -sSf \
      https://rustwasm.github.io/wasm-pack/installer/init.sh | sh; then
    echo "==> Prebuilt wasm-pack unavailable; building from source"
    cargo install wasm-pack --version "${WASM_PACK_VERSION#v}" --locked
  fi
fi

# --- cargo-nextest -----------------------------------------------------------
# The pump's GATE depends on this; `cargo test` is ~5x slower for identical
# results (35s vs 2m52s, issue #1695) because it runs the 183 test binaries
# nearly serially (~56% CPU vs nextest's ~507%).

if command -v cargo-nextest >/dev/null 2>&1; then
  echo "==> cargo-nextest already installed ($(cargo nextest --version 2>/dev/null | head -n1))"
else
  echo "==> Installing cargo-nextest"
  mkdir -p "${CARGO_BIN}"
  if ! curl --proto '=https' --tlsv1.2 -LsSf https://get.nexte.st/latest/linux \
      | tar zxf - -C "${CARGO_BIN}"; then
    echo "==> Prebuilt cargo-nextest unavailable; building from source"
    cargo install cargo-nextest --locked
  fi
fi

# --- cargo-deny --------------------------------------------------------------
# Mirrors the cargo-deny CI job so advisory/license breaks surface locally.
# Non-fatal: a dev environment is still usable without it.

if command -v cargo-deny >/dev/null 2>&1; then
  echo "==> cargo-deny already installed ($(cargo deny --version 2>/dev/null | head -n1))"
else
  echo "==> Installing cargo-deny (non-fatal if it fails)"
  cargo install cargo-deny --locked || echo "==> cargo-deny install failed; skipping"
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

# --- verification ------------------------------------------------------------
# Print what actually resolved. A setup script that exits 0 having silently
# skipped a tool is how a wave discovers at gate time that its gate isn't
# installed — name the gap here, where it's cheap to fix.

echo "==> Verifying toolchain"
missing=0
for tool in rustc cargo rustfmt wasm-pack cargo-nextest pnpm node; do
  if command -v "$tool" >/dev/null 2>&1; then
    printf '    %-16s OK\n' "$tool"
  else
    printf '    %-16s MISSING\n' "$tool"
    missing=$((missing + 1))
  fi
done

if [ "$missing" -gt 0 ]; then
  echo "==> WARNING: ${missing} required tool(s) missing — gates depending on them will fail."
fi

echo "==> Done. Next steps:"
echo "    cargo check --workspace"
echo "    pnpm install --frozen-lockfile"
echo "    cargo nextest run --workspace     # the pump's per-round gate"
