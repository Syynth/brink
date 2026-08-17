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
  looksLikeBrinkCliVersionOutput,
  looksLikeNativeExecutable,
  runVersionSmokeTest,
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

    // "x86_64-unknown-linux-gnu" is this test runner's own host triple, so
    // the #2699 smoke check's host-match branch fires for real; the content
    // above is not an actually-runnable executable, so `runFile` stands in
    // for a working `--version` the same way the sibling hostTriple() test
    // above does.
    const result = assertRealSidecarStaged({
      srcTauriDir,
      triple: "x86_64-unknown-linux-gnu",
      log: () => {},
      runFile: () => "brink 0.1.0\n",
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

    // On a host whose own triple happens to be aarch64-apple-darwin, the
    // #2699 smoke check's host-match branch fires for real; the content
    // above is not an actually-runnable executable, so `runFile` stands in
    // for a working `--version`, the same way the sibling tests above do.
    process.env.TAURI_ENV_TARGET_TRIPLE = "aarch64-apple-darwin";
    const result = assertRealSidecarStaged({
      srcTauriDir,
      log: () => {},
      runFile: () => "brink 0.1.0\n",
    });
    expect(result).toBe(destBin);
  });

  it("falls back to hostTriple() when TAURI_ENV_TARGET_TRIPLE is unset", () => {
    // Standalone/manual invocations (not through tauri-cli's beforeBundleCommand)
    // carry no TAURI_ENV_TARGET_TRIPLE — the default must still resolve via
    // hostTriple() exactly as it did before this default existed. Real
    // `rustc -vV` on PATH, same as the sibling "asks rustc for the host
    // triple" case below, just without emptying PATH first.
    //
    // Since the default triple here IS the real host triple (#2699), the
    // smoke check's host-match branch fires for real — `fakeNativeBinary`'s
    // content is not an actually-runnable executable, so `runFile` is
    // stubbed to stand in for a working `--version`. That branch is what
    // the dedicated "--version smoke check" describe block below exercises
    // directly; this test only needs the default-triple resolution to work.
    const srcTauriDir = scratch();
    const destBin = stageSidecar(srcTauriDir, hostTriple(), fakeNativeBinary(hostTriple()));

    expect(process.env.TAURI_ENV_TARGET_TRIPLE).toBeUndefined();
    const result = assertRealSidecarStaged({
      srcTauriDir,
      log: () => {},
      runFile: () => "brink 0.1.0\n",
    });
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

    // On a host whose own triple happens to be x86_64-pc-windows-msvc, the
    // #2699 smoke check's host-match branch fires for real; `fakeNativeBinary`
    // is not an actually-runnable executable, so `runFile` stands in for a
    // working `--version`.
    expect(
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "x86_64-pc-windows-msvc",
        log: () => {},
        runFile: () => "brink 0.1.0\n",
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

    // On a host whose own triple happens to be aarch64-apple-darwin, the
    // #2699 smoke check's host-match branch fires for real; `fakeNativeBinary`
    // is not an actually-runnable executable, so `runFile` stands in for a
    // working `--version`.
    expect(
      assertRealSidecarStaged({
        srcTauriDir,
        triple: "aarch64-apple-darwin",
        log: () => {},
        runFile: () => "brink 0.1.0\n",
      }),
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

  it("falls back to weaker checks — never a rejection — when EXECUTABLE_MAGIC has no entry for a format executableFormatFor names", () => {
    // #2687 review: `executableFormatFor` (ensure-cli-sidecar.mjs) and
    // `EXECUTABLE_MAGIC` (this file) are maintained in different files, and
    // nothing enforces that every format the former can return has an entry
    // in the latter's table — today they happen to line up (elf/macho/pe in
    // both), but that agreement is a cross-file invariant, not something the
    // type system or a shared constant guarantees. If they ever drift,
    // `looksLikeNativeExecutable` returns `undefined` (not `false`) for the
    // orphaned format, and `assertRealSidecarStaged` MUST treat that the
    // same as "no rule for this triple at all" — never as a rejection. This
    // simulates the drift directly, rather than only asserting
    // `looksLikeNativeExecutable`'s return value in isolation.
    const srcTauriDir = scratch();
    const triple = "x86_64-unknown-linux-gnu";
    expect(executableFormatFor(triple)).toBe("elf");

    const savedElfMagic = EXECUTABLE_MAGIC.elf;
    delete (EXECUTABLE_MAGIC as Record<string, unknown>).elf;
    try {
      expect(
        looksLikeNativeExecutable(Uint8Array.from([0x7f, 0x45, 0x4c, 0x46]), "elf"),
      ).toBeUndefined();

      // A plausible-looking binary — real ELF magic — must still PASS: this
      // is exactly the "cannot judge" case, and rejecting it would be worse
      // than not checking (#2687). `triple` here is a real, common host
      // triple, so on a matching CI runner the #2699 smoke check's
      // host-match branch fires for real (the fallback path runs it too,
      // per that same review) — the staged content is not an actually-
      // runnable executable, so `runFile` stands in for a working
      // `--version`.
      const destBin = stageSidecar(
        srcTauriDir,
        triple,
        Uint8Array.from([0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01]),
      );
      expect(
        assertRealSidecarStaged({
          srcTauriDir,
          triple,
          log: () => {},
          runFile: () => "brink 0.1.0\n",
        }),
      ).toBe(destBin);
    } finally {
      EXECUTABLE_MAGIC.elf = savedElfMagic;
    }
  });
});

// #2699: the magic check above proves the staged file's FORMAT, not that it
// IS `brink-cli` or that it runs — #2691's own passing observation stood in
// `/bin/true` for a real `brink-cli`, and that would pass the magic check
// exactly as a genuine wrong-build binary would. These cases pin the
// `--version` smoke check that closes that gap, and — per the issue's own
// warning — that cross-compilation is the trap: a sidecar staged for a
// non-host triple cannot be executed here at all, so the check must degrade
// to "verified via magic only" rather than fail a legitimate cross-build or
// silently claim to have run something it did not.
describe("assertRealSidecarStaged's --version smoke check (#2699)", () => {
  it("executes --version and passes when the staged triple matches this machine's host triple", () => {
    const srcTauriDir = scratch();
    const triple = hostTriple();
    const destBin = stageSidecar(srcTauriDir, triple, fakeNativeBinary(triple));

    const logs: string[] = [];
    const calls: Array<{ file: string; args: string[] }> = [];
    const result = assertRealSidecarStaged({
      srcTauriDir,
      triple,
      log: (message: string) => logs.push(message),
      runFile: (file: string, args: string[]) => {
        calls.push({ file, args });
        return "brink 0.1.0\n";
      },
    });

    expect(result).toBe(destBin);
    expect(calls).toEqual([{ file: destBin, args: ["--version"] }]);
    // The log must name which branch ran — never claim "verified" without
    // saying whether execution actually happened.
    expect(logs.some((line) => line.includes("ran successfully"))).toBe(true);
    expect(logs.some((line) => line.includes("host-triple match"))).toBe(true);
  });

  it("throws — refusing the bundle — when a host-triple-matched sidecar carries valid magic but fails to run", () => {
    const srcTauriDir = scratch();
    const triple = hostTriple();
    stageSidecar(srcTauriDir, triple, fakeNativeBinary(triple));

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple,
        log: () => {},
        runFile: () => {
          throw new Error("spawn ENOEXEC");
        },
      }),
    ).toThrow(/failed to run/);
  });

  it("throws — refusing the bundle — when a host-triple-matched sidecar runs but prints output that is not a brink-cli version string", () => {
    // The exact `/bin/true` scenario #2699 exists to close: something that
    // RUNS and exits 0 on `--version` but is not brink-cli. GNU coreutils'
    // `true` — #2691's own stand-in for brink-cli — prints exactly this.
    const srcTauriDir = scratch();
    const triple = hostTriple();
    stageSidecar(srcTauriDir, triple, fakeNativeBinary(triple));

    expect(() =>
      assertRealSidecarStaged({
        srcTauriDir,
        triple,
        log: () => {},
        runFile: () => "true (GNU coreutils) 9.4\n",
      }),
    ).toThrow(/not a brink-cli version string/);
  });

  it("skips the smoke check — without failing — for a non-host triple, and names the branch it took", () => {
    const srcTauriDir = scratch();
    // Guaranteed to differ from this test runner's real host triple,
    // whichever platform that happens to be — the cross-compilation case
    // the issue calls out as the trap: this sidecar CANNOT be executed on
    // this machine, so the check must not even try.
    const actualHost = hostTriple();
    const triple = actualHost.includes("windows")
      ? "x86_64-unknown-linux-gnu"
      : "x86_64-pc-windows-msvc";
    expect(triple).not.toBe(actualHost);
    const destBin = stageSidecar(srcTauriDir, triple, fakeNativeBinary(triple));

    let executed = false;
    const logs: string[] = [];
    const result = assertRealSidecarStaged({
      srcTauriDir,
      triple,
      log: (message: string) => logs.push(message),
      runFile: () => {
        executed = true;
        return "";
      },
    });

    expect(result).toBe(destBin);
    expect(executed).toBe(false);
    expect(logs.some((line) => line.includes("skipped the --version smoke check"))).toBe(true);
    expect(logs.some((line) => line.includes("does not match this machine's host triple"))).toBe(
      true,
    );
    // Must not also claim it ran successfully — no branch may claim more
    // than it actually did.
    expect(logs.some((line) => line.includes("ran successfully"))).toBe(false);
  });

  it("treats an undeterminable host triple as a skip, never a rejection (tri-state audit, #2699)", () => {
    // Mirrors the #2687 lesson directly: `looksLikeNativeExecutable`
    // returns `undefined` for "cannot judge", and a `!== true` call site
    // was caught collapsing that into "rejected". The host-triple lookup
    // here has the same three-way shape (match / mismatch / undeterminable)
    // — this pins that "rustc unavailable" degrades exactly like a genuine
    // cross-build mismatch, not like a failed run.
    const srcTauriDir = scratch();
    const triple = "x86_64-unknown-linux-gnu";
    const destBin = stageSidecar(srcTauriDir, triple, fakeNativeBinary(triple));

    const emptyPath = scratch();
    const original = process.env.PATH;
    process.env.PATH = emptyPath;
    const logs: string[] = [];
    let result: string;
    try {
      // `triple` is passed explicitly so the emptied PATH only defeats the
      // smoke check's own internal host lookup, not assertRealSidecarStaged's
      // default-triple parameter (which would throw first and prove nothing
      // about the smoke check itself).
      result = assertRealSidecarStaged({
        srcTauriDir,
        triple,
        log: (message: string) => logs.push(message),
        runFile: () => {
          throw new Error("should never be called — host triple is undeterminable");
        },
      });
    } finally {
      process.env.PATH = original;
    }

    expect(result).toBe(destBin);
    expect(logs.some((line) => line.includes("skipped the --version smoke check"))).toBe(true);
    expect(logs.some((line) => line.includes("rustc unavailable"))).toBe(true);
  });
});

