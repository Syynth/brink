# shellcheck shell=bash
# The ONE definition of `run_with_timeout`, sourced by every script under
# scripts/ that performs a network fetch (#2667), and — since #2677/#2678 —
# by the justfile's recipe bodies (`wasm`, `book-ts-check`, `book-assets`,
# `studio-build`) and by `benchmarks/setup.sh`.
#
# It lived inline in scripts/setup-dev.sh from #2531 until #2667, when
# scripts/refresh-excluded-lockfiles.sh needed the identical wrapper for its
# two `cargo update` invocations. A second copy would have been a second
# thing to keep in sync, and this area's whole history — #2591 → #2638 →
# #2642 → #2667 — is hand-maintained duplicates of "the network safety rule"
# drifting apart. So the definition moved here and both scripts source it.
#
# Usage, scripts/ callers (source relative to the sourcing file, then call):
#
#   here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
#   . "${here}/lib/run-with-timeout.sh"
#   run_with_timeout <timeout_seconds> <command> [args...]
#
# Usage, justfile recipes and benchmarks/setup.sh (source repo-root-relative):
#
#   . scripts/lib/run-with-timeout.sh
#   run_with_timeout <timeout_seconds> <command> [args...]
#
# The repo-root-relative form is correct ONLY under a repo-root cwd — that is
# a PRECONDITION on the caller, not something this file arranges. `just`
# recipes get it for free: `just` always cds to the justfile's own directory
# before running a recipe body, and the justfile lives at the repo root.
# `benchmarks/setup.sh` is NOT invoked from the repo root (`just
# cross-language-benchmark` runs `bash benchmarks/setup.sh` from wherever
# `just` was cd'd to, which is the same repo root, but the script is also
# runnable directly as `bash benchmarks/setup.sh` from any cwd) — it restores
# the precondition itself with `cd "$(dirname "$0")/.."` before sourcing this
# file. A caller added later that sources the repo-root-relative form without
# either guarantee will fail to find this file, or worse, source some other
# file at that relative path.
#
# Returns:
#   - 0   if the command succeeds
#   - 124 if the command timed out (GNU timeout's exit code)
#   - the command's own non-zero status if it failed normally
#
# CAPTURING THE STATUS. Callers run under `set -e`, so the status must be
# captured as `rc=0; run_with_timeout … || rc=$?`. NEVER
# `if ! run_with_timeout …; then rc=$?`, which reads $? off the negated
# pipeline and is therefore ALWAYS 0 — that made the `-eq 124` branch
# unreachable dead code once already (#2531/PR #2584).
#
# SUBSHELLS. This is a shell FUNCTION, so it is visible inside a subshell
# (`( cd dir && run_with_timeout … )`) — subshells inherit functions — and
# the subshell's exit status is the wrapped command's, so a 124 propagates
# out unchanged. It is NOT visible inside `bash -c`, which starts a fresh
# shell that inherits only the environment. If a call site ever does need
# `bash -c`, it must ALSO pass `-o pipefail` explicitly: SHELLOPTS is not
# exported, so the outer `set -o pipefail` does not carry into it, and a
# failing left-hand side of a pipe there is silently swallowed.
#
# Prefers GNU `timeout` (Linux), falling back to `gtimeout` (macOS + Homebrew
# coreutils) before degrading to no timeout protection at all. `-k 10` sends
# SIGKILL 10s after the initial SIGTERM, so a child wedged in a syscall (e.g.
# a proxied git fetch) that ignores SIGTERM still gets reaped instead of
# hanging the wrapper forever.
#
# DEGRADATION IS SILENT-ISH BY DESIGN: with neither binary on PATH the
# command runs UNBOUNDED after a printed warning. That is a real hole, but
# refusing to run at all would make every script here unusable on a machine
# without coreutils; the warning is the contract.
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
