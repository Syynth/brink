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
  ensureCliSidecar,
  hostTriple,
  sidecarPaths,
  STUB_SIDECAR,
} from "../../scripts/ensure-cli-sidecar.mjs";

// `ensure-cli-sidecar.mjs` was top-level imperative script code until #2452.
// Nothing could call it, so nothing could test it — which is why #2418's
// gap 4 was settled with lane-scoped `CARGO_PROFILE_RELEASE_*` env vars in
// `desktop-smoke.yml` instead of an option in the script. These tests
// exercise the exported seam directly: `runCommand` stands in for
// rustc/cargo, and the "built" binary is a temp file, so the staging logic
// runs end to end without a toolchain.

const temporaries: string[] = [];

function scratch(): string {
  const dir = mkdtempSync(join(tmpdir(), "ensure-cli-sidecar-"));
  temporaries.push(dir);
  return dir;
}

/** A repo root whose `target/release/brink` already exists, as after a build. */
function stagedRepoRoot(): string {
  const repoRoot = scratch();
  const releaseDir = join(repoRoot, "target", "release");
  mkdirSync(releaseDir, { recursive: true });
  writeFileSync(join(releaseDir, "brink"), "#!/bin/sh\nexit 0\n");
  chmodSync(join(releaseDir, "brink"), 0o644);
  return repoRoot;
}

// `ensureCliSidecar`'s `targetDir` default reads `process.env.CARGO_TARGET_DIR`
// (:161), so any test that means "the default `<repoRoot>/target` path" —
// i.e. every `stagedRepoRoot()` case except the one below that deliberately
// exercises the env var — has to run with it cleared, or it silently
// disagrees with an ambient value the way #2659 found. The pump mandates
// `CARGO_TARGET_DIR` on every gate invocation, so leaving it ambient here
// makes these three tests fail whenever the suite runs under the pump.
let originalCargoTargetDir: string | undefined;

beforeEach(() => {
  originalCargoTargetDir = process.env.CARGO_TARGET_DIR;
  delete process.env.CARGO_TARGET_DIR;
});

afterEach(() => {
  if (originalCargoTargetDir === undefined) {
    delete process.env.CARGO_TARGET_DIR;
  } else {
    process.env.CARGO_TARGET_DIR = originalCargoTargetDir;
  }

  while (temporaries.length > 0) {
    const dir = temporaries.pop();
    if (dir !== undefined) {
      rmSync(dir, { recursive: true, force: true });
    }
  }
});

describe("hostTriple", () => {
  it("reads the host triple out of `rustc -vV` rather than guessing it", () => {
    const triple = hostTriple(
      () => "rustc 1.90.0\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.90.0\n",
    );
    expect(triple).toBe("aarch64-apple-darwin");
  });

  it("throws when `rustc -vV` carries no host line", () => {
    expect(() => hostTriple(() => "rustc 1.90.0\n")).toThrow("host:");
  });
});

describe("sidecarPaths", () => {
  it("stages under the `brink-cli` name even though cargo builds `brink`", () => {
    const paths = sidecarPaths({
      triple: "x86_64-unknown-linux-gnu",
      repoRoot: "/repo",
      srcTauriDir: "/repo/packages/brink-desktop/src-tauri",
    });
    expect(paths.builtBin).toBe(join("/repo", "target", "release", "brink"));
    expect(paths.destBin).toBe(
      join("/repo/packages/brink-desktop/src-tauri", "binaries", "brink-cli-x86_64-unknown-linux-gnu"),
    );
  });

  it("suffixes both the built and the staged binary with .exe on windows hosts", () => {
    const paths = sidecarPaths({
      triple: "x86_64-pc-windows-msvc",
      repoRoot: "/repo",
      srcTauriDir: "/repo/src-tauri",
    });
    expect(paths.builtBin.endsWith("brink.exe")).toBe(true);
    expect(paths.destBin.endsWith("brink-cli-x86_64-pc-windows-msvc.exe")).toBe(true);
  });

  it("an explicit targetDir overrides <repoRoot>/target", () => {
    const paths = sidecarPaths({
      triple: "x86_64-unknown-linux-gnu",
      repoRoot: "/repo",
      srcTauriDir: "/repo/src-tauri",
      targetDir: "/shared/target",
    });
    expect(paths.builtBin).toBe(join("/shared/target", "release", "brink"));
  });
});

