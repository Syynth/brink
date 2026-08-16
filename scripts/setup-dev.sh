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
#   - pnpm, via corepack, pinned to the EXACT version in root package.json's
#     `packageManager` field — the single source of truth every CI lane's
#     `pnpm/action-setup` step reads too (#2604). Verified after activation,
#     so a drift fails here rather than surfacing as a different install
#     failure shape days later (#2479/#2593).
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
# Every network fetch below is bounded by run_with_timeout. Knobs, in the
# order their steps run, all overridable via environment variable:
#
#   Knob                              Default  On timeout
#   ---------------------------------------------------------------------
#   BRINK_SETUP_RUSTUP_TIMEOUT           120s   FAIL (exit 1) — nothing
#                                                else works without cargo/
#                                                rustc on PATH.
#   BRINK_SETUP_TOOLCHAIN_TIMEOUT        600s   FAIL (exit 1) — the pinned
#                                                toolchain install triggered
#                                                by `rustup show` (channel +
#                                                components + wasm32 target).
#                                                The largest download here,
#                                                hence the largest bound
#                                                (#2638).
#   BRINK_SETUP_WASM_PACK_TIMEOUT         60s   WARN, fall back to a
#                                                from-source `cargo install`
#                                                (see CARGO_INSTALL below).
#   BRINK_SETUP_BINARYEN_TIMEOUT          60s   WARN, continue — this
#                                                binary is a pure
#                                                accelerator; wasm-pack
#                                                downloads it itself
#                                                otherwise.
#   BRINK_SETUP_NEXTEST_TIMEOUT           60s   WARN, fall back to a
#                                                from-source `cargo install`
#                                                (see CARGO_INSTALL below).
#   BRINK_SETUP_CARGO_INSTALL_TIMEOUT    300s   Shared by the three
#                                                from-source `cargo install`
#                                                fallbacks (wasm-pack,
#                                                cargo-nextest, cargo-deny).
#                                                wasm-pack/nextest: FAIL
#                                                (exit 1). cargo-deny: WARN,
#                                                skip the audit.
#   BRINK_SETUP_AUDIT_TIMEOUT            300s   The two `cargo deny check`
#                                                audits themselves
#                                                (BRINK_SETUP_FULL=1 only).
#                                                Root workspace: FAIL.
#                                                src-tauri workspace: WARN
#                                                (matches desktop-smoke.yml's
#                                                continue-on-error, #2470).
#   BRINK_SETUP_COREPACK_TIMEOUT         120s   WARN, continue — the
#                                                npm-registry fetch of the
#                                                pinned pnpm tarball
#                                                (#2638). The pin
#                                                verification right after it
#                                                is what FAILS the run if
#                                                pnpm did not end up at the
#                                                pinned version.
#
# Also see BRINK_SETUP_FULL (below, and CLAUDE.md "Cloud / fresh-environment
# sessions") — gates whether cargo-deny installs/audits at all.
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
    echo "⚠  timeout command not found — running without timeout protection"
    "$@"
    return $?
  fi

  "$timeout_bin" -k 10 "$timeout_secs" "$@"
  return $?
}

# Shared bound for every from-source `cargo install <tool>` fallback in this
# script (wasm-pack, cargo-nextest, cargo-deny — see #2591). All three have
# the identical shape: a crates.io index fetch + dependency downloads
# followed by a genuinely multi-minute local compile, so they get one shared,
# generous default rather than a per-tool number that would just restate the
# same reasoning three times. 300s matches BRINK_SETUP_AUDIT_TIMEOUT's
# existing reasoning (cargo-deny's own from-source install, just below, used
# to be the only unbounded step here) — a cold cache legitimately takes a few
# minutes; override via BRINK_SETUP_CARGO_INSTALL_TIMEOUT if a healthy cold
# build is getting misclassified as a stall.
BRINK_SETUP_CARGO_INSTALL_TIMEOUT="${BRINK_SETUP_CARGO_INSTALL_TIMEOUT:-300}"

# --- rustup + toolchain -----------------------------------------------------

