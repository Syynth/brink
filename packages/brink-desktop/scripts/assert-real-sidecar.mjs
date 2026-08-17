// Refuse to let a Tauri bundle ship the STUB brink-cli sidecar (#2631).
//
// #2617 gave `src-tauri/build.rs` a way to auto-stage `STUB_SIDECAR`
// (ensure-cli-sidecar.mjs) for DEBUG builds so a fresh checkout's
// `cargo check`/`cargo test` gate does not die on `tauri-build`'s
// unconditional `bundle.externalBin` resolution. PR #2626 added the
// invariant this script exists to assert: "a real bundle must ship the real
// `brink-cli`." For `cargo tauri build` (release), `build.rs` enforces that
// directly — it only ever auto-stages a stub when `PROFILE == "debug"`. But
// `tauri build --debug` is a debug-profile BUNDLING path, and until this
// script existed the invariant held there only *indirectly*: via
// `beforeBuildCommand` -> `pnpm build` staging the real binary before
// `cargo build`/`build.rs` ever runs, plus `bundle.active: false` making
// bundling not happen at all today. Nothing asserted it — an ordering
// coincidence plus a feature flag that happens to be off (#2631).
//
// This script is wired into `tauri.conf.json`'s `beforeBundleCommand`, which
// tauri-cli runs immediately before the bundling phase of `tauri build` —
// after the crate has compiled (so `build.rs` has already run and either
// staged something or failed the build outright) and right before
// tauri-bundler reads `binaries/brink-cli-<triple>` off disk to package it.
// That is the latest point at which refusing is still useful, and — unlike
// `build.rs` — it fires only when a bundle is actually being produced, so it
// cannot mistake an ordinary `cargo check`/`cargo test` (which legitimately
// wants build.rs's auto-staged stub) for a real bundle.
//
// ⚠ `bundle.active` is `false` (D3, docs/desktop-shell-spec.md), and no CI
// lane and no documented developer command invokes `tauri build` — so this
// hook does not fire in the ordinary course of things (#2631's own ask:
// "correct and inert TODAY, and must start biting the moment someone flips
// that flag"). But that is narrower than "unreached": the firing condition
// is not just `bundle.active`. tauri-cli enters its bundling phase — and
// therefore runs this hook — on `!options.no_bundle && (config.bundle.active
// || options.bundles.is_some())` (tauri-cli's `src/build.rs`), so an
// explicit `tauri build --bundles <target>` / `-b <target>` fires it TODAY
// even with `bundle.active: false` — and an ad-hoc invocation of exactly
// that reached this hook for #2687's observation (see
// docs/desktop-shell-spec.md "Bundle-time sidecar assertion (#2631)"). D3
// flipping `bundle.active` to `true` makes the *default* `tauri build` reach
// it too; it is not the only door.
//
// Two checks, not one (#2687). The stub comparison below is a BLOCKLIST: it
// refuses the one placeholder that exists today and lets everything else
// through, so an empty, truncated, half-copied or wrong-architecture file at
// the staged path would still bundle clean — `tauri_build`'s `externalBin`
// resolution only tests that the path EXISTS. So the stub comparison is
// joined by a POSITIVE identity check: the staged file must begin with the
// executable magic its target triple's loader requires (ELF / Mach-O / PE).
// That covers the whole "the bundle shipped something that is not the CLI"
// class instead of the single placeholder we happen to have.
//
// The stub comparison is KEPT rather than replaced, because it is the only
// one of the two that can name what went wrong and what to do about it
// ("this is the stub — run `pnpm --filter @brink/desktop build`"); a bare
// "not an executable" would send a developer hunting. Stub check first
// (specific diagnosis), magic check second (general class).
//
// Detects the stub by content, not by a copied payload: `STUB_SIDECAR` is
// imported from `ensure-cli-sidecar.mjs`, the one place #2626's review
// established it may live. `before_bundle_command_asserts_the_staged_sidecar_is_real`
// in src-tauri/src/lib.rs is the guard that keeps that true for this script
// specifically — it asserts this file carries no shell-shebang payload of its own,
// the same way `build_script_stages_the_dev_sidecar_the_way_ci_does` guards
// `build.rs` (a different file; that guard does not read this one).
//
// #2699 (follow-up from #2691's own review of #2687): the magic check above
// proves the staged file's FORMAT — ELF/Mach-O/PE — not that it IS
// `brink-cli` or that it runs. #2691's own passing observation stood in
// `/bin/true` for a real `brink-cli`, and that stand-in would sail through
// the magic check exactly as a genuine wrong-build binary would. So a
// second, ADDITIONAL check runs `destBin --version`, requires exit 0, AND
// requires the printed output to actually start with `brink` (clap's
// `#[command(name = "brink", version)]` on `Cli` formats it that way for
// every real build — crates/brink-cli/src/main.rs). The content check is
// not optional decoration: GNU coreutils' `true` — #2691's own stand-in —
// ALSO accepts `--version` and exits 0 (`true (GNU coreutils) 9.4`,
// observed directly for #2699), so exit code alone would have caught
// nothing new. The whole smoke check is gated on the staged triple being
// EXECUTABLE on the triple this process is actually running on
// (`smokeCheckSidecar`, via `canExecuteStagedSidecar`): a cross-compiled
// sidecar CANNOT be executed on the build machine at all, and treating that
// failure as "not a real brink-cli" would reject a legitimate cross-build
// for the wrong reason. "Executable on" is not simply "equal to" (#2708): a
// `universal-apple-darwin` staged triple IS executable on a host whose real
// triple is `x86_64-apple-darwin` or `aarch64-apple-darwin` — a universal
// build is a fat Mach-O carrying both slices and runs natively on either
// arch, even though the staged triple never equals either host triple. See
// `canExecuteStagedSidecar` for that one deliberate widening. The
// non-executable case — including the case where the host triple itself
// cannot be determined — degrades to "verified via magic only," and SAYS SO
// in the log, rather than either failing a legitimate cross-build or silently
// claiming to have verified more than it did.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  executableFormatFor,
  hostTriple,
  sidecarPaths,
  STUB_SIDECAR,
} from "./ensure-cli-sidecar.mjs";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "../../..");
const defaultSrcTauriDir = resolve(here, "..", "src-tauri");

