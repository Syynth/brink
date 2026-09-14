# Desktop OTA web-bundle updates

**Status:** Stage 1 LANDED. Stage 2 in progress — the bundle store, serving
and the update channel are landed; the release pipeline that PRODUCES a
bundle is not, so nothing can be installed over the air yet.
Rulings 2026-09-14 (`docs/decision-log.md`).

Cutting a desktop release today means the full signed pipeline — build the
matrix, import the Apple certificate, codesign, notarize, staple, upload —
because the web bundle is compiled into the binary (`tauri.conf.json`'s
`frontendDist: "../dist"`) and the binary is what carries the signature. That
cost is paid even when the only thing that changed is JavaScript.

This spec adds a second, signature-free update channel for the web bundle,
and removes the one thing that would have made it unsafe.

## Why the payoff is large

The wasm is **inside** the web bundle. Vite emits `brink_web_bg-*.wasm` and
`brink_prose_bg-*.wasm` into `dist/assets/`, the webview fetches them like any
other asset, and neither is a native executable — no codesign, no
notarization, no Gatekeeper. They are also ~90% of the payload:

| asset | raw | gzip |
|---|---|---|
| `brink_prose_bg.wasm` | 11.9 MB | 6.87 MB |
| `brink_web_bg.wasm` | 9.30 MB | 3.06 MB |
| `index.js` | 1.68 MB | 0.50 MB |
| everything else | ~0.5 MB | ~0.06 MB |

So "web-only" is not a cosmetic channel: it carries the compiler and the prose
checker, which is where nearly all the work lands. Commits since 2026-08-01,
excluding merges:

| | count |
|---|---|
| touch `src-tauri/` or `brink-cli` → **needs a signed release** | 22 |
| touch only code that flows into the web bundle → **OTA-able** | 352 |
| docs/CI/tests only | 68 |

94% of code-touching commits would stop needing the signing pipeline.

## Stage 1 — delete the sidecar (prerequisite, not an optimisation) — DONE

**The coupling that makes naive OTA unsafe.** `brink-format`'s container check
is exact-match, not a range (`crates/internal/brink-format/src/inkb/read.rs`):

```rust
let version = read_u16(buf, &mut off)?;
if version != VERSION { return Err(DecodeError::UnsupportedVersion(version)); }
```

`VERSION` is 10 and moved four times since August (v7→v8→v9→v10, all from the
superinstruction work). The `brink-cli` **sidecar** is a native binary inside
the signed `.app`, so it cannot ride the OTA channel. D3 put it there "so both
cores ship from one workspace version" — OTA is exactly what breaks that
invariant, and there is no version handshake to catch it. The failure is
silent: xliff export starts rejecting artifacts the freshly-OTA'd wasm
produces, and nothing else looks wrong.

**RULED: remove the sidecar rather than gate around it.** Gating (refuse the
bundle, or ship it and disable the feature) preserves a coupling that has no
reason to exist. Deleting it removes the failure mode at the root.

**It is a small port, because the operations are already pure.** All three
intl entry points in `crates/brink-cli/src/main.rs` are `fs::read` → pure call
→ `fs::write`, with no filesystem traversal and nothing subprocess-shaped:

```rust
let doc = brink_intl::generate_locale(&data, checksum, src_lang, trg_lang);
let xml = xliff2::write::to_string(&doc)?;
```

`brink-web` already depends on `brink-intl` and already calls it
(`compile.rs:200`, `story_runner.rs:134`). The port is three `#[wasm_bindgen]`
functions over bytes and strings, plus adding `xliff2` to `brink-web`'s
dependencies:

- `export_xliff(inkb: &[u8], src_lang, trg_lang: Option<_>) -> String`
- `compile_locale(base: &[u8], xliff: &str, locale) -> Vec<u8>`
- `regenerate_xliff(base: &[u8], existing: &str, src_lang) -> String`

The IO moves to the TS side. `export-xliff.ts` now compiles through the
same `compile.run` road Export Story (.inkb) uses — the shared
`compiledStoryBytes` in `export.ts` — renders the bytes with the binding,
and writes through `saveBytesDialog`, the same dialog-and-write round trip
the `.inkb` export uses.

