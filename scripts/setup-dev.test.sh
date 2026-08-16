#!/usr/bin/env bash
# Regression test for scripts/setup-dev.sh.
#
# Part 1 (Tests 1-4, original — #2531/PR #2584): the cargo-deny audit
# timeout handling. Runs the REAL script end to end (not a reimplementation)
# against a PATH-injected `cargo-deny` stub that sleeps past the configured
# bound, so a regression of the `audit_exit_code=$?` capture bug —
# `audit_exit_code=$?` inside `if ! run_with_timeout ...; then` always reads
# 0 off the negated `!` pipeline, so the `-eq 124` branch is unreachable dead
# code and a genuine timeout gets reported (and exits) as a normal audit
# finding instead — fails this test instead of silently passing. A pure
# string/grep match on the script source would not catch this: the bug is in
# *control flow*, not in text that's present or absent.
#
# Part 2 (Tests 5-12 — #2591): the remaining network fetches #2531/#2584 left
# unbounded (rustup installer, wasm-pack tarball, binaryen/wasm-opt tarball,
# get.nexte.st tarball, and the three from-source `cargo install` fallbacks),
# plus a full end-to-end run of the script against a completely fresh,
# nothing-installed toolchain (`run_full_script`/`make_full_stub_bin`) — the
# "drive the script end to end against stubbed tools" harness the file
# lacked before this change. Each stubbed network step can be told to hang
# past its configured bound via an env toggle (HANG_RUSTUP_CURL=1, etc.), so
# these tests prove the same class of $?-capture bug Part 1 guards against
# cannot recur in the newly-bounded steps either — by actually making a stub
# sleep and observing what the script does, not by grepping its source.
#
# Usage: bash scripts/setup-dev.test.sh

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${here}/.." && pwd)"
script="${repo_root}/scripts/setup-dev.sh"

# The exact pnpm version root package.json pins (#2604). Read, never restated:
# a hardcoded copy here would be exactly the second, drifting pin the pin
# exists to abolish.
pinned_pnpm_version="$(node -p "require('${repo_root}/package.json').packageManager.split('@')[1].split('+')[0]")"

failures=0
fail() {
  echo "FAIL: $1" >&2
  failures=$((failures + 1))
}
pass() {
  echo "ok - $1"
}

# Stub bin dir: a `cargo-deny` that reports the pinned version instantly but
# sleeps past the timeout on the `check` invocation named by `$2`
# ("root"|"src-tauri"), plus stubs for every other external tool
# setup-dev.sh touches before reaching the cargo-deny block, so the test
# runs hermetically (no network, no minutes-long real installs) regardless
# of what's cached on the machine running it.
make_stub_bin() {
  local dir="$1"
  local hang_which="$2"

  mkdir -p "${dir}"

  # Note: `cargo <subcommand> …` invokes `cargo-<subcommand>` with the
  # subcommand name STILL as the first arg (cargo-deny is designed to run
  # standalone as `cargo-deny deny …` too) — so `--version` can land at any
  # position, not just `$1`.
  cat > "${dir}/cargo-deny" <<EOF
#!/usr/bin/env bash
for a in "\$@"; do
  if [ "\$a" = "--version" ]; then
    echo "cargo-deny 0.19.8"
    exit 0
  fi
done
case " \$* " in
  *" --manifest-path "*)
    if [ "${hang_which}" = "src-tauri" ]; then sleep 5; fi
    ;;
  *)
    if [ "${hang_which}" = "root" ]; then sleep 5; fi
    ;;
esac
exit 0
EOF

  cat > "${dir}/rustup" <<'EOF'
#!/usr/bin/env bash
[ "$1" = "--version" ] && { echo "rustup 1.0.0"; exit 0; }
exit 0
EOF

  cat > "${dir}/wasm-pack" <<'EOF'
#!/usr/bin/env bash
echo "wasm-pack 0.14.0"
EOF

  cat > "${dir}/cargo-nextest" <<'EOF'
#!/usr/bin/env bash
echo "cargo-nextest 0.0.0"
EOF

  cat > "${dir}/corepack" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF

  # setup-dev.sh now VERIFIES that the pnpm it activated is the exact version
  # root package.json's `packageManager` field pins (#2604), so this stub
  # reports the real pin rather than a hardcoded version that would go stale
  # the next time the pin moves. The third argument overrides it, which is how
  # Test 3 below plants a mismatch.
  local pnpm_reports="${3:-${pinned_pnpm_version}}"
  cat > "${dir}/pnpm" <<EOF
