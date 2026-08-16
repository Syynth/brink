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
#   - cargo-deny (BRINK_SETUP_FULL=1 only) — pinned to the version CI's
#     pinned EmbarkStudios/cargo-deny-action SHA runs, then run against BOTH
#     workspaces .github/workflows/ci.yml's "cargo-deny" job and
#     .github/workflows/desktop-smoke.yml's "cargo-deny (src-tauri)" step
#     audit, so advisory/licence breaks in either graph surface locally
#     instead of only in CI.
#
# Prebuilt binaries are preferred over `cargo install` wherever upstream
# publishes them: building nextest + wasm-pack from source costs minutes each,
# which matters when this runs as a cloud-session setup script on every start.
#
# Safe to re-run: every step checks current state before acting.

set -euo pipefail

echo "==> Brink dev environment setup"

# --- run_with_timeout helper -------------------------------------------------
# Wraps a command with a bounded timeout to prevent stalled network fetches
# (e.g., RUSTSEC DB fetch in cargo deny) from blocking indefinitely.
#
# Usage: run_with_timeout <timeout_seconds> <command> [args...]
#
# Returns:
#   - 0 if the command succeeds
#   - 124 if the command times out (timeout exit code)
#   - non-zero if the command fails normally
#
# Prefers GNU `timeout` (Linux), falling back to `gtimeout` (macOS + Homebrew
# coreutils) before degrading to no timeout protection at all. `-k 10` sends
# SIGKILL 10s after the initial SIGTERM, so a child wedged in a syscall (e.g.
# a proxied git fetch) that ignores SIGTERM still gets reaped instead of
# hanging the wrapper forever.
run_with_timeout() {
  local timeout_secs="$1"
  shift

  local timeout_bin=""
  if command -v timeout >/dev/null 2>&1; then
    timeout_bin="timeout"
  elif command -v gtimeout >/dev/null 2>&1; then
    timeout_bin="gtimeout"
  fi

  if [ -z "$timeout_bin" ]; then
    echo "⚠  timeout command not found — running audit without timeout protection"
    "$@"
    return $?
  fi

  "$timeout_bin" -k 10 "$timeout_secs" "$@"
  return $?
}

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
# Mirrors BOTH cargo-deny CI steps so advisory/licence breaks surface locally:
#   - .github/workflows/ci.yml's required "cargo-deny" job — root workspace,
#     blocking.
#   - .github/workflows/desktop-smoke.yml's "cargo-deny (src-tauri)" step —
#     packages/brink-desktop/src-tauri's OWN Cargo.lock (451 [[package]]
#     entries via the Tauri graph), which shares no resolution with the root
#     lock and gets no audit anywhere else in CI (#2470). continue-on-error:
#     true there, because that audit surfaces real findings: unmaintained-crate
#     RUSTSEC advisories inherent to Tauri v2 on Linux, plus brink-desktop's
#     own `unlicensed` entry. Neither is ruled on. (The MPL-2.0 licence
#     rejections that used to appear alongside them ARE ruled — admitted
#     per-crate as of 2026-08-15, see docs/decision-log.md.) For the current
#     count, defer to docs/desktop-shell-spec.md "Smoke-lane inputs and step
#     gating" rather than restating a number here that drifts every time the
#     graph moves.
#
# Both CI steps pin the SAME action SHA (guarded by
# desktop_smoke_audits_the_src_tauri_dependency_graph in
# packages/brink-desktop/src-tauri/src/lib.rs):
#   EmbarkStudios/cargo-deny-action@bb137d7af7e4fb67e5f82a49c4fce4fad40782fe # v2
# whose bundled image ships cargo-deny 0.19.8 (see the "--config" comment
# beside that step in desktop-smoke.yml). Installing whatever `cargo install
# cargo-deny` considers latest would let a local run disagree with CI in
# EITHER direction — a false pass as easily as a false failure, both look
# authoritative — so pin to the exact version instead. Bump CARGO_DENY_VERSION
# only alongside that action SHA.
#
# OPT-IN (BRINK_SETUP_FULL=1) because cargo-deny has no prebuilt binary and
# compiles from source in ~2m — real latency when this script runs at every
# cloud-session start, for gates CI already enforces on every PR.

CARGO_DENY_VERSION="0.19.8"

# Bound applied to each cargo-deny invocation below. 60s covers a warm
# advisory-db cache, but a cold `advisory-db` clone plus a full
# `--all-features` resolve of the 451-package src-tauri graph can legitimately
# exceed that on a first run — override via BRINK_SETUP_AUDIT_TIMEOUT if a
# healthy cold run is getting misclassified as a stall.
BRINK_SETUP_AUDIT_TIMEOUT="${BRINK_SETUP_AUDIT_TIMEOUT:-300}"

cargo_deny_ok() {
  command -v cargo-deny >/dev/null 2>&1 &&
    [ "$(cargo deny --version 2>/dev/null | awk '{print $2}')" = "${CARGO_DENY_VERSION}" ]
}

