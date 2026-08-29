// brink-cli sidecar preflight (docs/desktop-shell-spec.md, D3 / #2392).
//
// `brink-cli` builds a batch-ops sidecar for xliff/locale operations
// (export-xliff, compile-locale, regenerate-xliff, compile), so the desktop
// shell runs the same workspace version the editor was built from rather
// than whatever `brink` happens to be on the user's PATH. It lives in the
// ROOT cargo workspace (`crates/brink-cli`); `src-tauri` is deliberately its
// own EXCLUDED workspace (see its Cargo.toml) so Tauri's dependency graph
// never joins `cargo test --workspace`. That split makes this a
// cross-workspace build step: `cargo build -p brink-cli` must run against
// the root workspace's Cargo.toml, not `src-tauri`'s.
//
// Tauri's `externalBin` sidecar convention (bundle.externalBin in
// tauri.conf.json = ["binaries/brink-cli"]) requires the staged binary to
// carry the HOST target-triple suffix — e.g. `brink-cli-aarch64-apple-darwin`
// on Apple Silicon macOS, `brink-cli-x86_64-pc-windows-msvc.exe` on Windows —
// so this script asks `rustc` for the real host triple rather than guessing
// from `process.platform`/`process.arch`.
//
// Mirrors `ensure-wasm.mjs`'s role as a `dev`/`build` preflight, including
// passing `CARGO_TARGET_DIR` through from the environment untouched (the
// repo's shared-target conventions are a session concern, not this
// script's).
//
// The logic is EXPORTED and the standalone run sits behind a main-guard at
// the bottom (#2452). It used to be top-level imperative code, which is why
// #2418's gap 4 (this lane pays for an optimized build it never executes)
// was settled in PR #2446 with lane-scoped `CARGO_PROFILE_RELEASE_*` env
// vars rather than a profile option here: a branch inside a script with no
// export seam cannot be given a test that exercises it.
//
// #2469 spends that seam: the `stub` option below skips the build entirely
// and stages a placeholder, which is what `desktop-smoke.yml` now sets
// (`BRINK_SIDECAR_STUB: "1"` in its `env:` block) in place of those three
// `CARGO_PROFILE_RELEASE_*` vars. The stopgap only made the wasted build
// cheaper; the stub removes it. `desktop_smoke_stubs_the_staged_sidecar`
// in src-tauri/src/lib.rs is the guard that keeps that wiring in place.
//
// ⚠ The main-guard is an invariant of the `dev` preflight PAIR, not of this
// script alone — `ensure-wasm.mjs` carries the same one (#2468). See
// docs/desktop-shell-spec.md "The `dev` preflight pair".
//
// It is an invariant of the DIRECTORY too (#2478):
// src/__tests__/scripts-main-guard.test.ts scans every
// packages/brink-desktop/scripts/*.mjs rather than naming files, so a third
// preflight script is checked the moment it lands. Removing the guard line
// below fails that scan as well as this script's own tests.

import { execSync, execFileSync } from "node:child_process";
import {
  copyFileSync,
  chmodSync,
  mkdirSync,
  existsSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "../../..");
const defaultSrcTauriDir = resolve(here, "..", "src-tauri");

/**
 * Default bound for every command this module shells out to (#2697), matching
 * `ensure-wasm.mjs`'s `DEFAULT_EXEC_TIMEOUT_MS` (that module's own doc
 * comment explains the hazard: an unbounded `execSync` on this same
 * `pnpm --filter @brink/desktop dev` preflight path hangs forever on a
 * wedged proxy/toolchain fetch, with no diagnostic). 20 minutes here:
 * `hostTriple`'s `rustc -vV` is near-instant, but `ensureCliSidecar`'s
 * `cargo build -p brink-cli --release` is a real release build of the whole
 * crate's dependency graph, so the single default this module bakes in has
 * to be generous enough to cover the slower of the two commands routed
 * through it. Baked into `defaultRunCommand`'s own `execSync` call (before
 * `...options`) rather than into each call site — a caller can still
 * override it by spreading its own `timeout` in afterward.
 *
 * Overridable via BRINK_ENSURE_CLI_SIDECAR_TIMEOUT_MS (#2702 review),
 * matching scripts/setup-dev.sh's BRINK_SETUP_*_TIMEOUT convention one
 * language over (milliseconds here, not seconds, matching `execSync`'s own
 * `timeout` option's unit):
 *
 *   Knob                                   Default              On timeout
 *   ---------------------------------------------------------------------
 *   BRINK_ENSURE_CLI_SIDECAR_TIMEOUT_MS    20 * 60 * 1000 (20m)  FAIL —
 *                                                         `defaultRunCommand`
 *                                                         rethrows a
 *                                                         diagnostic naming
 *                                                         the bound and this
 *                                                         env var.
 */