#!/usr/bin/env bash
echo "${pnpm_reports}"
EOF

  chmod +x "${dir}"/*
}

run_script() {
  local hang_which="$1"
  local pnpm_reports="${2:-}"
  local stub_dir
  stub_dir="$(mktemp -d)"
  make_stub_bin "${stub_dir}" "${hang_which}" "${pnpm_reports}"

  local out
  out="$(PATH="${stub_dir}:${PATH}" BRINK_SETUP_FULL=1 BRINK_SETUP_AUDIT_TIMEOUT=1 bash "${script}" 2>&1)"
  local rc=$?

  rm -rf "${stub_dir}"
  printf '%s\n' "${out}"
  return "${rc}"
}

# --- Test 1: a hung ROOT-workspace audit must TIME OUT and exit non-zero ---
out="$(run_script root)"
rc=$?
if [ "${rc}" -ne 0 ]; then
  pass "root-workspace audit timeout: script exits non-zero (got ${rc})"
else
  fail "root-workspace audit timeout: script exited 0 — the hang was not detected as a timeout"
fi
if printf '%s' "${out}" | grep -q "TIMED OUT"; then
  pass "root-workspace audit timeout: prints a TIMED OUT message"
else
  fail "root-workspace audit timeout: no 'TIMED OUT' in output:\n${out}"
fi

# --- Test 2: a hung SRC-TAURI audit must warn and CONTINUE (non-blocking,
# desktop-smoke.yml's continue-on-error, #2470) rather than abort the script
# before the pnpm/corepack section. ---
out="$(run_script src-tauri)"
rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "src-tauri audit timeout: script still exits 0 (non-blocking)"
else
  fail "src-tauri audit timeout: script exited ${rc} — a non-blocking audit aborted the run"
fi
if printf '%s' "${out}" | grep -q "TIMED OUT"; then
  pass "src-tauri audit timeout: prints a TIMED OUT message"
else
  fail "src-tauri audit timeout: no 'TIMED OUT' in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "pnpm ready"; then
  pass "src-tauri audit timeout: script reached the pnpm section"
else
  fail "src-tauri audit timeout: script never reached the pnpm section:\n${out}"
fi

# --- Test 3: pnpm version drift must be LOUD (#2604). setup-dev.sh derives the
# version from root package.json's `packageManager` field and verifies what
# actually resolved; if corepack activates something else (the ambient-10.x
# situation that produced two different install-failure shapes across #2479
# and #2593), the script must fail rather than print "pnpm ready" and hand a
# mismatched toolchain to the next gate. Planted mismatch: a pnpm stub
# reporting a version that is not the pin. ---
out="$(run_script src-tauri "9.99.99")"
rc=$?
if [ "${rc}" -ne 0 ]; then
  pass "pnpm drift: script exits non-zero when the resolved pnpm is not the pin (got ${rc})"
else
  fail "pnpm drift: script exited 0 despite pnpm reporting 9.99.99, not ${pinned_pnpm_version}:\n${out}"
fi
if printf '%s' "${out}" | grep -q "pnpm resolved to '9.99.99'"; then
  pass "pnpm drift: names both the resolved version and the pin"
else
  fail "pnpm drift: no drift message in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "pnpm ready"; then
  fail "pnpm drift: printed 'pnpm ready' for a mismatched pnpm:\n${out}"
else
  pass "pnpm drift: does not report 'pnpm ready' for a mismatched pnpm"
fi

# --- Test 4: pnpm drift must name WHAT actually happened, not just THAT a
# mismatch happened, and must print a remedy (review follow-up on #2604).
# The old abort discarded `pnpm --version`'s stderr and named no fix at all —
# planting a stub that fails the way a corepack refusing/failing to fetch the
# pinned version would (stderr message, empty-string version on stdout) must
# surface that exact stderr text plus a `corepack prepare` remedy, not a bare
# "pnpm resolved to ''" with no explanation. ---
stub_dir="$(mktemp -d)"
make_stub_bin "${stub_dir}" "src-tauri"
cat > "${stub_dir}/pnpm" <<'EOF'
#!/usr/bin/env bash
echo "corepack: cannot fetch pnpm@10.34.5 (offline)" >&2
exit 1
EOF
chmod +x "${stub_dir}/pnpm"
out="$(PATH="${stub_dir}:${PATH}" BRINK_SETUP_FULL=1 BRINK_SETUP_AUDIT_TIMEOUT=1 bash "${script}" 2>&1)"
rc=$?
rm -rf "${stub_dir}"

if [ "${rc}" -ne 0 ]; then
  pass "pnpm drift (failed, not silent): script exits non-zero when pnpm --version itself fails (got ${rc})"
else
  fail "pnpm drift (failed, not silent): script exited 0 despite pnpm --version failing:\n${out}"
fi
if printf '%s' "${out}" | grep -q "cannot fetch pnpm@10.34.5 (offline)"; then
  pass "pnpm drift (failed, not silent): surfaces pnpm --version's own stderr instead of discarding it"
else
  fail "pnpm drift (failed, not silent): stub's stderr message missing from output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "corepack prepare \"pnpm@${pinned_pnpm_version}\" --activate"; then
  pass "pnpm drift (failed, not silent): names the remedy command"
else
  fail "pnpm drift (failed, not silent): no remedy command in output:\n${out}"
fi
if printf '%s' "${out}" | grep -qi "standalone pnpm"; then
  pass "pnpm drift (failed, not silent): hints at a standalone pnpm shadowing corepack's shim"
else
  fail "pnpm drift (failed, not silent): no shadowing-PATH hint in output:\n${out}"
fi

# =============================================================================
# Part 2 (#2591): end-to-end harness against a fresh, nothing-installed
# toolchain, plus per-step timeout coverage for every network fetch #2531/
# #2584 left unbounded.
# =============================================================================

# A restricted PATH for these tests, deliberately excluding this machine's
# REAL rustup/cargo-bin install (/root/.cargo/bin or equivalent) so that
# "rustup absent" scenarios actually exercise the script's install path
# instead of silently finding the real toolchain already on PATH further
# along $PATH. `node` stays reachable (needed to read the pnpm pin) via
# whatever standard location it's installed at on this machine.
safe_base_path="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
node_dir="$(dirname "$(command -v node)")"
case ":${safe_base_path}:" in
  *":${node_dir}:"*) ;;
  *) safe_base_path="${node_dir}:${safe_base_path}" ;;
esac

# The real `cargo` binary's absolute path, resolved once here (outside any
# PATH manipulation) so the `cargo` stub below can exec through to it for
# every subcommand it doesn't itself intercept (`install`) — in particular
# `cargo deny ...`, which real cargo dispatches to a `cargo-deny` plugin by
# searching PATH, and must still find the fake `cargo-deny` this harness
# plants in the stub dir.
real_cargo="$(command -v cargo)"

# Stub bin dir simulating a COMPLETELY fresh machine: no rustup, no
# wasm-pack, no wasm-opt, no cargo-nextest, no cargo-deny. Every network
# fetch setup-dev.sh performs is intercepted by a single `curl` stub
# (dispatching on the target URL) and a single `cargo` stub (intercepting
# `install`, exec-through otherwise), so the REAL script's tar/install/cargo
# dispatch logic runs against realistic — if fake — artifacts, not a
# reimplementation of the script's own logic.
#
# Every stubbed fetch defaults to succeeding immediately; set the matching
# HANG_* env var to 1 (read by the stub at RUN time, not baked in here) to
# make that one step sleep past its configured timeout instead:
#   HANG_RUSTUP_CURL, HANG_WASM_PACK_CURL, HANG_BINARYEN_CURL,
#   HANG_NEXTEST_CURL, HANG_CARGO_INSTALL_WASM_PACK,
#   HANG_CARGO_INSTALL_NEXTEST, HANG_CARGO_INSTALL_DENY
#
# `preseed_rustup=1` drops a working `rustup` stub straight into the stub
# dir (bypassing the curl-based install flow entirely) for tests that are
# about some OTHER step and want rustup out of the way with minimal fuss.
make_full_stub_bin() {
  local dir="$1"
  local preseed_rustup="$2"

  mkdir -p "${dir}"

  if [ "${preseed_rustup}" = "1" ]; then
    cat > "${dir}/rustup" <<'EOF'
#!/usr/bin/env bash
case "$1" in
  --version) echo "rustup 1.0.0" ;;
  *) exit 0 ;;
esac
EOF
    chmod +x "${dir}/rustup"
  fi

  # Dispatches on the URL (its last argument in every call site this script
  # makes) and either sleeps (HANG_* toggle) or emits a small, REAL tarball
  # (or, for the rustup case, a tiny installer script) containing a working
  # fake binary — so the script's own tar/install steps do real work against
  # it instead of the stub pre-empting them.
  cat > "${dir}/curl" <<'EOF'
#!/usr/bin/env bash
url="${*: -1}"
case "${url}" in
  *sh.rustup.rs*)
    if [ "${HANG_RUSTUP_CURL:-0}" = "1" ]; then sleep 5; exit 0; fi
    # A fake "installer script" for `sh -s -- ...` to run: plants a working
    # stub `rustup` and a PATH-extending env file under $HOME, simulating a
    # successful rustup-init run with no real network activity.
    cat <<'RUSTUP_INSTALLER'
#!/bin/sh
mkdir -p "$HOME/.cargo/bin"
cat > "$HOME/.cargo/bin/rustup" <<'INNER'
#!/usr/bin/env bash
case "$1" in
  --version) echo "rustup 1.0.0" ;;
  *) exit 0 ;;
esac
INNER
chmod +x "$HOME/.cargo/bin/rustup"
printf 'export PATH="%s/.cargo/bin:$PATH"\n' "$HOME" > "$HOME/.cargo/env"
RUSTUP_INSTALLER
    ;;
  *wasm-pack-v0.14.0*)
    if [ "${HANG_WASM_PACK_CURL:-0}" = "1" ]; then sleep 5; exit 0; fi
    t="$(mktemp -d)"
    mkdir -p "${t}/wasm-pack-v0.14.0-x86_64-unknown-linux-musl"
    cat > "${t}/wasm-pack-v0.14.0-x86_64-unknown-linux-musl/wasm-pack" <<'WP'
#!/usr/bin/env bash
echo "wasm-pack 0.14.0"
WP
    chmod +x "${t}/wasm-pack-v0.14.0-x86_64-unknown-linux-musl/wasm-pack"
    tar -C "${t}" -czf - "wasm-pack-v0.14.0-x86_64-unknown-linux-musl"
    rm -rf "${t}"
    ;;
  *binaryen-version_117*)
    if [ "${HANG_BINARYEN_CURL:-0}" = "1" ]; then sleep 5; exit 0; fi
    # Real binaryen releases nest as "binaryen-<version>/bin/wasm-opt"; the
    # script's `tar --strip-components=1` strips exactly that outer version
    # directory, expecting "bin/wasm-opt" to remain. Match that two-level
    # shape here rather than a flat "bin/wasm-opt" — a flat tarball would
    # strip "bin" itself instead, breaking the install step below it in a way
    # that's specific to this fake tarball, not the real one.
    t="$(mktemp -d)"
    mkdir -p "${t}/binaryen-version_117/bin"
    cat > "${t}/binaryen-version_117/bin/wasm-opt" <<'WO'
#!/usr/bin/env bash
echo "wasm-opt version 117"
WO
    chmod +x "${t}/binaryen-version_117/bin/wasm-opt"
    tar -C "${t}" -czf - "binaryen-version_117"
    rm -rf "${t}"
    ;;
  *get.nexte.st*)
    if [ "${HANG_NEXTEST_CURL:-0}" = "1" ]; then sleep 5; exit 0; fi
    t="$(mktemp -d)"
    cat > "${t}/cargo-nextest" <<'NX'
#!/usr/bin/env bash
echo "cargo-nextest 0.0.0"
NX
    chmod +x "${t}/cargo-nextest"
    tar -C "${t}" -czf - "cargo-nextest"
    rm -rf "${t}"
    ;;
  *)
    echo "stub curl: unrecognized URL: ${url}" >&2
    exit 1
    ;;
esac
EOF
  chmod +x "${dir}/curl"

  # Intercepts `cargo install <tool> ...` (the from-source fallback path for
  # wasm-pack/cargo-nextest/cargo-deny); execs through to the REAL cargo
  # binary for every other subcommand (in particular `cargo deny ...`, whose
  # plugin dispatch must still find this dir's fake `cargo-deny`).
  cat > "${dir}/cargo" <<EOF
#!/usr/bin/env bash
if [ "\${1:-}" = "install" ]; then
  target_bin="\${CARGO_HOME:-\$HOME/.cargo}/bin"
  mkdir -p "\${target_bin}"
  case " \$* " in
    *" wasm-pack "*)
      if [ "\${HANG_CARGO_INSTALL_WASM_PACK:-0}" = "1" ]; then sleep 5; exit 0; fi
      printf '#!/usr/bin/env bash\necho "wasm-pack 0.14.0"\n' > "\${target_bin}/wasm-pack"
      chmod +x "\${target_bin}/wasm-pack"
      exit 0
      ;;
    *" cargo-nextest "*)
      if [ "\${HANG_CARGO_INSTALL_NEXTEST:-0}" = "1" ]; then sleep 5; exit 0; fi
      printf '#!/usr/bin/env bash\necho "cargo-nextest 0.0.0"\n' > "\${target_bin}/cargo-nextest"
      chmod +x "\${target_bin}/cargo-nextest"
      exit 0
      ;;
    *" cargo-deny "*)
      if [ "\${HANG_CARGO_INSTALL_DENY:-0}" = "1" ]; then sleep 5; exit 0; fi
      {
        printf '#!/usr/bin/env bash\n'
        printf 'for a in "\$@"; do [ "\$a" = "--version" ] && { echo "cargo-deny 0.19.8"; exit 0; }; done\n'
        printf 'exit 0\n'
      } > "\${target_bin}/cargo-deny"
      chmod +x "\${target_bin}/cargo-deny"
      exit 0
      ;;
  esac
fi
exec "${real_cargo}" "\$@"
EOF
  chmod +x "${dir}/cargo"

  cat > "${dir}/corepack" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
  chmod +x "${dir}/corepack"

  cat > "${dir}/pnpm" <<EOF
#!/usr/bin/env bash
echo "${pinned_pnpm_version}"
EOF
  chmod +x "${dir}/pnpm"
}

# Runs the real script against `make_full_stub_bin`'s fresh-machine stubs.
# Isolates $HOME to a scratch dir per run (so the rustup-absent install path
# plants its fake binary somewhere private, never the real $HOME) and uses
# `safe_base_path` instead of the ambient $PATH (so this machine's real
# rustup/cargo can't be found ahead of — or behind — the stub dir).
run_full_script() {
  local preseed_rustup="$1"
  shift
  local stub_dir fake_home cargo_bin_dir out rc
  stub_dir="$(mktemp -d)"
  fake_home="$(mktemp -d)"
  # setup-dev.sh installs into "${CARGO_HOME:-$HOME/.cargo}/bin" and assumes
  # that directory is already on PATH (true on any real machine that already
  # has rustup, via its shell-profile PATH line) — pre-create and pre-PATH it
  # here so the `preseed_rustup=1` tests (which skip the rustup-install
  # branch, and with it the PATH-extending `source $HOME/.cargo/env` line)
  # see the same assumption hold, and freshly-installed tools are found by
  # later `command -v` / cargo-plugin-dispatch checks in the SAME run.
  cargo_bin_dir="${fake_home}/.cargo/bin"
  mkdir -p "${cargo_bin_dir}"
  make_full_stub_bin "${stub_dir}" "${preseed_rustup}"

  out="$(env -i \
    PATH="${stub_dir}:${cargo_bin_dir}:${safe_base_path}" \
    HOME="${fake_home}" \
    CARGO_HOME="" \
    "$@" \
    bash "${script}" 2>&1)"
  rc=$?

  rm -rf "${stub_dir}" "${fake_home}"
  printf '%s\n' "${out}"
  return "${rc}"
}

# --- Test 5: full end-to-end run against a completely fresh, nothing-
# installed toolchain (rustup, wasm-pack, wasm-opt, cargo-nextest, cargo-deny
# all absent) must complete successfully, installing everything via the
# stubbed network fetches, reaching pnpm and printing the final summary. This
# is the "drive the script end to end against stubbed tools" harness #2591
# asks for — every earlier test in this file exercises one branch in
# isolation; this one proves the branches compose into a working run. ---
out="$(run_full_script 0 BRINK_SETUP_FULL=1)"
rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "full e2e (fresh machine): script exits 0"
else
  fail "full e2e (fresh machine): script exited ${rc}:\n${out}"
fi
if printf '%s' "${out}" | grep -qF "Installed prebuilt wasm-pack 0.14.0"; then
  pass "full e2e (fresh machine): wasm-pack installed from the stubbed prebuilt tarball"
else
  fail "full e2e (fresh machine): missing wasm-pack install confirmation in output:\n${out}"
fi
if printf '%s' "${out}" | grep -qF "Installed wasm-opt version 117"; then
  pass "full e2e (fresh machine): wasm-opt installed from the stubbed binaryen tarball"
else
  fail "full e2e (fresh machine): missing wasm-opt install confirmation in output:\n${out}"
fi
# The nextest/cargo-deny fresh-install paths print no "installed" confirmation
# of their own (only wasm-pack's block does, on the prebuilt-fetch branch) —
# so check the toolchain-verification block's per-tool status lines instead,
# which is the strongest available signal that every tool this run installed
# actually landed on PATH by the time the script finished.
for tool in wasm-pack wasm-opt cargo-nextest pnpm; do
  if printf '%s' "${out}" | grep -qE "^[[:space:]]*${tool}[[:space:]]+OK$"; then
    pass "full e2e (fresh machine): toolchain verification reports ${tool} OK"
  else
    fail "full e2e (fresh machine): toolchain verification does not report ${tool} OK:\n${out}"
  fi
done
if printf '%s' "${out}" | grep -qF "pnpm ready"; then
  pass "full e2e (fresh machine): reaches 'pnpm ready'"
else
  fail "full e2e (fresh machine): missing 'pnpm ready' in output:\n${out}"
fi

# --- Test 6: the rustup installer curl|sh pipeline hanging must TIME OUT and
# exit non-zero — this is a hard blocker (nothing else in the script works
# without cargo/rustc on PATH). ---
out="$(run_full_script 0 HANG_RUSTUP_CURL=1 BRINK_SETUP_RUSTUP_TIMEOUT=1)"
rc=$?
if [ "${rc}" -ne 0 ]; then
  pass "rustup install timeout: script exits non-zero (got ${rc})"
else
  fail "rustup install timeout: script exited 0 — the hang was not detected:\n${out}"
fi
if printf '%s' "${out}" | grep -q "rustup install TIMED OUT"; then
  pass "rustup install timeout: prints a TIMED OUT message"
else
  fail "rustup install timeout: no 'TIMED OUT' in output:\n${out}"
fi

# --- Test 7: the wasm-pack tarball fetch hanging must WARN and fall back to
# a from-source `cargo install`, which succeeds — the script must still
# complete (rc 0). ---
out="$(run_full_script 1 HANG_WASM_PACK_CURL=1 BRINK_SETUP_WASM_PACK_TIMEOUT=1)"
rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "wasm-pack fetch timeout (fallback succeeds): script exits 0"
else
  fail "wasm-pack fetch timeout (fallback succeeds): script exited ${rc}:\n${out}"
fi
if printf '%s' "${out}" | grep -q "Prebuilt wasm-pack unavailable (fetch failed or timed out after 1s)"; then
  pass "wasm-pack fetch timeout (fallback succeeds): names the timeout and falls back"
else
  fail "wasm-pack fetch timeout (fallback succeeds): no fallback message in output:\n${out}"
fi

# --- Test 8: the wasm-pack tarball fetch AND the from-source `cargo install`
# fallback both hanging must TIME OUT the fallback and exit non-zero —
# matches this fallback's pre-existing behavior (no further fallback existed
# before this change either; an unguarded `cargo install` failure was already
# fatal under `set -euo pipefail`). ---
out="$(run_full_script 1 HANG_WASM_PACK_CURL=1 HANG_CARGO_INSTALL_WASM_PACK=1 BRINK_SETUP_WASM_PACK_TIMEOUT=1 BRINK_SETUP_CARGO_INSTALL_TIMEOUT=1)"
rc=$?
if [ "${rc}" -ne 0 ]; then
  pass "wasm-pack fetch + install both timeout: script exits non-zero (got ${rc})"
else
  fail "wasm-pack fetch + install both timeout: script exited 0 — the hang was not detected:\n${out}"
fi
if printf '%s' "${out}" | grep -q "cargo install wasm-pack TIMED OUT"; then
  pass "wasm-pack fetch + install both timeout: prints a TIMED OUT message for the install"
else
  fail "wasm-pack fetch + install both timeout: no TIMED OUT message in output:\n${out}"
fi

# --- Test 9: the binaryen/wasm-opt tarball fetch hanging must WARN and
# CONTINUE — this binary is a pure accelerator (wasm-pack downloads it
# itself otherwise), so it was already non-fatal before this change and a
# timeout must not gain new severity it didn't have. ---
out="$(run_full_script 1 HANG_BINARYEN_CURL=1 BRINK_SETUP_BINARYEN_TIMEOUT=1)"
rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "binaryen fetch timeout: script still exits 0 (non-fatal)"
else
  fail "binaryen fetch timeout: script exited ${rc} — a non-fatal fetch aborted the run:\n${out}"
fi
if printf '%s' "${out}" | grep -q "wasm-opt install failed or timed out after 1s"; then
  pass "binaryen fetch timeout: names the timeout"
else
  fail "binaryen fetch timeout: no timeout message in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "pnpm ready"; then
  pass "binaryen fetch timeout: script reached the pnpm section"
else
  fail "binaryen fetch timeout: script never reached the pnpm section:\n${out}"
fi

# --- Test 10: the get.nexte.st tarball fetch hanging must WARN and fall back
# to a from-source `cargo install`, which succeeds — the script must still
# complete (rc 0). ---
out="$(run_full_script 1 HANG_NEXTEST_CURL=1 BRINK_SETUP_NEXTEST_TIMEOUT=1)"
rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "nextest fetch timeout (fallback succeeds): script exits 0"
else
  fail "nextest fetch timeout (fallback succeeds): script exited ${rc}:\n${out}"
fi
if printf '%s' "${out}" | grep -q "Prebuilt cargo-nextest unavailable (fetch failed or timed out after 1s)"; then
  pass "nextest fetch timeout (fallback succeeds): names the timeout and falls back"
else
  fail "nextest fetch timeout (fallback succeeds): no fallback message in output:\n${out}"
fi

# --- Test 11: the get.nexte.st fetch AND its from-source `cargo install`
# fallback both hanging must TIME OUT the fallback and exit non-zero — same
# reasoning as Test 8 (nextest is required by the pump's per-round gate, and
# there was no softer pre-existing behavior to preserve). ---
out="$(run_full_script 1 HANG_NEXTEST_CURL=1 HANG_CARGO_INSTALL_NEXTEST=1 BRINK_SETUP_NEXTEST_TIMEOUT=1 BRINK_SETUP_CARGO_INSTALL_TIMEOUT=1)"
rc=$?
if [ "${rc}" -ne 0 ]; then
  pass "nextest fetch + install both timeout: script exits non-zero (got ${rc})"
else
  fail "nextest fetch + install both timeout: script exited 0 — the hang was not detected:\n${out}"
fi
if printf '%s' "${out}" | grep -q "cargo install cargo-nextest TIMED OUT"; then
  pass "nextest fetch + install both timeout: prints a TIMED OUT message for the install"
else
  fail "nextest fetch + install both timeout: no TIMED OUT message in output:\n${out}"
fi

# --- Test 12: the cargo-deny from-source install (BRINK_SETUP_FULL=1, no
# prebuilt binary ever exists for it) hanging must WARN and skip the audit,
# NOT abort the script — matches this install's pre-existing `|| echo ...
# skipping audit` non-fatal handling (cargo-deny is opt-in to begin with). ---
out="$(run_full_script 1 BRINK_SETUP_FULL=1 HANG_CARGO_INSTALL_DENY=1 BRINK_SETUP_CARGO_INSTALL_TIMEOUT=1)"
rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "cargo-deny install timeout: script still exits 0 (non-fatal)"
else
  fail "cargo-deny install timeout: script exited ${rc} — a non-fatal install aborted the run:\n${out}"
fi
if printf '%s' "${out}" | grep -q "cargo install cargo-deny TIMED OUT after 1s; skipping audit"; then
  pass "cargo-deny install timeout: names the timeout and skips the audit"
else
  fail "cargo-deny install timeout: no timeout message in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "pnpm ready"; then
  pass "cargo-deny install timeout: script reached the pnpm section"
else
  fail "cargo-deny install timeout: script never reached the pnpm section:\n${out}"
fi

if [ "${failures}" -gt 0 ]; then
  echo "${failures} failure(s)" >&2
  exit 1
fi
echo "all setup-dev.sh tests passed"