if ! command -v rustup >/dev/null 2>&1; then
  echo "==> Installing rustup"
  # Bound the WHOLE pipeline (outer script fetch + the installer script's own
  # internal download of the platform rustup-init binary), not just the
  # initial curl — sh.rustup.rs is a tiny shell script that does its own
  # network fetch once it starts running, and that inner fetch is the actual
  # multi-MB download at risk of a proxied stall. 120s is generous for that:
  # this step does NOT install the pinned toolchain itself (that's the
  # separate `rustup show` below, which can legitimately take longer for a
  # cold toolchain fetch). FAILS on timeout: every later step in this script,
  # and the workspace itself, needs cargo/rustc on PATH.
  BRINK_SETUP_RUSTUP_TIMEOUT="${BRINK_SETUP_RUSTUP_TIMEOUT:-120}"
  # `-o pipefail` here is load-bearing, not decoration: SHELLOPTS is not
  # exported, so the outer script's `set -euo pipefail` does NOT propagate
  # into this `bash -c` subshell on its own — without repeating it here, a
  # failing `curl` (403/407/TLS from a proxy) is masked by `sh`'s own exit
  # code reading empty stdin as success, and `rustup_install_rc` reads 0
  # instead of curl's real failure (verified: dies later at
  # `source $HOME/.cargo/env` with no diagnostic, or at `rustup: command not
  # found` if a stale `.cargo/env` is already present — either way silently,
  # never through the "rustup install failed" branch below).
  rustup_install_rc=0
  run_with_timeout "${BRINK_SETUP_RUSTUP_TIMEOUT}" bash -o pipefail -c \
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none" ||
    rustup_install_rc=$?
  if [ "$rustup_install_rc" -eq 124 ]; then
    echo "==> ✗ rustup install TIMED OUT after ${BRINK_SETUP_RUSTUP_TIMEOUT}s — the installer fetch never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_SETUP_RUSTUP_TIMEOUT."
    exit 1
  elif [ "$rustup_install_rc" -ne 0 ]; then
    echo "==> ✗ rustup install failed (exit ${rustup_install_rc})"
    exit 1
  fi
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
else
  echo "==> rustup already installed ($(rustup --version | head -n1))"
fi

# rustup reads rust-toolchain.toml from the repo root and installs/selects
# the pinned toolchain (channel, components, wasm32 target) on first use.
#
# On a fresh machine this is the LARGEST network operation in the whole
# script — hundreds of MB of channel + components + the wasm32 target —
# which is exactly why it needs the MOST generous bound here, not the
# tightest. 600s is deliberate: a cold toolchain fetch on a slow-but-healthy
# link legitimately runs into the minutes, and a bound that misclassifies
# that as a stall converts a working (if slow) setup into a hard failure,
# which is strictly worse than the stall it guards against. The number is
# therefore sized to catch "this is never going to finish" (a wedged proxy),
# not "this is slower than I'd like"; raise BRINK_SETUP_TOOLCHAIN_TIMEOUT if
# a genuinely healthy link is still getting cut off.
#
# FAILS on timeout (and on any other error): every later step in this
# script, every gate, and the workspace itself needs cargo/rustc from the
# pinned toolchain. There is no degraded mode worth continuing into.
BRINK_SETUP_TOOLCHAIN_TIMEOUT="${BRINK_SETUP_TOOLCHAIN_TIMEOUT:-600}"
echo "==> Resolving pinned toolchain from rust-toolchain.toml"
# `rc=0; cmd || rc=$?` — NOT `if ! cmd; then rc=$?`, which reads the
# NEGATION's status and so always captures 0, leaving the -eq 124 branch
# permanently dead (the exact bug #2584 shipped and #2531's test now guards).
toolchain_rc=0
run_with_timeout "${BRINK_SETUP_TOOLCHAIN_TIMEOUT}" rustup show || toolchain_rc=$?
if [ "$toolchain_rc" -eq 124 ]; then
  echo "==> ✗ toolchain resolution TIMED OUT after ${BRINK_SETUP_TOOLCHAIN_TIMEOUT}s — the pinned toolchain download never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_SETUP_TOOLCHAIN_TIMEOUT."
  exit 1