export const DEFAULT_EXEC_TIMEOUT_MS = Number(process.env.BRINK_ENSURE_CLI_SIDECAR_TIMEOUT_MS) || 20 * 60 * 1000;

/**
 * Run a command and capture its stdout. The single seam through which this
 * module talks to `rustc`/`cargo`, so a caller can drive the staging logic
 * without a toolchain.
 *
 * On a timeout (the `execSync` `timeout` option firing), rethrows with a
 * house-style diagnostic naming the bound and the env var to raise (#2702
 * review): a bare `Error: Command failed: cargo build …` left a developer
 * with no indication this was a bound firing rather than a real build
 * failure.
 *
 * The discriminator is `code === "ETIMEDOUT"`, NOT `killed`. Probed on this
 * Node rather than assumed: `execSync("sleep 5", { timeout: 30 })` throws a
 * spawnSync system error with `code: "ETIMEDOUT"`, `errno: -110`,
 * `signal: "SIGTERM"`, message `spawnSync /bin/sh ETIMEDOUT` — and
 * `killed` UNDEFINED, because `killed` lives on spawnSync's RESULT object,
 * not on the error it throws. A `killed`-only predicate never fires and
 * leaves this whole branch dead; `killed` is kept as a second arm for the
 * spawn paths that do set it.
 */
export function defaultRunCommand(command, options = {}) {
  try {
    return execSync(command, { encoding: "utf8", timeout: DEFAULT_EXEC_TIMEOUT_MS, ...options });
  } catch (error) {
    if (error && (error.code === "ETIMEDOUT" || error.killed)) {
      const effectiveTimeout = options.timeout ?? DEFAULT_EXEC_TIMEOUT_MS;
      throw new Error(
        `[ensure-cli-sidecar] ✗ \`${command}\` TIMED OUT after ${effectiveTimeout}ms — likely a stalled ` +
          `proxy/toolchain fetch, or a slow cold \`cargo build -p brink-cli --release\` of the whole ` +
          `dependency graph. Retry when network is stable, or raise BRINK_ENSURE_CLI_SIDECAR_TIMEOUT_MS.`,
        { cause: error },
      );
    }
    throw error;
  }
}

/**
 * The real host target triple, e.g. `aarch64-apple-darwin`, parsed out of
 * `rustc -vV` rather than guessed from `process.platform`/`process.arch`.
 */
export function hostTriple(runCommand = defaultRunCommand) {
  const out = runCommand("rustc -vV");
  const match = out.match(/^host:\s*(\S+)$/m);
  if (!match) {
    throw new Error(
      "[ensure-cli-sidecar] could not find a `host:` line in `rustc -vV` output",
    );
  }
  return match[1];
}

/**
 * The native executable format a binary staged for `triple` has to be in:
 * `"pe"` for Windows targets, `"macho"` for Apple ones, `"elf"` for the
 * ELF-based Unixes, and `null` for a triple this script has no rule for.
 *
 * This lives here, next to `sidecarPaths`, because it IS the rule
 * `sidecarPaths` already encoded implicitly for the `.exe` suffix (#2481) —
 * "what kind of file does this triple's loader expect?" — and #2626's review
 * established that triple-derived knowledge about the staged sidecar lives
 * in this module ALONE. `assert-real-sidecar.mjs` imports this rather than
 * re-testing `triple.includes("windows")` itself (#2687);
 * `before_bundle_command_asserts_the_staged_sidecar_is_real` in
 * src-tauri/src/lib.rs fails if a second copy appears there.
 *
 * `null` is deliberate, not an oversight: a positive identity check that
 * rejects a REAL binary on some platform is worse than no check at all, so
 * an unrecognised triple must be reported as "no rule known" and let the
 * caller fall back to a weaker test, never guessed at.
 */
