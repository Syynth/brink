# Desktop OTA web-bundle updates

**Status:** Stages 1-3 LANDED, end to end — sidecar deleted and intl moved
into wasm, the client half (store, serving, rollback, update channel), and the
release pipeline that produces a bundle are all on `main`. **Stage 4 is
SPECIFIED, not built**, and blocks the first signed release: it is the work
that must land while the shell's IPC surface is still free to change. Rulings
2026-09-14 and 2026-09-15 (`docs/decision-log.md`).

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

## Stage 2 — the OTA channel — DONE

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

## Publishing a bundle — LANDED

`.github/workflows/bundle-release.yml`, on a `bundle-v*` tag. **No cargo, no
`tauri build`, no codesign, no notarization** — that absence is the whole
point. It builds both wasm modules and `dist/`, archives it, signs the
archive with `TAURI_SIGNING_PRIVATE_KEY`, and publishes the archive to its
own release plus `bundle-latest.json` to the `desktop-latest` alias the app
polls. The archive keeps an immutable per-release URL while the manifest is
republished in place — the same split `latest.json` already uses.

### The two values no build step can compute

Both live in `packages/brink-desktop/ota-bundle.json`, hand-maintained.

**`version` is an INDEPENDENT sequence** (RULED 2026-09-14), not the app's.
Reusing `tauri.conf.json`'s version would make two web-only updates between
signed releases impossible: the second would carry the same version as the
first, and the shell would report "up to date". The release tag must match
it, checked in the workflow.

**`minShellVersion` is a judgement, and the naive default is actively
wrong.** "The app version that built this bundle" would refuse the bundle on
every install not already on the newest app release — precisely the
population OTA exists to serve. It rises only when the IPC surface loses or
changes a command an already-published bundle could call.

Deriving it automatically was considered and declined: a fingerprint cannot
tell an ADDED command (backward-compatible — old bundles never call it) from
a REMOVED or RENAMED one (breaking), so an automatic bump would refuse
bundles that are perfectly safe. Instead
`min_shell_version_is_reconsidered_when_the_ipc_surface_changes` pins a
`commandsFingerprint` and fails when the surface moves without it being
reconsidered — **the failure is a red check on the author's machine rather
than a broken app on someone else's.** A second guard refuses a
`minShellVersion` above the app version shipping it, which would be refused
by every install including one built from that very commit.

### What the pipeline refuses to publish

- a `dist/` with no `index.html`, or fewer than two wasm modules — an empty
  bundle would tar, hash and sign perfectly happily and then serve nothing;
- an archive with an empty signature — `build-bundle-manifest.mjs` throws
  rather than emit one, because an unsigned bundle is refused by every
  install *silently*, a refusal being indistinguishable from any other
  failed check;
- a tag that disagrees with `ota-bundle.json`'s version;
- a published manifest whose `url` does not resolve — which would read to
  the author as "update check failed", forever.

## Stage 4 — one update, policy, channels, in-session activation

Stages 1–3 shipped a channel that works but is visibly a *second* mechanism:
its own menu path, its own toast, its own vocabulary. Stage 4 makes it one
update from the author's side, and adds the policy surface that makes a
self-updating app tolerable to live with.

**RULED 2026-09-15, and it governs everything below: from the author's
perspective there is either an update or there isn't.** Which channel carries
it is our problem, not theirs. No toast, menu item or setting names "bundle"
or "shell". We still do the right thing per channel — a bundle-only update
must not pay for a process restart — but that is an implementation choice
hidden behind one verb.

### Why all of this lands before the first signed release

`desktop-v0.8.0` is the first release carrying an OTA client at all, so today
the shell's IPC surface and on-disk schemas are unconstrained: nothing in the
field consumes them. The moment it ships, every change here becomes a
`minShellVersion` bump that strands the installs we just created.

That draws a sharp line, and it is the organising principle of the stage:

- **Rust, IPC and on-disk schema must be right at 0.8.0.**
- **Anything purely frontend ships afterwards, over OTA** — settings panels,
  the version picker, the unified toast. That is the channel doing its job.

The one exception is anything that must survive a broken bundle. A control
rendered *by* the bundle cannot rescue you from the bundle; see "the escape
hatch" below.

