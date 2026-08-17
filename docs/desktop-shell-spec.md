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
| `createFile` / `deleteFile` / `renameFile` | `fs.writeTextFile` / `remove` / `rename` | `renameFile` is implemented natively (atomic) — `ProjectSession.renameFile`'s create+delete fallback for a provider lacking `renameFile` is dead code on this provider, since it always implements it. The native move is **not the whole op**: it carries the file's bytes, but the rename computed the moved file's own outbound `INCLUDE` rewrites into the `newContent` the contract hands over, so the provider writes that content at the new path straight after the move (#2425). Without it, a cross-directory rename leaves disk holding the moved file's own pre-rewrite `INCLUDE` paths at the new location until some unrelated edit dirties it — invisible in the studio (the session is correct) and wrong for anything reading disk directly, e.g. `brink compile`. This write closes that gap only for the moved file's own content, so disk at the new path agrees with the session; a referrer's rewritten `INCLUDE` (a file that pointed at the old path) is an ordinary edit that goes through `applyEdit` → `onFileChanged` and stays staged under D2, landing on disk only at the next `requestSave` — until then, disk can still disagree with the session for those referrer files. The follow-up write goes through the same serialized staged-write path as a save; see the `requestSave` row below for what a rejection does and does not do |
| `onExternalChange` | fs watcher on the project root | debounce; deliver `null` on delete; unsubscribe on teardown per the contract. This lights up the #320 conflict → kept-buffer → merge surface with a *real* watcher for the first time. A payload is **not always external**: `deleteFile`'s and `renameFile`'s own write-throughs echo back through the watcher too — a rename produces both a deletion echo (its old path) and a creation echo (its new path) — so self-write suppression (content match), self-delete suppression (a consumed-once `selfDeletes` marker keyed by path, #2404), and self-create suppression (a consumed-once `selfCreates` marker keyed by path, #2416) all run before a payload reaches the callback — only what survives all three is forwarded as genuinely external. **At most one marker is armed per path** (#2424): the watcher flushes at most one event per path per quiet window, so a marker armed while another is still outstanding could never be consumed — and an unconsumed `selfDeletes` goes on to swallow a genuinely external deletion. Every arming site therefore clears the other two kinds for that path, rather than leaving the outcome to whichever branch of `onExternalChange` checks first; a marker whose operation then rejects is disarmed too, since no echo will ever come for a write or delete that did not happen. `renameFile`'s follow-up content write is no exception: a rejection re-arms `selfCreates` for the destination path so the rename's own still-outstanding creation echo stays suppressed rather than reaching this callback with pre-rewrite bytes (#2438 review) |
| `requestSave` | write staged content | the `staged` map fed by `onFileChanged` (D2 overlay model) is the source of what to write — the #154 egress batch feeds the backup ring instead, orthogonal to dirty. `staged` is a provider-internal write queue, distinct from studio dirty state (`StudioPublicState.dirtyFiles`, computed by `FileChangeHub` from session content vs. baseline): a rename's own `record(newPath, "created")` already marks the moved file dirty the moment the session updates, independent of whether the provider's own follow-up content write (#2425) later succeeds or is rejected. Calls are serialized (#2403): an overlapping caller (the autosave ticker, a quit-time `saveAll`) queues behind whatever write is already in flight rather than racing it against the same `staged` snapshot — as does `renameFile`'s own follow-up content write, which is a staged write like any other. Quit-time `saveAll` is not always a single overlapping call: `awaitSaveAllBeforeQuit` re-dispatches it on an interval while the dirty set persists (#2434), so a hung write can see several of its own redispatches queue up behind it one after another — this same serialization is what keeps each one from racing the write ahead of it. A rejected write of this kind is retried only by the next UNNARROWED `requestSave` (the autosave ticker, `saveAll`) — a `file.save` narrowed to a different, currently-focused path does not touch it, since a narrowed `writeStaged` only writes the paths it is given |

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
(it reaches the compiler only through the `brink-cli` sidecar binary).