export function executableFormatFor(triple) {
  if (triple.includes("windows")) {
    return "pe";
  }
  // Every Apple target triple (macos/ios/tvos/watchos/visionos) carries the
  // `apple` vendor field, so one substring covers the whole family.
  if (triple.includes("apple") || triple.includes("darwin")) {
    return "macho";
  }
  if (ELF_TARGET_MARKERS.some((marker) => triple.includes(marker))) {
    return "elf";
  }
  return null;
}

/**
 * OS fields of the rustc target triples whose binaries are ELF. Not
 * exhaustive over every tier-3 target rustc knows — deliberately a list of
 * the ones a `brink-cli` sidecar could plausibly be staged for, since
 * anything absent falls through to `null` ("no rule known") rather than to a
 * wrong answer.
 */
const ELF_TARGET_MARKERS = [
  "linux",
  "android",
  "freebsd",
  "netbsd",
  "openbsd",
  "dragonfly",
  "solaris",
  "illumos",
  "fuchsia",
  "redox",
  "haiku",
];

/**
 * Where the sidecar is built and where it has to land, for one host triple.
 *
 * The `brink-cli` package's `[[bin]]` target is named `brink` (see
 * crates/brink-cli/Cargo.toml), not `brink-cli` — `cargo build -p brink-cli`
 * therefore produces `target/release/brink`. The sidecar is staged under the
 * `brink-cli` name regardless (matching `externalBin` in tauri.conf.json and
 * the `.sidecar("brink-cli")` call in `lib.rs`); a sidecar's staged name is
 * independent of its source binary's name.
 */
export function sidecarPaths({
  triple,
  repoRoot,
  srcTauriDir,
  targetDir = join(repoRoot, "target"),
}) {
  // The `.exe` suffix below is a hard Tauri `externalBin` requirement on
  // Windows triples, not a filename decoration — `ensureCliSidecar`'s `stub`
  // option has to respect that when deciding what it is safe to stage under
  // this name (see the guard next to `STUB_SIDECAR`, #2481). Asked through
  // `executableFormatFor` so "this triple's loader wants a PE" is stated
  // once in this module rather than as a bare substring test per site
  // (#2687).
  const exeSuffix = executableFormatFor(triple) === "pe" ? ".exe" : "";
  const binariesDir = join(srcTauriDir, "binaries");
  return {
    binariesDir,
    builtBin: join(targetDir, "release", `brink${exeSuffix}`),
    destBin: join(binariesDir, `brink-cli-${triple}${exeSuffix}`),
  };
}

/**
 * What gets staged instead of a real binary when `stub` is requested. A
 * loudly-failing shell script rather than an empty file: a stub lane is one
 * where nothing executes the sidecar, and if something ever starts to, it
 * must say so rather than look like a working `brink-cli`.
 *
 * POSIX-only, deliberately: `sidecarPaths` stages this under a
 * `.exe`-suffixed name on Windows triples (a hard Tauri `externalBin`
 * requirement, not a choice this script makes), and Windows loads a
 * `.exe`-named file through its PE loader regardless of the bytes inside
 * it — a `#!/bin/sh` script staged there is not "wrong content" that a
 * `.bat`/PowerShell rewrite would fix, it is a file the OS cannot start
 * running at all, so no text payload at that path can "fail loudly" the
 * way this stub does on POSIX. `ensureCliSidecar` below refuses to stage
 * this file for a Windows triple rather than ship one that silently cannot
 * do its one job (#2481; no non-Linux smoke lane exists yet to catch it —
 * see `docs/desktop-shell-spec.md` "CI coverage blind spots").
 */
export const STUB_SIDECAR = `#!/bin/sh
echo "brink-cli sidecar stub: staged by ensure-cli-sidecar.mjs (stub/BRINK_SIDECAR_STUB) so tauri-build's externalBin resolution finds a file; this is not the CLI" >&2
exit 127
`;