elif [ "$toolchain_rc" -ne 0 ]; then
  echo "==> ✗ toolchain resolution failed (exit ${toolchain_rc}) — rustup could not install/select the toolchain pinned in rust-toolchain.toml."
  exit 1
fi

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
  # A pinned release tarball (~10MB) from GitHub — 60s is generous for that
  # size. WARN-and-fallback on timeout: a from-source build is a working
  # (if slower) alternative, same as any other fetch failure here.
  BRINK_SETUP_WASM_PACK_TIMEOUT="${BRINK_SETUP_WASM_PACK_TIMEOUT:-60}"
  if run_with_timeout "${BRINK_SETUP_WASM_PACK_TIMEOUT}" curl --proto '=https' --tlsv1.2 -sSfL "${WP_URL}" | tar zxf - -C "${WP_TMP}" 2>/dev/null &&
     install -m 0755 "${WP_TMP}/${WP_TARBALL}/wasm-pack" "${CARGO_BIN}/wasm-pack"; then
    echo "==> Installed prebuilt $(wasm-pack --version)"
  else
    echo "==> Prebuilt wasm-pack unavailable (fetch failed or timed out after ${BRINK_SETUP_WASM_PACK_TIMEOUT}s); building ${WASM_PACK_VERSION} from source"
    # FAILS on timeout, matching this fallback's pre-existing behavior: there
    # was no further fallback before this change either (an unguarded `cargo
    # install` under `set -euo pipefail` already aborted the script on
    # failure) — a timeout is just a more diagnosable form of that same
    # failure, not a new, softer outcome.
    wasm_pack_install_rc=0
    run_with_timeout "${BRINK_SETUP_CARGO_INSTALL_TIMEOUT}" cargo install wasm-pack --version "${WASM_PACK_VERSION#v}" --locked --force ||
      wasm_pack_install_rc=$?
    if [ "$wasm_pack_install_rc" -eq 124 ]; then
      echo "==> ✗ cargo install wasm-pack TIMED OUT after ${BRINK_SETUP_CARGO_INSTALL_TIMEOUT}s — no prebuilt binary available and the from-source build never completed. Retry when network is stable, or raise BRINK_SETUP_CARGO_INSTALL_TIMEOUT."
      exit 1
    elif [ "$wasm_pack_install_rc" -ne 0 ]; then
      echo "==> ✗ cargo install wasm-pack failed (exit ${wasm_pack_install_rc})"
      exit 1
    fi
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
    # A GitHub release tarball (~20MB) — 60s is generous. WARN-and-continue on
    # timeout, same as any other failure of this step: it was already
    # non-fatal before this change (this binary is purely an optional
    # accelerator; wasm-pack falls back to its own — proxy-hostile — download
    # either way), so a stall here must not gain new severity it didn't have.
    BRINK_SETUP_BINARYEN_TIMEOUT="${BRINK_SETUP_BINARYEN_TIMEOUT:-60}"
    if run_with_timeout "${BRINK_SETUP_BINARYEN_TIMEOUT}" curl --proto '=https' --tlsv1.2 -sSfL "${BN_URL}" | tar zxf - -C "${BN_TMP}" --strip-components=1 &&
       install -m 0755 "${BN_TMP}/bin/wasm-opt" "${CARGO_BIN}/wasm-opt"; then
      echo "==> Installed $(wasm-opt --version 2>/dev/null | head -n1)"
    else
      echo "==> wasm-opt install failed or timed out after ${BRINK_SETUP_BINARYEN_TIMEOUT}s; wasm-pack will attempt its own download (expected to fail behind a proxy)"
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
  # A prebuilt tarball (a few MB) from get.nexte.st — 60s is generous.
  # WARN-and-fallback on timeout: a from-source build is a working (if
  # slower) alternative, same as any other fetch failure here.
  BRINK_SETUP_NEXTEST_TIMEOUT="${BRINK_SETUP_NEXTEST_TIMEOUT:-60}"
  if ! run_with_timeout "${BRINK_SETUP_NEXTEST_TIMEOUT}" curl --proto '=https' --tlsv1.2 -LsSf https://get.nexte.st/latest/linux \
      | tar zxf - -C "${CARGO_BIN}"; then
    echo "==> Prebuilt cargo-nextest unavailable (fetch failed or timed out after ${BRINK_SETUP_NEXTEST_TIMEOUT}s); building from source"
    # FAILS on timeout, matching this fallback's pre-existing behavior: there
    # was no further fallback before this change either (an unguarded `cargo
    # install` under `set -euo pipefail` already aborted the script on
    # failure), and nextest is required by the pump's per-round gate — a
    # timeout is a more diagnosable form of that same failure, not a softer
    # outcome.
    nextest_install_rc=0
    run_with_timeout "${BRINK_SETUP_CARGO_INSTALL_TIMEOUT}" cargo install cargo-nextest --locked ||
      nextest_install_rc=$?
    if [ "$nextest_install_rc" -eq 124 ]; then
      echo "==> ✗ cargo install cargo-nextest TIMED OUT after ${BRINK_SETUP_CARGO_INSTALL_TIMEOUT}s — no prebuilt binary available and the from-source build never completed. Retry when network is stable, or raise BRINK_SETUP_CARGO_INSTALL_TIMEOUT."
      exit 1
    elif [ "$nextest_install_rc" -ne 0 ]; then
      echo "==> ✗ cargo install cargo-nextest failed (exit ${nextest_install_rc})"
      exit 1
    fi
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

