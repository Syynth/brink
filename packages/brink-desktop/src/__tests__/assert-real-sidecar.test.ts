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
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  executableFormatFor,
  hostTriple,
  sidecarPaths,
  STUB_SIDECAR,
} from "../../scripts/ensure-cli-sidecar.mjs";
import {
  assertRealSidecarStaged,
  EXECUTABLE_MAGIC,
  looksLikeNativeExecutable,
} from "../../scripts/assert-real-sidecar.mjs";

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

function stageSidecar(
  srcTauriDir: string,
  triple: string,
  content: string | Uint8Array,
): string {
  const { binariesDir, destBin } = sidecarPaths({ triple, repoRoot: "/unused", srcTauriDir });
  mkdirSync(binariesDir, { recursive: true });
  writeFileSync(destBin, content);
  chmodSync(destBin, 0o755);
  return destBin;
}

// Stand-in for what `cargo build -p brink-cli --release` would have staged
// for `triple`: the executable magic that triple's loader requires, plus
// filler. Since #2687 the assertion is a POSITIVE identity check, so a
// "real binary" fixture can no longer be an arbitrary non-stub string.
function fakeNativeBinary(triple: string): Uint8Array {
  const format = executableFormatFor(triple);
  const magic: number[] = format === null ? [] : EXECUTABLE_MAGIC[format][0];
  return Uint8Array.from([...magic, ...Buffer.from("\0\0not-really-a-binary-but-not-the-stub")]);
}

// `assertRealSidecarStaged`'s `triple` default reads
// `process.env.TAURI_ENV_TARGET_TRIPLE` (falling back to `hostTriple()`),
// same seam `ensure-cli-sidecar.test.ts` isolates for `CARGO_TARGET_DIR`
// after #2659/#2668 — leaving an ambient value here would let a real
// tauri-cli invocation's env leak into an unrelated test.
let originalTauriEnvTargetTriple: string | undefined;

beforeEach(() => {
  originalTauriEnvTargetTriple = process.env.TAURI_ENV_TARGET_TRIPLE;
  delete process.env.TAURI_ENV_TARGET_TRIPLE;
});

