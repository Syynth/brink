#!/usr/bin/env bash
# Regression test for scripts/refresh-excluded-lockfiles.sh's timeout
# handling (#2667).
#
# The two `cargo update` invocations in that script were BARE until #2667 —
# the same wedged-proxy hang class #2591/#2638/#2642 bounded in setup-dev.sh,
# sitting in a script scripts/check-scripts.mjs (then named scripts/check-setup-dev.mjs)
# never looked at.
#
# This harness follows scripts/setup-dev.test.sh's precedent exactly: it runs
# the REAL script against a PATH-injected `cargo` stub whose behaviour is
# chosen at RUN time by env toggles, rather than grepping the script's source.
# That distinction is the whole point — the recurring bug in this area is a
# *control-flow* bug (`rc=$?` read off a negated pipeline is always 0, so the
# `-eq 124` branch is unreachable dead code), and text that is present or
# absent cannot tell you whether the branch is reachable.
#
# Toggles read by the stub:
#   HANG_CARGO_UPDATE=1   sleep well past the configured bound
#   FAIL_CARGO_UPDATE=1   exit 101 immediately (a proxy REJECTING rather than
#                         stalling — the shape that must NOT be misreported
#                         as a timeout)
#
# Usage: bash scripts/refresh-excluded-lockfiles.test.sh

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${here}/.." && pwd)"
script="${repo_root}/scripts/refresh-excluded-lockfiles.sh"

failures=0
fail() {
  echo "FAIL: $1" >&2
  failures=$((failures + 1))
}
pass() {
  echo "ok - $1"
}

# A `cargo` stub that appends every invocation to $CARGO_CALL_LOG, so tests
# can assert on how MANY times cargo ran (does the loop abort on the first
# directory?) and on whether it ran at all (--print-lockfiles must invoke no
# cargo). Behaviour is read from the environment at run time, not baked in.
make_stub_bin() {
  local dir="$1"
  mkdir -p "${dir}"

  cat > "${dir}/cargo" <<'EOF'
#!/usr/bin/env bash
[ -n "${CARGO_CALL_LOG:-}" ] && echo "cargo $*" >> "${CARGO_CALL_LOG}"
if [ "${HANG_CARGO_UPDATE:-0}" = "1" ]; then sleep 30; exit 0; fi
if [ "${FAIL_CARGO_UPDATE:-0}" = "1" ]; then
  echo "stub cargo: simulated resolution failure" >&2
  exit 101
fi
exit 0
EOF

  chmod +x "${dir}/cargo"
}

# Runs the real script with a stubbed cargo and 1s bounds. Echoes combined
# output; returns the script's exit status. $1 is the script's own argument
# ("--dry-run", "--print-lockfiles", or "" for the real refresh); remaining
# args are NAME=VALUE toggles passed into the environment.
run_script() {
  local mode_arg="$1"
  shift

  local stub_dir
  stub_dir="$(mktemp -d)"
  make_stub_bin "${stub_dir}"

  local out rc=0
  if [ -n "${mode_arg}" ]; then
    out="$(cd "${repo_root}" && env "$@" \
      CARGO_CALL_LOG="${CARGO_CALL_LOG:-}" \
      PATH="${stub_dir}:${PATH}" \
      BRINK_REFRESH_DRY_RUN_TIMEOUT=1 BRINK_REFRESH_UPDATE_TIMEOUT=1 \
      bash "${script}" "${mode_arg}" 2>&1)" || rc=$?
  else
    out="$(cd "${repo_root}" && env "$@" \
      CARGO_CALL_LOG="${CARGO_CALL_LOG:-}" \
      PATH="${stub_dir}:${PATH}" \
      BRINK_REFRESH_DRY_RUN_TIMEOUT=1 BRINK_REFRESH_UPDATE_TIMEOUT=1 \
      bash "${script}" 2>&1)" || rc=$?
  fi

  rm -rf "${stub_dir}"
  printf '%s\n' "${out}"
  return "${rc}"
}

# --- Test 1: a hung --dry-run resolution must TIME OUT and exit non-zero ----
# The dry run is release-plz.yml's `verify-lockfile-refresh` job. If a stall
# there exited 0 the job would be vacuously green — it exists precisely to
# prove resolution still works (#1427).
CARGO_CALL_LOG="$(mktemp)"
start=$SECONDS
out="$(run_script --dry-run HANG_CARGO_UPDATE=1)"
rc=$?
elapsed=$((SECONDS - start))

