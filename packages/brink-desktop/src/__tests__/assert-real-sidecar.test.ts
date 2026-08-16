import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { sidecarPaths, STUB_SIDECAR } from "../../scripts/ensure-cli-sidecar.mjs";
import { assertRealSidecarStaged } from "../../scripts/assert-real-sidecar.mjs";

// #2631: PR #2626's "a real bundle must ship the real brink-cli" invariant
// held for `tauri build --debug` only through step-ordering — `pnpm build`
// (beforeBuildCommand) happens to stage the real binary before build.rs runs
// — plus `bundle.active: false` making the point moot in practice. These
// tests exercise the exported seam directly, the same way
// ensure-cli-sidecar.test.ts does for the script this one reads
// `STUB_SIDECAR` from, rather than a copy of it.

const temporaries: string[] = [];

function scratch(): string {
  const dir = mkdtempSync(join(tmpdir(), "assert-real-sidecar-"));
  temporaries.push(dir);
  return dir;
}

function stageSidecar(srcTauriDir: string, triple: string, content: string): string {
  const { binariesDir, destBin } = sidecarPaths({ triple, repoRoot: "/unused", srcTauriDir });
  mkdirSync(binariesDir, { recursive: true });
  writeFileSync(destBin, content);
  chmodSync(destBin, 0o755);
  return destBin;
}

afterEach(() => {
  while (temporaries.length > 0) {
    const dir = temporaries.pop();
    if (dir !== undefined) {
      rmSync(dir, { recursive: true, force: true });
    }
  }
});

describe("assertRealSidecarStaged", () => {
  it("throws when the staged sidecar is byte-for-byte the STUB placeholder", () => {
    const srcTauriDir = scratch();
    const destBin = stageSidecar(srcTauriDir, "x86_64-unknown-linux-gnu", STUB_SIDECAR);

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        log: () => {},
      }),
    ).toThrow(/STUB brink-cli placeholder/);
    // The message names the destination path so a developer knows exactly
    // what to rebuild.
    try {
      assertRealSidecarStaged({ srcTauriDir, triple: "x86_64-unknown-linux-gnu", log: () => {} });
    } catch (error) {
      expect((error as Error).message).toContain(destBin);
    }
  });

  it("passes when the staged sidecar's content differs from the stub", () => {
    const srcTauriDir = scratch();
    // Stand-in for a real compiled binary: content that is not STUB_SIDECAR.
    const destBin = stageSidecar(
      srcTauriDir,
      "x86_64-unknown-linux-gnu",
      "\x7fELF-not-really-but-not-the-stub-either",
    );

    const result = assertRealSidecarStaged({
      srcTauriDir,
      triple: "x86_64-unknown-linux-gnu",
      log: () => {},
    });
    expect(result).toBe(destBin);
  });

  it("throws when nothing is staged at all", () => {
    const srcTauriDir = scratch();

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        log: () => {},
      }),
    ).toThrow(/no brink-cli sidecar staged/);
  });

  it("asks rustc for the host triple when the caller names none", () => {
    // Mirrors ensure-cli-sidecar.test.ts's equivalent case: `hostTriple()`'s
    // default reaches `rustc -vV` on PATH, so this only proves the default
    // parameter is wired, not that it always resolves correctly offline —
    // covered directly by hostTriple's own tests.
    const srcTauriDir = scratch();
    stageSidecar(srcTauriDir, "x86_64-unknown-linux-gnu", "not-the-stub");

    // Without a real `rustc` on PATH this throws inside `hostTriple()`
    // rather than silently resolving the wrong triple.
    const emptyPath = scratch();
    const original = process.env.PATH;
    process.env.PATH = emptyPath;
    try {
      expect(() => assertRealSidecarStaged({ srcTauriDir, log: () => {} })).toThrow();
    } finally {
      process.env.PATH = original;
    }
  });
});

describe("the main-guard", () => {
  const scriptPath = resolve(
    fileURLToPath(import.meta.url),
    "../../../scripts/assert-real-sidecar.mjs",
  );

  it("leaves the module inert on import", () => {
    // Same shape as ensure-cli-sidecar.test.ts's equivalent: an empty PATH
    // means `hostTriple()`'s `rustc -vV` would fail immediately if merely
    // importing the module ran the assertion as a side effect.
    const emptyPath = scratch();
    const source = `await import(${JSON.stringify(pathToFileURL(scriptPath).href)});`;
    const output = execFileSync(
      process.execPath,
      ["--input-type=module", "-e", `${source}\nconsole.log("inert");`],
      { encoding: "utf8", env: { ...process.env, PATH: emptyPath } },
    );
    expect(output.trim()).toBe("inert");
  });

  it.skipIf(process.platform === "win32")(
    "still runs the assertion when the script is invoked standalone, and fails loudly on a staged stub",
    () => {
      // `tauri.conf.json`'s `beforeBundleCommand` runs
      // `node scripts/assert-real-sidecar.mjs` directly, so the guard has to
      // fire for that invocation. Laid out the same way ensure-cli-sidecar's
      // standalone test is: a throwaway tree mirroring the real
      // `packages/brink-desktop` layout, with `rustc` shimmed on PATH so no
      // real toolchain is needed.
      const base = scratch();
      const packageDir = join(base, "packages", "brink-desktop");
      mkdirSync(join(packageDir, "scripts"), { recursive: true });
      copyFileSync(scriptPath, join(packageDir, "scripts", "assert-real-sidecar.mjs"));
      copyFileSync(
        resolve(fileURLToPath(import.meta.url), "../../../scripts/ensure-cli-sidecar.mjs"),
        join(packageDir, "scripts", "ensure-cli-sidecar.mjs"),
      );

      const binariesDir = join(packageDir, "src-tauri", "binaries");
      mkdirSync(binariesDir, { recursive: true });
      writeFileSync(join(binariesDir, "brink-cli-stub-unknown-linux-gnu"), STUB_SIDECAR);

      const shims = scratch();
      writeFileSync(join(shims, "rustc"), "#!/bin/sh\necho 'host: stub-unknown-linux-gnu'\n");
      chmodSync(join(shims, "rustc"), 0o755);

      let status: number | undefined;
      let stderr = "";
      try {
        execFileSync(
          process.execPath,
          [join(packageDir, "scripts", "assert-real-sidecar.mjs")],
          { encoding: "utf8", env: { ...process.env, PATH: shims }, stdio: "pipe" },
        );
      } catch (error) {
        const failure = error as { status?: number; stderr?: string };
        status = failure.status;
        stderr = failure.stderr ?? "";
      }

      expect(status).not.toBe(0);
      expect(stderr).toContain("STUB brink-cli placeholder");
    },
  );
});
