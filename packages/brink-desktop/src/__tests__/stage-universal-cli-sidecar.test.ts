import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  sidecarPaths,
  stageUniversalCliSidecar,
  STUB_SIDECAR,
  UNIVERSAL_DARWIN_SLICE_TRIPLES,
} from "../../scripts/ensure-cli-sidecar.mjs";

// #2715: #2708's widened `canExecuteStagedSidecar` branch (PR #2714) is
// correct but was, until this PR, UNREACHABLE — nothing anywhere staged a
// sidecar under the `universal-apple-darwin` triple, so
// `triple === "universal-apple-darwin"` never held. This suite drives
// `stageUniversalCliSidecar` — the mechanism that makes it reachable — the
// same way `ensure-cli-sidecar.test.ts` drives `ensureCliSidecar`:
// `runCommand`/`runLipo` stand in for `cargo`/`lipo`, so the staging and
// combining LOGIC runs end to end without an Apple toolchain.
//
// Disclosed plainly, per house rule (w174): this environment is a Linux
// container. `lipo`, the `x86_64-apple-darwin`/`aarch64-apple-darwin` rustc
// targets, and a real `tauri build --target universal-apple-darwin` are all
// unavailable here. Nothing in this file claims to have run any of them —
// every "built"/"combined" binary below is a fake written by an injected
// `runCommand`/`runLipo`, standing in for a real cargo/lipo invocation the
// same way `stagedRepoRoot()` does for `ensureCliSidecar` in the sibling
// suite.

const temporaries: string[] = [];

function scratch(): string {
  const dir = mkdtempSync(join(tmpdir(), "stage-universal-cli-sidecar-"));
  temporaries.push(dir);
  return dir;
}

let originalCargoTargetDir: string | undefined;
let originalBrinkSidecarStub: string | undefined;

beforeEach(() => {
  originalCargoTargetDir = process.env.CARGO_TARGET_DIR;
  delete process.env.CARGO_TARGET_DIR;
  originalBrinkSidecarStub = process.env.BRINK_SIDECAR_STUB;
  delete process.env.BRINK_SIDECAR_STUB;
});

afterEach(() => {
  if (originalCargoTargetDir === undefined) {
    delete process.env.CARGO_TARGET_DIR;
  } else {
    process.env.CARGO_TARGET_DIR = originalCargoTargetDir;
  }
  if (originalBrinkSidecarStub === undefined) {
    delete process.env.BRINK_SIDECAR_STUB;
  } else {
    process.env.BRINK_SIDECAR_STUB = originalBrinkSidecarStub;
  }

  while (temporaries.length > 0) {
    const dir = temporaries.pop();
    if (dir !== undefined) {
      rmSync(dir, { recursive: true, force: true });
    }
  }
});

// A fake `cargo build --target <triple>` that writes a distinguishable
// "built binary" for that triple under cargo's real triple-scoped output
// directory (`<targetDir>/<triple>/release/brink`) — the layout
// `stageUniversalCliSidecar` expects `--target` builds to use, which is real
// cargo behavior, not a convention this test invents.
function fakeCargoBuildTarget(
  targetDir: string,
): (command: string, options?: Record<string, unknown>) => string {
  return (command: string) => {
    const match = command.match(/--target (\S+)/);
    if (match) {
      const releaseDir = join(targetDir, match[1], "release");
      mkdirSync(releaseDir, { recursive: true });
      writeFileSync(join(releaseDir, "brink"), `fake ${match[1]} slice\n`);
      chmodSync(join(releaseDir, "brink"), 0o644);
    }
    return "";
  };
}