### Consent

**RULED: ask before installing.** Stage 2 shipped `bundle_update_check` as
check-and-install in one call. That was inconsistent with the full-app
channel's standing "nothing installs without consent" (2026-08-22), and under
the one-update rule an inconsistency the author can feel is a bug. The command
splits: a check that reports what is available, and an apply that acts on a
yes.

### Activation without a process restart

A bundle is web assets. Restarting the OS process to pick them up is a cost
with no cause — but three things made it the only safe option in Stage 2, and
all three are fixed here rather than worked around:

1. **`BundleRuntime.dir` was captured once, in `setup`.** A reload re-requested
   assets from the same directory, so a swap was inert. It gains interior
   mutability and `serve_bundle_asset` reads it per request.
2. **The rollback sentinel was per process launch** — stamped in `setup`,
   cleared by the frontend. Swapping mid-session left a new bundle
   unwitnessed, which is the one property the sentinel exists for. The
   handshake moves to per *activation*.
3. **`index.html` is the only unhashed URL in a bundle.** Every other asset is
   content-hashed by vite and therefore cannot go stale, but a cached entry
   document would keep pointing at the old hashed entry. It is served
   `Cache-Control: no-store`.

Workers are not an obstacle, which is worth stating because it looks like one.
Both worker trees (`ink-editor`'s session worker, `brink-studio`'s prose
worker) are constructed from content-hashed URLs, and a full reload destroys
the document — terminating every worker with it. Nothing survives the swap, so
nothing can be stale. What *is* lost is in-memory editor state, exactly as a
relaunch loses it, so activation is still gated on `awaitSaveAllBeforeQuit`.
It is cheaper than a restart, not free.

### The handshake

**RULED: confirm on editor-mounted, not on parsed.** Stage 2 cleared the
sentinel when the bundle's JS reached module scope. That proves it parsed and
nothing more: a bundle that parses but cannot mount the editor clears its own
sentinel and is never rolled back — precisely the "locked on the welcome
screen" case.

The confirm moves to the point the studio surface actually exists, with a
generous timer, and `bundle-boot.ts`'s standing warning still applies with
full force: *the confirm must not be conditional on anything that can fail.*
A deeper signal was considered and declined — confirming on "a project opened"
would roll back a perfectly good bundle every time an author launches to an
empty landing screen.

### The escape hatch

**A rollback control rendered by the bundle cannot rescue you from a broken
bundle.** It is made of the thing that is broken. So the primary affordance is
a native menu item in the shell, which works when the webview renders nothing
at all. A version list inside the studio is a convenience layered on top,
never the only door.

### Update policy

**RULED: one enum, because two booleans can express a state that must not
exist.** An author cannot be pinned *and* on auto-update; representing that
and then defending against it is worse than making it unrepresentable.

```rust
enum UpdatePolicy {
    Auto   { channel: Stable | Beta },   // everything moves
    Manual { channel: Stable | Beta },   // the author is asked first
    Pinned { version: String },          // nothing moves
}
```

Persisted in `AppSettings` (`settings.json`), whose existing `#[serde(default)]`
plus ignore-unknown-keys discipline already guarantees that a settings file
written by a *future* bundle will not reset an older shell's knobs — the exact
property a self-updating app needs.

`Pinned` is a channel rather than a flag on one. That is not a naming
preference: it means "a pin suspends updates" stops being a rule anybody has
to implement. A pinned install has no manifest to consult, so a check finds
nothing by construction rather than by suppression.

**`Pinned` stops the shell too, and that is what makes pinning safe.**
`minShellVersion` protects a new bundle from an old shell; there is no
symmetric guard protecting an old pinned bundle from a *new* shell that has
since renamed or removed a command it calls. Rather than invent a
`maxShellVersion` — or misuse `commandsFingerprint`, which changes on
backward-compatible additions and would refuse pins that are perfectly fine —
`Pinned` freezes both channels. The shell cannot move out from under a pinned
bundle because the shell does not move. The hazard is designed out, not
documented.

Everything degrades toward "does not do what you wanted" and never toward a
brick:

- A manual check while pinned **reports the pin** rather than offering an
  update that would strand the author.
