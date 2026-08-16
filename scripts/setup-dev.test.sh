#!/usr/bin/env bash
# Regression test for scripts/setup-dev.sh's cargo-deny audit timeout
# handling (review follow-up on #2531, PR #2584).
#
# Runs the REAL script end to end (not a reimplementation) against a
# PATH-injected `cargo-deny` stub that sleeps past the configured bound, so a
# regression of the `audit_exit_code=$?` capture bug — `audit_exit_code=$?`
# inside `if ! run_with_timeout ...; then` always reads 0 off the negated `!`
# pipeline, so the `-eq 124` branch is unreachable dead code and a genuine
# timeout gets reported (and exits) as a normal audit finding instead — fails
# this test instead of silently passing. A pure string/grep match on the
# script source would not catch this: the bug is in *control flow*, not in
# text that's present or absent.
#
# Usage: bash scripts/setup-dev.test.sh

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${here}/.." && pwd)"
script="${repo_root}/scripts/setup-dev.sh"

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

  cat > "${dir}/pnpm" <<'EOF'
#!/usr/bin/env bash
echo "10.0.0"
EOF

  chmod +x "${dir}"/*
}

run_script() {
  local hang_which="$1"
  local stub_dir
  stub_dir="$(mktemp -d)"
  make_stub_bin "${stub_dir}" "${hang_which}"

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

if [ "${failures}" -gt 0 ]; then
  echo "${failures} failure(s)" >&2
  exit 1
fi
echo "all setup-dev.sh audit-timeout tests passed"