/**
 * Build `brink-cli` out of the ROOT workspace and stage it as the Tauri
 * sidecar. Returns the staged path.
 *
 * Every input defaults to the real one this script has always used, so the
 * standalone invocation below stays a bare `ensureCliSidecar()`.
 *
 * `stub` stages [`STUB_SIDECAR`] under the same triple-suffixed name and
 * skips `cargo build -p brink-cli --release` altogether, for a lane that
 * needs the file to EXIST (tauri-build resolves `bundle.externalBin`
 * unconditionally, not only on `tauri build`) but never runs it. It
 * defaults to the `BRINK_SIDECAR_STUB` environment variable rather than a
 * flag, because the smoke lane reaches this script two ways — its own
 * "Stage brink-cli sidecar" step and, indirectly, `pnpm build` — and only
 * an env var covers the nested one.
 *
 * `stub` throws for a Windows `triple` instead of staging anything (#2481):
 * `STUB_SIDECAR` is a POSIX `#!/bin/sh` script, and `sidecarPaths` stages it
 * under a `.exe`-suffixed name there, which Windows would load through its
 * PE loader and refuse to start — a broken file staged silently, not a
 * loudly-failing one. No lane requests a Windows stub today (the smoke lane
 * is `ubuntu-latest` only, #2428), so this only guards a host that could be
 * added later.
 */
export function ensureCliSidecar({
  repoRoot = defaultRepoRoot,
  srcTauriDir = defaultSrcTauriDir,
  targetDir = process.env.CARGO_TARGET_DIR,
  runCommand = defaultRunCommand,
  triple = hostTriple(runCommand),
  log = console.log,
  stub = process.env.BRINK_SIDECAR_STUB === "1",
} = {}) {
  const { binariesDir, builtBin, destBin } = sidecarPaths({
    triple,
    repoRoot,
    srcTauriDir,
    targetDir,
  });

  if (stub && executableFormatFor(triple) === "pe") {
    throw new Error(
      `[ensure-cli-sidecar] BRINK_SIDECAR_STUB has no Windows-compatible payload yet (#2481): ` +
        `${triple} stages under a \`.exe\`-suffixed name (see sidecarPaths), and Windows loads ` +
        `that file through its PE loader regardless of the bytes inside it, so STUB_SIDECAR's ` +
        `POSIX \`#!/bin/sh\` script — or any other text payload staged at a \`.exe\` path — could ` +
        `not run there. Build the real sidecar on this host instead of requesting a stub, or add ` +
        `a Windows-compatible stub before enabling BRINK_SIDECAR_STUB on a Windows lane.`,
    );
  }

  mkdirSync(binariesDir, { recursive: true });

  if (stub) {
    log("[ensure-cli-sidecar] stub requested — skipping the brink-cli release build");
    writeFileSync(destBin, STUB_SIDECAR);
  } else {
    log("[ensure-cli-sidecar] cargo build -p brink-cli --release (root workspace)");
    runCommand("cargo build -p brink-cli --release", {
      cwd: repoRoot,
      stdio: "inherit",
    });

    if (!existsSync(builtBin)) {
      throw new Error(
        `[ensure-cli-sidecar] release build did not produce the expected binary at ${builtBin}`,
      );
    }

    copyFileSync(builtBin, destBin);
  }

  // cargo's own output is already executable, but `copyFileSync` on some
  // platforms/filesystems does not reliably preserve the mode bit — set it
  // explicitly rather than trust the copy.
  chmodSync(destBin, 0o755);
  log(`[ensure-cli-sidecar] staged ${stub ? "stub " : ""}sidecar at ${destBin}`);
  return destBin;
}