/**
 * The leading bytes a native executable of each format starts with. Every
 * entry is a literal on-disk byte sequence, so no endianness reasoning is
 * needed at comparison time.
 *
 * - `elf` — `\x7fELF`, the only ELF magic there is.
 * - `macho` — the four Mach-O header magics (32/64-bit, each in both byte
 *   orders) PLUS the four fat/universal-binary magics, because a macOS
 *   release binary may legitimately be a universal wrapper rather than a
 *   thin Mach-O and rejecting one would be worse than not checking.
 * - `pe` — `MZ`, the DOS stub every PE image still begins with. Two bytes
 *   only: the `PE\0\0` signature sits at a variable offset named by the
 *   header, and chasing it would buy nothing this check needs.
 */
export const EXECUTABLE_MAGIC = {
  elf: [[0x7f, 0x45, 0x4c, 0x46]],
  macho: [
    [0xfe, 0xed, 0xfa, 0xce],
    [0xce, 0xfa, 0xed, 0xfe],
    [0xfe, 0xed, 0xfa, 0xcf],
    [0xcf, 0xfa, 0xed, 0xfe],
    [0xca, 0xfe, 0xba, 0xbe],
    [0xbe, 0xba, 0xfe, 0xca],
    [0xca, 0xfe, 0xba, 0xbf],
    [0xbf, 0xba, 0xfe, 0xca],
  ],
  pe: [[0x4d, 0x5a]],
};

/**
 * Whether `bytes` begins with one of `format`'s executable magics.
 *
 * Returns `undefined` — not `false` — for a format with no rule (i.e. an
 * unrecognised triple, where `executableFormatFor` returned `null`). The
 * caller must treat that as "cannot judge" and fall back to a weaker test:
 * a positive check that REJECTS a real binary on some platform is worse
 * than the blocklist it replaces (#2687).
 */
