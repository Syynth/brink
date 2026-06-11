# Publishing the npm packages

Two packages publish to npm under the `@brink-lang` org
(decision logged 2026-06-11, issue #148):

| Package | Workspace dir | Contents |
|---|---|---|
| `@brink-lang/web` | `packages/wasm` | wasm compiler/IDE/runtime + TS wrappers; `@brink/wasm-types` rolled into its declarations |
| `@brink-lang/studio` | `packages/brink-studio` | `mountStudio` IDE surface; bundles the private `@brink/*` internals; depends on `@brink-lang/web`; react/react-dom peers |

Everything else under `packages/` stays `private: true` and is never
published.

## The release flow (after first publish)

Versioning is driven by [changesets](https://github.com/changesets/changesets):

1. **Land a change** with a changeset: run `pnpm changeset` in the PR,
   pick the affected public package(s) and bump level, write a summary.
   Changes that don't touch the published packages need no changeset.
2. **Version PR** — on push to main, `.github/workflows/npm-release.yml`
   sees pending changesets and opens/updates a "Version Packages" PR that
   applies the bumps and writes `CHANGELOG.md`s.
3. **Merge the version PR** — the same workflow now finds no pending
   changesets, builds the wasm (`wasm-pack`), builds both packages
   (`pnpm run release`), and runs `changeset publish`, which publishes any
   version not yet on npm and pushes git tags.

The workflow gates publishing behind typecheck + vitest unit tests (e2e
runs in `ci.yml` on every PR, not in the release path).

## One-time npm setup

### 1. The org and package names

The `@brink-lang` org must exist on npmjs.com with you as owner. The
package *names* only come into existence at first publish — and npm
trusted publishing can't be configured for a package that doesn't exist
yet, so the **first publish is manual, from your machine** (see below).

### 2. First publish (manual, from each package dir)

From a clean checkout of main, logged in as an npmjs.com user with
publish rights in `@brink-lang` (`npm whoami` to check, `npm login` if
needed):

```sh
# Build the wasm bundle, install, build both packages
just wasm
pnpm install --frozen-lockfile
pnpm --filter @brink-lang/web build
pnpm --filter @brink-lang/studio build

# Publish @brink-lang/web first (studio depends on it).
# Use pnpm publish (NOT npm publish): it rewrites the workspace:^ range in
# studio's dependencies to the real version.
cd packages/wasm
npm_config_provenance=false pnpm publish --access public

cd ../brink-studio
npm_config_provenance=false pnpm publish --access public
```

Notes:

- `npm_config_provenance=false` overrides `publishConfig.provenance: true`
  in both package.jsons — provenance attestation only works from a
  supported CI; a local publish fails without the override.
- The repo carries a seed changeset (`.changeset/initial-release.md`)
  describing the 0.1.0 release. After the manual 0.1.0 publish, the next
  push to main will open a version PR bumping to 0.2.0 with that summary —
  delete the seed changeset first if 0.1.0 should remain current, or merge
  it when the next real release is ready.

### 3. Trusted publishing (preferred) or token fallback

**Trusted publishing (OIDC, no secrets):** once both packages exist, on
npmjs.com for *each* package: Package → Settings → Trusted Publisher →
GitHub Actions, with:

- Organization/user: `Syynth`
- Repository: `brink`
- Workflow filename: `npm-release.yml`
- Environment: (leave empty, or create one and mirror it in the workflow)

While there, set "Publishing access" to "Require two-factor authentication
or an automation or granular access token" (or disallow tokens entirely
once trusted publishing is proven). The workflow already requests
`id-token: write`, which also gives npm provenance attestations
(`publishConfig.provenance: true`).

**Token fallback:** create a granular automation token on npmjs.com with
read/write access to both `@brink-lang` packages and store it as the
`NPM_TOKEN` repository secret (Settings → Secrets and variables →
Actions). The workflow feeds it to npm via `NODE_AUTH_TOKEN` when present.
With trusted publishing configured, the secret can be removed.

## Local dry-runs

```sh
# What would a release contain?
pnpm changeset status

# Inspect the exact tarballs npm would publish
pnpm --filter @brink-lang/web build && cd packages/wasm && pnpm pack
pnpm --filter @brink-lang/studio build && cd packages/brink-studio && pnpm pack
tar -tzf brink-lang-web-*.tgz
```

## Related pipelines

- `release-plz.yml` — crates.io releases for the Rust crates.
- `release.yml` — cargo-dist installers (GitHub releases). The npm
  workflow is intentionally separate (`npm-release.yml`).