// ---------------------------------------------------------------------------
// Universal (lipo'd) macOS staging (#2715).
//
// #2708 (PR #2714) widened `canExecuteStagedSidecar` in assert-real-sidecar.mjs
// to recognize a staged `universal-apple-darwin` triple as executable on a
// real `x86_64-apple-darwin`/`aarch64-apple-darwin` host — but that branch
// was DEAD CODE the moment it shipped: nothing anywhere in this repo ever
// staged a sidecar under that triple. `ensureCliSidecar` above always
// resolves its own default `triple` from `hostTriple()`, and even a caller
// naming `triple: "universal-apple-darwin"` explicitly would only get a
// mislabeled single-arch binary out of it — `ensureCliSidecar`'s `cargo
// build` command has no `--target` flag, so it always builds for whatever
// architecture this process is already running on and stages that under
// whatever name it's told to, real or not. A universal sidecar is not "one
// build labeled differently" — it is a fat Mach-O produced by `lipo`
// combining two REAL, independently-built single-arch binaries. This
// section is what performs that: build both Apple slices for real (each via
// its own `--target`-scoped `cargo build`), then `lipo -create` them
// together into `binaries/brink-cli-universal-apple-darwin`.

/**
 * The two real Apple single-arch triples a `universal-apple-darwin` sidecar
 * is `lipo`'d from. Not `hostTriple()`'s output for either arch — `rustc -vV`
 * never reports `universal-apple-darwin` as a `host:` value because there is
 * no such rustc target to be the host of (see `canExecuteStagedSidecar`'s own
 * doc comment in assert-real-sidecar.mjs) — these are the two real ones that
 * exist to be built.
 */
export const UNIVERSAL_DARWIN_SLICE_TRIPLES = ["x86_64-apple-darwin", "aarch64-apple-darwin"];

/**
 * Run `lipo` and return its stdout. The single seam through which
 * `stageUniversalCliSidecar` talks to the real Apple toolchain, so a test can
 * drive the staging/combining logic without `lipo` on PATH — which it never
 * is in this repo's Linux CI/dev containers; `lipo` is an Apple-toolchain
 * binary. `execFileSync`, not `execSync`, for the same reason
 * `defaultRunFile` in assert-real-sidecar.mjs uses it: every argument here is
 * a real filesystem path, not a shell command line, so there is no quoting
 * hazard to accept by going through a shell.
 *
 * `timeout`/`killSignal` are load-bearing, the same way they are on
 * `defaultRunFile` in assert-real-sidecar.mjs (and the hazard class
 * `scripts/check-scripts.mjs`'s `findUnboundedExecCalls` — #2697 — checks
 * every `packages/*\/scripts/*.mjs` exec call for): an unbounded
 * `execFileSync` here would hang this preflight forever if `lipo` ever
 * wedged, with no diagnostic.
 */
export function defaultRunLipo(args, options = {}) {
  return execFileSync("lipo", args, {
    encoding: "utf8",
    timeout: DEFAULT_EXEC_TIMEOUT_MS,
    killSignal: "SIGKILL",
    ...options,
  });
}

/**
 * Where a `cargo build -p brink-cli --release --target <triple>` build
 * lands. Distinct from `sidecarPaths`' `builtBin` (`<targetDir>/release/…`),
 * which only ever describes a build run WITHOUT `--target` — `ensureCliSidecar`'s
 * command. Passing `--target` moves cargo's output into a triple-scoped
 * subdirectory, which is real cargo behavior, not a convention this script
 * invents.
 */
function sliceBuiltBin({ targetDir, triple }) {
  const exeSuffix = executableFormatFor(triple) === "pe" ? ".exe" : "";
  return join(targetDir, triple, "release", `brink${exeSuffix}`);
}

/**
 * Build one universal-build slice for real — `cargo build -p brink-cli
 * --release --target <triple>`, explicitly cross/native-compiling for
 * `triple` rather than reusing whatever architecture this process happens to
 * be running on — and stage it under its own triple-suffixed sidecar name
 * via the same `sidecarPaths` convention `ensureCliSidecar` uses, so nothing
 * downstream (`assertRealSidecarStaged`, `canExecuteStagedSidecar`) needs a
 * separate rule for a slice built this way.
 */