if [ "${rc}" -ne 0 ]; then
  pass "dry-run hang: script exits non-zero (got ${rc})"
else
  fail "dry-run hang: script exited 0 — the stall was not detected:\n${out}"
fi
if printf '%s' "${out}" | grep -q "cargo update --dry-run TIMED OUT"; then
  pass "dry-run hang: prints a TIMED OUT message naming the dry-run step"
else
  fail "dry-run hang: no dry-run 'TIMED OUT' in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "BRINK_REFRESH_DRY_RUN_TIMEOUT"; then
  pass "dry-run hang: names the knob to raise"
else
  fail "dry-run hang: does not name BRINK_REFRESH_DRY_RUN_TIMEOUT:\n${out}"
fi
# The stub sleeps 30s; a 1s bound that actually fired returns in ~1s. This is
# what separates "the bound is wired to the fetching command" from "the text
# run_with_timeout appears somewhere on the line" — the lexical scan in
# check-scripts.mjs can only prove the latter, and says so in its header.
if [ "${elapsed}" -lt 15 ]; then
  pass "dry-run hang: the bound actually fired (${elapsed}s elapsed, stub sleeps 30s)"
else
  fail "dry-run hang: took ${elapsed}s — the 1s bound did not fire on the hung cargo"
fi
# 124 must propagate OUT of the `( cd "$dir" && run_with_timeout ... )`
# subshell, and the resulting exit must abort the loop rather than carrying on
# to the remaining two excluded dirs with a lockfile left unrefreshed.
calls=$(wc -l < "${CARGO_CALL_LOG}")
if [ "${calls}" -eq 1 ]; then
  pass "dry-run hang: aborts after the FIRST directory (1 cargo invocation)"
else
  fail "dry-run hang: cargo ran ${calls} times — the timeout did not abort the loop"
fi
rm -f "${CARGO_CALL_LOG}"
unset CARGO_CALL_LOG

# --- Test 2: a hung REAL refresh must TIME OUT, exit non-zero, and say the
# lockfile is unrefreshed. This is the release path: exiting 0 here would let
# release-plz.yml commit + push lockfiles that were never regenerated, which
# is the stale-lockfile release #1418 the script exists to prevent. ---
out="$(run_script "" HANG_CARGO_UPDATE=1)"
rc=$?

if [ "${rc}" -ne 0 ]; then
  pass "refresh hang: script exits non-zero (got ${rc})"
else
  fail "refresh hang: script exited 0 — a stalled refresh would be committed as if it had run:\n${out}"
fi
if printf '%s' "${out}" | grep -q "cargo update TIMED OUT"; then
  pass "refresh hang: prints a TIMED OUT message"
else
  fail "refresh hang: no 'TIMED OUT' in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "has NOT been refreshed"; then
  pass "refresh hang: warns the lockfile is unrefreshed (do not commit)"
else
  fail "refresh hang: no stale-lockfile warning in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "BRINK_REFRESH_UPDATE_TIMEOUT"; then
  pass "refresh hang: names the knob to raise"
else
  fail "refresh hang: does not name BRINK_REFRESH_UPDATE_TIMEOUT:\n${out}"
fi

# --- Test 3: a FAST-FAILING cargo (proxy rejects rather than stalls) must
# surface as an ordinary failure, NOT be misreported as a timeout, and must
# propagate cargo's own exit status. This is the twin FAIL_* shape that
# exposed the pipefail bug in setup-dev.sh's rustup step — a HANG_* toggle
# alone never exercises this path. ---
out="$(run_script --dry-run FAIL_CARGO_UPDATE=1)"
rc=$?

if [ "${rc}" -eq 101 ]; then
  pass "fast failure: propagates cargo's own exit status (101)"
else
  fail "fast failure: script exited ${rc}, expected cargo's 101:\n${out}"
fi
if printf '%s' "${out}" | grep -q "TIMED OUT"; then
  fail "fast failure: misreported an immediate failure as a TIMEOUT:\n${out}"
else
  pass "fast failure: does not claim a timeout"
fi
if printf '%s' "${out}" | grep -q "cargo update failed (exit 101)"; then
  pass "fast failure: names the real exit status"
