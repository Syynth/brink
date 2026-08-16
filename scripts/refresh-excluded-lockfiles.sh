#!/usr/bin/env bash
# Refreshes the brink/bevy-brink path-dependency entries in the Cargo.lock of
# every workspace-excluded crate, so a workspace version bump (release-plz)
# doesn't leave them stale. Without this, the NEXT release would leave
# demos/compound and benchmarks/{gen-input,brink-loop} lockfiles pointing at
# a version older than what root Cargo.toml now specifies, and the
# `--locked` checks in demo.yml/benchmarks.yml would go red the moment a PR
# touches those crates (#1418, priority item 3 — release.yml is
# cargo-dist-generated, house rule 5, so this lives here instead).
#
# `cargo update -p <name>` (not a blanket `cargo update`) so only the
# brink/bevy-brink path-dependency entries actually re-resolve; each
# excluded crate's *external* deps (bevy, rand, ...) are left exactly as
# committed.
#
# Usage: refresh-excluded-lockfiles.sh [--dry-run|--print-lockfiles]
#
#   --dry-run          Uses `cargo update --dry-run`, which resolves and
#               reports what would change but never writes a Cargo.lock.
#               Lets this logic — the part release-plz.yml can't safely
#               exercise outside a real release, since the real step
#               pushes with a PAT — be run from a plain PR checkout with
#               no release branch, no git identity, and no push. It still
#               genuinely fails if a package can't be resolved (a bad path
#               dep, a version constraint that no longer matches, a
#               missing crate), which is the same failure mode the real
#               refresh would hit (#1427).
#
#   --print-lockfiles  Prints the Cargo.lock path for every excluded_dirs
#               entry below, one per line, and exits — no cargo invoked.
#               This is the single source of truth for *which* lockfiles
#               the refresh touches: release-plz.yml's `git diff`/`git add`
#               step reads this list instead of hardcoding the same three
#               paths a second time, so adding a fourth excluded_dirs entry
#               here can't silently go unreflected there (#1427 review).
#
# Callers: release-plz.yml (real refresh, then commits + pushes the diff —
# driving its `git diff`/`git add` off `--print-lockfiles`) and the
# `verify-lockfile-refresh` job in the same workflow (dry run only).
#
# Both `cargo update` invocations below hit the network (crates.io sparse
# index + dependency resolution) and are therefore bounded by
# run_with_timeout, the same wedged-proxy hang class #2591/#2638/#2642
# bounded in setup-dev.sh. They were left BARE until #2667 because
# scripts/check-scripts.mjs (then named scripts/check-setup-dev.mjs) only ever
# looked at setup-dev.sh — a whole script outside the scan. Knobs:
#
#   Knob                                     Default  On timeout
#   ---------------------------------------------------------------------
#   BRINK_REFRESH_DRY_RUN_TIMEOUT               180s   FAIL (exit 1). The
#                                                      --dry-run path only
#                                                      resolves — no crate
#                                                      downloads, no writes —
#                                                      so 180s is already
#                                                      generous for a cold
#                                                      sparse-index fetch,
#                                                      and this path runs on
#                                                      every PR touching the
#                                                      verify job, where a
#                                                      tight bound is cheap
#                                                      (a red job you re-run)
#                                                      and fast feedback is
#                                                      worth more.
#   BRINK_REFRESH_UPDATE_TIMEOUT                300s   FAIL (exit 1). The real
#                                                      refresh does the same
#                                                      resolution and then
#                                                      REWRITES three
#                                                      Cargo.locks, and it
#                                                      runs once per release
#                                                      where a spurious
#                                                      timeout aborts a
#                                                      release — so it gets
#                                                      the more generous
#                                                      bound.
#
# NEITHER step may warn-and-continue, which is why both say FAIL. A timeout
# that exits 0 would make the dry run vacuously green (its entire purpose is
# to prove resolution still works, #1427) and would make the real refresh
# commit-and-push lockfiles it never refreshed — a stale-lockfile release,
# the exact #1418 failure this script exists to prevent. "Bounded but exits
# 0" converts a visible hang into a silent wrong answer.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/run-with-timeout.sh
. "${here}/lib/run-with-timeout.sh"

