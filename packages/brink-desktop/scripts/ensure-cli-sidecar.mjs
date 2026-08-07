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

import { execSync } from "node:child_process";
import { copyFileSync, chmodSync, mkdirSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
const repoRoot = resolve(here, "../../..");
const srcTauriDir = resolve(here, "..", "src-tauri");
const binariesDir = join(srcTauriDir, "binaries");

/** The real host target triple, e.g. `aarch64-apple-darwin`. */
function hostTriple() {
  const out = execSync("rustc -vV", { encoding: "utf8" });
  const match = out.match(/^host:\s*(\S+)$/m);
  if (!match) {
    throw new Error(
      "[ensure-cli-sidecar] could not find a `host:` line in `rustc -vV` output",
    );
  }
  return match[1];
}

const triple = hostTriple();
const exeSuffix = triple.includes("windows") ? ".exe" : "";
const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target");
// The `brink-cli` package's `[[bin]]` target is named `brink` (see
// crates/brink-cli/Cargo.toml), not `brink-cli` — `cargo build -p
// brink-cli` therefore produces `target/release/brink`. The sidecar is
// staged under the `brink-cli` name regardless (matching `externalBin` in
// tauri.conf.json and the `.sidecar("brink-cli")` call in `lib.rs`); a
// sidecar's staged name is independent of its source binary's name.
const builtBin = join(targetDir, "release", `brink${exeSuffix}`);
const destBin = join(binariesDir, `brink-cli-${triple}${exeSuffix}`);

console.log("[ensure-cli-sidecar] cargo build -p brink-cli --release (root workspace)");
execSync("cargo build -p brink-cli --release", {
  cwd: repoRoot,
  stdio: "inherit",
});

if (!existsSync(builtBin)) {
  throw new Error(
    `[ensure-cli-sidecar] release build did not produce the expected binary at ${builtBin}`,
  );
}

mkdirSync(binariesDir, { recursive: true });
copyFileSync(builtBin, destBin);
// cargo's own output is already executable, but `copyFileSync` on some
// platforms/filesystems does not reliably preserve the mode bit — set it
// explicitly rather than trust the copy.
chmodSync(destBin, 0o755);
console.log(`[ensure-cli-sidecar] staged sidecar at ${destBin}`);