else
  fail "fast failure: no 'cargo update failed (exit 101)' in output:\n${out}"
fi
if printf '%s' "${out}" | grep -q "simulated resolution failure"; then
  pass "fast failure: surfaces cargo's own stderr rather than swallowing it"
else
  fail "fast failure: cargo's stderr missing from output:\n${out}"
fi

# --- Test 4: the happy path still visits every excluded dir and exits 0 —
# the bound must not have changed what the script DOES when cargo behaves. ---
CARGO_CALL_LOG="$(mktemp)"
out="$(run_script --dry-run)"
rc=$?
calls=$(wc -l < "${CARGO_CALL_LOG}")

if [ "${rc}" -eq 0 ]; then
  pass "happy path: exits 0"
else
  fail "happy path: exited ${rc}:\n${out}"
fi
expected_dirs=$(bash "${script}" --print-lockfiles | wc -l)
if [ "${calls}" -eq "${expected_dirs}" ]; then
  pass "happy path: ran cargo once per excluded dir (${calls})"
else
  fail "happy path: ran cargo ${calls} times for ${expected_dirs} excluded dirs"
fi
if grep -q -- "--dry-run" "${CARGO_CALL_LOG}"; then
  pass "happy path: --dry-run reaches cargo (the bound did not eat the flag)"
else
  fail "happy path: cargo never saw --dry-run:\n$(cat "${CARGO_CALL_LOG}")"
fi
if grep -q -- "-p brink" "${CARGO_CALL_LOG}"; then
  pass "happy path: the -p package args survive the wrapper"
else
  fail "happy path: no '-p brink…' args reached cargo:\n$(cat "${CARGO_CALL_LOG}")"
fi
rm -f "${CARGO_CALL_LOG}"
unset CARGO_CALL_LOG

# --- Test 5: --print-lockfiles must still invoke no cargo at all. It runs in
# release-plz.yml purely to enumerate paths for `git add`; making it network-
# dependent would be a regression the timeout work could easily introduce. ---
CARGO_CALL_LOG="$(mktemp)"
out="$(run_script --print-lockfiles)"
rc=$?
calls=$(wc -l < "${CARGO_CALL_LOG}")

if [ "${rc}" -eq 0 ] && [ "${calls}" -eq 0 ]; then
  pass "--print-lockfiles: exits 0 and invokes no cargo"
else
  fail "--print-lockfiles: exit ${rc}, ${calls} cargo invocations:\n${out}"
fi
if printf '%s' "${out}" | grep -q "Cargo.lock"; then
  pass "--print-lockfiles: still prints lockfile paths"
else
  fail "--print-lockfiles: printed no lockfile paths:\n${out}"
fi
rm -f "${CARGO_CALL_LOG}"
unset CARGO_CALL_LOG

# --- Test 6: run_with_timeout must be VISIBLE inside the `( cd … && … )`
# subshell. A shell function is inherited by a subshell but NOT by `bash -c`,
# so this asserts the property the script's call sites depend on — and would
# go red if someone "simplified" the call sites into `bash -c`. ---
subshell_probe="$(
  # shellcheck source=lib/run-with-timeout.sh
  . "${repo_root}/scripts/lib/run-with-timeout.sh"
  (cd "${repo_root}" && run_with_timeout 5 true && echo VISIBLE) 2>&1
)"
if [ "${subshell_probe}" = "VISIBLE" ]; then
  pass "subshell: run_with_timeout is callable inside ( cd … && … )"
else
  fail "subshell: run_with_timeout not usable inside a subshell: ${subshell_probe}"
fi

subshell_rc=0
(
  # shellcheck source=lib/run-with-timeout.sh
  . "${repo_root}/scripts/lib/run-with-timeout.sh"
  set -e
  (cd "${repo_root}" && run_with_timeout 1 sleep 30)
) >/dev/null 2>&1 || subshell_rc=$?
if [ "${subshell_rc}" -eq 124 ]; then
  pass "subshell: a timeout's 124 propagates out of the subshell unchanged"
else
  fail "subshell: expected 124 out of the subshell, got ${subshell_rc}"
fi

echo
if [ "${failures}" -eq 0 ]; then
  echo "All refresh-excluded-lockfiles.sh tests passed."
  exit 0
fi
echo "${failures} test(s) FAILED." >&2
exit 1