**As landed**, the three are `export_xliff` / `compile_locale` /
`regenerate_xliff` in `crates/brink-web/src/intl.rs`, wrapped as
`exportXliff` / `compileLocale` / `regenerateXliff` in `@brink-lang/web`.
Each `#[wasm_bindgen]` entry point does nothing but map the error of a plain
`Result<_, String>` inner function: `JsError::new` is a wasm import stub
that **panics on a native target**, so a `JsError`-returning signature would
make every failure path unreachable under `cargo test -p brink-web --lib`.
Splitting the seam keeps the error paths covered by the same suite as the
happy ones.

⚠ One observable change. The CLI accepts a non-`.inkb` input, compiles it in
memory, and passes checksum `0` because there is no header to read one out
of — and the desktop took exactly that branch, so its exported `.xlf`
carried `brink:checksum="0x00000000"`. Through wasm the input is always
`.inkb` bytes, so the document now carries the artifact's real CRC. Nothing
reads the attribute back (`compile_locale` stamps the `.inkl`'s
`base_checksum` from the base it is handed, never from the document), so it
is provenance only — and the real value is the useful one: it names which
compile a translator's file was cut from, which `0` cannot.

**Only one flow is actually wired.** `ALLOWED_CLI_SUBCOMMANDS`
(`src-tauri/src/lib.rs:581`) lists four subcommands, but the single UI path is
`menu:export-xliff` → `handleExportXliff` → `export-xliff.ts` → `runCli` with
`subcommand: "export-xliff"`. The other three were future-proofing that never
shipped. Porting them anyway keeps the capability without the process.

**What this deletes.** `externalBin` and `beforeBundleCommand` from
`tauri.conf.json`; `packages/brink-desktop/scripts/ensure-cli-sidecar.mjs`
(603 lines); `assert-real-sidecar.mjs`; the stub-staging half of
`src-tauri/build.rs` and `BRINK_SIDECAR_STUB`; `ALLOWED_CLI_SUBCOMMANDS`,
`validate_cli_subcommand`, the `run_cli` command and `CliOutputLine`;
`src/cli.ts`; and five dedicated test files. The desktop spec spends 111
mentions on sidecar staging — most of that apparatus goes with it.

⚠ The **`brink-cli` crate stays**. It is a published binary for terminal users
and cargo-dist ships it. Only its embedding as a Tauri sidecar is removed.

