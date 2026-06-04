# Releasing brink

Brink's Rust crates are published to [crates.io] with [release-plz], and the
`brink-cli` / `brink-lsp` binaries are shipped to GitHub Releases with
[cargo-dist] (`dist`). This doc covers the one-time setup and the release flow.

## What gets published

crates.io flattens dependencies — there is no "private dependency". Every
front-door crate depends transitively into `crates/internal/`, so the whole
dependency closure must be on the registry. We publish the closure.

**Published (~20 crates):** the front doors `brink-runtime`, `bevy-brink`,
`brink-compiler`, `brink-cli`, `brink-lsp`, plus every internal crate in their
closures (`brink-format`, `brink-syntax`, `brink-ir`, `brink-analyzer`,
`brink-db`, `brink-driver`, `brink-fmt`, `brink-ide`, `brink-intl`,
`brink-json`, `brink-converter`, `brink-codegen-inkb`, `brink-codegen-json`,
`bevy-brink-derive`, `xliff2`).

The internal crates carry **no semver guarantees** — depend on the front doors,
not on these directly.

**Not published (`publish = false`):** `brink` (empty umbrella / name
reservation), `brink-web` (WASM → npm/JSR, separate pipeline),
`zed-brink` (editor extension), `brink-test-harness` (tests only).

Versioning is **unified**: all crates inherit `version.workspace = true`, and
internal deps are centralized in root `[workspace.dependencies]` with versions.
release-plz bumps the workspace version and those dep versions in lockstep.

## One-time setup

### 1. GitHub secrets

Add these in the repo settings (Settings → Secrets and variables → Actions):

- **`RELEASE_PLZ_TOKEN`** — a GitHub Personal Access Token (fine-grained: this
  repo, `contents: read/write`, `pull-requests: read/write`). **Required** —
  the default `GITHUB_TOKEN` cannot trigger another workflow, so tags pushed
  with it would *not* fire the cargo-dist `release.yml`. A PAT (or GitHub App
  token) makes the binary release fire.
- **`CARGO_REGISTRY_TOKEN`** — a crates.io API token (scope: publish-update /
  publish-new). Generate at <https://crates.io/settings/tokens>. **Needed only
  for the first release** (see below) — once Trusted Publishing is configured,
  delete this secret and the `CARGO_REGISTRY_TOKEN` line in `release-plz.yml`.

### 2. First release + Trusted Publishing (OIDC)

Steady-state publishing uses **crates.io Trusted Publishing**: the release job
mints a short-lived token over OIDC (`id-token: write`), so no registry secret
is stored. But Trusted Publishing **cannot create a brand-new crate** (crates.io
has no "pending publisher"), and all ~20 crates are new. So the first release is
token-based, then we switch to OIDC:

1. **First release** — with `CARGO_REGISTRY_TOKEN` set, either merge the first
   release PR (release-plz publishes the whole set) or do it by hand in one
   command — `cargo publish --workspace` packages every publishable crate and
   uploads them in dependency order, so the all-new-crates ordering is handled
   for you (`publish = false` crates are skipped automatically):

   ```sh
   cargo publish --workspace --dry-run   # verify the whole closure first
   cargo publish --workspace             # claim + publish all ~20 crates
   ```

   (`cargo publish --workspace` was stabilized in Rust 1.90; our pinned 1.95
   has it. It handles ordering only — release-plz still owns version bumps,
   changelogs, tags, and the OIDC flow for ongoing releases.)
2. **Register trusted publishers** — for *each* published crate, go to its
   crates.io page → Settings → Trusted Publishing → add: repository
   `syynth/brink`, workflow `release-plz.yml`, environment `crates-io`.
3. **Drop the token** — delete the `CARGO_REGISTRY_TOKEN` secret and the
   `CARGO_REGISTRY_TOKEN:` line in `release-plz.yml`. All later releases
   authenticate via OIDC with nothing stored.

### 3. cargo-dist (prebuilt binaries)

