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

import { execSync } from "node:child_process";
import { copyFileSync, chmodSync, mkdirSync, existsSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "../../..");
const defaultSrcTauriDir = resolve(here, "..", "src-tauri");

/**
 * Run a command and capture its stdout. The single seam through which this
 * module talks to `rustc`/`cargo`, so a caller can drive the staging logic
 * without a toolchain.
 */
function defaultRunCommand(command, options = {}) {
  return execSync(command, { encoding: "utf8", ...options });
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

// Main-guard: `node scripts/ensure-cli-sidecar.mjs` (what the `dev` and
// `build` package scripts and the smoke lane's "Stage brink-cli sidecar"
// step run) still does the whole job, while `import`ing this module does
// nothing but hand over the functions.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  ensureCliSidecar();
}