**Bonus: this unblocks iOS.** `docs/desktop-shell-spec.md` D4 names exactly two
couplings that would foreclose iOS, and the sidecar is one of them ("iOS
cannot ship subprocess binaries"). Removing it retires that blocker outright;
only `FileProvider`'s arbitrary-directory access remains.

## Stage 2 — the OTA channel (in progress)

**RULED: it sits beside the full-app updater, not instead of it.** The Tauri
updater keeps handling `src-tauri`/shell changes; OTA handles the bundle. The
app checks both.

**RULED: whole bundle, not deltas.** ~10 MB gzipped per update. The wasm
dominates and does not diff kindly; deltas can come later against measured
need.

### Where the bundle lives, and why not in the `.app`

On macOS the code signature covers everything inside the `.app`. Writing
updated assets into `Contents/Resources` invalidates it and Gatekeeper refuses
to launch. **The OTA payload must live outside the bundle**, under
`app_data_dir()`:

```
<app_data_dir>/bundles/
  current.json          {version, dir, installed_at}   — the pointer
  <version>/            extracted, read-only once active
  staging/              download + extract target, renamed into place
```

The embedded copy stays exactly as it is, as a known-good floor. A bad OTA is
recovered by deleting a directory, never by reinstalling.

### Serving — LANDED

An asynchronous URI-scheme protocol (`brink://`) on `tauri::Builder`, with
the production window pointed at it. The handler resolves each request
against the active bundle directory and **falls back to the embedded asset**
when there is no active bundle, the file is missing, or anything about the
bundle fails validation. `frontendDist` is unchanged, so the embedded floor
stays exactly as it is.

The window is built in `setup` rather than by `tauri.conf.json`
(`"create": false` on the config window, which `from_config` still supplies
every other property from). A config `url` cannot be chosen at runtime, and
**dev must keep `devUrl`** — `tauri::is_dev()` is the switch, so the vite
server and HMR are untouched. `the_main_window_is_created_in_setup_not_by_the_config`
pins both halves: `create` false, and no `url` pinned in config. That guard
matters because `create` DEFAULTS to true — the regression is a deletion,
not an edit, and it would open a second window serving embedded assets
forever while OTA silently never applied.

⚠ **The request path is the security boundary.** It comes from the webview,
and `bundles::resolve_asset` is what confines it: percent-decode first (so
`%2e%2e` is judged as what it decodes to), accept only `Component::Normal`,
reject a decoded component that still carries a separator or NUL, then
canonicalize both sides and require the result to stay inside the bundle
directory — that last step is the only one that catches a **symlink** planted
in the archive, which no string rule can see. Tested in every encoding, with
a real escaping symlink and a precondition asserting the target is genuinely
reachable, so a pass is a real escape rather than a missing file.

### The origin change — RULED, and not free

The custom scheme changes the webview's origin from `tauri://localhost` to
`brink://localhost` (`http://brink.localhost` on Windows), which strands the
studio's `localStorage`: layout, theme, keymap overrides, editor settings,
breakpoints, open tabs, problems/todos prefs **and the story save stores**.
A JS-side migration is impossible — the new origin cannot read the old one's
storage — and a shell-side one would mean parsing WKWebView/WebKitGTK/
WebView2 storage files per platform.

**RULED (2026-09-14): take the one-time loss.** The install base is the
maintainer's own. Two alternatives were priced and declined:

- **Migrate persistence to shell-side files first, then switch** — two signed
  releases, because the migration has to run on the OLD origin to see the old
  data at all. The better end state (data survives reinstalls, is inspectable,
  is backed up), and the right answer for a wider install base.
- **Keep the origin; make the embedded `index.html` a thin loader** that
  pulls JS/wasm from `brink://` with CORS — one release, no loss, but it adds
  a loader plus an asset manifest restating what vite emits, and **the loader
  itself can never be updated OTA**: a permanent hand-maintained coupling
  inside the channel whose purpose is shipping without a signed release.

⚠ This ruling is scoped to the current install base. It does not survive the
app being distributed to anyone else — at that point the migration above is
the prerequisite it always was.

### The manifest — LANDED

Served beside the full-app `latest.json`, at
`releases/download/desktop-latest/bundle-latest.json`:

```json
{
  "version": "0.7.1",
  "minShellVersion": "0.7.0",
  "url": "https://…/bundle-0.7.1.tar.gz",
  "sha256": "…",
  "signature": "…",
  "pubDate": "…"
}
```

`minShellVersion` is mandatory **at parse**, not defaulted — a manifest
without one is refused rather than treated as "any shell will do".

⚠ **`.tar.gz`, not the `.tar.zst` this spec first drafted.** `flate2` and
`tar` were already in `src-tauri`'s dependency graph (`tauri-plugin-updater`
pulls both and ships its own macOS payload as `.app.tar.gz`); `zstd` was
not, and would add a C toolchain dependency for roughly 15% off a ~10 MB
download. One archive reader now covers both channels.

`minShellVersion` is **mandatory, not advisory**. With the sidecar gone, the
only remaining native coupling is the IPC surface — 23 `#[tauri::command]`
functions. OTA'd JS that calls a command the installed shell does not have is
a hard break, and the manifest is the only place to catch it. A shell refuses
any bundle whose `minShellVersion` exceeds its own version, and says so.

### Install order — LANDED

Verification happens **before** anything is extracted:

1. download to a temp file
2. verify `sha256`
3. verify the minisign signature over the archive
4. extract into `staging/`
5. `rename` staging → `<version>/` (atomic within one filesystem)
6. write `current.json`

`bundle_update_check` is ONE IPC command rather than check/download/install
steps, because that ordering is a safety property: splitting it across calls
would put it in the webview's hands, which is exactly where an OTA'd
bundle's own JS runs. Two tests pin the ordering by asserting that a bad
hash and a bad signature each leave the destination directory **empty** — a
hostile archive that reached `staging/` is one `rename` away from being
served.

