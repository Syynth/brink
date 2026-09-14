# Desktop shell spec — `brink-desktop` (Tauri)

**Status:** v1 ruled 2026-08-06 (`docs/decision-log.md`, "Desktop app v1: Tauri
shell, local build first; mobile deferred"). Revives ruling-ledger #28
(2026-03-17, "brink-studio standalone app uses Tauri"), which the ledger
flagged as built-by-nobody with no owning issue. This spec is the owner;
`docs/brink-studio-spec.md` §"Desktop app (Tauri)" and
`docs/studio-shell-spec.md`'s desktop mentions defer here.

## Scope ruling

**v1 is a local build.** No code signing, no notarization, no updater, no
multi-OS release matrix — a macOS dev build the maintainer runs locally.
Promotion to a distributable is a later, separate stage (D4) with its own
workflow file (never `release.yml` — cargo-dist owns that file and edits
break the release `plan` check).

**Mobile is recorded as an interest and explicitly deferred.** It shapes one
choice today: Tauri 2's iOS/Android targets keep a future mobile client on
this same stack and this same webview frontend, which is part of why the
shell choice stands. Nothing else in this spec may take a dependency that
forecloses mobile, and nothing may be added *for* mobile before it is
scheduled.

## Architecture (reaffirmed from the March ruling)

The webview runs the **existing studio frontend** — the same
`@brink-lang/studio` `mountStudio` build the playground uses — with the
**wasm backend** (`@brink-lang/web`). One integration path: browser,
embedded, and desktop all exercise the same `EditorSession`. A native
Rust core spoken to over Tauri IPC remains the *perf escape hatch only*,
explicitly out of v1 (and out of v2; it gets specced if a real project
measurably outgrows the wasm session, not before).

The shell is deliberately thin. Its entire job:

| Concern | Mechanism |
|---|---|
| Window, chrome, shortcuts | Tauri window + native menu bar |
| Open a project | native folder dialog → `TauriFileProvider` → `mountStudio` |
| File I/O | `TauriFileProvider` implementing `packages/ink-editor/src/provider.ts` |
| External edits | fs watcher → `onExternalChange` → the existing #320 conflict/merge UI |
| Project config | nothing — `ProjectSession` already discovers `brink.toml` and re-runs on every change (#2324) |
| Recent projects | `recents.json` in app-data (#2394): most-recent-first, capped at 10, deduplicated by exact path, pruned lazily on a failed open — never a proactive existence sweep |
| Export | `compile_project` bytes → native save dialog |

Everything below the shell — analysis, diagnostics, claiming, completions,
the Player — is the editor packages' business and is protected by the
editor acceptance gate (`crates/brink-web/src/editor/acceptance_gate.rs`).

## The one new component: `TauriFileProvider`

Implements the full `FileProvider` contract over Tauri's fs API. The
interface was designed for this (its doc comment names "Tauri/FS" as an
anticipated implementation); every method maps directly:

| Contract method | Tauri mapping | Notes |
|---|---|---|
| `listFiles()` | recursive dir walk | include `*.brink`, `*.ink`, `brink.toml`; skip dotfiles, `target/`, `node_modules/` |
| `readFile` / `requestFile` | `fs.readTextFile` | `requestFile` returns null for paths outside the project root — never escape the opened folder |
| `onFileChanged` | **buffer, don't write** | v1 keeps the studio's explicit-save model: dirty state lives in the editor; disk writes happen on `file.save`/`saveAll` via `requestSave`. Autosave ships unconditionally: a 2-minute ticker (`AUTOSAVE_MS`, `120_000` ms, a module-level exported constant in `packages/brink-desktop/src/main.tsx`) dispatches `file.saveAll` whenever dirty files exist, queuing behind any write already in flight (#2403). The ticker is armed when a project opens and cleared on all three teardown paths — project close, reopen (not app lifetime), and app quit (see `autosaveTimer` in `packages/brink-desktop/src/main.tsx`) — pinned by `packages/brink-desktop/src/__tests__/autosave-reopen.test.ts` (#2486, close/reopen) and `packages/brink-desktop/src/__tests__/autosave-quit.test.ts` (#2517, quit): the quit path clears the timer before awaiting the final save so a tick can never fire against a project mid-teardown. `AUTOSAVE_MS`'s exact value is pinned by the same reopen test (#2517) — it imports the constant rather than restating it, so a shortened interval fails the pin, not silently. A Settings surface can configure the interval in future versions; this is the intended extensibility point already noted in the code comment. This ticker is the production caller both #2435 and #2434's fixes exist to serve correctly |
| `createFile` / `deleteFile` / `renameFile` | `fs.writeTextFile` / `remove` / `rename` | `renameFile` is implemented natively (atomic) — `ProjectSession.renameFile`'s create+delete fallback for a provider lacking `renameFile` is dead code on this provider, since it always implements it. The native move is **not the whole op**: it carries the file's bytes, but the rename computed the moved file's own outbound `INCLUDE` rewrites into the `newContent` the contract hands over, so the provider writes that content at the new path straight after the move (#2425). Without it, a cross-directory rename leaves disk holding the moved file's own pre-rewrite `INCLUDE` paths at the new location until some unrelated edit dirties it — invisible in the studio (the session is correct) and wrong for anything reading disk directly, e.g. `brink compile`. This write closes that gap only for the moved file's own content, so disk at the new path agrees with the session; a referrer's rewritten `INCLUDE` (a file that pointed at the old path) is an ordinary edit that goes through `applyEdit` → `onFileChanged` and stays staged under D2, landing on disk only at the next `requestSave` — until then, disk can still disagree with the session for those referrer files. The follow-up write goes through the same serialized staged-write path as a save; see the `requestSave` row below for what a rejection does and does not do. Like every write behind `requestSave`, this follow-up write is temp-write-then-rename (#2445): an interruption can lose it (leaving the pre-rewrite content on disk at the new path) but never leaves the file half-written |
| `onExternalChange` | fs watcher on the project root | debounce; deliver `null` on delete; unsubscribe on teardown per the contract. This lights up the #320 conflict → kept-buffer → merge surface with a *real* watcher for the first time. A payload is **not always external**: `deleteFile`'s and `renameFile`'s own write-throughs echo back through the watcher too — a rename produces both a deletion echo (its old path) and a creation echo (its new path) — so self-write suppression (content match), self-delete suppression (a consumed-once `selfDeletes` marker keyed by path, #2404), and self-create suppression (a consumed-once `selfCreates` marker keyed by path, #2416) all run before a payload reaches the callback — only what survives all three is forwarded as genuinely external. **At most one marker is armed per path** (#2424): the watcher flushes at most one event per path per quiet window, so a marker armed while another is still outstanding could never be consumed — and an unconsumed `selfDeletes` goes on to swallow a genuinely external deletion. Every arming site therefore clears the other two kinds for that path, rather than leaving the outcome to whichever branch of `onExternalChange` checks first; a marker whose operation then rejects is disarmed too, since no echo will ever come for a write or delete that did not happen. `renameFile`'s follow-up content write is no exception: a rejection re-arms `selfCreates` for the destination path so the rename's own still-outstanding creation echo stays suppressed rather than reaching this callback with pre-rewrite bytes (#2438 review) |
| `requestSave` | write staged content | the `staged` map fed by `onFileChanged` (D2 overlay model) is the source of what to write — the #154 egress batch feeds the backup ring instead, orthogonal to dirty. `staged` is a provider-internal write queue, distinct from studio dirty state (`StudioPublicState.dirtyFiles`, computed by `FileChangeHub` from session content vs. baseline): a rename's own `record(newPath, "created")` already marks the moved file dirty the moment the session updates, independent of whether the provider's own follow-up content write (#2425) later succeeds or is rejected. Calls are serialized (#2403): an overlapping caller (the autosave ticker, a quit-time `saveAll`) queues behind whatever write is already in flight rather than racing it against the same `staged` snapshot — as does `renameFile`'s own follow-up content write, which is a staged write like any other. Quit-time `saveAll` is not always a single overlapping call: `awaitSaveAllBeforeQuit` re-dispatches it on an interval while the dirty set persists (#2434), so a hung write can see several of its own redispatches queue up behind it one after another — this same serialization is what keeps each one from racing the write ahead of it. A rejected write of this kind is retried only by the next UNNARROWED `requestSave` (the autosave ticker, `saveAll`) — a `file.save` narrowed to a different, currently-focused path does not touch it, since a narrowed `writeStaged` only writes the paths it is given. The canonical write underneath every one of these is temp-write-then-rename (#2445): an interrupted write can lose the last save but can never leave a half-written file on disk |

Path discipline: provider keys are project-relative with `/` separators
(the studio's convention); the provider owns the mapping to absolute OS
paths and never leaks them into the session.

Adding a save/retire path here (or anywhere else in `src-tauri`) is not
covered by the TS-only `SAVE_PATHS` enrolment guard — see
`docs/embedder-api.md`'s "Confirm and retire in ONE synchronous step"
section, "Enrolment blind spot — Rust-side save paths (issue #2545)".

## Workspace placement

`packages/brink-desktop/` — a small TS host package (the mount wrapper,
menu wiring, provider) plus `src-tauri/` (the Rust shell crate).

**The `src-tauri` crate is excluded from the root cargo workspace.** The
workspace already carries Bevy's dependency graph as its dominant build
cost; Tauri's graph is comparably heavy and would land in every
`cargo test --workspace`, every wave agent's shared target, and CI's
required lanes for zero coverage benefit (the shell has almost no logic).
Cost of exclusion: its dep versions are managed in its own `Cargo.toml`
rather than the workspace table — acceptable for a leaf artifact.

**Second cost, named late (#2415): lint policy does not cross the fence
either.** A crate inherits `[workspace.lints]` only as a workspace *member*,
and clippy stops searching for `clippy.toml` at the workspace root — which
this crate is. So the repo-wide `unwrap_used`/`expect_used`/`panic`/`todo`/
`print_stdout`/`print_stderr` denies and clippy pedantic had never once been
applied here, and `desktop-smoke.yml`'s `cargo clippy -- -D warnings` was
plain default clippy, not the repo's policy. The fix is duplication, since
`[lints] workspace = true` needs a parent to inherit from: `src-tauri`
carries its own `[lints]` table and its own `clippy.toml`, both byte-for-byte
copies of the root ones. **Keep all four files in sync** — the two
`*_matches_the_root_workspace` tests in `src/lib.rs` fail when either copy
drifts, and are the only thing that notices. If a shared lint-defaults file
is ever extracted, both sides should point at it and those tests should
follow.

**Third cost, named later (#2451): the lockfile does not cross the fence
either.** `src-tauri` has its own `Cargo.lock`, and `cargo test --locked` in
the smoke lane only proves that lock is internally consistent with the
`Cargo.toml` beside it — never that it still tracks the root workspace's
resolved versions. `dependency_versions_track_the_root_workspace` in
`src/lib.rs` closes that: for every crate BOTH manifests declare (today
`serde`, `serde_json`, `thiserror`), it fails when this lock is behind the
root's resolved version, and when a root major bump has no compatible copy
here at all. Scope is deliberately the declared overlap, not the whole
graph — the two dependency graphs resolve transitive crates differently for
legitimate reasons, and `src-tauri` depends on no first-party crate at all
(the compiler reaches this shell only through the wasm the webview loads).

**Fourth cost, named later (#2507): `run_cli`'s subcommand allowlist didn't
cross the fence either — RETIRED.** `ALLOWED_CLI_SUBCOMMANDS` hand-mirrored a
subset of `brink-cli`'s `clap` surface, and
`cli_allowlist_subcommands_exist_in_brink_cli_surface` closed the gap by
reading `crates/brink-cli/src/main.rs` as plain text. Both are gone with the
sidecar (`docs/desktop-ota-spec.md` Stage 1): there is no longer any
`brink-cli` surface this crate mirrors, so there is nothing for a fourth
cross-fence guard to guard. #2466 — whether a cross-workspace guard needs a
home with merge-blocking teeth — stays open on the three remaining ones.

CI in v1: none required. A non-required smoke job (`cargo check` the shell
crate + `pnpm build` the package) may be added if drift appears. The
required lanes must not grow a Tauri build.

Drift appeared (#2402: `src-tauri`'s `Cargo.toml` declares `edition =
"2021"`, but nothing pinned `rustfmt` to match, so it silently inherited the
root `rustfmt.toml`'s `edition = "2024"` and drifted uncaught). The
non-required smoke job now exists: `.github/workflows/desktop-smoke.yml`,
covering `cargo check`/`clippy` and `cargo test` in `src-tauri/` (still its
own excluded workspace — the job's cargo commands run with cwd inside it,
never from the repo root), `tsc --noEmit`, and `pnpm build` for the desktop
package. It is deliberately **not** in branch protection's required-checks
list, per the ruling above.

This is the desktop package's first *cargo/`tsc`/`pnpm build`* coverage, not
its first CI coverage of any kind: `.github/workflows/ci.yml`'s `frontend` job
already runs the desktop vitest suite on every PR (step "Unit tests (vitest,
`@brink/desktop`)" → `pnpm --filter @brink/desktop test`). That step builds no
Tauri graph, so it never violated the "required lanes must not grow a Tauri
build" fence, and the smoke lane deliberately does not duplicate it. The
desktop unit suite's only home is therefore that one step inside another job —
deleting it would drop the suite entirely, so `src-tauri`'s
`ci_workflow_still_runs_the_desktop_vitest_suite` test asserts the step is
still there (#2418). That assertion lives on the *cargo* side deliberately: a
test inside the vitest suite could not fail for its own removal.

### One alias map (#2418)

`packages/brink-desktop/alias-map.ts` is the single source of truth for the
package's module resolution. `vite.config.ts` and `vitest.config.ts` both
call `desktopAliases(__dirname)`; `tsconfig.json`'s `paths` is JSON and
cannot import, so it stays a copy that `src/__tests__/alias-map.test.ts`
compares against `desktopTsconfigPaths()`. Only `brink-web` differs between
consumers — bundlers resolve the ESM glue file, `tsc` needs the package
directory — and the entry carries both rather than leaving the divergence
implicit. Three hand-maintained copies of this map are what let most of the
unit suite stop running behind a green step (#2409).

### Alias map parity with the playground (#2450)

The invariant above is intra-package. A second invariant of the same
standing spans packages: `DESKTOP_ALIASES` is meant to mirror
`packages/brink-studio`'s own alias map — the "playground" — and until
#2450 that mirroring was an honour-system claim in a comment, checked by
nothing. `src/__tests__/playground-alias-parity.test.ts` now compares this
package's map against the playground's five hand-maintained copies —
`vite.config.ts` (both `serve` and `build`), `vite.config.embed.ts`,
`vitest.config.ts`, `tsconfig.json`'s `paths` and `tsconfig.build.json`'s
`paths` (the last against the narrower `DTS_ROLLUP_EXCLUDES` expectation) —
and fails on divergence. It loads the playground's config modules by calling
their exported factories rather than scraping their text, so it compares what
vite would actually resolve.

Since #2464 the playground owns the same copies from its own side, against
`packages/brink-studio/alias-map.ts` (`docs/brink-studio-spec.md` § "One
alias map, owned by this package"). That does not make this guard redundant:
a studio-side test can only see the studio's copies, so the relationship
between the two packages' maps is still checked only here.

The two maps are not required to be identical, and the guard names each
exception rather than treating a mismatch as automatic drift:

- **`DESKTOP_ONLY = ["@brink-lang/studio"]`** — the desktop shell aliases
  the studio package to workspace source; the studio cannot alias itself,
  so this one specifier is expected on the desktop side only.
- **The wasm pair is serve-only in the playground.** `brink-web` and
  `@brink-lang/web` are aliased under `command === "serve"` in
  `vite.config.ts` but dropped from the library build, which externalizes
  `@brink-lang/web` instead (`rollupOptions.external`) so the published
  npm bundle does not inline the wasm wrapper.
- **`vitest.config.ts` mocks `brink-web`.** The playground's unit suite
  runs under jsdom and must not touch real wasm-bindgen glue; the desktop
  suite resolves the real glue on purpose (`vitest.config.ts`'s own
  comment records why — the mock would make `export-artifact.test.ts`
  prove nothing about a compiled artifact).

Because the guard runs inside `pnpm --filter @brink/desktop test` — the
step `.github/workflows/ci.yml:668` runs as this package's required CI
gate — an alias edit that breaks the relationship reddens that step even
when the edit itself lives entirely in `packages/brink-studio`.

### Smoke-lane inputs and step gating (#2418)

Nine properties of `desktop-smoke.yml` and `desktop-bundle-smoke.yml` are
asserted by tests in `src-tauri/src/lib.rs` rather than left to review:

- **The `pull_request` path filter lists every input that can break the
  job**, not just the trees it checks: `pnpm-lock.yaml`, root `Cargo.toml`/
  `Cargo.lock`, `clippy.toml`, `rust-toolchain.toml`, `ci.yml`, (#2504)
  the `.github/workflows/**` glob, and (#2522) `deny.toml` — which is both
  the policy the `cargo-deny (src-tauri)` step resolves and the file
  `deny_toml_admits_mpl_for_the_transitive_tauri_dependencies` parses, so a
  PR editing only it must still trigger this lane — on top of the
  package/crate globs — the
  individual `ci.yml` entry alone left a reordered `npm-release.yml`, or a
  brand-new workflow file, free to skip this lane on the PR that broke it.
  Without these a lockfile or root-lint-policy change
  ran this lane only on the post-merge push to `main` — including the two
  `*_matches_the_root_workspace` drift tests, which could not fail the PR
  that caused the drift.
  (`desktop_smoke_path_filter_covers_its_shared_inputs`) `crates/brink-cli/**`
  was one of those crate globs until #2477, when stubbing the sidecar build
  left nothing in the lane reading `brink-cli` source; with the sidecar
  deleted outright (`docs/desktop-ota-spec.md` Stage 1) that is now true by
  construction, and the same test asserts the entry stays **absent**.
- **Checks are non-blocking for their siblings but gated on their setup
  steps.** A bare `if: '!cancelled()'` also overrides the implicit
  `success()` on a failed *prerequisite*, so a dying setup step let the
  dependent steps run and fail too, burying the root cause. Each check now
  reads `!cancelled() && steps.<setup>.outcome == 'success'` for the setup
  steps it needs (`checkout`, `linux_deps`, `wasm_build`, `pnpm_install`,
  `check_wasm_pkg`); the format check needs only the runner's
  toolchain and the checkout, so it is gated on `checkout` alone —
  `actions/checkout` carries no `id` by default, so this lane gives its
  checkout step one. (`check_wasm_pkg` (#2514) is itself gated on
  `pnpm_install`, not a setup step other checks read directly — "Typecheck
  (tsc --noEmit)" and "pnpm build" gate on `check_wasm_pkg` instead of
  `pnpm_install` because `pnpm install --frozen-lockfile`'s own exit code
  can report success even when the `file:` link it creates silently failed
  to resolve; `check_wasm_pkg` verifies the resolved link itself, so
  `pnpm_install` is no longer a direct prerequisite of any check.)
  `desktop_smoke_gates_dependent_steps_on_setup_success` checks both the
  `if:` text and that every prerequisite id it names still names a real
  step, since a stale id (e.g. from a renamed or `id:`-stripped setup step)
  reads as `steps.<id>.outcome == ''` and the guard is simply always false.
- **The lane's `wasm-pack build` is deliberately a release build.** PR
  #2446 set `CARGO_PROFILE_RELEASE_OPT_LEVEL` / `_DEBUG` / `_CODEGEN_UNITS`
  job-wide to flatten the (now-deleted) `brink-cli` sidecar build, and in
  doing so was also flattening "Build brink-web wasm package", this lane's
  largest build. Those vars are gone. #2482 asked whether the wasm build
  should get the stub treatment the sidecar's staged file got; it should not
  (#2502) — unlike that file, whose content was never read, the wasm-pack
  output is genuinely consumed by "Typecheck (tsc --noEmit)" and `pnpm
  build` below, so nothing can stand in for it. The release-vs-dev
  optimisation level and reusing `ci.yml`'s own artefact remain open,
  tracked by #2482.
- **This workspace's dependency graph is audited here, and nowhere else**
  (#2470). `ci.yml`'s `cargo-deny` job runs `check` exactly once, at the
  repo root, and the root `Cargo.lock` shares no resolution with this
  crate's — so `src-tauri`'s own lock (451 `[[package]]` entries via the
  Tauri graph) received no RUSTSEC advisory check and no licence check at
  all. #2451's `dependency_versions_track_the_root_workspace` closes a
  *different* hole across the same workspace fence — version drift, not
  audit coverage — and stayed green throughout. The step reuses the root
  `deny.toml` via cargo-deny's own `<cwd>/deny.toml` fallback (no explicit
  `--config`: the pinned action's cargo-deny 0.19.8 treats `--config` as a
  `check` subcommand flag, not a top-level one, and `action.yml` places
  `arguments` before `command` on the assembled line, so passing it there
  is a clap parse failure), so one policy file governs both workspaces and
  an accepted advisory is recorded in exactly one place. A **second, local**
  audit path exists for developers: `scripts/setup-dev.sh` under
  `BRINK_SETUP_FULL=1` runs both workspaces at `CARGO_DENY_VERSION`, which
  mirrors the version the pinned action's image ships and must move with that
  action SHA (#2498). Each invocation is bounded by
  `BRINK_SETUP_AUDIT_TIMEOUT` (default 300s) so a stalled RUSTSEC DB fetch
  can't block setup indefinitely (#2531). That knob is **one of a family**,
  not a special case: every network step in `setup-dev.sh` is now bounded by
  its own `BRINK_SETUP_*_TIMEOUT` (#2584/#2591/#2638), and the authoritative
  knob/default/fail-vs-warn table lives in that script's header block — the
  audit bound is simply the one this section is about. A genuine timeout is
  distinguished from normal audit findings and reported as `TIMED OUT`,
  exiting the script non-zero for the required root-workspace audit but only
  warning and continuing for the non-blocking `src-tauri` audit below, so the
  timeout itself can never abort setup before the pnpm/toolchain-verification
  steps that follow — though #2604 gives the pnpm block itself a new hard
  `exit 1` on a resolved-version/pin mismatch, ahead of `Verifying
  toolchain`, so the audit's timeout is no longer the only thing that can end
  setup early; that new abort is a version-pin failure, not an audit outcome,
  and is reported separately by `scripts/check-pnpm-pin.mjs`. PROVISIONAL —
  no maintainer ruling establishes the local mirror; it exists so a developer
  sees what CI sees.
  (`desktop_smoke_audits_the_src_tauri_dependency_graph`)

  **STILL REPORTING, NOT BLOCKING — but the licence half is now ruled.**
  The audit first surfaced 22 errors. The **2026-08-15 maintainer ruling**
  (`docs/decision-log.md`, "MPL-2.0 admitted for the five transitive Tauri
  dependencies") settled the licence class: `deny.toml`'s `[licenses]
  exceptions` now admits MPL-2.0 per-crate for `cssparser`,
  `cssparser-macros`, `dtoa-short`, `option-ext` and `selectors`, so those
  5 `error[rejected]` findings are gone. **17 errors remain** — measured by
  re-running the step's exact invocation against cargo-deny 0.19.8, the
  version the pinned action image ships, not by subtraction:

  ```
  advisories FAILED, bans ok, licenses FAILED, sources ok
  ```

  Those 17 are 16 unmaintained-crate advisories inherent to Tauri v2 on
  Linux (`RUSTSEC-2024-0411`..`0420` gtk-rs GTK3 bindings,
  `RUSTSEC-2024-0370` `proc-macro-error`,
  `RUSTSEC-2025-0075`/`0080`/`0081`/`0098`/`0100` for the five `unic-*`
  crates reached via `urlpattern` → `tauri-utils`; every one of them
  "no safe upgrade available"), plus one `error[unlicensed]: brink-desktop
  = 0.1.0 is unlicensed` — our own `publish = false` crate; `[licenses]
  private.ignore` is the documented cargo-deny knob for it. **Neither of
  those two classes is ruled on**; the 2026-08-15 ruling covers the MPL-2.0
  crates and nothing else, so `[licenses] private.ignore` stays unset and no
  advisory is added to `ignore`. The five admitted crate names, and the fact
  that the admission is per-crate rather than a blanket `allow` entry, are
  asserted by `deny_toml_admits_mpl_for_the_transitive_tauri_dependencies`.
  Accepting either remaining class is a policy
  call, so the step keeps `continue-on-error: true` rather than a blanket
  `ignore`, and it lives in this non-required lane: a non-blocking step
  inside `ci.yml`'s required `cargo-deny` job would raise the "the required
  lanes must not grow a Tauri build" question (#2402/#2346) for exactly
  zero blocking power. Once those remaining 17 are ruled on, promote the
  step to `ci.yml` and drop the `continue-on-error` assertion from the
  guard. Note the audit builds
  nothing — it resolves metadata only, no compilation and none of the
  webkit2gtk system deps — so the fence question, when it is asked, is
  about graph *resolution*, not a Tauri build; "seconds, not a build" is
  the claim that stays true; a wall-clock figure would also have to count
  the advisory-DB fetch and the entrypoint's own `rustup show`/toolchain
  step, which the ~2s metadata-resolution number does not.
- **Every `pnpm install --frozen-lockfile` step, in every job in every
  `.github/workflows/*.yml` file, is preceded by a `wasm-pack build
  crates/brink-web` step in the same job** (#2504, follow-up to
  #2479/#2492). `pnpm install --frozen-lockfile`'s exit code is not
  trustworthy evidence the install actually happened when the `file:` link
  from `@brink-lang/web` to `crates/brink-web/www/pkg` is missing: on the
  pnpm #2479 was filed against it exited 0 with the link silently
  unresolved, while on pnpm 10.34.5 (#2593) it instead exits 1 but two of
  its four reproduced permutations still write nothing to `node_modules` at
  all — see `scripts/check-wasm-pkg.mjs`'s header and
  `scripts/guarded-install.mjs` (`pnpm install:checked`) for the full
  account, both of which #2604 has since corrected: which shape a machine
  saw used to depend on whatever 10.x resolved there that day (the repo
  pinned only the major), and #2604 pins an exact version instead (root
  `package.json`'s `packageManager` field), which makes the shape
  reproducible, not harmless — either shape still means a future reorder —
  or a new lane adding the install step without the wasm build first —
  would otherwise re-open #2479 with nothing catching it. The walk
  enumerates every job in every
  workflow file from disk, not a hard-coded list of the four known lanes,
  and separately pins the exact set of `pnpm install`-prefixed lanes found
  today (`ci.yml`'s `frontend` and `e2e`, `desktop-smoke.yml`'s own job,
  `npm-release.yml`'s `release`), so a fifth lane — correctly ordered or
  not — has to be added to that list on purpose. `ci.yml`'s `book` job's
  plain `npm install --no-audit --no-fund` is this guard's one declared,
  pinned exemption (a different command, against its own lockfile, with no
  `file:` dependency on the wasm-pack output); `benchmarks-inkjs.yml`'s
  `inkjs-gate` job's `npm ci` is out of scope for the same reason but is
  not separately pinned — the guard's own exact-list assertion already
  rejects a silent rename there too.
  (`every_pnpm_install_lane_builds_wasm_first_in_the_same_job`,
  `book_job_install_is_a_plain_npm_install_not_a_pnpm_lane`)
- **The pnpm version itself is pinned to one exact value, in one place**
  (#2604, closing the gap the bullet above assumed away). Root
  `package.json`'s `packageManager` field (`pnpm@10.34.5`) is the single
  source; `scripts/setup-dev.sh` derives it and verifies what actually
  resolved, and all five `pnpm/action-setup` workflow steps pass no
  `version:` input so the action reads the same field — two pins that can
  disagree was the shape of the original problem, so nothing restates the
  version a second time. `scripts/check-pnpm-pin.mjs` (wired into `pnpm
  test:scripts`) is the drift assertion, and it also guards a precondition
  this change made load-bearing: `actions/checkout` must precede
  `pnpm/action-setup` in every job, because removing the `version:` input
  makes each lane depend on the checked-out `package.json`
  (`checkActionSetupFollowsCheckout`). It further rejects a workflow
  `version:` input that disagrees with the pin (`checkWorkflowPins`). This
  is the first guard of this cross-lane-invariant class enforced from a
  plain Node script rather than from `src-tauri/src/lib.rs`'s Rust
  workflow-parsing tests above — it needs no Cargo build, so `pnpm
  test:scripts` catches drift before the Rust gate would.
- **Every job in every workflow file sets an explicit `timeout-minutes`**
  (#2697), not just `desktop-smoke.yml`'s own —
  `every_workflow_job_sets_timeout_minutes` walks every job in every
  `.github/workflows/*.yml` file from disk (the same enumeration discipline
  as the pnpm-install guard above), asserting each sets a job-level
  `timeout-minutes:` rather than inheriting Actions' 360-minute default. Its
  one allowlist entry is `release.yml`'s five jobs (`plan`,
  `build-local-artifacts`, `build-global-artifacts`, `host`, `announce`) —
  that file is `cargo-dist`-autogenerated, so hand-editing it fights the
  next `cargo dist generate` regeneration. As of the pinned
  `cargo-dist-version = "0.32.0"` (checked 2026-08-17 against the
  `release.yml.j2` template upstream), that template has only step-level
  `timeout-minutes` support for an optional custom setup step, not
  job-level support for any of the five allowlisted jobs — so neither of
  the allowlist's two removal conditions (upstream job-level support
  landing, or a maintainer accepting a hand-edit despite house rule 5)
  holds yet; re-check this point-in-time finding whenever
  `cargo-dist-version` bumps. #2710 gap 2 added a SECOND, independent
  assertion on top of presence: a `TIMEOUT_MINUTES_CEILING` of 120 minutes
  (2x the highest `timeout-minutes` this repo has ever set, ~4-6x the
  measured headroom of its heaviest lanes at the time it was picked) that
  rejects any job-level value above it; a `timeout-minutes` this guard
  cannot parse is treated as a violation too, not silently waved through.
  Raise the ceiling only with a fresh measurement justifying the new
  value, the same discipline that set 120 in the first place. This
  ceiling only catches values that are too HIGH — a cap sitting BELOW a
  job's typical runtime (the case that motivated #2710's Ask #2) is
  explicitly NOT covered by any in-tree guard today; see the docstring
  above `every_workflow_job_sets_timeout_minutes` in
  `src-tauri/src/lib.rs`.
- **The two non-required desktop lanes document their own standing**
  (#2717) — `non_required_desktop_lanes_document_their_standing` asserts
  `desktop-smoke.yml` and `desktop-bundle-smoke.yml` each carry a header
  comment naming its NON-REQUIRED status and citing both rulings that back
  it (#2346, #2402), scoped to the same contiguous comment block the
  standing claim itself lives in so an unrelated later mention of both
  issue numbers elsewhere in the file can't satisfy the check. This is
  deliberately the smallest thing checkable in-tree: branch protection's
  actual required-checks list lives in GitHub Settings, not this
  repository, and no test here reads or asserts it — a repo-admin-scoped
  API token would be needed for that, which is an open design/cost call
  left to the maintainer, not built by this guard.
- **`desktop-bundle-smoke.yml`'s `push` trigger carries the same `paths:`
  filter as its `pull_request` trigger.** The `push` trigger previously had
  no `paths:` filter at all, so every push to `main` — regardless of what
  changed — re-ran a real `cargo build -p brink-cli --release` plus the
  full `src-tauri` Tauri build graph and re-saved its ~784 MB rust-cache
  entry against `ci.yml`'s shared 10 GB repo-wide cache quota (confirmed
  against a real run, job 95324823667:
  `Cache Size: ~784 MB (822197295 B)`), risking another lane's cache being
  silently LRU-evicted with no visible error. The change mirrors
  `pull_request`'s `paths:` list onto `push` rather than touch the cache
  key or its content — dropping `push` entirely was considered and set
  aside in the same breath, since a direct push to `main` bypassing a PR
  could then go unverified by this lane until the next matching change.
  `desktop_bundle_smoke_push_and_pull_request_paths_match` keeps the two
  lists from silently drifting apart, using positionally-bounded readers
  for both sides (`push_path_filter`/`pull_request_path_filter`) so the
  assertion cannot pass tautologically if `push`'s `paths:` block is ever
  deleted again or the two triggers are reordered.
  ⚠ **The trigger-scope choice itself is UNRULED** (#2716 explicitly asked
  for a maintainer ruling among three options — narrower cache key,
  narrower trigger, or an explicit accept-the-risk call — and named it "not
  obviously mechanical"). This bullet documents what the code and test do,
  not a settled design decision; #2716 stays open for the maintainer's
  call rather than being closed by this change.

### The `dev` preflight (#2452, #2468)

`pnpm --filter @brink/desktop dev` runs `scripts/ensure-wasm.mjs`. It
exports its logic — `ensureWasm` / `newestSource` — behind an
`import.meta.url === pathToFileURL(argv[1])` main-guard, takes every input
as an option defaulting to the real one, and routes external commands
through an injectable `runCommand`. Running it standalone still does the
whole job; importing it does nothing but hand over the functions, so
`src/__tests__/ensure-wasm.test.ts` drives the real decisions without a
toolchain.

This was a *pair* until `docs/desktop-ota-spec.md` Stage 1 — the second
script, `ensure-cli-sidecar.mjs`, staged the `brink-cli` sidecar, and is
deleted with it. Everything below is stated as an invariant of the class,
not of either script, which is what makes it survive one of them going
away.

The default `runCommand` (`defaultRunCommand`, exported from the script)
carries a bound: `DEFAULT_EXEC_TIMEOUT_MS` (#2697), overridable via
`BRINK_ENSURE_WASM_TIMEOUT_MS` (#2702) — before this, `wasm-pack build` ran
on no clock at all on this exact preflight path, the same wedged-proxy hang
class `scripts/check-scripts.mjs` bounds for shell scripts, one language
over. A timeout fails `dev`/`build` with a house-style diagnostic naming the
bound and the env var to raise, rather than hanging forever or surfacing a
bare `Command failed`. The default is sized against the COLD case
specifically — the `built === 0` branch ("no wasm pkg found") is the
fresh-clone path, where a release-mode wasm build of the whole compiler
graph plus `wasm-opt` is the slow case, not an incremental rebuild on a warm
toolchain cache.

Treat the main-guard as an invariant of every preflight script, not of one:
without it, an unguarded module runs its build as a side effect of being
imported. `ensure-wasm.mjs`'s red-first failed outright, because its
already-fresh path called `process.exit(0)` and killed the importing
process; the since-deleted `ensure-cli-sidecar.mjs`'s took 178s, because the
import ran `cargo build --release`. The script's `describe("the
main-guard")` block holds the two tests that pin it (inert on import; still
acts when run standalone). A new preflight script gets the same treatment.

That last sentence used to be enforced by nothing (#2478), which is how the
pair came to be named one script at a time in the first place.
`src/__tests__/scripts-main-guard.test.ts` now closes the class instead of
the instances: it directory-scans `packages/brink-desktop/scripts/*.mjs`
rather than holding a list of filenames, and for every file it finds asserts
a named export and the exact
`process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href`
line. The `process.argv[1] &&` half is required deliberately — with no
script path (`node -e`, which is how the inert-on-import test loads the
module) `pathToFileURL(undefined)` throws, so a guard missing it does not
make the module inert. Because a scan matching nothing would pass forever,
the same file pins the exact expected roster of scripts; adding a preflight
script means adding one name there, and its guard is then checked
automatically.

**Scope, and what is NOT ruled here.** That scan covers
`packages/brink-desktop/scripts/` only — the directory this section governs.
The repo root's `scripts/check-wasm-pkg.mjs` (#2479) and
`scripts/guarded-install.mjs` (#2593) carry the identical idiom but sit
outside this package, are covered by Node's built-in test runner
(`pnpm test:scripts`) rather than Vitest, and are not part of the
`dev` preflight pair; nothing currently asserts *their* main-guards
directly, though `guarded-install.test.mjs` spawns its script as a real
process, which exercises that script's guard incidentally. Whether
the invariant should be repo-wide rather than desktop-scoped is a real
question and is **NOT settled here** — it is raised on #2478 rather than
answered by a package test reaching across the fence.

### The sidecar apparatus, and why it is gone (#2617, #2631, #2687, #2699, #2715, #2729)

`brink-cli` shipped as a Tauri sidecar (`bundle.externalBin`) so that batch
xliff/locale operations ran against the exact workspace version the shell
was built from. It is **deleted** — `docs/desktop-ota-spec.md` Stage 1, and
`docs/decision-log.md` (2026-09-14). The three intl operations it existed
for run in the wasm now (`crates/brink-web/src/intl.rs`,
`@brink-lang/web`'s `exportXliff`/`compileLocale`/`regenerateXliff`), on the
same side of the wire as the compile that produced the bytes.

The reason is not tidiness. `brink-format`'s container check is exact-match
and `VERSION` moved four times in one month, so a wasm updated ahead of the
sidecar would emit artifacts the sidecar's reader silently refuses — which
made the over-the-air web-bundle channel unsafe by construction. Gating
around the coupling was considered and rejected in favour of removing it.

What went with it, recorded so nobody reconstructs a piece of it in
isolation:

- `bundle.externalBin` and `beforeBundleCommand` (`tauri.conf.json`), and
  the `binaries/` staging directory with its gitignore entry.
- `scripts/ensure-cli-sidecar.mjs` (603 lines: host-triple detection,
  `STUB_SIDECAR`, the Windows `.exe` refusal of #2481, `runLipo` and
  `stageUniversalCliSidecar` of #2715/#2729) and
  `scripts/assert-real-sidecar.mjs` (#2631's bundle-time check, #2687's
  ELF/Mach-O/PE magic-byte check, #2699's `--version` smoke check).
- `src-tauri/build.rs`'s `stage_dev_sidecar_if_missing` and
  `BRINK_SIDECAR_STUB` (#2617). The problem it solved — `tauri_build`
  resolving `externalBin` unconditionally, so `cargo test` failed on every
  fresh worktree — is solved at the root instead: there is no `externalBin`
  entry to resolve. `build.rs` is now `tauri_build::build()` and nothing
  else.
- `ALLOWED_CLI_SUBCOMMANDS`, `validate_cli_subcommand`,
  `prepare_cli_invocation`, the `run_cli` command and `CliOutputLine`
  (`src-tauri/src/lib.rs`), plus the `tauri-plugin-shell` dependency — this
  shell now spawns no subprocess at all — and `src/cli.ts`.
- The smoke lane's "Stage brink-cli sidecar" step and the `sidecar`
  prerequisite on four of its checks, plus five dedicated test files and
  three workflow-guard tests.

⚠ The **`brink-cli` crate stays.** It is a published binary for terminal
users and cargo-dist ships it. Only its embedding as a Tauri sidecar is
gone.

**This retires one of D4's two named iOS blockers** — "iOS cannot ship
subprocess binaries" no longer applies to anything here. `FileProvider`'s
arbitrary-directory access is the one that remains.

### CI coverage blind spots

⚠ The smoke lane is `ubuntu-latest` only, so the `#[cfg(any(target_os =
"macos", target_os = "ios", target_os = "android"))]` file-association
surface — `opened_url_to_path`, `handle_opened`, the `RunEvent::Opened` arm
and their three tests — is compiled, linted and run by **no** lane (#2428).
That is the surface D3 keeps growing, and it is currently reviewed by eye.
Whether to buy a macOS runner (or a `--target`-only check job) is a cost
question and is **NOT settled here** — this section records the gap, it does
not rule on it.

The same Linux-only lane hides a cost of the #2415 lint policy: on the first
mobile target, `tauri-macros`' `mobile_entry_point` expansion discards
`run()`'s `tauri::Result<()>` (`unused_must_use`) and uses `eprintln!`
(`clippy::print_stderr`), both fatal under `-D warnings`, and will need a
per-site `#[expect]`. A ⚠ marker above `opened_url_to_path` in
`src-tauri/src/lib.rs` carries the detail next to the cfg gate that will
first switch on.

Three blind spots recorded here were sidecar-specific and are retired with
it (`docs/desktop-ota-spec.md` Stage 1): `executableFormatFor`'s
`macho`/`pe` branches and the `.exe`-suffixed staging path (#2699), the
`--version` smoke check's host-triple-match execute branch, and the whole
`universal-apple-darwin` two-slice-plus-`lipo` staging path (#2715/#2729),
which no CI lane or machine this repo's automation can see had ever run
against a real Apple toolchain. None of them has a subject any more.

`.github/workflows/desktop-bundle-smoke.yml` (#2709), added to close the
ELF slice of those gaps, is **kept** — its remaining value outlives them:
it is the only lane in the repo that runs a real `tauri build --debug
--bundles deb` end to end and asserts a `.deb` was produced. Without it, a
break in the bundling phase would first surface in `desktop-release.yml`,
on a tag, with a release half-cut. The macOS/Windows runner question this
section already declines to rule on for the file-association surface is
unchanged by any of that.

## Menus

Generated from the studio's **command registry**, per
`docs/studio-shell-spec.md`'s own forward-pointer ("the same registry could
feed a native menu bar in a future desktop shell"). The shell maps
registry commands into the native menu and calls `dispatch(commandId)` —
no hand-maintained parallel menu logic. v1 menu surface: App (about/quit),
File (Open Folder…, Open Recent, Save, Save All, Export `.inkb`…, Close
Window), Edit (native webview roles), Story (Play/Restart, from the
existing player commands), View (panel toggles), Help (docs link).

Quit is a plain `MenuItem` (not Tauri's `PredefinedMenuItem::quit`),
forwarded to the webview as a `menu:quit` shell event — the same pattern
already used for Open/Close Project. Ruled 2026-08-07 (#2370): the
predefined Quit item's native teardown is not guaranteed to reach the
webview (`on_menu_event`/`CloseRequested`) on every platform, which would
silently bypass the guarded quit path (`awaitSaveAllBeforeQuit`) for ⌘Q,
the most common real quit action on macOS. That guarded path is not a
single dispatch-and-poll: it re-dispatches `file.saveAll` on a
750ms interval for as long as the dirty set persists, still bounded by
the same overall ~3s cap (#2434, `docs/decision-log.md`) — a path left
dirty by a mid-write edit (#2426/#2431) is retried rather than left to
burn the whole cap unsaved.

**macOS Dock Quit is NOT yet covered (#2400 remains open).** Right-clicking
the running app's Dock icon and choosing Quit — or Dock-icon Cmd-Q — is a
distinct OS-level quit surface from both the app-menu Quit item above and
the window's `CloseRequested`. An earlier version of this PR added a
`RunEvent::ExitRequested` arm in `run()`'s event closure intending to catch
it, but that event does not carry Dock Quit's intent: `tauri-runtime-wry`
only emits `ExitRequested{code: None}` when the last window is destroyed —
a side effect of `handleQuitRequested`'s own `getCurrentWindow().destroy()`
call on the two paths that already ARE guarded (⌘Q, red-button close), not
a distinct signal Dock Quit raises. Dock Quit itself reaches macOS as
`applicationShouldTerminate:` -> tao's `LoopDestroyed` -> `RunEvent::Exit`,
which nothing in this crate's dependency tree redirects. A real fix needs a
mechanism Dock Quit actually reaches (overriding
`applicationShouldTerminate:`, replying via `NSTerminateLater`) — a
maintainer design call, tracked on #2400 — not an `ExitRequested` guard.
Left unhandled, Dock Quit still terminates the process immediately,
bypassing the awaited final `saveAll` (unrecoverable loss of unsaved work);
that gap is unchanged by this PR. The canonical write itself is
temp-write-then-rename (#2445), so a process killed mid-write this way can
still lose the interrupted save, but it can no longer leave a half-written
file on disk — the loss is of the last edit, not of file integrity.

**Close Project carries the same redispatch discipline as quit (#2444).**
`closeProject` (`main.tsx`) tears down the open project on an explicit
Close, on reopen (`openProject` always calls it first), and on the D3
file-association "open a new project while one is already open" path. It
now awaits `awaitSaveAllBeforeQuit` — the identical seam `handleQuitRequested`
uses, unconditional dispatch plus redispatch-until-drained-or-capped — in
place of the single, gated, fire-and-forget `dispatch("file.saveAll")` it
used before: that single dispatch could be skipped entirely if the 500ms
debounce hadn't yet recorded a just-made edit as dirty, and even when it
fired, `current.unmount()` ran immediately after without ever awaiting the
write landing.

`handleQuitRequested`'s own `getCurrentWindow().destroy()` call is wrapped
in a `try`/`catch` (#2401): it runs after the close/exit request has
already been prevented on the native side, so an unhandled rejection there
previously left the window closable only via Force Quit. A caught failure
re-arms the autosave ticker (otherwise left permanently disarmed) and
notifies the author that quit failed, rather than leaving the app silently
wedged.

**Decision: rebuild-on-change menu, not a dynamic submenu (#2394).** The
File → Open Recent submenu is regenerated in full — the whole native `Menu`
is rebuilt from the just-persisted `recents.json` list and installed with
`app.set_menu` — on every `push_recent`/`prune_recent`, rather than
splicing an item into a live submenu in place. Tauri v2 (muda) has no
in-place "insert/remove from this submenu" affordance that composes
cleanly with a list rebuilt from disk on every change, and at this size (a
handful of items) tracking item identity across calls to use one would buy
nothing. Rebuilding from scratch is simple, always correct (the menu can
never drift from `recents.json`, since it is built from the same list that
was just written), and unmeasurably cheap next to the fs write already
done in the same command. `on_menu_event` is registered once on the `App`
in `run()`, not per-`Menu`, so it keeps firing correctly across rebuilds.

## Entry flow

1. Open Folder… → folder dialog → instantiate `TauriFileProvider` at that
   root → `listFiles` → `mountStudio` with the provider.
2. Entry file: let `ProjectSession`'s `brink.toml` discovery decide (#2324
   recorded the precedence); when no `brink.toml` names an entry, fall back
   to `main.ink` / single-file heuristics — whatever the studio already
   does for the playground, unchanged.
3. Reopening: recent-projects list → same flow.
4. File association (D3, #2393), bundled `.app` only: the OS delivers a
   double-clicked (or Dock-dropped) `.ink`/`.brink` file as `RunEvent::Opened`;
   the file's **containing folder** becomes the project root, opened via the
   same `openProject` path as (1). If the file is already inside the
   currently-open project, this focuses it in place instead of reopening the
   project; if it's outside, the ruled close-save flow runs first (same
   teardown `openProject` always does before mounting a new root). A dev run
   (`pnpm tauri dev`) never receives `RunEvent::Opened`, so this path is
   unreached outside the bundled build.

## Stages

- **D1 — the spike.** Scaffold (`pnpm tauri dev` against the Vite build),
  `TauriFileProvider` (open/read/save; no watcher), Open Folder flow.
  Acceptance: open a real on-disk copy of the acceptance-gate project,
  see zero diagnostics, edit, save, verify on disk.
- **D2 — a real host.** fs watcher → `onExternalChange` (acceptance: edit
  a file in another editor, see the #320 conflict surface), native
  rename, recent projects, registry-driven menus, window title = project
  name, quit awaits the final `saveAll` — re-dispatching it on an interval
  while the dirty set persists, capped by the same overall wait — before
  the window closes (#2370; ruled 2026-08-07 — no dirty-state
  close-confirmation prompt, that's dead UI given autosave + save-on-close;
  #2434, 2026-08-14 — the redispatch policy, see `docs/decision-log.md`).
- **D3 — output.** Export `.inkb` via `compile_project` bytes + save
  dialog; xliff export through the wasm. (The batch ops originally shipped
  `brink-cli` as a Tauri **sidecar** "so both cores ship from one workspace
  version" — that sidecar is deleted, and the reasoning inverted: one core,
  in the wasm, is what actually guarantees it. `docs/desktop-ota-spec.md`
  Stage 1.) File associations (`.ink`, `.brink`).
- **D4 — distribution (ACTIVE, ruled 2026-08-22).** Public distribution.
  `.github/workflows/desktop-release.yml` (NOT `release.yml` — that is
  cargo-dist-generated and forbidden to edit) builds on `desktop-v*` tags
  across macOS-arm64 / Windows / Linux. macOS is signed + notarized;
  **Windows ships unsigned for now** (no cheap notarization equivalent;
  SmartScreen warns until reputation accrues). Signing steps are
  CONDITIONAL on secrets, so the pipeline runs and is testable before the
  Apple credentials exist. Versioning is independent of crates/npm —
  `tauri.conf.json`'s `version` is the source of truth. The **updater**
  lands separately, once a real `tauri signer generate` keypair exists; a
  placeholder public key would ship an update channel that can verify
  nothing.

  ⚠ **iOS is not built, but must not be foreclosed.** Two couplings would
  have blocked it. The `brink-cli` **sidecar** (iOS cannot ship subprocess
  binaries) is **gone** — `docs/desktop-ota-spec.md` Stage 1 deleted it, and
  that blocker is retired outright rather than merely contained. What
  remains is **arbitrary-directory access** (iOS has only a sandboxed
  document picker), behind `FileProvider`; it must not become load-bearing
  in the core editing loop.

## Out of scope, recorded so nobody relitigates

- Native-core-over-IPC (perf escape hatch only, needs evidence first).
- The studio shell redesign (`docs/studio-shell-spec.md`) — lands
  independently; the desktop app consumes whichever shell exists.
- Scrivenings mode and other editor features — editor-package work, not
  shell work.
- Mobile (deferred as above).
- Multi-window / multi-project (one window, one project in v1).