# Bound applied to each cargo-deny invocation below. 300s, raised from the
# hardcoded 60s this step originally carried (#2531): 60s covers a warm
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
    # WARN-and-continue on timeout, matching this install's pre-existing `||
    # echo ... skipping audit` non-fatal handling: cargo-deny is opt-in
    # (BRINK_SETUP_FULL=1) to begin with, and cargo_deny_ok() below already
    # skips the audit cleanly when the binary isn't the pinned version for
    # any reason, install failure or timeout alike.
    cargo_deny_install_rc=0
    run_with_timeout "${BRINK_SETUP_CARGO_INSTALL_TIMEOUT}" cargo install cargo-deny --version "${CARGO_DENY_VERSION}" --locked ||
      cargo_deny_install_rc=$?
    if [ "$cargo_deny_install_rc" -eq 124 ]; then
      echo "==> ⚠ cargo install cargo-deny TIMED OUT after ${BRINK_SETUP_CARGO_INSTALL_TIMEOUT}s; skipping audit (retry later, or raise BRINK_SETUP_CARGO_INSTALL_TIMEOUT)"
    elif [ "$cargo_deny_install_rc" -ne 0 ]; then
      echo "==> cargo-deny install failed (exit ${cargo_deny_install_rc}); skipping audit"
    fi
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
# The pnpm version is pinned in ONE place — the root package.json
# `packageManager` field, corepack's own mechanism — and derived here rather
# than restated (#2604). This script used to carry `PNPM_MAJOR="10"`, a second
# pin that pinned only the MAJOR: which 10.x a machine resolved was ambient,
# and the failure shape of a missing `crates/brink-web/www/pkg` link demonstrably
# differed across that range (exit 0 + `ENOENT … scandir` in #2479/#2492 vs
# exit 1 + `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` on 10.34.5 in #2593/#2596).
# `scripts/check-pnpm-pin.mjs` (run by `pnpm test:scripts`) fails if this block
# ever hardcodes a version again, or drifts from the field.

if ! command -v corepack >/dev/null 2>&1; then
  echo "==> corepack not found; it ships with Node.js >= 16.9 — install Node first"
  exit 1
fi

# Resolved from this script's own location, not the caller's cwd.
brink_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# `packageManager` is "pnpm@<exact>"; take the version part.
PNPM_VERSION="$(node -p "require('${brink_repo_root}/package.json').packageManager.split('@')[1].split('+')[0]")"