afterEach(() => {
  if (originalTauriEnvTargetTriple === undefined) {
    delete process.env.TAURI_ENV_TARGET_TRIPLE;
  } else {
    process.env.TAURI_ENV_TARGET_TRIPLE = originalTauriEnvTargetTriple;
  }

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
    // Stand-in for a real compiled binary: content that is not STUB_SIDECAR
    // and — since #2687 — also carries the target's executable magic. The
    // `\x7fELF` prefix below is exactly that magic for a linux triple.
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

  it("reads TAURI_ENV_TARGET_TRIPLE when the caller names no triple and tauri-cli set it", () => {
    // tauri-cli exports TAURI_ENV_TARGET_TRIPLE (from app_settings.target_triple,
    // resolved from the real `--target` the build was invoked with) into every
    // hook it runs, `beforeBundleCommand` included — see the JSDoc above
    // assertRealSidecarStaged. A cross-compiled bundle stages its sidecar
    // under that triple, not the host's, so the assertion has to read the
    // same one or it would check the wrong file.
    const srcTauriDir = scratch();
    const destBin = stageSidecar(
      srcTauriDir,
      "aarch64-apple-darwin",
      fakeNativeBinary("aarch64-apple-darwin"),
    );

    process.env.TAURI_ENV_TARGET_TRIPLE = "aarch64-apple-darwin";
    const result = assertRealSidecarStaged({ srcTauriDir, log: () => {} });
    expect(result).toBe(destBin);
  });

  it("falls back to hostTriple() when TAURI_ENV_TARGET_TRIPLE is unset", () => {
    // Standalone/manual invocations (not through tauri-cli's beforeBundleCommand)
    // carry no TAURI_ENV_TARGET_TRIPLE — the default must still resolve via
    // hostTriple() exactly as it did before this default existed. Real
    // `rustc -vV` on PATH, same as the sibling "asks rustc for the host
    // triple" case below, just without emptying PATH first.
    const srcTauriDir = scratch();
    const destBin = stageSidecar(srcTauriDir, hostTriple(), fakeNativeBinary(hostTriple()));

    expect(process.env.TAURI_ENV_TARGET_TRIPLE).toBeUndefined();
    const result = assertRealSidecarStaged({ srcTauriDir, log: () => {} });
    expect(result).toBe(destBin);
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

// #2687: the stub comparison alone is a BLOCKLIST — it refuses the one
// placeholder that exists today and passes everything else, including an
// empty or truncated or wrong-architecture file, because `tauri_build`'s
// externalBin resolution only tests existence. These cases pin the positive
// half: the staged file must actually be a native executable for the target.
describe("looksLikeNativeExecutable", () => {
  it("accepts ELF magic for an ELF target and rejects it for a PE one", () => {
    const elf = Uint8Array.from([0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01]);
    expect(looksLikeNativeExecutable(elf, "elf")).toBe(true);
    expect(looksLikeNativeExecutable(elf, "pe")).toBe(false);
    expect(looksLikeNativeExecutable(elf, "macho")).toBe(false);
  });

  it("accepts every Mach-O header magic, in both byte orders, thin and fat", () => {
    // A real macOS release binary can legitimately be a universal (fat)
    // wrapper rather than a thin Mach-O; rejecting one would be a false
    // negative on a real binary, which is worse than not checking at all.
    for (const magic of EXECUTABLE_MAGIC.macho as number[][]) {
      expect(looksLikeNativeExecutable(Uint8Array.from([...magic, 0x00]), "macho")).toBe(true);
    }
  });

  it("accepts the DOS `MZ` stub for PE", () => {
    expect(looksLikeNativeExecutable(Uint8Array.from([0x4d, 0x5a, 0x90, 0x00]), "pe")).toBe(true);
  });

  it("rejects a truncated file that only has part of the magic", () => {
    expect(looksLikeNativeExecutable(Uint8Array.from([0x7f, 0x45]), "elf")).toBe(false);
    expect(looksLikeNativeExecutable(Uint8Array.from([]), "elf")).toBe(false);
  });

  it("reports `undefined` — not `false` — for a format with no rule", () => {
    // "Cannot judge" must be distinguishable from "judged and rejected":
    // a positive check that rejects a REAL binary on an unanticipated
    // platform is worse than the blocklist it replaces.
    expect(looksLikeNativeExecutable(Uint8Array.from([0x00]), "wasm")).toBeUndefined();
  });
});

describe("assertRealSidecarStaged's positive identity check", () => {
  it("rejects an EMPTY file, which the stub blocklist alone let through", () => {
    const srcTauriDir = scratch();
    stageSidecar(srcTauriDir, "x86_64-unknown-linux-gnu", Uint8Array.from([]));

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        log: () => {},
      }),
    ).toThrow(/does not begin with ELF executable magic/);
  });

  it("rejects a truncated copy of a real binary", () => {
    const srcTauriDir = scratch();
    // Two bytes of ELF magic — a half-finished `copyFileSync`, say. Not
    // equal to STUB_SIDECAR, so the blocklist would have passed it.
    stageSidecar(srcTauriDir, "x86_64-unknown-linux-gnu", Uint8Array.from([0x7f, 0x45]));

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        log: () => {},
      }),
    ).toThrow(/does not begin with ELF executable magic/);
  });

  it("rejects a binary built for the WRONG platform's loader", () => {
    const srcTauriDir = scratch();
    // A genuine Mach-O binary staged for a Windows bundle: a real
    // executable, just not one the target can run.
    stageSidecar(
      srcTauriDir,
      "x86_64-pc-windows-msvc",
      fakeNativeBinary("aarch64-apple-darwin"),
    );

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-pc-windows-msvc",
        log: () => {},
      }),
    ).toThrow(/does not begin with PE executable magic/);
  });

  it("names the offending bytes and the target so the message is actionable", () => {
    const srcTauriDir = scratch();
    stageSidecar(srcTauriDir, "x86_64-unknown-linux-gnu", Uint8Array.from([0xde, 0xad]));

    try {
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        log: () => {},
      });
      throw new Error("expected assertRealSidecarStaged to throw");
    } catch (error) {
      const message = (error as Error).message;
      expect(message).toContain("de ad");
      expect(message).toContain("x86_64-unknown-linux-gnu");
    }
  });

  it("accepts a real PE binary staged for a Windows triple", () => {
    // The `.exe` suffix `sidecarPaths` adds for Windows triples (#2481) is
    // not a reason to reject: this check must pass the real thing on every
    // platform the shell can bundle for.
    const srcTauriDir = scratch();
    const destBin = stageSidecar(
      srcTauriDir,
      "x86_64-pc-windows-msvc",
      fakeNativeBinary("x86_64-pc-windows-msvc"),
    );
    expect(destBin.endsWith(".exe")).toBe(true);

    expect(
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-pc-windows-msvc",
        log: () => {},
      }),
    ).toBe(destBin);
  });

  it("accepts a real Mach-O binary staged for an Apple triple", () => {
    const srcTauriDir = scratch();
    const destBin = stageSidecar(
      srcTauriDir,
      "aarch64-apple-darwin",
      fakeNativeBinary("aarch64-apple-darwin"),
    );

    expect(
      assertRealSidecarStaged({ srcTauriDir, triple: "aarch64-apple-darwin", log: () => {} }),
    ).toBe(destBin);
  });

  it("still names the STUB specifically, rather than degrading to 'not an executable'", () => {
    // The stub comparison is kept ALONGSIDE the magic check, not replaced by
    // it: STUB_SIDECAR is a `#!` script and would fail the ELF magic too, but
    // only the blocklist can say WHICH placeholder it is and what to rebuild.
    const srcTauriDir = scratch();
    stageSidecar(srcTauriDir, "x86_64-unknown-linux-gnu", STUB_SIDECAR);

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-unknown-linux-gnu",
        log: () => {},
      }),
    ).toThrow(/STUB brink-cli placeholder/);
  });

  it("falls back to weaker checks — never a rejection — on a triple with no format rule", () => {
    const srcTauriDir = scratch();
    const triple = "aarch64-unknown-mysteryos";
    expect(executableFormatFor(triple)).toBeNull();

    // A real binary for an unanticipated target must still bundle.
    const destBin = stageSidecar(srcTauriDir, triple, Uint8Array.from([0x01, 0x02, 0x03, 0x04]));
    expect(assertRealSidecarStaged({ srcTauriDir, triple, log: () => {} })).toBe(destBin);

    // …but the obviously-wrong cases are still refused.
    stageSidecar(srcTauriDir, triple, Uint8Array.from([]));
    expect(() => assertRealSidecarStaged({ srcTauriDir, triple, log: () => {} })).toThrow(
      /is empty/,
    );

    stageSidecar(srcTauriDir, triple, Uint8Array.from([0x23, 0x21, 0x2f, 0x62]));
    expect(() => assertRealSidecarStaged({ srcTauriDir, triple, log: () => {} })).toThrow(
      /interpreter script/,
    );
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