Already set up: `[workspace.metadata.dist]` in root `Cargo.toml` and the
generated `.github/workflows/release.yml` build the `brink` (from `brink-cli`)
and `brink-lsp` binaries for macOS (arm64/x64), Linux (x64), and Windows (x64),
plus shell/PowerShell installers.

The dist workflow is **decoupled from crate publishing** (`dispatch-releases =
true`): it's triggered manually via **workflow_dispatch**, not by tag pushes, so
release-plz's ~20 per-crate tags never collide with dist or hit GitHub's
3-tags-per-push limit.

Re-run `dist init`/`dist generate` only when changing targets/installers or
upgrading `dist` (the config drives everything — don't hand-edit `release.yml`,
or the `plan` CI check fails).

## Release flow

1. Land changes on `main` with [conventional commits] (`feat:`, `fix:`, …) —
   these drive the version bump and changelog.
2. The **release-plz** workflow opens/updates a "release PR" with the bumped
   versions + `CHANGELOG.md` updates. Review it.
3. Merge the release PR. release-plz publishes the changed crates to crates.io
   (over OIDC) in dependency order and pushes per-crate git tags + GitHub releases.
4. **To ship binaries** for that version: GitHub → **Actions** → **Release**
   workflow → **Run workflow**, and enter the tag (e.g. `v0.0.3`). cargo-dist
   builds the `brink` / `brink-lsp` binaries + installers and attaches them to a
   `v0.0.3` GitHub release. (Use the default `dry-run` to test the build without
   publishing a release.)

## Adding a new published crate later

1. Give it full package metadata (`description`, and inherit `keywords` /
   `categories` / `readme` from the workspace).
2. If anything depends on it, add it to root `[workspace.dependencies]` with a
   `version`, and have consumers use `<crate>.workspace = true`.
3. Make sure its *entire* dependency closure is also published (no
   `publish = false` crate in the closure).
4. If it ships a binary you want distributed, it's picked up by dist
   automatically; re-run `dist init` to refresh the workflow.

## Security & supply chain

The release path is hardened:

- **No long-lived registry secret** in steady state — crates.io Trusted
  Publishing (OIDC) mints a short-lived token per run.
- **Actions pinned to commit SHAs** (not mutable tags) across all workflows,
  with a trailing version comment. [Dependabot](../.github/dependabot.yml) bumps
  the pins (and cargo + npm deps) weekly.
- **Least-privilege permissions** — every workflow starts at `permissions: {}`
  and grants the minimum per job; checkouts use `persist-credentials: false`.
- **Environment gate** — the publish job runs in the `crates-io` environment
  (add required reviewers under Settings → Environments for manual approval).
- **Pinned toolchain** — [`rust-toolchain.toml`](../rust-toolchain.toml) fixes
  the Rust version for reproducible builds.
- **`cargo-deny`** runs in CI ([`deny.toml`](../deny.toml)): advisories,
  licenses, banned/wildcard crates, and a crates.io-only source allowlist.

### Dependency audit (baseline)

As of the initial setup, the resolved graph is **750 crates** (727 third-party).
Breadth is driven almost entirely by `bevy-brink` (~151 transitive deps; the
bevy/wgpu/winit ecosystem alone is ~88 crates) — the core is lean
(`brink-runtime` ~12, `brink-compiler` ~34).

- **Licenses:** 100% permissive (MIT/Apache dominant; Unicode-3.0, Zlib, BSD,
  ISC, CC0, etc.). No copyleft obligations — every crate offers a permissive
  option. Enforced by `deny.toml`'s allowlist.
- **Advisories:** 0 vulnerabilities. Three informational advisories, all
  transitive and accepted (ignored with rationale in `deny.toml`): `paste`
  unmaintained (RUSTSEC-2024-0436) and `rand` unsoundness under a custom-logger
  edge case (RUSTSEC-2026-0097).

Re-check anytime with `cargo deny check` (and `cargo audit` for the latest
advisory DB).

[crates.io]: https://crates.io
[release-plz]: https://release-plz.dev
[cargo-dist]: https://opensource.axo.dev/cargo-dist/
[conventional commits]: https://www.conventionalcommits.org