- The version picker **refuses an incompatible version at the point of
  choosing**, from the index's own `minShellVersion`, rather than letting it
  be selected and discovered afterwards.
- Leaving `Pinned` restores everything. It is a door, not a trapdoor.

### Channels, and one index

**RULED: a channel switch is an install, not an update.** `decide()` installs
only strictly-newer versions, so moving beta → stable would otherwise be
refused forever (a beta `0.2.0-beta.1` sorts above a stable `0.1.9`). Treating
a switch as "resolve the target version for this channel, install it if
absent, activate it" sidesteps ordering entirely — and it is the same path
that serves picking a past version, with the version chosen explicitly instead
of by recency.

All three policies therefore share one resolve-and-activate path.

`bundle-latest.json` is replaced by a single append-only index:

```json
{ "entries": [
  { "version": "0.0.4", "channel": "stable",
    "url": "…/bundle-v0.0.4/bundle.tar.gz", "sha256": "…",
    "signature": "…", "minShellVersion": "0.8.0", "publishedAtMs": 0 }
] }
```

One fetch serves all three jobs — latest-for-my-channel (the newest matching
entry), the picker list, and resolving a pinned version's URL. At roughly 300
bytes an entry it is cheaper than the two round trips a split design would
cost, and the release workflow republishes exactly one file.

**Trust is unchanged.** Each entry carries its own archive's signature, and
`verify_and_unpack` checks that against the public key in `tauri.conf.json`.
A tampered index can at worst offer a differently-signed valid bundle or one
that fails verification; it cannot introduce unsigned code. The index itself
needs no signature of its own.

It is capped — both by the existing `MAX_MANIFEST_BYTES` fetch bound and by an
entry-count limit, per the repo's standing guard against unbounded growth.

### Retention

**RULED: keep three, and exempt the pin.** Bundles are tens of megabytes
unpacked, so history is not free. Three covers rollback plus a usable recent
history.

The exemption is load-bearing rather than tidy: with plain "keep the 3 most
recent", a pin set three updates ago is pruned out from under the author, and
the forever-bundle they chose silently vanishes. `promote` keeps the three
most recent **plus the pinned version when it is not among them**.

A version that is picked but not present is downloaded on demand — the index
entry carries everything `verify_and_unpack` needs, so it is an ordinary
install with the version named rather than inferred.

### Spellchecking (it belongs to this stage, for a reason that is not obvious)

`brink-prose` (Harper) is 11.9 MB of wasm, 6.15 MB gzipped, against the whole
compiler's 2.61 MB. The OTA budget is ~10 MB per update, so Harper is roughly
60% of every download.

`prose-checker.ts` lazily `import()`s it so nobody pays unless they write a
sentence — **but OTA ships the tree as one archive, so code-splitting buys
nothing here.** Every OTA download pays for Harper whether or not the author
ever types prose. That is what makes an apparently unrelated editor-quality
question part of this stage.

Measured, the weight is the grammar rules, not the dictionary:

| part | size |
|---|---|
| `src/linting` (307 rule files) | 4.8 MB |
| `dictionary.dict` | 769 KB |
| `src/spell` | 132 KB |

So "Harper for grammar, the OS for spelling" is entirely possible — `LintGroup`
is per-rule configurable — but it is a **quality** decision with no size
dividend, because the grammar rules are the payload. The dictionary cannot be
dropped in either direction regardless: `lib.rs`'s `Document::new` does its own
word lookup while tokenising, which every grammar rule depends on.

That resolves the sequencing cleanly:

- **0.8.0 ships the OS spellcheck IPC command**, shaped to feed the existing
  `ProseChecker` interface (`packages/ink-editor/src/prose.ts`). Small, and it
  freezes the surface while freezing is free.
- **Which checker does what is decided later and shipped over OTA**, because it
  is entirely bundle-side and therefore reversible.

Fetching Harper on demand as a separately-signed payload stays open and looks
considerably better than it did: 4.8 MB of grammar rules is a real opt-in
rather than a rounding error.

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
- Hot-swapping a bundle into a running webview *without a reload*. Stage 4
  swaps the pointer and reloads, which discards the document (and with it
  every worker); it does not replace modules under a live page.
- Staged rollout / percentage cohorts.
- OTA for anything native. The channel is web assets only, by construction.