function buildAndStageSlice({ repoRoot, srcTauriDir, targetDir, runCommand, triple, log }) {
  const { binariesDir, destBin } = sidecarPaths({ triple, repoRoot, srcTauriDir, targetDir });
  mkdirSync(binariesDir, { recursive: true });

  log(
    `[ensure-cli-sidecar] cargo build -p brink-cli --release --target ${triple} ` +
      "(root workspace, universal-build slice, #2715)",
  );
  runCommand(`cargo build -p brink-cli --release --target ${triple}`, {
    cwd: repoRoot,
    stdio: "inherit",
  });

  const builtBin = sliceBuiltBin({ targetDir, triple });
  if (!existsSync(builtBin)) {
    throw new Error(
      `[ensure-cli-sidecar] release build for the ${triple} universal-build slice did not ` +
        `produce the expected binary at ${builtBin} (#2715) — is \`rustup target add ${triple}\` ` +
        "installed?",
    );
  }

  copyFileSync(builtBin, destBin);
  chmodSync(destBin, 0o755);
  log(`[ensure-cli-sidecar] staged universal-build slice ${triple} at ${destBin}`);
  return destBin;
}

/**
 * Build both Apple slices and `lipo` them together into one
 * `binaries/brink-cli-universal-apple-darwin` sidecar (#2715) — the staging
 * mechanism #2708's widened `canExecuteStagedSidecar` branch had no path to
 * ever reach, because nothing produced a file under that triple at all.
 * Returns the staged universal binary's path.
 *
 * `stub` (default: `BRINK_SIDECAR_STUB=1`, matching `ensureCliSidecar`'s own
 * default) skips both the slice builds and `lipo` entirely: a stub lane's
 * whole point is running without a real toolchain, and there is no real
 * Mach-O slice to combine without one. It delegates to `ensureCliSidecar({
 * triple, stub: true, … })`, which already knows how to stage
 * `STUB_SIDECAR` under an arbitrary triple-suffixed name — no second
 * stub-staging code path is written here.
 *
 * It stages THREE files this way, not one (#2715 review): `tauri_build::build()`
 * resolves `bundle.externalBin` against the per-arch `TARGET` during EACH of
 * the two cargo passes a universal build runs, not only against the final
 * `universal-apple-darwin` name — so a stub lane that staged only the
 * universal file would still die partway through a universal build at the
 * exact unreachability #2715 was filed about, just one step later. Staging
 * both `sliceTriples` names plus `universal-apple-darwin` covers every
 * triple `tauri_build` could probe for this target.
 *
 * The non-stub path cannot be exercised end-to-end outside a real macOS host
 * with Xcode's command line tools installed (`lipo` and the
 * `x86_64-apple-darwin`/`aarch64-apple-darwin` rustc targets) — this repo's
 * CI/dev containers are Linux. `runCommand`/`runLipo` are both injectable
 * seams precisely so the staging/combining LOGIC (which slices get built,
 * what `lipo` is invoked with, where the result lands) can be driven and
 * tested without that toolchain; see `stage-universal-cli-sidecar.test.ts`'s
 * own disclosure of exactly what it does and does not prove.
 */
export function stageUniversalCliSidecar({
  repoRoot = defaultRepoRoot,
  srcTauriDir = defaultSrcTauriDir,
  targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target"),
  runCommand = defaultRunCommand,
  runLipo = defaultRunLipo,
  log = console.log,
  stub = process.env.BRINK_SIDECAR_STUB === "1",
  sliceTriples = UNIVERSAL_DARWIN_SLICE_TRIPLES,
} = {}) {
  const universalTriple = "universal-apple-darwin";

  if (stub) {
    log(
      "[ensure-cli-sidecar] stub requested for a universal build — staging the stub under " +
        `both slice triples and ${universalTriple}, no real slice builds or lipo needed (#2715)`,
    );
    let universalStubDest = "";
    for (const triple of [...sliceTriples, universalTriple]) {
      universalStubDest = ensureCliSidecar({
        repoRoot,
        srcTauriDir,
        targetDir,
        runCommand,
        log,
        stub: true,
        triple,
      });
    }
    return universalStubDest;
  }

  const slicePaths = sliceTriples.map((triple) =>
    buildAndStageSlice({ repoRoot, srcTauriDir, targetDir, runCommand, triple, log }),
  );

  const { binariesDir, destBin } = sidecarPaths({
    triple: universalTriple,
    repoRoot,
    srcTauriDir,
    targetDir,
  });
  mkdirSync(binariesDir, { recursive: true });

  log(`[ensure-cli-sidecar] lipo -create -output ${destBin} ${slicePaths.join(" ")}`);
  try {
    runLipo(["-create", "-output", destBin, ...slicePaths]);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      `[ensure-cli-sidecar] \`lipo -create\` failed combining ${slicePaths.join(" and ")} into ` +
        `${destBin} (#2715): ${message}. lipo is an Apple-toolchain binary — this staging step ` +
        "must run on a real macOS host with Xcode command line tools installed.",
      { cause: error },
    );
  }

  if (!existsSync(destBin)) {
    throw new Error(
      `[ensure-cli-sidecar] lipo reported success but ${destBin} does not exist (#2715)`,
    );
  }

  chmodSync(destBin, 0o755);
  log(
    `[ensure-cli-sidecar] staged universal sidecar at ${destBin} ` +
      `(slices: ${sliceTriples.join(", ")}) (#2715)`,
  );
  return destBin;
}