BRINK_REFRESH_DRY_RUN_TIMEOUT="${BRINK_REFRESH_DRY_RUN_TIMEOUT:-180}"
BRINK_REFRESH_UPDATE_TIMEOUT="${BRINK_REFRESH_UPDATE_TIMEOUT:-300}"

excluded_dirs=(demos/compound benchmarks/tools/gen-input benchmarks/drivers/brink-loop)

mode=refresh
if [[ "${1:-}" == "--dry-run" ]]; then
  mode=dry-run
elif [[ "${1:-}" == "--print-lockfiles" ]]; then
  mode=print-lockfiles
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--dry-run|--print-lockfiles]" >&2
  exit 2
fi

if [[ "$mode" == print-lockfiles ]]; then
  for dir in "${excluded_dirs[@]}"; do
    echo "$dir/Cargo.lock"
  done
  exit 0
fi

for dir in "${excluded_dirs[@]}"; do
  pkgs=$(grep -E '^name = "(brink|bevy-brink)' "$dir/Cargo.lock" | sed -E 's/^name = "(.*)"$/\1/' | sort -u)
  args=()
  for pkg in $pkgs; do
    args+=(-p "$pkg")
  done

  # The bound goes INSIDE the subshell, not around it: `run_with_timeout` is
  # a shell function, and a subshell inherits functions (unlike `bash -c`,
  # which would not see it at all — and would additionally need an explicit
  # `-o pipefail`, since SHELLOPTS is not exported). Wrapping the subshell
  # from outside is not expressible anyway — run_with_timeout takes a command
  # and its args, not a compound statement. With the bound inside, `timeout`
  # is cargo's direct parent, so `-k 10`'s SIGKILL reaches the real process,
  # and the subshell's exit status IS the wrapped command's status, so a 124
  # propagates out unchanged. `scripts/refresh-excluded-lockfiles.test.sh`
  # proves that propagation against a sleeping stub rather than asserting it.
  #
  # `rc=0; … || rc=$?` — NOT `if ! …; then rc=$?`, which reads $? off the
  # negated pipeline and is therefore always 0, making the `-eq 124` branch
  # dead code (#2531/PR #2584). The `|| rc=$?` also suppresses `set -e`,
  # which would otherwise abort before the branch below could classify the
  # failure.
  rc=0
  if [[ "$mode" == dry-run ]]; then
    echo "==> cargo update --dry-run ${args[*]} (in $dir)"
    (cd "$dir" && run_with_timeout "${BRINK_REFRESH_DRY_RUN_TIMEOUT}" cargo update --dry-run "${args[@]}") || rc=$?
    if [[ "$rc" -eq 124 ]]; then
      echo "==> ✗ cargo update --dry-run TIMED OUT after ${BRINK_REFRESH_DRY_RUN_TIMEOUT}s in $dir — the crates.io index fetch/resolution never completed, likely a stalled proxy. Retry when network is stable, or raise BRINK_REFRESH_DRY_RUN_TIMEOUT." >&2
      exit 1
    fi
  else
    echo "==> cargo update ${args[*]} (in $dir)"
    (cd "$dir" && run_with_timeout "${BRINK_REFRESH_UPDATE_TIMEOUT}" cargo update "${args[@]}") || rc=$?
    if [[ "$rc" -eq 124 ]]; then
      echo "==> ✗ cargo update TIMED OUT after ${BRINK_REFRESH_UPDATE_TIMEOUT}s in $dir — the crates.io index fetch/resolution never completed, likely a stalled proxy. $dir/Cargo.lock has NOT been refreshed; committing now would ship a stale lockfile (#1418). Retry when network is stable, or raise BRINK_REFRESH_UPDATE_TIMEOUT." >&2
      exit 1
    fi
  fi

  # Any other non-zero status is a genuine cargo failure (unresolvable path
  # dep, version constraint that no longer matches, missing crate — the
  # failure mode #1427 wants surfaced). `set -e` was suppressed above, so
  # re-raise it explicitly rather than falling through to the next directory.
  if [[ "$rc" -ne 0 ]]; then
    echo "==> ✗ cargo update failed (exit $rc) in $dir" >&2
    exit "$rc"
  fi
done