describe("runVersionSmokeTest", () => {
  it("reports ok:true with the trimmed stdout on a successful run", () => {
    const result = runVersionSmokeTest({
      destBin: "/fake/brink-cli",
      runFile: () => "brink 0.1.0\n",
    });
    expect(result).toEqual({ ok: true, output: "brink 0.1.0" });
  });

  it("reports ok:false with a detail message rather than throwing, on a failed run", () => {
    const result = runVersionSmokeTest({
      destBin: "/fake/brink-cli",
      runFile: () => {
        throw new Error("ENOEXEC");
      },
    });
    expect(result.ok).toBe(false);
    expect(result).toHaveProperty("detail");
    expect((result as { detail: string }).detail).toContain("ENOEXEC");
  });
});

describe("looksLikeBrinkCliVersionOutput", () => {
  it("accepts clap's `<name> <version>` format for a real brink-cli build", () => {
    expect(looksLikeBrinkCliVersionOutput("brink 0.0.11")).toBe(true);
  });

  it("accepts the bare name with no version suffix", () => {
    expect(looksLikeBrinkCliVersionOutput("brink")).toBe(true);
  });

  it("rejects GNU coreutils' `true --version` output — #2691's own stand-in for brink-cli", () => {
    expect(looksLikeBrinkCliVersionOutput("true (GNU coreutils) 9.4")).toBe(false);
  });

  it("rejects a name that merely starts with the same letters", () => {
    expect(looksLikeBrinkCliVersionOutput("brinkly 1.0")).toBe(false);
  });

  it("rejects empty output", () => {
    expect(looksLikeBrinkCliVersionOutput("")).toBe(false);
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
