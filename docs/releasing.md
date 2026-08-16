# Releasing brink

Brink's Rust crates are published to [crates.io] with [release-plz], and the
`brink-cli` / `brink-lsp` binaries are shipped to GitHub Releases with
[cargo-dist] (`dist`). This doc covers the one-time setup and the release flow.

## npm packages: lockstep versioning

The three published npm packages under `@brink-lang` (`web`, `editor`, `studio`)
use **lockstep versioning** via changesets' `fixed` group:
[`.changeset/config.json`](../.changeset/config.json) pins all three to a single
version across every release. This mirrors the Rust side, where `release-plz`
versions all crates in lockstep, and gives consumers a single version number for
the entire `@brink-lang` namespace.

On the next release after this setting lands, all three packages jump to the same
version (the next minor bump from the current max: e.g., from `web` 0.11.0 →
`0.12.0`). Every subsequent release bumps all three together; a package with no
changes in that release still gets the version bump and an empty/passthrough
changelog entry — that's the accepted cost of the lockstep guarantee.

## What gets published

crates.io flattens dependencies — there is no "private dependency". Every
front-door crate depends transitively into `crates/internal/`, so the whole
dependency closure must be on the registry. We publish the closure.

**Published (~20 crates):** the front doors `brink-runtime`, `bevy-brink`,
`brink-compiler`, `brink-cli`, `brink-lsp`, plus every internal crate in their
closures (`brink-format`, `brink-syntax`, `brink-ir`, `brink-analyzer`,
`brink-db`, `brink-driver`, `brink-fmt`, `brink-ide`, `brink-intl`,
`brink-codegen-inkb`, `bevy-brink-derive`, `xliff2`).

The internal crates carry **no semver guarantees** — depend on the front doors,
not on these directly.