describe("stageUniversalCliSidecar", () => {
  it("builds both Apple slices for real and lipos them into one universal sidecar", () => {
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    const targetDir = join(repoRoot, "target");
    const commands: string[] = [];
    const lipoCalls: Array<string[]> = [];

    const runCommand = (command: string, options?: Record<string, unknown>) => {
      commands.push(command);
      const match = command.match(/--target (\S+)/);
      if (match) {
        const releaseDir = join(targetDir, match[1], "release");
        mkdirSync(releaseDir, { recursive: true });
        writeFileSync(join(releaseDir, "brink"), `fake ${match[1]} slice\n`);
        chmodSync(join(releaseDir, "brink"), 0o644);
      }
      expect(options?.cwd).toBe(repoRoot);
      return "";
    };

    const runLipo = (args: string[]) => {
      lipoCalls.push(args);
      // Real `lipo -create -output <dest> <slice1> <slice2>` produces a fat
      // Mach-O whose first bytes are the FAT_MAGIC header. This fake stands
      // in for that (a synthetic FAT magic, not a real Mach-O — the same
      // disclosure as this file's header comment) so downstream tests of
      // the *identity* check (assert-real-sidecar.mjs, already covered by
      // #2708/#2714's suite) have something plausible to look at; this
      // suite only asserts the argv `lipo` was invoked with and that the
      // destination file exists afterward.
      const destIndex = args.indexOf("-output") + 1;
      const dest = args[destIndex];
      const slices = args.slice(destIndex + 1);
      const combined = slices.map((slicePath) => readFileSync(slicePath)).join("");
      writeFileSync(dest, Buffer.from([0xca, 0xfe, 0xba, 0xbe, ...Buffer.from(combined)]));
      return "";
    };

    const logs: string[] = [];
    const destBin = stageUniversalCliSidecar({
      repoRoot,
      srcTauriDir,
      targetDir,
      runCommand,
      runLipo,
      log: (message: string) => logs.push(message),
    });

    // Both slices were built via their OWN `--target`-scoped cargo
    // invocation, in the declared order.
    expect(commands).toEqual([
      `cargo build -p brink-cli --release --target ${UNIVERSAL_DARWIN_SLICE_TRIPLES[0]}`,
      `cargo build -p brink-cli --release --target ${UNIVERSAL_DARWIN_SLICE_TRIPLES[1]}`,
    ]);

    // Each slice was staged under its own triple-suffixed name, using the
    // same sidecarPaths convention every other triple uses.
    const slicePaths = UNIVERSAL_DARWIN_SLICE_TRIPLES.map(
      (triple) => sidecarPaths({ triple, repoRoot, srcTauriDir, targetDir }).destBin,
    );
    for (const slicePath of slicePaths) {
      expect(existsSync(slicePath)).toBe(true);
      expect(statSync(slicePath).mode & 0o111).not.toBe(0);
    }

    // `lipo -create -output <universal-dest> <slice1> <slice2>` — the exact
    // argv shape `defaultRunLipo` hands the real Apple `lipo` binary.
    expect(lipoCalls).toHaveLength(1);
    expect(lipoCalls[0][0]).toBe("-create");
    expect(lipoCalls[0][1]).toBe("-output");
    expect(lipoCalls[0].slice(3)).toEqual(slicePaths);

    // The staged universal sidecar lands at the triple `assertRealSidecarStaged`
    // resolves `TAURI_ENV_TARGET_TRIPLE=universal-apple-darwin` to.
    const expectedDest = sidecarPaths({
      triple: "universal-apple-darwin",
      repoRoot,
      srcTauriDir,
      targetDir,
    }).destBin;
    expect(destBin).toBe(expectedDest);
    expect(lipoCalls[0][2]).toBe(expectedDest);
    expect(existsSync(destBin)).toBe(true);
    expect(statSync(destBin).mode & 0o111).not.toBe(0);

    expect(logs.some((line) => line.includes("staged universal sidecar"))).toBe(true);
  });

  it("throws a diagnostic naming lipo and macOS when the combine step fails", () => {
    // Stands in for the one thing this Linux container genuinely cannot do:
    // running `lipo` at all (ENOENT — no such binary on PATH here).
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    const targetDir = join(repoRoot, "target");

    expect(() =>
      stageUniversalCliSidecar({
        repoRoot,
        srcTauriDir,
        targetDir,
        runCommand: fakeCargoBuildTarget(targetDir),
        runLipo: () => {
          throw new Error("spawnSync lipo ENOENT");
        },
        log: () => {},
      }),
    ).toThrow(/lipo.*ENOENT|`lipo -create` failed/s);

    // Both slices WERE built and staged before the lipo failure — a partial
    // failure should not hide that progress from a developer diagnosing it.
    for (const triple of UNIVERSAL_DARWIN_SLICE_TRIPLES) {
      expect(existsSync(sidecarPaths({ triple, repoRoot, srcTauriDir, targetDir }).destBin)).toBe(
        true,
      );
    }
  });

  it("throws when a slice build did not produce the expected binary", () => {
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    const targetDir = join(repoRoot, "target");

    expect(() =>
      stageUniversalCliSidecar({
        repoRoot,
        srcTauriDir,
        targetDir,
        runCommand: () => "", // Never writes anything under targetDir.
        runLipo: () => {
          throw new Error("must not be reached — no slices exist to combine");
        },
        log: () => {},
      }),
    ).toThrow("did not produce the expected binary");
  });

  it("skips both slice builds and lipo entirely under the stub option (no toolchain needed)", () => {
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    const commands: string[] = [];
    let lipoCalled = false;

    const destBin = stageUniversalCliSidecar({
      repoRoot,
      srcTauriDir,
      stub: true,
      runCommand: (command: string) => {
        commands.push(command);
        return "";
      },
      runLipo: () => {
        lipoCalled = true;
        return "";
      },
      log: () => {},
    });

    expect(commands).toEqual([]);
    expect(lipoCalled).toBe(false);
    expect(destBin).toBe(join(srcTauriDir, "binaries", "brink-cli-universal-apple-darwin"));
    expect(existsSync(destBin)).toBe(true);
    expect(readFileSync(destBin, "utf8")).toBe(STUB_SIDECAR);
  });

  it("reads BRINK_SIDECAR_STUB from the environment when the caller names no stub (mirrors ensureCliSidecar)", () => {
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    process.env.BRINK_SIDECAR_STUB = "1";

    const destBin = stageUniversalCliSidecar({
      repoRoot,
      srcTauriDir,
      runCommand: () => "",
      runLipo: () => {
        throw new Error("must not be reached under BRINK_SIDECAR_STUB=1");
      },
      log: () => {},
    });

    expect(readFileSync(destBin, "utf8")).toBe(STUB_SIDECAR);
  });
});