if [ -z "${PNPM_VERSION}" ]; then
  echo "==> could not read the pnpm pin from package.json's \"packageManager\" field"
  exit 1
fi

echo "==> Enabling corepack + pinning pnpm@${PNPM_VERSION}"
# `corepack enable` only writes local shims — no network, so no bound.
corepack enable

# `corepack prepare` DOES hit the network: it downloads the pinned pnpm
# tarball from the npm registry. A few MB, so 120s is generous for the
# transfer itself — set at double the plain-tarball bounds (wasm-pack,
# binaryen, nextest at 60s) because this one additionally does registry
# resolution and integrity verification, and registry latency behind a proxy
# is routinely worse than a GitHub release CDN's; and well under the 300s
# cargo-install bound, since nothing is compiled here.
BRINK_SETUP_COREPACK_TIMEOUT="${BRINK_SETUP_COREPACK_TIMEOUT:-120}"
# WARNS and continues, rather than aborting, on BOTH a timeout and a plain
# failure — because the pin verification immediately below is the better
# judge of whether this run can proceed, and it cannot judge anything if the
# script has already died. Two cases the warn path handles correctly that an
# abort does not: an idempotent re-run where the pinned pnpm is ALREADY
# activated (nothing was actually lost, so aborting is pure false negative),
# and a genuinely broken fetch, where the verification's diagnostic
# distinguishes "corepack could not fetch the pin" from "a standalone pnpm
# shadows corepack's shim" — which is what that block was written for and,
# before this change, could never print for a hard corepack failure, since a
# bare command under `set -e` killed the run first.
corepack_rc=0
run_with_timeout "${BRINK_SETUP_COREPACK_TIMEOUT}" \
  corepack prepare "pnpm@${PNPM_VERSION}" --activate || corepack_rc=$?
if [ "$corepack_rc" -eq 124 ]; then
  echo "==> ⚠  corepack prepare TIMED OUT after ${BRINK_SETUP_COREPACK_TIMEOUT}s — the pnpm tarball fetch never completed. Verifying what pnpm resolves to anyway; raise BRINK_SETUP_COREPACK_TIMEOUT if the link is healthy but slow."
elif [ "$corepack_rc" -ne 0 ]; then
  echo "==> ⚠  corepack prepare failed (exit ${corepack_rc}) — verifying what pnpm resolves to anyway."
fi

# Verify what actually resolved. A pin nothing checks is a pin that drifts
# silently, which is the whole point of #2604. Capture stderr too — a
# corepack that refused or failed to fetch the pinned version, or a
# standalone pnpm sitting earlier on PATH than corepack's shim, both report a
# mismatch here, and the abort must say which one happened, not just that one
# did.
resolved_pnpm_stderr="$(mktemp)"
# Bounded the same as `corepack prepare` above (#2642 review): on a cache
# miss, this invokes corepack's shim, which downloads the pinned pnpm
# tarball itself — through the same possibly-wedged proxy, with no bound of
# its own otherwise. Without this, the WARN-and-continue path above just
# relocates the hang here instead of eliminating it.
resolved_pnpm="$(run_with_timeout "${BRINK_SETUP_COREPACK_TIMEOUT}" pnpm --version 2>"${resolved_pnpm_stderr}" || true)"
if [ "${resolved_pnpm}" != "${PNPM_VERSION}" ]; then
  echo "==> ERROR: pnpm resolved to '${resolved_pnpm}' but package.json pins ${PNPM_VERSION}"
  if [ -s "${resolved_pnpm_stderr}" ]; then
    echo "==> pnpm --version reported:"
    sed 's/^/    /' "${resolved_pnpm_stderr}"
  fi
  echo "==> Remedy: corepack prepare \"pnpm@${PNPM_VERSION}\" --activate"
  echo "==> If that doesn't fix it, check for a standalone pnpm earlier on PATH than corepack's shim (\`which -a pnpm\`) — it can shadow the pin."
  rm -f "${resolved_pnpm_stderr}"
  exit 1
fi
rm -f "${resolved_pnpm_stderr}"

echo "==> pnpm ready (${resolved_pnpm})"

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