// Main-guard: `node scripts/ensure-cli-sidecar.mjs` (what the `dev` and
// `build` package scripts and the smoke lane's "Stage brink-cli sidecar"
// step run) still does the whole job, while `import`ing this module does
// nothing but hand over the functions.
//
// The dispatch below is the other half of #2715's fix: `pnpm build` is
// tauri.conf.json's `beforeBuildCommand`, and tauri-cli sets
// `TAURI_ENV_TARGET_TRIPLE` in that hook's environment to the `--target`
// the build was invoked with — for `tauri build --target
// universal-apple-darwin` that value IS the literal string
// `"universal-apple-darwin"`, the same env var `assertRealSidecarStaged` in
// assert-real-sidecar.mjs already reads for its own default triple (#2687).
// Before this, this script never consulted that env var at all — its
// default `triple` came only from `hostTriple()` — so a universal build's
// `beforeBuildCommand` always staged a single-arch, host-triple-suffixed
// sidecar no matter what `--target` was requested, and
// `stageUniversalCliSidecar` had no caller.
// #2729: `TAURI_ENV_TARGET_TRIPLE === "universal-apple-darwin"` above is the
// only entry point into `stageUniversalCliSidecar` — set only by tauri-cli's
// `beforeBuildCommand` hook during a real `tauri build --target
// universal-apple-darwin`. A macOS developer who wants to dry-run this
// staging path by hand (with or without `BRINK_SIDECAR_STUB=1`) had no way
// to reach it without faking that env var themselves. `--universal` is that
// documented entry point: `node scripts/ensure-cli-sidecar.mjs --universal`
// (wired to `pnpm --filter @brink/desktop stage:universal` in
// package.json) dispatches here the same way tauri-cli's real hook would,
// without needing a `tauri build` invocation at all. It does NOT make the
// non-stub branch runnable anywhere this repo's CI/dev containers reach —
// `lipo` and the Apple slice rustc targets are still required for that, see
// `docs/desktop-shell-spec.md` "CI coverage blind spots".
const requestsUniversal =
  process.argv.includes("--universal") ||
  process.env.TAURI_ENV_TARGET_TRIPLE === "universal-apple-darwin";

// Compared as REAL paths. `import.meta.url` is symlink-resolved by Node
// while `process.argv[1]` is not, so on macOS — where `/var` is a symlink
// to `/private/var` — a script run from anywhere under `$TMPDIR` compared
// unequal and this guard silently did not fire. That is the worst shape a
// safety check can fail in: `tauri.conf.json`'s `beforeBundleCommand` runs
// this file directly, and an inert guard ships whatever it was meant to
// stop. Wrapped because `realpathSync` throws on a path that no longer
// exists.
const invokedDirectly = (() => {
  if (!process.argv[1]) return false;
  try {
    return realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
  } catch {
    return false;
  }
})();
if (invokedDirectly) {
  if (requestsUniversal) {
    stageUniversalCliSidecar();
  } else {
    ensureCliSidecar();
  }
}