export function looksLikeNativeExecutable(bytes, format) {
  const magics = EXECUTABLE_MAGIC[format];
  if (magics === undefined) {
    return undefined;
  }
  return magics.some(
    (magic) =>
      bytes.length >= magic.length && magic.every((byte, index) => bytes[index] === byte),
  );
}

/**
 * The two-byte prefix of an interpreter script. Only consulted on the
 * "cannot judge by magic" fallback path — either an unrecognised triple, or
 * a format `executableFormatFor` named that `EXECUTABLE_MAGIC` has no entry
 * for (see `weakFallbackCheck`) — the known-and-covered formats reject a
 * script by magic already.
 *
 * ⚠ Written as its own two bytes, NOT as a copy of `STUB_SIDECAR`'s first
 * line: #2626's review put the stub payload in `ensure-cli-sidecar.mjs`
 * alone, and `before_bundle_command_asserts_the_staged_sidecar_is_real`
 * fails this file on a shell-shebang payload appearing here.
 */
const SCRIPT_PREFIX = [0x23, 0x21];

/**
 * Human-readable name for each recognised executable format, used only in
 * log/error text. `format.toUpperCase()` alone read `macho` as `"MACHO"`
 * instead of `"Mach-O"` — the stated reason for keeping both checks is that
 * the message must be actionable, and a misspelled format name sends a
 * developer hunting (#2687).
 */
const FORMAT_DISPLAY = {
  elf: "ELF",
  macho: "Mach-O",
  pe: "PE",
};

/**
 * The weakest checks that cannot produce a false rejection of a real binary:
 * a zero-byte file is never an executable, and an interpreter script is
 * never what `cargo build -p brink-cli --release` produced. Everything else
 * is let through with a log line, not a throw.
 *
 * Shared by BOTH "cannot judge" cases (#2687 review): a triple
 * `executableFormatFor` has no rule for at all (`format === null`), and a
 * format it DID name that `EXECUTABLE_MAGIC` has no entry for
 * (`looksLikeNativeExecutable` returns `undefined`). The two functions live
 * in different files and nothing enforces that every value the first can
 * return has an entry in the second's table — today they happen to line up,
 * but a future format added to one and not the other must degrade here too,
 * not silently reject a real binary.
 *
 * Lacking format evidence is not a reason to also skip execution evidence
 * (#2699 review): this path still runs `smokeCheckSidecar`, which self-gates
 * on the staged triple matching this machine's host triple, so it cannot
 * false-reject a legitimate cross-build here any more than it can on the
 * magic-confirmed path below — it only ever adds evidence, never removes
 * the fallback's "never a rejection on format alone" guarantee.
 */
function weakFallbackCheck({ staged, destBin, triple, runFile, log, reasonNote }) {
  if (staged.length === 0) {
    throw new Error(
      `[assert-real-sidecar] ${destBin} is empty — refusing to let this bundle ship a ` +
        "zero-byte brink-cli (#2687). Stage the real binary first: run " +
        "`pnpm --filter @brink/desktop build`.",
    );
  }
  if (SCRIPT_PREFIX.every((byte, index) => staged[index] === byte)) {
    throw new Error(
      `[assert-real-sidecar] ${destBin} is an interpreter script, not a compiled ` +
        "brink-cli — refusing to let this bundle ship it (#2687). Stage the real binary " +
        "first: run `pnpm --filter @brink/desktop build`.",
    );
  }

  smokeCheckSidecar({ destBin, triple, runFile, log });

  log(
    `[assert-real-sidecar] ${destBin} is not the stub — proceeding with the bundle. ` +
      `(${reasonNote}, so its magic bytes were not checked; triple ${triple}.)`,
  );
  return destBin;
}

