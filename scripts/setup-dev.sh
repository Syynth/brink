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

# The wasm-pack version CI installs — the `version:` input to
# jetli/wasm-pack-action in .github/workflows/ci.yml (the action itself is
# pinned at v0.4.0; don't confuse the two). Keep in sync.
WASM_PACK_VERSION="v0.14.0"

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"

# ⚠ Fetch the PINNED release tarball, never the upstream `init.sh` installer:
# init.sh serves whatever it considers latest (observed: 0.13.1), which would
# silently leave local wasm-pack on a different version than CI builds with.
wasm_pack_ok() {
  command -v wasm-pack >/dev/null 2>&1 &&
    [ "$(wasm-pack --version 2>/dev/null | awk '{print $2}')" = "${WASM_PACK_VERSION#v}" ]
}

if wasm_pack_ok; then
  echo "==> wasm-pack already installed ($(wasm-pack --version))"
else
  if command -v wasm-pack >/dev/null 2>&1; then
    echo "==> wasm-pack present but not ${WASM_PACK_VERSION} ($(wasm-pack --version)) — replacing"
  fi
  echo "==> Installing wasm-pack ${WASM_PACK_VERSION}"
  mkdir -p "${CARGO_BIN}"
  WP_TARBALL="wasm-pack-${WASM_PACK_VERSION}-x86_64-unknown-linux-musl"
  WP_URL="https://github.com/rustwasm/wasm-pack/releases/download/${WASM_PACK_VERSION}/${WP_TARBALL}.tar.gz"
  WP_TMP="$(mktemp -d)"
  if curl --proto '=https' --tlsv1.2 -sSfL "${WP_URL}" | tar zxf - -C "${WP_TMP}" 2>/dev/null &&
     install -m 0755 "${WP_TMP}/${WP_TARBALL}/wasm-pack" "${CARGO_BIN}/wasm-pack"; then
    echo "==> Installed prebuilt $(wasm-pack --version)"
  else
    echo "==> Prebuilt wasm-pack unavailable; building ${WASM_PACK_VERSION} from source"
    cargo install wasm-pack --version "${WASM_PACK_VERSION#v}" --locked --force
  fi
  rm -rf "${WP_TMP}"
fi

# --- binaryen / wasm-opt -----------------------------------------------------
# wasm-pack's FINAL step shells out to wasm-opt, and it downloads binaryen
# itself using an internal HTTP client that honors neither HTTPS_PROXY nor a
# custom CA bundle. Behind a proxying sandbox that download is the ONLY part of
# `wasm-pack build` that fails — everything else (crates.io, wasm-bindgen-cli)
# succeeds. Fetching the same asset with curl works, so pre-seeding wasm-opt on
# PATH makes the full wasm gate pass: wasm-pack logs `found wasm-opt at …` and
# skips its own download entirely.
#
# The released wasm-opt is self-contained (no libbinaryen dependency), so a
# single binary dropped in CARGO_BIN is sufficient.

BINARYEN_VERSION="version_117" # the version wasm-pack ${WASM_PACK_VERSION} fetches

if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt already installed ($(wasm-opt --version 2>/dev/null | head -n1))"
else
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64)  BINARYEN_ASSET="x86_64-linux" ;;
    Linux/aarch64) BINARYEN_ASSET="aarch64-linux" ;;
    Darwin/arm64)  BINARYEN_ASSET="arm64-macos" ;;
    Darwin/x86_64) BINARYEN_ASSET="x86_64-macos" ;;
    *)             BINARYEN_ASSET="" ;;
  esac

  if [ -z "${BINARYEN_ASSET}" ]; then
    echo "==> No binaryen asset for $(uname -s)/$(uname -m); skipping (wasm-pack will try its own download)"
  else
    echo "==> Installing wasm-opt (binaryen ${BINARYEN_VERSION}, ${BINARYEN_ASSET})"
    mkdir -p "${CARGO_BIN}"
    BN_TMP="$(mktemp -d)"
    BN_URL="https://github.com/WebAssembly/binaryen/releases/download/${BINARYEN_VERSION}/binaryen-${BINARYEN_VERSION}-${BINARYEN_ASSET}.tar.gz"
    if curl --proto '=https' --tlsv1.2 -sSfL "${BN_URL}" | tar zxf - -C "${BN_TMP}" --strip-components=1 &&
       install -m 0755 "${BN_TMP}/bin/wasm-opt" "${CARGO_BIN}/wasm-opt"; then
      echo "==> Installed $(wasm-opt --version 2>/dev/null | head -n1)"
    else
      echo "==> wasm-opt install failed; wasm-pack will attempt its own download (expected to fail behind a proxy)"
    fi
    rm -rf "${BN_TMP}"
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

# --- cargo-deny (opt-in) -----------------------------------------------------
# Mirrors the cargo-deny CI job so advisory/license breaks surface locally.
# OPT-IN because it has no prebuilt binary and compiles from source in ~2m —
# real latency when this script runs at every cloud-session start, for a gate
# CI already enforces on every PR. Set BRINK_SETUP_FULL=1 to include it.

if [ "${BRINK_SETUP_FULL:-0}" = "1" ]; then
  if command -v cargo-deny >/dev/null 2>&1; then
    echo "==> cargo-deny already installed ($(cargo deny --version 2>/dev/null | head -n1))"
  else
    echo "==> Installing cargo-deny (BRINK_SETUP_FULL=1; ~2m from source)"
    cargo install cargo-deny --locked || echo "==> cargo-deny install failed; skipping"
  fi
else
  echo "==> Skipping cargo-deny (set BRINK_SETUP_FULL=1 to install; CI gates it anyway)"
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
for tool in rustc cargo rustfmt wasm-pack wasm-opt cargo-nextest pnpm node; do
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
