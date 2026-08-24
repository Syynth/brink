#!/usr/bin/env bash
# Local desktop release — the repeatable fallback for when CI's macOS lane
# hangs in codesign (the 2026-08-24 outage: six runs dead at "replacing
# existing signature" while local signing took 0.7s).
#
# One command, no pasting:
#
#   scripts/release-desktop-local.sh [desktop-vX.Y.Z]
#
# The tag defaults to desktop-v<version> from the current checkout's
# tauri.conf.json. The script mirrors .github/workflows/desktop-release.yml's
# macOS lane + publish job step for step: build (signed) → notarize + staple
# the DMG → collect the tag's Linux artifacts from CI → normalize names →
# build latest.json with the TESTED manifest builder → create the release →
# verify every manifest URL resolves → publish the manifest to the
# desktop-latest alias.
#
# ── One-time setup (secrets stay on YOUR machine; this script never prints
#    them and the agent driving it never sees them) ──────────────────────
#   1. Developer ID cert in the login keychain (auto-discovered).
#   2. xcrun notarytool store-credentials brink-notary --key-id … --issuer …
#      --key /path/to/AuthKey.p8
#   3. Tauri updater private key at ~/.config/brink/updater.key
#      (plus updater.key.password beside it if the key has one).
#
# Timeouts: every network-touching step is bounded below with hardcoded
# generous ceilings (this is an interactive rescue path, not CI — no
# BRINK_*_TIMEOUT knob table on purpose; if a bound is wrong, fix it here).

set -euo pipefail

say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# Minimal local mirror of setup-dev.sh's run_with_timeout: bound a network
# command; degrade to unbounded with a warning when GNU timeout is absent.
run_with_timeout() {
  local secs="$1"; shift
  if command -v gtimeout >/dev/null 2>&1; then gtimeout "$secs" "$@"
  elif command -v timeout >/dev/null 2>&1; then timeout "$secs" "$@"
  else
    echo "warning: no timeout binary — running unbounded: $*" >&2
    "$@"
  fi
}

repo_root=$(git rev-parse --show-toplevel)
main_git=$(git -C "$repo_root" rev-parse --path-format=absolute --git-common-dir)
main_root=$(dirname "$main_git")

# ── Tag ──────────────────────────────────────────────────────────────
tag="${1:-}"
if [ -z "$tag" ]; then
  # No tag argument: default from the INVOKING checkout's version.
  conf="$repo_root/packages/brink-desktop/src-tauri/tauri.conf.json"
  [ -f "$conf" ] || die "no tag argument and no tauri.conf.json in this checkout — pass desktop-vX.Y.Z"
  tag="desktop-v$(node -e "console.log(require('$conf').version)")"
fi
# The version ALWAYS comes from the tag — the invoking checkout may be on
# any branch (it only needs to be somewhere in the repo).
version="${tag#desktop-v}"
case "$tag" in desktop-v*) ;; *) die "tag must look like desktop-vX.Y.Z, got: $tag";; esac
git -C "$main_root" rev-parse -q --verify "refs/tags/$tag" >/dev/null || die "tag $tag not found"
say "Releasing $tag (tag commit $(git -C "$main_root" rev-parse --short "$tag"))"

# ── Preflight: identity, notary profile, updater key ─────────────────
identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)
[ -n "$identity" ] && export APPLE_SIGNING_IDENTITY="$identity" \
  || die "no Developer ID Application identity in the keychain"
say "Signing as: $APPLE_SIGNING_IDENTITY"

run_with_timeout 60 xcrun notarytool history --keychain-profile brink-notary >/dev/null 2>&1 \
  || die "notary keychain profile 'brink-notary' missing — run the store-credentials one-time setup (header comment, step 2)"

key_file="$HOME/.config/brink/updater.key"
[ -f "$key_file" ] || die "updater key not found at $key_file (one-time setup step 3)"
TAURI_SIGNING_PRIVATE_KEY="$(cat "$key_file")"
export TAURI_SIGNING_PRIVATE_KEY
if [ -f "$key_file.password" ]; then
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$key_file.password")"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
fi

# ── Workspace: a pristine worktree at the tag ────────────────────────
ws="$main_root/.claude/worktrees/release-local"
if [ -d "$ws" ]; then
  if [ -n "$(git -C "$ws" status --porcelain)" ] \
     || [ "$(git -C "$ws" rev-parse HEAD)" != "$(git -C "$main_root" rev-parse "$tag^{commit}")" ]; then
    die "$ws exists but is dirty or not at $tag — remove it (git worktree remove --force) and re-run"
  fi
  say "Reusing workspace at $ws"
else
  say "Creating workspace at $ws"
  git -C "$main_root" worktree add "$ws" "$tag"
fi
cd "$ws"

say "Building the wasm package + verified install"
run_with_timeout 1800 wasm-pack build crates/brink-web --target web --out-dir www/pkg
run_with_timeout 1800 pnpm install:checked -- --frozen-lockfile