// Proves the OTHER half of #2715's fix: the main-guard dispatch that lets a
// real `tauri build --target universal-apple-darwin` (via `pnpm build`,
// tauri.conf.json's `beforeBuildCommand`) reach `stageUniversalCliSidecar`
// at all. Driven as a real subprocess — `node scripts/ensure-cli-sidecar.mjs`
// with `TAURI_ENV_TARGET_TRIPLE=universal-apple-darwin` in its env, the
// exact env var tauri-cli sets for that hook — rather than only asserted
// in prose. Uses the stub path so the dispatch itself is provable without
// any Apple toolchain: proving THIS branch fires does not require proving
// the slice-build/lipo branch also works on this machine.
describe("the main-guard dispatch for a universal build (#2715)", () => {
  const scriptPath = resolve(
    fileURLToPath(import.meta.url),
    "../../../scripts/ensure-cli-sidecar.mjs",
  );

  it("stages under universal-apple-darwin, not the host triple, when TAURI_ENV_TARGET_TRIPLE says so", () => {
    const base = scratch();
    const packageDir = join(base, "packages", "brink-desktop");
    mkdirSync(join(packageDir, "scripts"), { recursive: true });
    copyFileSync(scriptPath, join(packageDir, "scripts", "ensure-cli-sidecar.mjs"));

    const emptyPath = scratch();
    const env: NodeJS.ProcessEnv = {
      ...process.env,
      PATH: emptyPath, // No rustc/cargo/lipo reachable — the stub path needs none of them.
      TAURI_ENV_TARGET_TRIPLE: "universal-apple-darwin",
      BRINK_SIDECAR_STUB: "1",
    };
    delete env.CARGO_TARGET_DIR;

    execFileSync(process.execPath, [join(packageDir, "scripts", "ensure-cli-sidecar.mjs")], {
      encoding: "utf8",
      env,
    });

    const staged = join(packageDir, "src-tauri", "binaries", "brink-cli-universal-apple-darwin");
    expect(existsSync(staged)).toBe(true);
    expect(readFileSync(staged, "utf8")).toBe(STUB_SIDECAR);
  });

  it.skipIf(process.platform === "win32")(
    "still dispatches to the ordinary host-triple ensureCliSidecar when TAURI_ENV_TARGET_TRIPLE is unset (regression guard)",
    () => {
      const base = scratch();
      const packageDir = join(base, "packages", "brink-desktop");
      mkdirSync(join(packageDir, "scripts"), { recursive: true });
      copyFileSync(scriptPath, join(packageDir, "scripts", "ensure-cli-sidecar.mjs"));
      mkdirSync(join(base, "target", "release"), { recursive: true });
      writeFileSync(join(base, "target", "release", "brink"), "stub\n");

      const shims = scratch();
      writeFileSync(join(shims, "rustc"), "#!/bin/sh\necho 'host: stub-unknown-linux-gnu'\n");
      chmodSync(join(shims, "rustc"), 0o755);
      writeFileSync(join(shims, "cargo"), "#!/bin/sh\nexit 0\n");
      chmodSync(join(shims, "cargo"), 0o755);

      const env: NodeJS.ProcessEnv = { ...process.env, PATH: shims };
      delete env.CARGO_TARGET_DIR;
      delete env.BRINK_SIDECAR_STUB;
      delete env.TAURI_ENV_TARGET_TRIPLE;

      execFileSync(process.execPath, [join(packageDir, "scripts", "ensure-cli-sidecar.mjs")], {
        encoding: "utf8",
        env,
      });

      expect(
        existsSync(join(packageDir, "src-tauri", "binaries", "brink-cli-stub-unknown-linux-gnu")),
      ).toBe(true);
      expect(
        existsSync(
          join(packageDir, "src-tauri", "binaries", "brink-cli-universal-apple-darwin"),
        ),
      ).toBe(false);
    },
  );
});