**Not published (`publish = false`):** `brink` (empty umbrella / name
reservation), `brink-web` (WASM → npm/JSR, separate pipeline),
`brink-test-harness` (tests only).

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

   (`cargo publish --workspace` was stabilized in Rust 1.90; our pinned 1.97
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

> **⚠️ The step everyone forgets: a brand-new crate needs ONE manual publish.**
> crates.io **Trusted Publishing cannot create a crate** — it can only publish
> new *versions* of a crate that already exists. Since release-plz publishes over
> OIDC, it will **abort the entire workspace release** the first time it meets a
> publishable crate that crates.io has never heard of. Everything else stops
> shipping too, and because `npm-release` keeps succeeding, nothing looks broken.
> This has already cost ~20 consecutive failed releases (see #1232).

### Checklist

1. **Decide whether it should be published at all.** If nothing outside the
   workspace consumes it and it is not in a published crate's dependency
   closure, set `publish = false` and stop here (precedent:
   `brink-test-harness`). **But note:** `publish = false` is only legal if *no
   published crate depends on it* — crates.io flattens dependencies, so a
   published crate cannot depend on an unpublished one. Marking a crate
   `publish = false` while a published crate depends on it breaks `release-pr`
   with `failed to select a version for the requirement <crate> = "^X.Y.Z"`.
2. Give it full package metadata (`description`, and inherit `keywords` /
   `categories` / `readme` from the workspace).
3. If anything depends on it, add it to root `[workspace.dependencies]` with a
   `version`, and have consumers use `<crate>.workspace = true`.
4. Make sure its *entire* dependency closure is also published (no
   `publish = false` crate in the closure).
5. **Verify it actually packages** before merging:
   ```sh
   cargo package -p <crate>
   ```
   This builds the packaged tarball in isolation, exactly as `cargo publish`
   will. It is the only way to catch the workspace-vs-registry skew described
   under *Troubleshooting* below — the workspace build will happily stay green
   while the packaged build is broken.
6. **Publish it once, by hand**, from a maintainer machine with crates.io
   credentials (this cannot be automated — it is the whole point of the
   warning above):
   ```sh
   cargo login                      # once per machine; token from crates.io/settings/tokens
   cargo publish -p <crate>
   ```
   Publish in dependency order if you are adding several at once.
7. **Add it to the Trusted Publishing config** on crates.io (crate settings →
   Trusted Publishing) so subsequent releases can use OIDC.
8. If it ships a binary you want distributed, it's picked up by dist
   automatically; re-run `dist init` to refresh the workflow.

CI enforces step 6: the **`publishable`** job in `ci.yml` checks every crate with
`publish` unset/true against the crates.io API and fails the PR that introduces
an unpublished one. If that job fails, you are in exactly this situation.

## Troubleshooting a stuck release

Two failure signatures have actually happened; both stop *all* crate publishing.

### `Trusted Publishing tokens do not support creating new crates`

```
403 Forbidden: Trusted Publishing tokens do not support creating new crates.
Publish the crate manually, first
```

A new crate was added without the manual first publish. Fix: follow step 6
above. There is no automated workaround — a human with crates.io credentials
must run `cargo publish -p <crate>` once.

### `failed to verify package tarball` / `no field X on type Y`

```
error[E0609]: no field `types` on type `&mut AnalysisOptions`
error: failed to verify package tarball
```

**Workspace-vs-registry skew.** Inside the workspace, path dependencies resolve
to the local source, so everything compiles. `cargo publish` strips path
dependencies and resolves from crates.io instead — so the packaged crate builds
against the *last published* version of its workspace siblings. If the workspace
version was not bumped since a sibling's API changed (which happens whenever
releases have been failing for a while), the packaged build sees a stale API and
fails.

Fix, in order of preference:

1. **Let a release go out.** release-plz publishes in dependency order, so once
   the sibling publishes at the new version, the dependent packages cleanly.
   This is self-healing *provided the release is not also blocked by something
   else*.
2. **Cut the dependency.** If the crate does not really need its workspace
   sibling (e.g. a config parser depending on the analyzer only to fill in one
   struct field), inverting that dependency removes the whole class of problem
   and makes the crate publishable in isolation.

Detect it early with `cargo package -p <crate>` — see step 5.

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

The project audits **two separate dependency graphs** via `cargo-deny`:

1. **Root workspace** (enforced in CI): 762 crates resolved. Breadth is driven
   almost entirely by `bevy-brink` (~151 transitive deps; the bevy/wgpu/winit
   ecosystem alone is ~88 crates) — the core is lean (`brink-runtime` ~12,
   `brink-compiler` ~34).

   - **Licenses:** 100% permissive (MIT/Apache dominant; Unicode-3.0, Zlib,
     BSD, ISC, CC0, etc.). No copyleft obligations — every crate offers a
     permissive option. Enforced by `deny.toml`'s allowlist.
   - **Advisories:** three accepted, all transitive, each ignored with a
     rationale in `deny.toml` — so the job reports `advisories ok`. One is
     informational (`ttf-parser` unmaintained, RUSTSEC-2026-0192); the other
     two are quadratic-parsing DoS advisories (`quick-xml`,
     RUSTSEC-2026-0194/0195), accepted on `deny.toml`'s own reasoning that we
     parse only local XLIFF files, never attacker-controlled input.

2. **`packages/brink-desktop/src-tauri` workspace** (reported, non-required):
   451 crates resolved. This excluded workspace brings in additional dependencies
   for the Tauri desktop shell. The audit runs as the `cargo-deny (src-tauri)`
   step in [`.github/workflows/desktop-smoke.yml`](../.github/workflows/desktop-smoke.yml)
   under `continue-on-error: true`, so it reports without blocking; the
   governing text is `docs/desktop-shell-spec.md` § "Smoke-lane inputs and step
   gating".

   - **Licenses:** Includes 5 MPL-2.0 crates (cssparser, cssparser-macros,
     dtoa-short, option-ext, selectors) — copyleft obligations ruled as
     admitted as of 2026-08-15 (`docs/decision-log.md`).
   - **Advisories:** 16 unmaintained-crate RUSTSEC advisories, inherent to
     Tauri v2 on Linux (the gtk-rs GTK3 bindings, `proc-macro-error`, and five
     `unic-*` crates via `urlpattern` → `tauri-utils`); cargo-deny reports "no
     safe upgrade is available" for each. **Not ruled on** — reporting-only.
   - **Our own crate:** one `error[unlicensed]: brink-desktop = 0.1.0`. This is
     *our* `publish = false` crate, not an unlicensed third-party dependency;
     the documented fix is `[licenses] private.ignore`. Also not ruled on.
   - **Count:** 17 findings total after the MPL admission above (22 before it).
     Take the current number from a run rather than from this doc — see
     `docs/desktop-shell-spec.md` § "Smoke-lane inputs and step gating".

Re-check them anytime. One `deny.toml` governs both policies, but the two
graphs share no resolution, so each needs its own invocation — `cargo deny
check` at the root covers only the root `Cargo.lock`:

```sh
cargo deny check                                     # root workspace (CI-enforced)
cargo deny --manifest-path packages/brink-desktop/src-tauri/Cargo.toml \
  --all-features --locked check                      # src-tauri (reported only)
```

Note `--manifest-path`/`--all-features`/`--locked` are **top-level** cargo-deny
flags and must precede `check`. `scripts/setup-dev.sh` with `BRINK_SETUP_FULL=1`
runs both at CI's pinned cargo-deny version, each bounded by
`BRINK_SETUP_AUDIT_TIMEOUT` (default 300s) so a stalled RUSTSEC DB fetch can't
block setup indefinitely (#2531) — a timeout is reported distinctly from
normal findings and only aborts the script for the required root-workspace
audit, not the non-blocking `src-tauri` one. That knob is one of a family:
every network step in `setup-dev.sh` carries its own `BRINK_SETUP_*_TIMEOUT`
(#2584/#2591/#2638), some fatal on timeout and some warn-and-continue, and the
authoritative knob/default/fail-vs-warn table lives in that script's header
block. `cargo audit` remains useful for the latest advisory DB.

[crates.io]: https://crates.io
[release-plz]: https://release-plz.dev
[cargo-dist]: https://opensource.axo.dev/cargo-dist/
[conventional commits]: https://www.conventionalcommits.org