say "Building signed bundles (app,dmg) — this is the long part"
# Local CPU-bound release build; pnpm may still fetch through corepack.
run_with_timeout 5400 pnpm --filter @brink/desktop exec tauri build --bundles app,dmg

bundle_dir="$ws/packages/brink-desktop/src-tauri/target/release/bundle"
dmg=$(ls "$bundle_dir"/dmg/*.dmg 2>/dev/null | head -1)
[ -n "$dmg" ] || die "no DMG produced under $bundle_dir/dmg"

say "Notarizing $(basename "$dmg") (waits on Apple; bounded at 2h)"
run_with_timeout 7200 xcrun notarytool submit "$dmg" --keychain-profile brink-notary --wait
run_with_timeout 600 xcrun stapler staple "$dmg"
say "Gatekeeper verdict"
run_with_timeout 120 spctl -a -t open --context context:primary-signature -v "$dmg"

# ── Collect artifacts: local macOS + CI Linux (that lane always passed) ──
work=$(mktemp -d)
art="$work/artifacts"
mkdir -p "$art"
cp "$dmg" "$art/"
cp "$bundle_dir"/macos/*.app.tar.gz "$bundle_dir"/macos/*.app.tar.gz.sig "$art/"

say "Fetching the tag's Linux artifacts from CI"
run=$(run_with_timeout 120 gh run list --repo Syynth/brink --workflow=desktop-release.yml \
  --branch "$tag" --limit 5 --json databaseId --jq '.[0].databaseId // empty')
[ -n "$run" ] || die "no CI run found for $tag — the Linux lane must have run at this tag"
# The workflow names artifacts brink-desktop-<matrix.artifact>.
run_with_timeout 600 gh run download "$run" --repo Syynth/brink -n brink-desktop-linux-x86_64 -D "$art" \
  || die "brink-desktop-linux-x86_64 artifact missing on run $run"
find "$art" -mindepth 2 -type f -exec sh -c 'for f do mv -n "$f" "$1"; done' sh "$art" {} +
find "$art" -mindepth 1 -type d -empty -delete

# GitHub rewrites asset names on upload ([^A-Za-z0-9._-] -> '.'); normalize
# BEFORE the manifest sees them so both read the same bytes (the
# desktop-v0.1.0 dead-update-channel lesson, verbatim from the workflow).
for f in "$art"/*; do
  [ -f "$f" ] || continue
  b=$(basename "$f")
  n=$(printf '%s' "$b" | tr -c 'A-Za-z0-9._-' '.')
  [ "$b" = "$n" ] || mv -n "$f" "$art/$n"
done
say "Artifacts to publish"
ls -1 "$art"

say "Building latest.json (tested manifest builder — refuses unsigned/empty)"
node "$ws/packages/brink-desktop/scripts/build-update-manifest.mjs" \
  "$art" "$version" "$tag" "Syynth/brink" "$art/latest.json"
cat "$art/latest.json"

say "Creating release $tag"
prev=$(run_with_timeout 120 gh release list --repo Syynth/brink --limit 100 \
  --json tagName --jq "[.[] | select(.tagName | startswith(\"desktop-v\")) | select(.tagName != \"$tag\")][0].tagName // empty")
if run_with_timeout 60 gh release view "$tag" --repo Syynth/brink >/dev/null 2>&1; then
  die "release $tag already exists — delete the existing release first if this is a redo"
fi
if [ -n "$prev" ]; then
  run_with_timeout 600 gh release create "$tag" --repo Syynth/brink \
    --title "Brink Studio $tag" --generate-notes --notes-start-tag "$prev" "$art"/*
else
  run_with_timeout 600 gh release create "$tag" --repo Syynth/brink \
    --title "Brink Studio $tag" --notes "Brink Studio $tag (published locally)." "$art"/*
fi

say "Verifying every manifest URL resolves (the 0.1.0 lesson: fetch them)"
for url in $(node -e "
  const m=require('$art/latest.json');
  for (const p of Object.values(m.platforms)) console.log(p.url);
"); do
  code=$(run_with_timeout 60 curl -s -o /dev/null -w '%{http_code}' -IL "$url")
  [ "$code" = "200" ] || die "manifest URL not fetchable (HTTP $code): $url"
  echo "ok $code $url"
done

say "Publishing the manifest to the desktop-latest alias"
run_with_timeout 120 gh release view desktop-latest --repo Syynth/brink >/dev/null 2>&1 || \
  run_with_timeout 120 gh release create desktop-latest --repo Syynth/brink \
    --title "Desktop update channel" --latest=false \
    --notes "Permanent alias release: holds only latest.json, the manifest the shipped app polls."
run_with_timeout 300 gh release upload desktop-latest "$art/latest.json" --repo Syynth/brink --clobber

say "DONE — $tag is live, update channel refreshed"
