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

set -euo pipefail

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

  if [[ "$mode" == dry-run ]]; then
    echo "==> cargo update --dry-run ${args[*]} (in $dir)"
    (cd "$dir" && cargo update --dry-run "${args[@]}")
  else
    echo "==> cargo update ${args[*]} (in $dir)"
    (cd "$dir" && cargo update "${args[@]}")
  fi
done
