import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { ensureCliSidecar, hostTriple, sidecarPaths } from "../../scripts/ensure-cli-sidecar.mjs";

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

afterEach(() => {
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
