# Desktop OTA web-bundle updates

**Status:** designed, not implemented. Rulings 2026-09-14 (`docs/decision-log.md`).

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

## Stage 1 — delete the sidecar (prerequisite, not an optimisation)

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

The IO moves to the TS side, through the save dialog `export-xliff.ts`
already uses for `--output`.

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

## Stage 2 — the OTA channel

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

### Serving

Register an asynchronous URI-scheme protocol on `tauri::Builder` and point the
window at it. The handler resolves each request against the active bundle
directory and **falls back to the embedded asset** when there is no active
bundle, the file is missing, or anything about the bundle fails validation.
`frontendDist` is unchanged, so the dev flow and the embedded floor both stay
as they are.

### The manifest

Served beside the full-app `latest.json`:

```json
{
  "version": "0.7.1",
  "minShellVersion": "0.7.0",
  "url": "https://…/bundle-0.7.1.tar.zst",
  "sha256": "…",
  "signature": "…",
  "pubDate": "…"
}
```

`minShellVersion` is **mandatory, not advisory**. With the sidecar gone, the
only remaining native coupling is the IPC surface — 23 `#[tauri::command]`
functions. OTA'd JS that calls a command the installed shell does not have is
a hard break, and the manifest is the only place to catch it. A shell refuses
any bundle whose `minShellVersion` exceeds its own version, and says so.

### Install order

Verification happens **before** anything is extracted:

1. download to a temp file
2. verify `sha256`
3. verify the minisign signature over the archive
4. extract into `staging/`
5. `rename` staging → `<version>/` (atomic within one filesystem)
6. write `current.json`

**Activation is on next launch, not hot-swap.** The running webview already
holds the old JS and instantiated wasm; swapping underneath it is a class of
bug with no upside for a local-first editor. Offer a restart.

### Rollback

Write an `attempting: <version>` sentinel into `current.json` before the
webview loads, and clear it from the frontend once the shell is ready (one new
IPC command). A sentinel that survives a launch means that bundle did not
boot: revert to the previous bundle, or to embedded if there is none, and
report it. Keep exactly one previous bundle.

## What still requires a signed release

`src-tauri` Rust, `tauri.conf.json`, icons, file associations, entitlements,
the plugin set, and Tauri version bumps. Plus any bundle whose
`minShellVersion` exceeds what is installed — which is the honest statement of
"this change is not OTA-able".

## Signing

The archive is minisign-signed, the same scheme the Tauri updater already
uses, verified before extract. **Open: reuse the existing updater keypair, or
mint a second one.** Reuse is one fewer secret to manage; a distinct key keeps
the trust domains separable and rotatable independently. Recommend a distinct
key — the channels have different blast radii — but this is a maintainer call
and either is defensible.

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