describe("ensureCliSidecar", () => {
  it("builds brink-cli from the root workspace and stages an executable sidecar", () => {
    const repoRoot = stagedRepoRoot();
    const srcTauriDir = scratch();
    const commands: Array<[string, Record<string, unknown> | undefined]> = [];

    const destBin = ensureCliSidecar({
      repoRoot,
      srcTauriDir,
      triple: "x86_64-unknown-linux-gnu",
      runCommand: (command: string, options?: Record<string, unknown>) => {
        commands.push([command, options]);
        return "";
      },
      log: () => {},
    });

    // The build must run against the ROOT workspace, not src-tauri's own.
    expect(commands).toHaveLength(1);
    expect(commands[0][0]).toBe("cargo build -p brink-cli --release");
    expect(commands[0][1]?.cwd).toBe(repoRoot);

    expect(destBin).toBe(
      join(srcTauriDir, "binaries", "brink-cli-x86_64-unknown-linux-gnu"),
    );
    expect(existsSync(destBin)).toBe(true);
    // `copyFileSync` does not reliably carry the mode bit across, so the
    // script sets it explicitly — Tauri spawns this file.
    expect(statSync(destBin).mode & 0o111).not.toBe(0);
  });

  it("throws when the build produced no binary instead of staging a stale one", () => {
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    expect(() =>
      ensureCliSidecar({
        repoRoot,
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        runCommand: () => "",
        log: () => {},
      }),
    ).toThrow("did not produce the expected binary");
  });

  it("asks rustc for the host triple when the caller names none", () => {
    const repoRoot = stagedRepoRoot();
    const srcTauriDir = scratch();

    const destBin = ensureCliSidecar({
      repoRoot,
      srcTauriDir,
      runCommand: (command: string) =>
        command === "rustc -vV" ? "host: x86_64-unknown-linux-gnu\n" : "",
      log: () => {},
    });

    expect(destBin.endsWith("brink-cli-x86_64-unknown-linux-gnu")).toBe(true);
  });

  it("honours a CARGO_TARGET_DIR environment variable when the caller names no targetDir", () => {
    // `sidecarPaths` never reads `process.env` — only `ensureCliSidecar`'s
    // `targetDir` default does (env var reading happens in exactly one
    // place: reading `rustc -vV`'s `host:` line is the other seam that
    // reaches out to the environment). This drives that default end to
    // end: the "built" binary sits only under `$CARGO_TARGET_DIR/release`,
    // not `<repoRoot>/target/release`, so staging can only succeed if the
    // env var was actually honoured.
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    const sharedTargetDir = scratch();
    const releaseDir = join(sharedTargetDir, "release");
    mkdirSync(releaseDir, { recursive: true });
    writeFileSync(join(releaseDir, "brink"), "#!/bin/sh\nexit 0\n");
    chmodSync(join(releaseDir, "brink"), 0o644);

    const originalTargetDir = process.env.CARGO_TARGET_DIR;
    process.env.CARGO_TARGET_DIR = sharedTargetDir;
    try {
      const destBin = ensureCliSidecar({
        repoRoot,
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        runCommand: () => "",
        log: () => {},
      });
      expect(destBin).toBe(
        join(srcTauriDir, "binaries", "brink-cli-x86_64-unknown-linux-gnu"),
      );
    } finally {
      if (originalTargetDir === undefined) {
        delete process.env.CARGO_TARGET_DIR;
      } else {
        process.env.CARGO_TARGET_DIR = originalTargetDir;
      }
    }
  });
});