if [ "${BRINK_SETUP_FULL:-0}" = "1" ]; then
  if cargo_deny_ok; then
    echo "==> cargo-deny already installed ($(cargo deny --version 2>/dev/null | head -n1))"
  else
    if command -v cargo-deny >/dev/null 2>&1; then
      echo "==> cargo-deny present but not ${CARGO_DENY_VERSION} ($(cargo deny --version 2>/dev/null | head -n1)) — replacing"
    fi
    echo "==> Installing cargo-deny ${CARGO_DENY_VERSION} (BRINK_SETUP_FULL=1; ~2m from source, no prebuilt binary)"
    cargo install cargo-deny --version "${CARGO_DENY_VERSION}" --locked || echo "==> cargo-deny install failed; skipping audit"
  fi

  # ⚠ Gate on cargo_deny_ok, NOT `command -v` — the latter is true in exactly
  # the present-but-wrong-version case that triggered the reinstall above, so
  # a FAILED reinstall would fall through to auditing under the wrong binary
  # while printing that it mirrors CI, reintroducing the version skew this
  # block exists to remove.
  if cargo_deny_ok; then
    echo "==> Running cargo-deny check (root workspace) — mirrors ci.yml's required \"cargo-deny\" job"
    # `--all-features` matches what CI actually runs: ci.yml's required job
    # passes only `command: check`, so the action's own default
    # `arguments: "--all-features"` applies. No `--locked` here — ci.yml does
    # not pass it, and a mirror must not be stricter than the job it mirrors.
    # Wrapped in timeout to prevent a stalled RUSTSEC DB fetch from blocking
    # indefinitely (issue #2531).
    audit_exit_code=0
    run_with_timeout "${BRINK_SETUP_AUDIT_TIMEOUT}" cargo deny --all-features check || audit_exit_code=$?
    if [ "$audit_exit_code" -eq 124 ]; then
      echo "==> ✗ cargo-deny check (root workspace) TIMED OUT after ${BRINK_SETUP_AUDIT_TIMEOUT}s — audit never completed, likely due to stalled RUSTSEC DB fetch. Retry when network/proxy is stable, or raise BRINK_SETUP_AUDIT_TIMEOUT."
      exit 1
    elif [ "$audit_exit_code" -ne 0 ]; then
      echo "==> cargo-deny check (root workspace) reported findings — this job is REQUIRED in ci.yml; fix before pushing"
    fi

    echo "==> Running cargo-deny check (packages/brink-desktop/src-tauri) — mirrors desktop-smoke.yml's \"cargo-deny (src-tauri)\" step"
    # NOTE: --manifest-path/--all-features/--locked are top-level cargo-deny
    # flags, not `check` subcommand flags (`cargo deny --help` vs
    # `cargo deny check --help`) — they MUST precede `check` on the command
    # line, matching how the action assembles `arguments` before `command`
    # (see the comment beside this step in desktop-smoke.yml).
    # Wrapped in timeout to prevent a stalled RUSTSEC DB fetch from blocking
    # indefinitely (issue #2531). This audit is non-blocking end-to-end
    # (desktop-smoke.yml's continue-on-error, #2470), so a timeout here warns
    # and continues rather than aborting the script — an `exit 1` at this
    # point would skip the pnpm/corepack section and the verification block
    # below, leaving a cloud session with no pnpm over a non-required audit.
    audit_exit_code=0
    run_with_timeout "${BRINK_SETUP_AUDIT_TIMEOUT}" cargo deny --manifest-path packages/brink-desktop/src-tauri/Cargo.toml --all-features --locked check || audit_exit_code=$?
    if [ "$audit_exit_code" -eq 124 ]; then
      echo "==> ⚠ cargo-deny check (src-tauri) TIMED OUT after ${BRINK_SETUP_AUDIT_TIMEOUT}s — audit never completed, likely due to stalled RUSTSEC DB fetch. Non-blocking (matches desktop-smoke.yml's continue-on-error, #2470); retry with BRINK_SETUP_FULL=1 later, or raise BRINK_SETUP_AUDIT_TIMEOUT."
    elif [ "$audit_exit_code" -ne 0 ]; then
      echo "==> cargo-deny check (src-tauri) reported findings — non-blocking, matches desktop-smoke.yml's continue-on-error (#2470). The MPL-2.0 licences are RULED and admitted per-crate (docs/decision-log.md, 2026-08-15); what remains is the unmaintained-crate RUSTSEC advisories plus brink-desktop's own unlicensed entry, neither of which is ruled on."
    fi
  else
    echo "==> Skipping audits — cargo-deny is not ${CARGO_DENY_VERSION} (install failed, or a different version is on PATH). Auditing under a mismatched binary would disagree with CI in either direction while looking authoritative."
  fi
else
  echo "==> Skipping cargo-deny (set BRINK_SETUP_FULL=1 to install + audit; CI gates it anyway)"
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
# @brink-lang/web (packages/wasm) has a file: dependency on this build
# output. Build it BEFORE installing — ordering that matters because a bare
# `pnpm install --frozen-lockfile` does NOT reliably fail loudly when it's
# skipped. Two distinct shapes have been observed for that same skipped
# ordering: an install that exits 0 with the link silently unresolved
# (#2479), and an install that writes NO node_modules at all (#2593, where
# the only visible symptom was a bare "vitest: not found" from the NEXT
# command). Which one a given machine gets depends on the pnpm 10.x that
# corepack resolved there, so the printed sequence must not rely on pnpm's
# exit code at all.
#
# `pnpm install:checked` (scripts/guarded-install.mjs) is what makes this
# ENFORCED rather than merely printed: it runs check:wasm-pkg's cause check
# BEFORE spawning pnpm — refusing to install at all, so no half-written tree
# appears — and re-verifies afterwards that an installed tree actually
# materialised, exiting non-zero when it did not even if pnpm reported
# success. A pnpm `preinstall` hook cannot do this job: pnpm skips every
# project lifecycle script when a per-package link fails, so the hook is
# dead code in exactly this case (re-verified on pnpm 10.34.5 for #2593).
echo "    wasm-pack build crates/brink-web --target web --out-dir www/pkg"
echo "    pnpm install:checked -- --frozen-lockfile   # guarded; see #2479/#2593"
echo "    cargo nextest run --workspace     # the pump's per-round gate"
