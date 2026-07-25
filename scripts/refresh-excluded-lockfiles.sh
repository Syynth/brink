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
# Usage: refresh-excluded-lockfiles.sh [--dry-run]
#
#   --dry-run   Uses `cargo update --dry-run`, which resolves and reports
#               what would change but never writes a Cargo.lock. Lets this
#               logic — the part release-plz.yml can't safely exercise
#               outside a real release, since the real step pushes with a
#               PAT — be run from a plain PR checkout with no release
#               branch, no git identity, and no push. It still genuinely
#               fails if a package can't be resolved (a bad path dep, a
#               version constraint that no longer matches, a missing
#               crate), which is the same failure mode the real refresh
#               would hit (#1427).
#
# Callers: release-plz.yml (real refresh, then commits + pushes the diff)
# and the `verify-lockfile-refresh` job in the same workflow (dry run only).

set -euo pipefail

dry_run=false
if [[ "${1:-}" == "--dry-run" ]]; then
  dry_run=true
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--dry-run]" >&2
  exit 2
fi

excluded_dirs=(demos/compound benchmarks/tools/gen-input benchmarks/drivers/brink-loop)

for dir in "${excluded_dirs[@]}"; do
  pkgs=$(grep -E '^name = "(brink|bevy-brink)' "$dir/Cargo.lock" | sed -E 's/^name = "(.*)"$/\1/' | sort -u)
  args=()
  for pkg in $pkgs; do
    args+=(-p "$pkg")
  done

  if $dry_run; then
    echo "==> cargo update --dry-run ${args[*]} (in $dir)"
    (cd "$dir" && cargo update --dry-run "${args[@]}")
  else
    echo "==> cargo update ${args[*]} (in $dir)"
    (cd "$dir" && cargo update "${args[@]}")
  fi
done
