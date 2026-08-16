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
// ⚠ Inert today, by design (#2631's own ask: "correct and inert TODAY, and
// must start biting the moment someone flips that flag"). `tauri.conf.json`'s
// `bundle.active` is `false` (D3, docs/desktop-shell-spec.md), so tauri-cli's
// bundling phase — and therefore this hook — never runs; nothing in this
// repo invokes `tauri build`/`tauri build --debug` at all yet, in CI or in a
// documented developer command. The moment D3 flips `bundle.active` to
// `true` and something actually runs `tauri build` (debug or release), this
// hook starts firing before every single bundle, unconditionally.
//
// Detects the stub by content, not by a copied payload: `STUB_SIDECAR` is
// imported from `ensure-cli-sidecar.mjs`, the one place #2626's review
// established it may live (its own `build_script_stages_the_dev_sidecar_the_way_ci_does`
// guard in src-tauri/src/lib.rs fails on a second copy of the payload
// appearing anywhere, including here).

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { hostTriple, sidecarPaths, STUB_SIDECAR } from "./ensure-cli-sidecar.mjs";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "../../..");
const defaultSrcTauriDir = resolve(here, "..", "src-tauri");

/**
 * Throw unless the brink-cli sidecar staged for `triple` is real content,
 * not `STUB_SIDECAR`. Returns the staged path on success.
 *
 * Only the HOST triple is checked (`hostTriple()` by default) — the same
 * scope `build.rs`'s auto-staging accepts (it skips staging outright for a
 * cross-target `cargo test/check --target <other>`, per its `HOST` vs
 * `TARGET` comparison). A cross-compiled bundle is not covered by either
 * mechanism yet; that is a pre-existing gap, not one this script widens.
 */
export function assertRealSidecarStaged({
  repoRoot = defaultRepoRoot,
  srcTauriDir = defaultSrcTauriDir,
  triple = hostTriple(),
  log = console.log,
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

  log(`[assert-real-sidecar] ${destBin} is not the stub — proceeding with the bundle.`);
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
