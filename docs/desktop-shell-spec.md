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
| `onFileChanged` | **buffer, don't write** | v1 keeps the studio's explicit-save model: dirty state lives in the editor; disk writes happen on `file.save`/`saveAll` via `requestSave`. Autosave ships unconditionally: a 2-minute ticker (120 s) dispatches `file.saveAll` whenever dirty files exist, queuing behind any write already in flight (#2403). The ticker is armed when a project opens and cleared on project close or reopen (not app lifetime; see `autosaveTimer` in `packages/brink-desktop/src/main.tsx`). A Settings surface can configure the interval in future versions; this is the intended extensibility point already noted in the code comment. This ticker is the production caller both #2435 and #2434's fixes exist to serve correctly |
| `createFile` / `deleteFile` / `renameFile` | `fs.writeTextFile` / `remove` / `rename` | `renameFile` is implemented natively (atomic) — `ProjectSession.renameFile`'s create+delete fallback for a provider lacking `renameFile` is dead code on this provider, since it always implements it. The native move is **not the whole op**: it carries the file's bytes, but the rename computed the moved file's own outbound `INCLUDE` rewrites into the `newContent` the contract hands over, so the provider writes that content at the new path straight after the move (#2425). Without it, a cross-directory rename leaves disk holding the moved file's own pre-rewrite `INCLUDE` paths at the new location until some unrelated edit dirties it — invisible in the studio (the session is correct) and wrong for anything reading disk directly, e.g. `brink compile`. This write closes that gap only for the moved file's own content, so disk at the new path agrees with the session; a referrer's rewritten `INCLUDE` (a file that pointed at the old path) is an ordinary edit that goes through `applyEdit` → `onFileChanged` and stays staged under D2, landing on disk only at the next `requestSave` — until then, disk can still disagree with the session for those referrer files. The follow-up write goes through the same serialized staged-write path as a save; see the `requestSave` row below for what a rejection does and does not do |
| `onExternalChange` | fs watcher on the project root | debounce; deliver `null` on delete; unsubscribe on teardown per the contract. This lights up the #320 conflict → kept-buffer → merge surface with a *real* watcher for the first time. A payload is **not always external**: `deleteFile`'s and `renameFile`'s own write-throughs echo back through the watcher too — a rename produces both a deletion echo (its old path) and a creation echo (its new path) — so self-write suppression (content match), self-delete suppression (a consumed-once `selfDeletes` marker keyed by path, #2404), and self-create suppression (a consumed-once `selfCreates` marker keyed by path, #2416) all run before a payload reaches the callback — only what survives all three is forwarded as genuinely external. **At most one marker is armed per path** (#2424): the watcher flushes at most one event per path per quiet window, so a marker armed while another is still outstanding could never be consumed — and an unconsumed `selfDeletes` goes on to swallow a genuinely external deletion. Every arming site therefore clears the other two kinds for that path, rather than leaving the outcome to whichever branch of `onExternalChange` checks first; a marker whose operation then rejects is disarmed too, since no echo will ever come for a write or delete that did not happen. `renameFile`'s follow-up content write is no exception: a rejection re-arms `selfCreates` for the destination path so the rename's own still-outstanding creation echo stays suppressed rather than reaching this callback with pre-rewrite bytes (#2438 review) |
| `requestSave` | write staged content | the `staged` map fed by `onFileChanged` (D2 overlay model) is the source of what to write — the #154 egress batch feeds the backup ring instead, orthogonal to dirty. `staged` is a provider-internal write queue, distinct from studio dirty state (`StudioPublicState.dirtyFiles`, computed by `FileChangeHub` from session content vs. baseline): a rename's own `record(newPath, "created")` already marks the moved file dirty the moment the session updates, independent of whether the provider's own follow-up content write (#2425) later succeeds or is rejected. Calls are serialized (#2403): an overlapping caller (the autosave ticker, a quit-time `saveAll`) queues behind whatever write is already in flight rather than racing it against the same `staged` snapshot — as does `renameFile`'s own follow-up content write, which is a staged write like any other. Quit-time `saveAll` is not always a single overlapping call: `awaitSaveAllBeforeQuit` re-dispatches it on an interval while the dirty set persists (#2434), so a hung write can see several of its own redispatches queue up behind it one after another — this same serialization is what keeps each one from racing the write ahead of it. A rejected write of this kind is retried only by the next UNNARROWED `requestSave` (the autosave ticker, `saveAll`) — a `file.save` narrowed to a different, currently-focused path does not touch it, since a narrowed `writeStaged` only writes the paths it is given |

Path discipline: provider keys are project-relative with `/` separators
(the studio's convention); the provider owns the mapping to absolute OS
paths and never leaks them into the session.

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

Four properties of `desktop-smoke.yml` are asserted by tests in
`src-tauri/src/lib.rs` rather than left to review:

- **The `pull_request` path filter lists every input that can break the
  job**, not just the trees it checks: `pnpm-lock.yaml`, root `Cargo.toml`/
  `Cargo.lock`, `clippy.toml`, `rust-toolchain.toml` and `ci.yml` on top of
  the package/crate globs. Without them a lockfile, sidecar-dependency or
  root-lint-policy change ran this lane only on the post-merge push to
  `main` — including the two `*_matches_the_root_workspace` drift tests,
  which could not fail the PR that caused the drift.
  (`desktop_smoke_path_filter_covers_its_shared_inputs`)
- **Checks are non-blocking for their siblings but gated on their setup
  steps.** A bare `if: '!cancelled()'` also overrides the implicit
  `success()` on a failed *prerequisite*, so a dying setup step let the
  dependent steps run and fail too, burying the root cause. Each check now
  reads `!cancelled() && steps.<setup>.outcome == 'success'` for the setup
  steps it needs (`checkout`, `linux_deps`, `wasm_build`, `pnpm_install`,
  `sidecar`); the format check needs only the runner's toolchain and the
  checkout, so it is gated on `checkout` alone — `actions/checkout` carries
  no `id` by default, so this lane gives its checkout step one.
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
  that wasm build too: the lane now deliberately runs a fully-optimised
  `wasm-pack build`, rather than keep vars that would be dead configuration
  for the (now-gone) sidecar build while still quietly de-optimising the
  wasm one. The guard asserts the stub is wired **and** that the three
  stopgap vars are gone, so the lane cannot drift back or carry both. Every
  other caller — `pnpm --filter @brink/desktop build` on a developer
  machine, where the sidecar really is shipped and run — still builds a
  real release binary, since the option defaults to off.
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
  an accepted advisory is recorded in exactly one place.
  (`desktop_smoke_audits_the_src_tauri_dependency_graph`)

  **PROVISIONAL, pending a maintainer ruling — it reports, it does not
  block.** The first audit surfaces 22 errors: 16 unmaintained-crate
  advisories inherent to Tauri v2 on Linux (`RUSTSEC-2024-0411`..`0420`
  gtk-rs GTK3 bindings, `RUSTSEC-2024-0370` `proc-macro-error`,
  `RUSTSEC-2025-0075`/`0080`/`0081`/`0098`/`0100` for the five `unic-*`
  crates reached via `urlpattern` → `tauri-utils`; every one of them
  "no safe upgrade available"), 5 MPL-2.0 crates (`cssparser`,
  `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`) against a
  licence allowlist whose stated policy is "100% permissive, no copyleft
  obligations", and one `error[unlicensed]: brink-desktop = 0.1.0 is
  unlicensed` — our own `publish = false` crate; `[licenses]
  private.ignore` is the documented cargo-deny knob for it, but that edits
  the shared root policy, so it is left for the same ruling as the other
  21. Accepting any of these classes is a policy call, so the step carries
  `continue-on-error: true` rather than a blanket `ignore`, and it lives in
  this non-required lane: a non-blocking step inside `ci.yml`'s required
  `cargo-deny` job would raise the "the required lanes must not grow a
  Tauri build" question (#2402/#2346) for exactly zero blocking power.
  Once the findings are ruled on, promote the step to `ci.yml` and drop the
  `continue-on-error` assertion from the guard. Note the audit builds
  nothing — it resolves metadata only, no compilation and none of the
  webkit2gtk system deps — so the fence question, when it is asked, is
  about graph *resolution*, not a Tauri build; "seconds, not a build" is
  the claim that stays true; a wall-clock figure would also have to count
  the advisory-DB fetch and the entrypoint's own `rustup show`/toolchain
  step, which the ~2s metadata-resolution number does not.

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

The sidecar seam is also what made the stub option above testable — it was
added (#2452) as a prerequisite and spent by #2469.

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