**Fourth cost, named later (#2507): `run_cli`'s subcommand allowlist doesn't
cross the fence either.** `ALLOWED_CLI_SUBCOMMANDS` in `src-tauri/src/lib.rs`
hand-mirrors a subset of `brink-cli`'s real `clap` subcommand surface
(`crates/brink-cli/src/main.rs`'s `Commands` enum), and — same shape as the
two costs above — `src-tauri` cannot take a dev-dependency on `brink-cli` to
introspect that surface without pulling the excluded crate back across the
fence it was pushed out of. `cli_allowlist_subcommands_exist_in_brink_cli_surface`
in `src/lib.rs` closes the gap by reading `crates/brink-cli/src/main.rs` as
plain text (not a Cargo dependency), extracting each top-level `Commands`
variant name and applying clap's default kebab-case rename, then asserting
every entry in `ALLOWED_CLI_SUBCOMMANDS` is present in that derived set. It
is deliberately a subset check, not an equality one: `brink-cli` has more
subcommands than the sidecar exposes (`play`, `fmt`, `convert`,
`migrate-xliff`, `replay`, `ide` are intentionally not sidecar-invokable) —
`brink-cli` growing one of those must not fail this test, only a rename or
removal of a subcommand the allowlist actually depends on should. Both files
carry a pointer comment to the other (`ALLOWED_CLI_SUBCOMMANDS`'s doc comment
here, and a comment on `enum Commands` in `crates/brink-cli/src/main.rs`).
Same standing as every guard above: it lives in `src-tauri`'s own,
non-required test suite (see the ruling immediately below) — a subcommand
rename on the `brink-cli` side alone fails `cargo test` in this crate, not
any check branch protection requires. #2466 is the still-open question of
whether a cross-workspace guard like this one needs a home with
merge-blocking teeth; this one inherits the existing (unruled-on) pattern's
non-required standing rather than resolving that question.

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

Five properties of `desktop-smoke.yml` are asserted by tests in
`src-tauri/src/lib.rs` rather than left to review:

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
  Without these a lockfile, sidecar-dependency or root-lint-policy change
  ran this lane only on the post-merge push to `main` — including the two
  `*_matches_the_root_workspace` drift tests, which could not fail the PR
  that caused the drift.
  (`desktop_smoke_path_filter_covers_its_shared_inputs`) `crates/brink-cli/**`
  was one of those crate globs until #2477: once `BRINK_SIDECAR_STUB` (below)
  made the sidecar step a placeholder, nothing left in the lane read
  `brink-cli` source, so the same test now asserts the entry stays **absent**
  rather than present.
- **Checks are non-blocking for their siblings but gated on their setup
  steps.** A bare `if: '!cancelled()'` also overrides the implicit
  `success()` on a failed *prerequisite*, so a dying setup step let the
  dependent steps run and fail too, burying the root cause. Each check now
  reads `!cancelled() && steps.<setup>.outcome == 'success'` for the setup
  steps it needs (`checkout`, `linux_deps`, `wasm_build`, `pnpm_install`,
  `check_wasm_pkg`, `sidecar`); the format check needs only the runner's
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
- **The sidecar is staged, not shipped, in this lane — and since #2469 not
  even built.** A file has to exist on disk before `tauri-build`'s externalBin
  resolution will let `cargo check` run, but nothing in this lane executes it
  (`run_cli` is the sidecar's only caller and it needs a running app, not a
  `cargo test`). The lane therefore sets `BRINK_SIDECAR_STUB: "1"`, which
  makes `ensureCliSidecar` write a loudly-failing placeholder under the real
  triple-suffixed name and skip `cargo build -p brink-cli --release`
  altogether. It is an `env:` var rather than a step flag because the lane
  runs the script twice — its own "Stage brink-cli sidecar" step and, nested,
  `pnpm build`. This **replaces** PR #2446's
  `CARGO_PROFILE_RELEASE_OPT_LEVEL` / `_DEBUG` / `_CODEGEN_UNITS` stopgap —
  but that stopgap was job-wide, not scoped to the sidecar build, so it was
  also flattening the "Build brink-web wasm package" step's `wasm-pack
  build` (release by default, and this lane's largest build), not only the
  sidecar build it was written to excuse. Removing the vars un-flattens
  that wasm build too: the lane now runs a fully-optimised `wasm-pack
  build`, rather than keep vars that would be dead configuration for the
  (now-gone) sidecar build while still quietly de-optimising the wasm one.
  #2482 asked whether that build should get the sidecar's stub treatment;
  it should not (#2502) — unlike the sidecar's staged file, whose content is
  never read, the wasm-pack output is genuinely consumed by "Typecheck (tsc
  --noEmit)" and `pnpm build` below, so a stub cannot stand in for it. The
  release-vs-dev optimisation level and reusing `ci.yml`'s own artefact
  remain open, tracked by #2482. The guard asserts the stub is wired **and**
  that the three
  stopgap vars are gone, so the lane cannot drift back or carry both. Every
  other caller that runs a *release* build — `pnpm --filter @brink/desktop
  build` on a developer machine, where the sidecar really is shipped and
  run — still builds a real release binary, since the option defaults to
  off. As of #2617 this is no longer the only caller that sets the var at
  all: `src-tauri/build.rs` now sets it too, for every **debug** build with
  no sidecar staged, on a developer machine and not only in CI — see
  "Build-script sidecar auto-staging" below.
  (`desktop_smoke_stubs_the_staged_sidecar`)
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

### The `dev` preflight pair (#2452, #2468)

`pnpm --filter @brink/desktop dev` runs `scripts/ensure-wasm.mjs` and then
`scripts/ensure-cli-sidecar.mjs`. Both export their logic — `ensureWasm` /
`newestSource`, and `ensureCliSidecar` / `hostTriple` / `sidecarPaths` /
`STUB_SIDECAR` — behind an `import.meta.url === pathToFileURL(argv[1])`
main-guard, take every input as an option defaulting to the real one, and
route external commands (`wasm-pack`, `rustc`, `cargo`) through a single
injectable `runCommand`. Running either script standalone still does the
whole job; importing it does nothing but hand over the functions, so
`src/__tests__/ensure-wasm.test.ts` and
`src/__tests__/ensure-cli-sidecar.test.ts` drive the real decisions without
a toolchain.

Treat this as an invariant of the pair, not of one script: without the
guard, an unguarded module runs its build as a side effect of being
imported. `ensure-cli-sidecar.mjs`'s own red-first took 178s because the
import ran `cargo build --release`; `ensure-wasm.mjs`'s failed outright,
because its already-fresh path called `process.exit(0)` and killed the
importing process. Each script's `describe("the main-guard")` block holds
the two tests that pin it (inert on import; still acts when run
standalone). A third preflight script gets the same treatment.

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

The sidecar seam is also what made the stub option above testable — it was
added (#2452) as a prerequisite and spent by #2469.

`ensure-cli-sidecar.mjs` gained a third caller in #2617, outside this pair
and outside CI: `src-tauri/build.rs` itself. See "Build-script sidecar
auto-staging" below.

### Build-script sidecar auto-staging (#2617)

`tauri_build::build()` resolves `bundle.externalBin` unconditionally — not
only when a bundle is actually produced — so `binaries/brink-cli-<triple>`
has to exist on disk before this crate will even `cargo check`. That path is
gitignored (the triple suffix is host-specific), and until #2617 nothing on
the local path staged it, so CLAUDE.md's documented gate (`cd
packages/brink-desktop/src-tauri && cargo test`) failed on every fresh
checkout and every fresh git worktree before a single test ran.

`src-tauri/build.rs`'s `stage_dev_sidecar_if_missing` now stages a stub when
the file is missing, by invoking the same script the smoke lane's "Stage
brink-cli sidecar" step invokes — `ensure-cli-sidecar.mjs` — under the same
`BRINK_SIDECAR_STUB=1` that step sets. This is a delegation, not a second
mechanism: the stub payload, host-triple detection and staged filename
(including #2481's Windows `.exe` refusal) stay owned by that script alone;
`build.rs` only decides *whether* to invoke it, per
`build_script_stages_the_dev_sidecar_the_way_ci_does`.

As the "Smoke-lane inputs" bullet above now notes, this makes `build.rs` a
second caller that sets `BRINK_SIDECAR_STUB=1` outside the smoke lane's own
step — and, unlike that step, it fires on a plain developer machine, not
just CI, whenever a debug build finds no sidecar staged.

- **`PROFILE == "debug"` gates all of it.** `cargo tauri build` (release)
  must keep failing loudly on a missing sidecar: a real bundle ships the
  real `brink-cli`, and silently substituting a stub there would turn a
  build-time error into a shipped, `exit 127` one. Release staging stays on
  its existing path — `beforeBuildCommand` -> `pnpm build` ->
  `ensure-cli-sidecar.mjs` with no stub variable set.
- **Three cases degrade to a `cargo:warning=` instead of staging, and
  `tauri_build::build()` is left to fail on the missing file exactly as
  before #2617:** the target triple is a Windows one (`ensure-cli-sidecar.mjs`
  refuses to stage the POSIX stub under a `.exe`-suffixed name, #2481); the
  build is cross-target (`cargo test/check --target <other>`) — the script
  stages under `hostTriple()` from `rustc -vV`, not the `TARGET` this
  function probes, so a mismatched `HOST` would otherwise leave a
  wrong-triple file staged for no benefit, which is why the check compares
  `HOST` to `TARGET` before invoking the script rather than after; or `node`
  is not on `PATH` / the script is not where expected (`Command::new("node")`
  fails, or `script.is_file()` is false). All three are "could not stage" —
  a developer who hits one runs the script by hand, per the warning text.
- **The staged file lands in `src-tauri/binaries/`, not `OUT_DIR`.** This
  contradicts the usual Cargo build-script guidance (write generated
  artifacts under `OUT_DIR`, never back into the source tree), and the
  contradiction is deliberate rather than an oversight:
  `tauri.conf.json`'s `bundle.externalBin` is a path Tauri resolves
  relative to the manifest directory, not to `OUT_DIR` — Tauri reads that
  config independently of this crate's build-script output, so a stub
  staged under `OUT_DIR` would be invisible to the exact resolution step
  this function exists to satisfy. `binaries/` is gitignored specifically
  because it now holds build output despite living in the source tree.

### Bundle-time sidecar assertion (#2631)

PR #2626's stated invariant is "a real bundle must ship the real
`brink-cli`." For `cargo tauri build` (release), `build.rs` enforces that
directly — its `stage_dev_sidecar_if_missing` only auto-stages
`STUB_SIDECAR` when `PROFILE == "debug"`, so a release build with no sidecar
staged keeps failing loudly on `tauri-build`'s `bundle.externalBin`
resolution exactly as before #2617. But `tauri build --debug` is a
debug-profile **bundling** path, and until this section's fix landed, the
invariant held there only *indirectly*: via `beforeBuildCommand` ->
`pnpm build` staging the real binary before `build.rs` ever ran, plus
`bundle.active: false` making the question moot in practice. Nothing
asserted it — an ordering coincidence (the pnpm build script happens to run
before Tauri's own resource resolution) plus a feature flag that happens to
be off. If the ordering assumption ever broke — a `build.rs` change, a
`beforeBuildCommand` change, or a developer/CI shell that happens to carry
`BRINK_SIDECAR_STUB=1` (the smoke lane's own `env:` var, #2469) into a real
`--debug` bundle invocation — nothing would have caught the STUB shipping.

The fix is `tauri.conf.json`'s `beforeBundleCommand`, a Tauri hook distinct
from `beforeBuildCommand`: it runs immediately before the **bundling
phase** of `tauri build`, i.e. after the crate has already compiled
(`build.rs` has already run and either staged something or failed the build
outright) and right before tauri-bundler reads
`binaries/brink-cli-<triple>` off disk to package it. That is the latest
point at which refusing is still useful, and it fires only when a bundle is
actually being produced — unlike a check inside `build.rs` itself, it
cannot be confused with an ordinary `cargo check`/`cargo test`, which
legitimately wants `build.rs`'s auto-staged stub and must keep getting it.

`beforeBundleCommand` runs `node scripts/assert-real-sidecar.mjs`
(`packages/brink-desktop/scripts/`), which:

- resolves the same triple-suffixed path `ensure-cli-sidecar.mjs` stages
  (via that script's own `sidecarPaths`, not a re-derived path);
- reads the staged file's bytes and compares them against
  `STUB_SIDECAR` — **imported** from `ensure-cli-sidecar.mjs`, not
  redefined. #2626's review established that the stub payload, host-triple
  detection and staged filename live in that script alone; a second copy
  anywhere (including here) is exactly the drift
  `build_script_stages_the_dev_sidecar_the_way_ci_does` (`src-tauri/src/lib.rs`)
  already guards against for `build.rs`, and
  `before_bundle_command_asserts_the_staged_sidecar_is_real` (same file)
  extends that same guard to this script;
- throws — refusing the bundle — when they match;
- and then, separately, checks **positively** that the staged file begins
  with the executable magic its target triple's loader requires (#2687),
  throwing if it does not.

**Why two checks and not one (#2687).** The stub comparison on its own is a
**blocklist**: it refuses the one placeholder that exists today and passes
everything else. That fails open, because `tauri_build`'s `externalBin`
resolution only tests that the path EXISTS — an empty file, a half-finished
copy, or a binary built for a different platform's loader all bundled clean
under #2660's version (measured directly against it: a zero-byte file, a
two-byte truncated ELF, and a Mach-O staged for a linux bundle were all
passed). Any future placeholder that is not byte-identical to `STUB_SIDECAR`
would sail through too. The positive check covers the whole "the bundle
shipped something that is not the CLI" class instead. The stub comparison is
**kept alongside** it rather than replaced, because it is the only one of the
two that can say *which* placeholder is staged and what to rebuild; it runs
first for the specific diagnosis, and the magic check follows for the general
class.

The triple → format rule is **`executableFormatFor` in
`ensure-cli-sidecar.mjs`**, not in the hook: `\x7fELF` for the ELF Unixes
(linux/android/the BSDs/solaris/illumos/fuchsia/redox/haiku), any of the
eight Mach-O header magics for Apple triples — 32- and 64-bit, both byte
orders, thin *and* fat/universal, since a macOS release binary may
legitimately be a universal wrapper — and the `MZ` DOS stub for Windows
triples. It lives there because it generalises the `.exe`-suffix rule
`sidecarPaths` already encoded for exactly the same reason (#2481), and
because #2626's review established that triple-derived knowledge about the
staged sidecar lives in that module **alone**; `sidecarPaths` and the
Windows-stub guard now ask `executableFormatFor` rather than testing the
triple substring themselves, so the rule is stated once.

`executableFormatFor` returns `null` for a triple it has no rule for, and the
hook then falls back to rejecting only an empty file or an interpreter
script — it does **not** fall back to skipping the `--version` smoke check
below too: `weakFallbackCheck` still calls `smokeCheckSidecar`, which
self-gates on the staged triple matching this machine's host triple, so
lacking format evidence is never a reason to also forgo execution evidence
when execution is actually possible (#2699 review). That asymmetry on the
magic side is deliberate and load-bearing: **a positive check that rejects a
REAL binary on an unanticipated platform would be worse than the blocklist
it replaces**, so "no rule known" must stay distinguishable from "judged and
rejected" (`looksLikeNativeExecutable` returns `undefined`, not `false`, for
an unknown format) and must never harden into a guess.

Like the two preflight scripts it joins, it is main-guarded and exports its
core logic (`assertRealSidecarStaged`), so
`src/__tests__/scripts-main-guard.test.ts`'s directory scan (#2478) covers
it automatically, and `src/__tests__/assert-real-sidecar.test.ts` drives the
stub/non-stub/missing-file decisions directly plus the main-guard's
inert-on-import and still-acts-standalone properties, the same shape
`ensure-cli-sidecar.test.ts` uses for the script it imports `STUB_SIDECAR`
from.

**Deliberately inert by default, not a gap.** No CI lane and no documented
developer command invokes `tauri build` (grepped at the time of #2631: only
`pnpm --filter @brink/desktop dev`/`build` exist, neither of which reaches
tauri-cli's bundler) — so the hook does not fire in the ordinary course of
things, by design. But its firing condition is not simply "`bundle.active`
flips to `true`": tauri-cli enters its bundling phase (and therefore runs
this hook) on `!options.no_bundle && (config.bundle.active ||
options.bundles.is_some())`, so an explicit `tauri build --bundles
<target>` / `-b <target>` already fires it **today**, with `bundle.active`
still `false` — D3 flipping `bundle.active` to `true` only widens which
invocation reaches it (the *default*, bundle-less `tauri build` starts
doing so too); it is not the sole door. No CI lane or documented command
invokes either door — but an ad-hoc `--bundles` invocation reaches the hook
today, as #2687's observation (below) did; that is a narrower claim than
"unreachable."
`before_bundle_command_asserts_the_staged_sidecar_is_real` pins that
`bundle.active` stays `false` here specifically so a later, unrelated PR
that does flip it does not silently change what this hook's presence means
without anyone noticing — that assertion should be deleted (not edited)
once D3 makes it legitimately `true`.

**The firing point is OBSERVED, not inferred (#2687).** Up to and including
#2660 it rested on Tauri's documented ordering plus a reading of tauri-cli
2.11.4's source; nothing in-repo invokes `tauri build`, so nobody had watched
it happen. It has now been watched, three times, by driving a real
`pnpm tauri build --debug --bundles deb` in a worktree with `bundle.active`
left at `false`:

| staged at `binaries/brink-cli-<triple>` | observed |
|---|---|
| `STUB_SIDECAR` (via `BRINK_SIDECAR_STUB=1`) | `Built application at …` → `Running beforeBundleCommand` → refused, `exit 1`, no bundle produced |
| an **empty file** — which #2660's blocklist passed | same firing point, refused by the #2687 magic check, `exit 1`, no bundle produced |
| a real ELF binary | hook logged `carries ELF executable magic — proceeding`, and tauri-bundler went on to produce `Brink Studio_0.1.0_amd64.deb` |

Three things that were previously only argued are now facts on the record:
the hook runs **after** the crate compiles and **before** tauri-bundler
touches anything; `--bundles deb` really does reach it with `bundle.active`
still `false` (confirming tauri-cli's `config.bundle.active ||
options.bundles.is_some()`); and a refusal genuinely **stops** the bundle
rather than merely printing. The third row also demonstrates the positive
check does not false-reject a real native binary at the real firing point.
No CI lane runs any of this — a `tauri build --debug --bundles deb` lane is
still the standing follow-up, and this observation does not substitute for
one, it only removes the doubt about *where* the hook fires.

**The `--version` executable smoke check (#2699).** The magic check above
proves the staged file's FORMAT (ELF/Mach-O/PE); it cannot prove the file
IS `brink-cli` or that it runs — PR #2691's own passing observation of this
hook stood in **GNU coreutils' `true`** for a real `brink-cli`, and that
binary satisfies the magic check exactly as a genuine wrong-build binary
would. `assertRealSidecarStaged` now runs `destBin --version` in addition
to the magic check, and requires BOTH exit `0` AND that the printed output
starts with `brink` — clap's `#[command(name = "brink", version)]` on `Cli`
(`crates/brink-cli/src/main.rs`) formats every real build's output that
way. The content half of that check is load-bearing, not decoration: exit
code alone is not sufficient evidence, because `true --version` *also*
exits `0`.

This is not limited to the magic-confirmed acceptance path: the weak-fallback
path above (an unrecognised triple, or a format `EXECUTABLE_MAGIC` has no
entry for) runs the same smoke check too, not just the stub/empty/script
checks it already had. Skipping it there would have meant the one path with
*zero* format evidence also shipped with zero execution evidence, accepting
on "not the stub, not empty, not a `#!` script" alone (#2699 review).
`smokeCheckSidecar` is the single call site both paths share, so this is one
behavior, not two copies that could drift.

This is gated on the staged triple matching the triple the check is
actually running on (`smokeCheckSidecar` in `assert-real-sidecar.mjs`): a
sidecar staged for any other triple is a cross-build and **cannot be
executed on this machine at all** — trying would fail for a reason that has
nothing to do with whether the binary is a genuine `brink-cli`, and
treating that as a rejection would refuse a legitimate cross-compiled
bundle. That case — and the case where the host triple itself cannot be
determined (no `rustc` on PATH) — degrades to "verified via magic only,"
and the log line says so explicitly rather than silently claiming to have
run something it did not.

Driven directly (not merely argued) via `assertRealSidecarStaged` in a
scratch tree, mirroring the shape of the table above:

| staged at `binaries/brink-cli-<triple>`, triple = host | observed |
|---|---|
| a real release build of `brink-cli` | `--version` exited 0, printed `brink 0.0.11` → logged `ran successfully … confirmed a working brink-cli`, then `proceeding with the bundle` |
| `/bin/true` (GNU coreutils, #2691's own stand-in) | `--version` exited 0, printed `true (GNU coreutils) 9.4…` → **refused**: `ran (--version exited 0) but printed "…" — that is not a brink-cli version string`, `exit 1` |
| a synthetic Mach-O-magic file staged for `aarch64-apple-darwin` while running on `x86_64-unknown-linux-gnu` | smoke check **not executed** — logged `skipped the --version smoke check: staged triple … does not match this machine's host triple …`, then still `proceeding with the bundle` on the magic check alone |

The middle row is the one that matters: it is the exact scenario #2691's PR
body disclosed as its own limit (`/bin/true` standing in for `brink-cli`),
and it is now refused where it previously would have passed. No CI lane
exercises any of this either — same standing gap as the magic check itself,
and now also named in "CI coverage blind spots" for the macho/pe/.exe
formats this smoke check's execute branch never reaches in CI.

Scope note the fix does **not** widen: `build.rs`'s own auto-staging only
checks the **host** triple (`hostTriple()`), comparing `HOST` to `TARGET`
before staging anything — a cross-compiled `cargo test/check --target
<other>` gets nothing staged, and that gap is pre-existing, not one #2631
introduces. This hook does not inherit that limit: `triple` defaults to
`TAURI_ENV_TARGET_TRIPLE` when tauri-cli set it — the exact `--target`
triple `app_settings` resolved for the build, exported into every hook
tauri-cli runs, `beforeBundleCommand` included — and only falls back to
`hostTriple()` for a standalone/manual invocation outside tauri-cli. A
cross-compiled `--target` **bundle** is therefore checked correctly when
run through `tauri build`; the unchecked case is `build.rs`/
`ensure-cli-sidecar.mjs` not staging anything for a cross-target `cargo
test`/`check` in the first place, which this hook cannot fix because there
is nothing staged yet to check.

### CI coverage blind spots

⚠ The smoke lane is `ubuntu-latest` only, so the `#[cfg(any(target_os =
"macos", target_os = "ios", target_os = "android"))]` file-association
surface — `opened_url_to_path`, `handle_opened`, the `RunEvent::Opened` arm
and their three tests — is compiled, linted and run by **no** lane (#2428).
That is the surface D3 keeps growing, and it is currently reviewed by eye.
Whether to buy a macOS runner (or a `--target`-only check job) is a cost
question and is **NOT settled here** — this section records the gap, it does
not rule on it.

The same blind spot let `STUB_SIDECAR` (above) ship as a POSIX `#!/bin/sh`
script with no host awareness: `sidecarPaths` stages it under a
`.exe`-suffixed name on Windows triples, same as it would a real binary, and
Windows loads `.exe`-named files through its PE loader regardless of the
bytes inside them — a shell script staged there could not run if anything
ever executed it. `ensureCliSidecar` now throws rather than stage that file
for a Windows `triple` when `stub` is requested, since no text payload
staged at a `.exe` path can be made to "fail loudly" the way the POSIX stub
does; a real Windows-compatible stub is future work if a non-Linux smoke
lane is ever added (#2481, follow-up from #2474's review of #2469). The
guard is pinned by a synthetic-triple test in
`src/__tests__/ensure-cli-sidecar.test.ts` (`describe("the stub option")`),
since the ubuntu-only lane itself cannot exercise it.

The same Linux-only lane hides a cost of the #2415 lint policy: on the first
mobile target, `tauri-macros`' `mobile_entry_point` expansion discards
`run()`'s `tauri::Result<()>` (`unused_must_use`) and uses `eprintln!`
(`clippy::print_stderr`), both fatal under `-D warnings`, and will need a
per-site `#[expect]`. A ⚠ marker above `opened_url_to_path` in
`src-tauri/src/lib.rs` carries the detail next to the cfg gate that will
first switch on.

The same blind spot reaches `executableFormatFor`'s `macho`/`pe` branches
and the `.exe`-suffixed staging path in `sidecarPaths` (#2699): both are
exercised only by unit tests over synthetic byte arrays
(`src/__tests__/assert-real-sidecar.test.ts`,
`src/__tests__/ensure-cli-sidecar.test.ts`) — the ubuntu-only smoke lane
never observes the real magic bytes of an actual cross-built `brink-cli`
for either format, and never stages anything under a real `.exe` name. The
`--version` smoke check added alongside the positive magic check (above,
"Bundle-time sidecar assertion") inherits the same gap one layer up: its
host-triple-match branch — the one that actually executes the staged
binary — is likewise proven only by unit tests with a mocked `runFile`
plus the ad-hoc, by-hand `node scripts/assert-real-sidecar.mjs` drive
recorded in that section, not by any CI lane; the macho/pe/.exe paths
specifically only ever reach the check's cross-build skip branch, which
never executes anything at all. Documenting this here does not close it —
it names the same "which formats does `ubuntu-latest` actually observe"
gap the file-association surface above has, for the sidecar-verification
surface instead. Whether to buy a macOS/Windows runner is the same
unsettled cost question as above.

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
  dialog; `brink-cli` as a Tauri **sidecar** for batch ops (xliff
  export/locale compile) so both cores ship from one workspace version.
  File associations (`.ink`, `.brink`).
- **D4 — distribution (deferred).** Signing, notarization, updater, own
  release workflow, and the promote-to-distributable decision. Not
  planned until explicitly scheduled.

## Out of scope, recorded so nobody relitigates

- Native-core-over-IPC (perf escape hatch only, needs evidence first).
- The studio shell redesign (`docs/studio-shell-spec.md`) — lands
  independently; the desktop app consumes whichever shell exists.
- Scrivenings mode and other editor features — editor-package work, not
  shell work.
- Mobile (deferred as above).
- Multi-window / multi-project (one window, one project in v1).