describe("the stub option", () => {
  // #2469: `desktop-smoke.yml` stages the sidecar only so `tauri-build`'s
  // externalBin resolution finds a file on disk — nothing in that check-only
  // lane executes it — so it now asks for a stub instead of the three
  // `CARGO_PROFILE_RELEASE_*` env vars PR #2446 added as a stopgap. Those
  // only made the wasted build cheaper; this skips it.

  it("stages a stub without building anything", () => {
    // `scratch()`, not `stagedRepoRoot()`: there is no `target/release/brink`
    // anywhere in this tree, so the non-stub path could not possibly have
    // produced the staged file — it would have thrown.
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    const commands: string[] = [];

    const destBin = ensureCliSidecar({
      repoRoot,
      srcTauriDir,
      triple: "x86_64-unknown-linux-gnu",
      stub: true,
      runCommand: (command: string) => {
        commands.push(command);
        return "";
      },
      log: () => {},
    });

    expect(commands).toEqual([]);
    // Same triple-suffixed name as a real sidecar: tauri.conf.json's
    // `binaries/brink-cli` resolution is what has to be satisfied.
    expect(destBin).toBe(
      join(srcTauriDir, "binaries", "brink-cli-x86_64-unknown-linux-gnu"),
    );
    expect(existsSync(destBin)).toBe(true);
    expect(statSync(destBin).mode & 0o111).not.toBe(0);
    expect(readFileSync(destBin, "utf8")).toBe(STUB_SIDECAR);
  });

  it.skipIf(process.platform === "win32")(
    "stages a stub that fails loudly rather than impersonating brink-cli",
    () => {
      const repoRoot = scratch();
      const srcTauriDir = scratch();
      const destBin = ensureCliSidecar({
        repoRoot,
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        stub: true,
        runCommand: () => "",
        log: () => {},
      });

      // If a lane ever DOES execute the staged sidecar, it must break there
      // and say why, not exit 0 and let a stubbed lane look like it ran the
      // real CLI.
      let status: number | undefined;
      let stderr = "";
      try {
        execFileSync(destBin, ["export-xliff"], { encoding: "utf8", stdio: "pipe" });
      } catch (error) {
        const failure = error as { status?: number; stderr?: string };
        status = failure.status;
        stderr = failure.stderr ?? "";
      }
      expect(status).toBe(127);
      expect(stderr).toContain("sidecar stub");
    },
  );

  it("reads BRINK_SIDECAR_STUB from the environment when the caller names no stub", () => {
    // The smoke lane reaches this script twice — its own "Stage brink-cli
    // sidecar" step and `pnpm build`, which re-runs it — and only the first
    // could carry a command-line flag. The workflow therefore sets the env
    // var, so this default is the wiring the lane actually depends on.
    const repoRoot = scratch();
    const srcTauriDir = scratch();
    const original = process.env.BRINK_SIDECAR_STUB;
    process.env.BRINK_SIDECAR_STUB = "1";
    try {
      const destBin = ensureCliSidecar({
        repoRoot,
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        runCommand: () => "",
        log: () => {},
      });
      expect(readFileSync(destBin, "utf8")).toBe(STUB_SIDECAR);
    } finally {
      if (original === undefined) {
        delete process.env.BRINK_SIDECAR_STUB;
      } else {
        process.env.BRINK_SIDECAR_STUB = original;
      }
    }
  });

  it("refuses to stage a stub for a Windows triple rather than write a broken .exe (#2481)", () => {
    // The smoke lane is ubuntu-only (#2428), so this drives the branch with
    // a synthetic triple the way the real lane never does — that is the
    // whole point of the guard: nothing else in this suite (or in CI) would
    // ever exercise a Windows triple otherwise.
    const repoRoot = scratch();
    const srcTauriDir = scratch();

    expect(() =>
      ensureCliSidecar({
        repoRoot,
        srcTauriDir,
        triple: "x86_64-pc-windows-msvc",
        stub: true,
        runCommand: () => "",
        log: () => {},
      }),
    ).toThrow(/BRINK_SIDECAR_STUB has no Windows-compatible payload/);

    // Nothing gets written — a thrown error, not a `.exe`-suffixed file
    // holding a POSIX shell script Windows cannot start.
    expect(
      existsSync(join(srcTauriDir, "binaries", "brink-cli-x86_64-pc-windows-msvc.exe")),
    ).toBe(false);
  });

  it("still stages the ordinary POSIX stub for a non-Windows triple (regression guard)", () => {
    // Confirms the new check in the previous test is triple-specific, not a
    // blanket refusal that would also break the Linux/macOS stub path this
    // option exists for.
    const repoRoot = scratch();
    const srcTauriDir = scratch();

    const destBin = ensureCliSidecar({
      repoRoot,
      srcTauriDir,
      triple: "aarch64-apple-darwin",
      stub: true,
      runCommand: () => "",
      log: () => {},
    });

    expect(readFileSync(destBin, "utf8")).toBe(STUB_SIDECAR);
  });

  it("builds for real when BRINK_SIDECAR_STUB is set to anything else", () => {
    // Only the exact string "1" opts in — a leftover `BRINK_SIDECAR_STUB=0`
    // in a developer's shell must not silently turn `pnpm --filter
    // @brink/desktop build` into a shell that ships a stub for a sidecar it
    // really does run.
    const repoRoot = stagedRepoRoot();
    const srcTauriDir = scratch();
    const commands: string[] = [];
    const original = process.env.BRINK_SIDECAR_STUB;
    process.env.BRINK_SIDECAR_STUB = "0";
    try {
      const destBin = ensureCliSidecar({
        repoRoot,
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        runCommand: (command: string) => {
          commands.push(command);
          return "";
        },
        log: () => {},
      });
      expect(commands).toEqual(["cargo build -p brink-cli --release"]);
      expect(readFileSync(destBin, "utf8")).not.toBe(STUB_SIDECAR);
    } finally {
      if (original === undefined) {
        delete process.env.BRINK_SIDECAR_STUB;
      } else {
        process.env.BRINK_SIDECAR_STUB = original;
      }
    }
  });
});