/**
 * Run `file` with `args` and return its captured stdout. The single seam
 * through which `runVersionSmokeTest` executes the staged sidecar, so a
 * caller can drive the smoke-check logic without actually spawning a
 * process (see `assert-real-sidecar.test.ts`'s `runFile` injections).
 * `execFileSync` (not `execSync`) deliberately — `destBin` is a real
 * filesystem path, not a shell command line, and going through a shell
 * would add quoting hazards this check has no reason to accept.
 *
 * `timeout`/`killSignal` are load-bearing, not defensive decoration: this
 * deliberately executes a staged binary of unknown provenance at bundle
 * time, and a staged file that ignores `--version` and loops would hang the
 * bundle indefinitely without them — against the repo's "guard against
 * unbounded growth" rule. A blocked stdin read is not the risk (the child's
 * stdin is a closed pipe here, so a `cat`-shaped blocker returns almost
 * immediately) — an infinite-looping binary is.
 */
function defaultRunFile(file, args) {
  return execFileSync(file, args, { encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" });
}

/**
 * The triple `rustc` reports for the machine actually running this script —
 * queried independently of whatever `triple` the caller is checking a
 * staged sidecar against. `undefined` (never thrown) when it cannot be
 * determined, e.g. no `rustc` on PATH: `smokeCheckSidecar` treats that the
 * same as a genuine cross-build — "cannot safely execute this," not
 * "reject" — since a script that cannot even find `rustc` has no basis for
 * either running the binary or judging it broken for not running.
 */
function actualHostTriple() {
  try {
    return hostTriple();
  } catch {
    return undefined;
  }
}

/**
 * The real host triples on which a `universal-apple-darwin` sidecar can
 * actually run. `rustc -vV`'s `host:` line — what `actualHostTriple()`
 * queries — never reports `universal-apple-darwin` itself: there is no
 * `--target universal-apple-darwin` rustc target to be the host of, only
 * these two single-arch ones. A universal build stages under
 * `universal-apple-darwin` regardless of which of these built it, since it
 * is one fat Mach-O carrying both slices (#2708).
 */
const UNIVERSAL_DARWIN_HOST_TRIPLES = new Set(["x86_64-apple-darwin", "aarch64-apple-darwin"]);

/**
 * Whether a sidecar staged for `triple` can actually be executed on a
 * machine whose real host triple is `host` (`undefined` when it could not
 * be determined at all — see `actualHostTriple`).
 *
 * The ordinary case is an exact match. The one deliberate exception
 * (#2708): `triple === "universal-apple-darwin"` on a real macOS host
 * (`host` one of [`UNIVERSAL_DARWIN_HOST_TRIPLES`]). A universal macOS
 * build stages under the triple `universal-apple-darwin`, which
 * `triple === host` never equals — not even when running ON a macOS host
 * that unambiguously CAN execute it — because a universal binary is a fat
 * Mach-O carrying BOTH the x86_64 and aarch64 slices, and the kernel picks
 * the matching one to run natively on either arch. Without this exception
 * `smokeCheckSidecar` would silently and PERMANENTLY take the
 * "cannot execute here" branch for the exact artifact macOS users install,
 * on every macOS machine there is, never once running `--version` against
 * it for real.
 *
 * Deliberately does NOT widen the other direction: `universal-apple-darwin`
 * on a non-Darwin `host` (or `host === undefined`) still returns `false` —
 * that IS a genuine cross-build (no non-Apple machine can execute a Mach-O
 * of any kind), and `smokeCheckSidecar` must still degrade to the
 * magic-only fallback for it, never collapse "cannot execute here" into a
 * rejection (the same tri-state lesson #2687's review already caught one
 * `looksLikeNativeExecutable` call site getting wrong via `!== true`).
 */
export function canExecuteStagedSidecar(triple, host) {
  if (host === undefined) {
    return false;
  }
  if (triple === host) {
    return true;
  }
  return triple === "universal-apple-darwin" && UNIVERSAL_DARWIN_HOST_TRIPLES.has(host);
}

/**
 * Execute the staged sidecar as `destBin --version` and report success or
 * failure. Never throws itself — `smokeCheckSidecar`, its only caller,
 * decides what a failure means. Exported for direct unit coverage of the
 * success/failure summarizing; NOT meant to be called on a binary staged
 * for a triple that is not executable on this host — see
 * `canExecuteStagedSidecar` (#2708 — not simply "a different triple": a
 * `universal-apple-darwin` staged triple IS executable on either single-arch
 * Apple host) and `smokeCheckSidecar`, which is the only place that decides
 * it is safe to call this at all.
 */
export function runVersionSmokeTest({ destBin, runFile = defaultRunFile }) {
  try {
    const stdout = runFile(destBin, ["--version"]);
    return { ok: true, output: String(stdout).trim() };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const stderr =
      error && typeof error === "object" && "stderr" in error && error.stderr
        ? String(error.stderr).trim()
        : "";
    return { ok: false, detail: stderr ? `${message}: ${stderr}` : message };
  }
}

/**
 * Whether `output` — the trimmed stdout from `destBin --version` — plausibly
 * identifies the binary as `brink-cli`, not merely as SOME program that
 * happens to accept `--version` and exit 0. `clap`'s
 * `#[command(name = "brink", version)]` (crates/brink-cli/src/main.rs)
 * formats `--version` output as `<name> <version>` (e.g. `brink 0.0.11`),
 * so the check is the leading token, not a full-string match against a
 * version number this script has no business pinning.
 *
 * This exists because exit code ALONE is not sufficient evidence: GNU
 * coreutils' `true` — the exact stand-in #2691's own passing observation
 * used for `brink-cli` — ALSO accepts `--version` and exits 0. Observed
 * directly for #2699: `true --version` prints `true (GNU coreutils) 9.4`
 * and exits 0; `brink --version` (a real release build) prints
 * `brink 0.0.11` and exits 0. An exit-code-only smoke check would pass
 * both, which defeats the entire point of adding it.
 */
export function looksLikeBrinkCliVersionOutput(output) {
  return output === "brink" || output.startsWith("brink ");
}

/**
 * Runs (or explains skipping) the `--version` executable smoke check for a
 * sidecar that has already passed the magic-byte identity check above, and
 * logs exactly which branch ran. Throws only when the sidecar WAS executed
 * and failed; a skip is never a failure (#2699).
 *
 * The magic check proves the staged file's FORMAT; it cannot prove the file
 * IS `brink-cli` or that it runs — any correctly-formatted binary of the
 * wrong build passes it, exactly as #2691's own `/bin/true` stand-in did.
 * This closes that gap for the one case where actually running the binary
 * is safe to attempt: `destBin`'s triple is executable on the triple this
 * process is running on RIGHT NOW (`canExecuteStagedSidecar`, #2708 — not
 * simply "matches": a `universal-apple-darwin` staged triple IS executable
 * on either single-arch Apple host). Any triple that is not executable here
 * is a cross-build — the staged binary cannot be executed on this machine
 * at all, and a failure to do so says nothing about whether the binary is a
 * genuine, working `brink-cli`. Collapsing that "cannot execute here" case
 * into a rejection would refuse a legitimate cross-compiled bundle for the
 * wrong reason, so it instead degrades to the magic check alone and logs
 * that it did — never silently.
 *
 * Deliberately the ONLY call site that inspects executability-on-this-host
 * at all: #2687's review caught a `looksLikeNativeExecutable !== true` call
 * site elsewhere in this file silently collapsing "cannot judge" into
 * "reject." This function returns nothing to its caller — success is
 * "returned normally," a skip is also "returned normally," and only an
 * executed failure throws — so there is no boolean for a second call site
 * to ever mis-test the same way. Since #2708 the executability test is
 * `canExecuteStagedSidecar(triple, host)`, not a bare `host === triple` —
 * see that function for the one deliberate widening (a universal macOS
 * build on a real macOS host) and why the reverse direction is NOT widened.
 */
function smokeCheckSidecar({ destBin, triple, runFile, log }) {
  const host = actualHostTriple();
  if (!canExecuteStagedSidecar(triple, host)) {
    log(
      `[assert-real-sidecar] skipped the --version smoke check: staged triple ${triple} ` +
        (host === undefined
          ? "cannot be compared against a host triple (rustc unavailable)"
          : `does not match this machine's host triple (${host})`) +
        " — a sidecar built for a different target cannot be executed here, so running it " +
        "would prove nothing about whether it is a real brink-cli. Verified via " +
        "executable-format magic only (#2699).",
    );
    return;
  }

  const result = runVersionSmokeTest({ destBin, runFile });
  if (!result.ok) {
    throw new Error(
      `[assert-real-sidecar] ${destBin} carries valid ${triple} executable magic but failed ` +
        `to run (\`--version\`: ${result.detail}) — refusing to let this bundle ship a binary ` +
        "that does not work (#2699). Rebuild it: `pnpm --filter @brink/desktop build` (or " +
        "`cargo build -p brink-cli --release` directly), and check BRINK_SIDECAR_STUB is unset.",
    );
  }

  if (!looksLikeBrinkCliVersionOutput(result.output)) {
    throw new Error(
      `[assert-real-sidecar] ${destBin} ran (\`--version\` exited 0) but printed ` +
        `"${result.output}" — that is not a brink-cli version string, so whatever executed is ` +
        "not brink-cli even though it runs and carries valid executable magic (#2699; exit " +
        "code alone would have missed this — GNU coreutils' `true`, #2691's own stand-in for " +
        "brink-cli, also accepts --version and exits 0). Refusing to let this bundle ship it. " +
        "Rebuild it: `pnpm --filter @brink/desktop build`, and check BRINK_SIDECAR_STUB is unset.",
    );
  }

  log(
    `[assert-real-sidecar] ${destBin} ran successfully (${
      triple === host ? "host-triple match" : "universal build on this host's arch, #2708"
    }, \`--version\` exited 0 and printed "${result.output}") — confirmed a working brink-cli, ` +
      "not merely a correctly-formatted file (#2699).",
  );
}

/**
 * Throw unless the brink-cli sidecar staged for `triple` is a real native
 * executable for that triple — i.e. it is not `STUB_SIDECAR`, AND it begins
 * with the executable magic `triple`'s loader requires (#2687). Returns the
 * staged path on success.
 *
 * The magic check is skipped, with a logged note (`weakFallbackCheck`), for
 * a triple `executableFormatFor` has no rule for AND for a format it DID
 * name that `EXECUTABLE_MAGIC` has no entry for; the stub comparison and an
 * empty/interpreter-script rejection still apply in both cases. That
 * asymmetry is the point — this check must never be the reason a genuine
 * `brink-cli` fails to bundle on a platform nobody anticipated, whether
 * because the triple is unrecognised or because the two format tables
 * drifted apart.
 *
 * `triple` defaults to `TAURI_ENV_TARGET_TRIPLE` when tauri-cli set it, and
 * only falls back to `hostTriple()` when it did not (a standalone/manual
 * invocation, not through `beforeBundleCommand`). In the released
 * tauri-cli this hook runs under, `TAURI_ENV_TARGET_TRIPLE` is exactly the
 * `--target` triple `tauri-bundler` is about to package — `command_env()`
 * merges it in from `app_settings.target_triple`
 * (`interface/rust.rs`), which is built from the real `--target` the
 * build was invoked with. So a cross-compiled `--target` bundle IS covered
 * when run through `tauri build`; the host-only limit belongs to
 * `build.rs`/`ensure-cli-sidecar.mjs`, which have no better source for a
 * triple than the host they're running on — it is not a limit this hook
 * inherits or needs to repeat.
 *
 * Once the staged file clears the stub comparison, `smokeCheckSidecar`
 * (#2699) additionally runs `destBin --version` when `triple` is executable
 * on the triple this process is running on (`canExecuteStagedSidecar`,
 * #2708 — not simply equal: a `universal-apple-darwin` staged triple is
 * executable on either single-arch Apple host) — on BOTH the magic-confirmed
 * path
 * below and the weak-fallback path (`weakFallbackCheck`, for a triple with
 * no format rule or a format `EXECUTABLE_MAGIC` has no entry for), closing
 * the "correctly-formatted but not actually `brink-cli`" gap the magic
 * check alone leaves open, and — on the fallback path — closing the "zero
 * format evidence AND zero execution evidence" gap that lacking a magic
 * rule would otherwise leave wide open. `runFile` is that check's
 * process-execution seam, defaulted to a real `execFileSync` call and
 * overridable for tests the same way `log` is.
 */
export function assertRealSidecarStaged({
  repoRoot = defaultRepoRoot,
  srcTauriDir = defaultSrcTauriDir,
  triple = process.env.TAURI_ENV_TARGET_TRIPLE ?? hostTriple(),
  log = console.log,
  runFile = defaultRunFile,
} = {}) {
  const { destBin } = sidecarPaths({ triple, repoRoot, srcTauriDir });

  if (!existsSync(destBin)) {
    // `tauri_build::build()` resolves `bundle.externalBin` unconditionally
    // and would already have failed the crate's compile step on a missing
    // file, well before this hook (which runs after a successful compile)
    // could fire — so this branch should be unreachable in practice. It is
    // a defensive backstop, not the load-bearing check below.
    throw new Error(
      `[assert-real-sidecar] no brink-cli sidecar staged at ${destBin} — refusing to let ` +
        "a bundle proceed with none. `tauri-build`'s externalBin resolution should have " +
        "already failed the build on this; if it did not, run `pnpm --filter @brink/desktop " +
        "build` to stage one before bundling.",
    );
  }

  const staged = readFileSync(destBin);
  const stub = Buffer.from(STUB_SIDECAR, "utf8");
  if (Buffer.compare(staged, stub) === 0) {
    throw new Error(
      `[assert-real-sidecar] ${destBin} is the STUB brink-cli placeholder ` +
        "(ensure-cli-sidecar.mjs's STUB_SIDECAR, #2617) — refusing to let this bundle ship " +
        "it (#2631). Stage the real binary first: run `pnpm --filter @brink/desktop build` " +
        "(or `node scripts/ensure-cli-sidecar.mjs` directly) with BRINK_SIDECAR_STUB unset.",
    );
  }

  const format = executableFormatFor(triple);
  if (format === null) {
    // No magic rule for this triple at all.
    return weakFallbackCheck({
      staged,
      destBin,
      triple,
      runFile,
      log,
      reasonNote:
        "no executable-format rule for this triple (see executableFormatFor in " +
        "ensure-cli-sidecar.mjs)",
    });
  }

  const identity = looksLikeNativeExecutable(staged, format);
  if (identity === undefined) {
    // executableFormatFor named a format, but EXECUTABLE_MAGIC has no entry
    // for it — "cannot judge", the same as format === null above, NOT
    // "judged and rejected". Collapsing this into a throw would refuse a
    // genuine binary the moment the two tables drift (#2687 review).
    return weakFallbackCheck({
      staged,
      destBin,
      triple,
      runFile,
      log,
      reasonNote: `no executable magic is known for format "${format}"`,
    });
  }

  if (identity === false) {
    const leading = [...staged.subarray(0, 4)]
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join(" ");
    const display = FORMAT_DISPLAY[format];
    throw new Error(
      `[assert-real-sidecar] ${destBin} does not begin with ${display} executable magic ` +
        `(first bytes: ${leading || "<empty file>"}) — ${triple} requires ${display} format ` +
        "for its binaries, so whatever is staged there is not a runnable brink-cli for this " +
        "target (#2687). Refusing to let this bundle ship it. Stage the real binary first: " +
        "run `pnpm --filter @brink/desktop build`, and check BRINK_SIDECAR_STUB is unset.",
    );
  }

  smokeCheckSidecar({ destBin, triple, runFile, log });

  log(
    `[assert-real-sidecar] ${destBin} is not the stub and carries ${FORMAT_DISPLAY[format]} ` +
      "executable magic — proceeding with the bundle.",
  );
  return destBin;
}

// Main-guard: `node scripts/assert-real-sidecar.mjs` (what `tauri.conf.json`'s
// `beforeBundleCommand` runs) does the whole job, while `import`ing this
// module does nothing but hand over the function — same idiom as
// `ensure-cli-sidecar.mjs`/`ensure-wasm.mjs`, enforced repo-wide for this
// directory by `src/__tests__/scripts-main-guard.test.ts` (#2478).
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  assertRealSidecarStaged();
}