⚠ **A signature proves who built the archive, never that its contents are
well-formed.** Every entry is judged on its own after the signature passes:
`Component::Normal` only, and regular files and directories only — a symlink
or hardlink entry is refused outright rather than sanitised, because `tar`'s
own `unpack` follows links and a link is the one entry type whose target is
not the path it declares. The escape tests write the hostile name straight
into the raw tar header, since `tar::Builder` refuses to produce one through
its safe API — and so would not have tested anything.

**Activation is on next launch, not hot-swap.** The running webview already
holds the old JS and instantiated wasm; swapping underneath it is a class of
bug with no upside for a local-first editor. Offer a restart.

### Rollback — LANDED

An `attempting: <version>` sentinel is written into `current.json` before the
webview loads, and cleared by the frontend through `bundle_ready` once it is
up. A sentinel that survives a launch means that bundle did not boot: the
bundle's directory is **removed**, and the store reverts to the previous
bundle or to the embedded floor. Exactly one previous bundle is kept.

Two properties of the frontend half decide whether any of this works:

- **The confirm must be unconditional.** It runs at `main.tsx` module scope —
  reaching there already proves the bundle's JS parsed and ran, which is the
  property being witnessed. Gating it behind a project being open, or behind
  `bootLanding` resolving, rolls back a working bundle every time the author
  launches to an empty landing screen. `confirmBundleBoot` takes no argument
  it could branch on, and a test pins that arity.
- **A failed confirm errs toward the floor.** If the IPC call throws, the
  sentinel survives and the next launch reverts — the safe direction. It is
  logged rather than swallowed, because otherwise a persistently-throwing
  confirm is indistinguishable from a persistently-broken bundle.

Successive failures walk the ladder down (bundle → previous → embedded)
rather than pinning the author on a second bundle that also cannot boot.

## What still requires a signed release

`src-tauri` Rust, `tauri.conf.json`, icons, file associations, entitlements,
the plugin set, and Tauri version bumps. Plus any bundle whose
`minShellVersion` exceeds what is installed — which is the honest statement of
"this change is not OTA-able".

## Signing

The archive is minisign-signed, the same scheme the Tauri updater already
uses, verified before extract.

`signature_is_valid` mirrors `tauri-plugin-updater`'s own `verify_signature`
exactly — base64-decode both the public key and the signature into their
minisign *text* forms, then `decode` those, then `verify(.., allow_legacy:
true)`. With one keypair across both channels a different parse would reject
signatures the updater accepts, so the two agree by construction rather than
by coincidence. The public key is read from the same `tauri.conf.json` field
the updater uses, never duplicated.

**RULED: it shares the updater's existing keypair.** The recommendation on the
table was a second key, on the reasoning that the two channels have different
blast radii. That was over-thought: the two channels are served by the same
infrastructure, from the same place, to the same clients, for the same purpose
— shipping signed update payloads to an installed app. A second key buys
separability that nothing is asking for and adds a secret to rotate, store and
get wrong. `TAURI_SIGNING_PRIVATE_KEY` and the `pubkey` already in
`tauri.conf.json` cover both.

## Gates

Per CLAUDE.md's table, the diff spans rows that do not share a gate:

- `packages/brink-desktop/src-tauri/**` → `cd packages/brink-desktop/src-tauri
  && cargo test` **and** `cargo fmt --check` **in that directory** (the root
  workspace never sees it)
- `packages/brink-desktop/scripts/*.mjs` → `pnpm --filter @brink/desktop test`,
  **not** `pnpm test:scripts`
- `crates/brink-web/**` → `cargo test -p brink-web --lib` + the studio suite
- `.github/workflows/*.yml` → the workflow-YAML guards live in the excluded
  `src-tauri` workspace, so they need that directory's `cargo test` too
- deleting `ensure-cli-sidecar.mjs` removes files the CI-self-enforcing wasm
  ordering test reasons about — re-read
  `every_pnpm_install_lane_builds_wasm_first_in_the_same_job` before touching
  the workflows

## Not in scope

- Delta/patch bundles (revisit against measured download pain).
- Hot-swapping a bundle into a running webview.
- Staged rollout / percentage cohorts.
- OTA for anything native. The channel is web assets only, by construction.