describe("the main-guard", () => {
  const scriptPath = resolve(
    fileURLToPath(import.meta.url),
    "../../../scripts/ensure-cli-sidecar.mjs",
  );

  it("leaves the module inert on import", () => {
    // Import in a child node with an EMPTY `PATH`, so `rustc`/`cargo` are
    // unreachable: if merely importing still ran the staging logic, the
    // spawn would exit non-zero on the very first `rustc -vV`. Node itself
    // is spawned by absolute path, so the empty PATH costs nothing else.
    // Without the guard this exits 1 rather than 0.
    const emptyPath = scratch();
    const source = `await import(${JSON.stringify(pathToFileURL(scriptPath).href)});`;
    const output = execFileSync(
      process.execPath,
      ["--input-type=module", "-e", `${source}\nconsole.log("inert");`],
      { encoding: "utf8", env: { ...process.env, PATH: emptyPath } },
    );
    expect(output.trim()).toBe("inert");
  });

  // Shims cannot be made executable the same way on Windows, and the
  // desktop lanes are ubuntu/macOS.
  it.skipIf(process.platform === "win32")(
    "still stages the sidecar when the script is run standalone",
    () => {
      // `pnpm --filter @brink/desktop build` and the smoke lane's "Stage
      // brink-cli sidecar" step both run `node scripts/ensure-cli-sidecar.mjs`
      // directly, so the guard has to fire for that invocation. The script
      // is copied into a throwaway tree laid out the same way (its defaults
      // are all relative to its own location) and given `rustc`/`cargo`
      // shims on `PATH`, so the real compiler is never built.
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
      // The default target dir is `<repoRoot>/target`, which is what this
      // throwaway tree provides; an inherited CARGO_TARGET_DIR would point
      // the script at the session's shared one instead.
      delete env.CARGO_TARGET_DIR;
      // An ambient BRINK_SIDECAR_STUB=1 would make the script's default
      // opt into the stub path, so this assertion would pass without the
      // invocation ever reaching `cargo build -p brink-cli --release` — the
      // exact path this test exists to cover.
      delete env.BRINK_SIDECAR_STUB;
      execFileSync(process.execPath, [join(packageDir, "scripts", "ensure-cli-sidecar.mjs")], {
        encoding: "utf8",
        env,
      });

      expect(
        existsSync(join(packageDir, "src-tauri", "binaries", "brink-cli-stub-unknown-linux-gnu")),
      ).toBe(true);
    },
  );
});
