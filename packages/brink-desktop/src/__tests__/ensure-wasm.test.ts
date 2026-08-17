import { execFileSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { DEFAULT_EXEC_TIMEOUT_MS, defaultRunCommand, ensureWasm, newestSource } from "../../scripts/ensure-wasm.mjs";

// `ensure-wasm.mjs` was top-level imperative script code until #2468 — the
// structurally parallel sibling of `ensure-cli-sidecar.mjs` (given the same
// treatment by #2452), and the script `pnpm --filter @brink/desktop dev`
// runs immediately BEFORE it. Nothing could call it, so nothing could test
// it: merely importing it ran the freshness scan and, on a stale tree, a
// real `wasm-pack build`. These tests exercise the exported seam directly —
// `runCommand` stands in for wasm-pack and the "crates" tree is a temp
// directory, so the whole decision runs without a toolchain.

const temporaries: string[] = [];

function scratch(): string {
  const dir = mkdtempSync(join(tmpdir(), "ensure-wasm-"));
  temporaries.push(dir);
  return dir;
}

/** Write `path`, creating parents, and stamp it at `mtimeSeconds`. */
function writeAt(path: string, contents: string, mtimeSeconds: number): void {
  mkdirSync(resolve(path, ".."), { recursive: true });
  writeFileSync(path, contents);
  utimesSync(path, mtimeSeconds, mtimeSeconds);
}

/**
 * A repo root with one crate source and, optionally, a built pkg. Times are
 * stamped explicitly so "fresh" and "stale" are decided by the comparison
 * under test rather than by how fast the filesystem is.
 */
function repoWith({
  sourceMtime,
  pkgMtime,
}: {
  sourceMtime: number;
  pkgMtime?: number;
}): { repoRoot: string; cratesDir: string; pkgWasm: string } {
  const repoRoot = scratch();
  const cratesDir = join(repoRoot, "crates");
  const pkgWasm = join(cratesDir, "brink-web/www/pkg/brink_web_bg.wasm");
  writeAt(join(cratesDir, "brink-web/src/lib.rs"), "// source\n", sourceMtime);
  if (pkgMtime !== undefined) {
    writeAt(pkgWasm, "\0asm", pkgMtime);
  }
  return { repoRoot, cratesDir, pkgWasm };
}

afterEach(() => {
  while (temporaries.length > 0) {
    const dir = temporaries.pop();
    if (dir !== undefined) {
      rmSync(dir, { recursive: true, force: true });
    }
  }
});

describe("newestSource", () => {
  it("takes the newest .rs / Cargo.toml mtime under the tree", () => {
    const cratesDir = scratch();
    writeAt(join(cratesDir, "a/src/lib.rs"), "// a\n", 1_000);
    writeAt(join(cratesDir, "b/Cargo.toml"), "[package]\n", 3_000);
    writeAt(join(cratesDir, "b/src/main.rs"), "// b\n", 2_000);
    expect(newestSource(cratesDir)).toBe(3_000_000);
  });

  it("ignores files that are neither .rs nor Cargo.toml", () => {
    const cratesDir = scratch();
    writeAt(join(cratesDir, "a/src/lib.rs"), "// a\n", 1_000);
    writeAt(join(cratesDir, "a/README.md"), "# a\n", 9_000);
    writeAt(join(cratesDir, "a/Cargo.lock"), "# lock\n", 9_000);
    expect(newestSource(cratesDir)).toBe(1_000_000);
  });

  it("skips pkg, target, node_modules and dotdirs — build output is not a source", () => {
    const cratesDir = scratch();
    writeAt(join(cratesDir, "a/src/lib.rs"), "// a\n", 1_000);
    // `pkg` is the OUTPUT the freshness check compares against: counting it
    // as a source would make every rebuild immediately look stale again.
    writeAt(join(cratesDir, "a/www/pkg/snippets/glue.rs"), "// glue\n", 9_000);
    writeAt(join(cratesDir, "a/target/debug/build/generated.rs"), "// gen\n", 9_000);
    writeAt(join(cratesDir, "a/node_modules/dep/Cargo.toml"), "[package]\n", 9_000);
    writeAt(join(cratesDir, "a/.git/hook.rs"), "// hook\n", 9_000);
    expect(newestSource(cratesDir)).toBe(1_000_000);
  });
});

describe("ensureWasm", () => {
  it("leaves a fresh pkg alone and runs no command", () => {
    const { repoRoot, cratesDir, pkgWasm } = repoWith({
      sourceMtime: 1_000,
      pkgMtime: 2_000,
    });
    const logs: string[] = [];
    const commands: string[] = [];

    const rebuilt = ensureWasm({
      repoRoot,
      cratesDir,
      pkgWasm,
      runCommand: (command: string) => {
        commands.push(command);
        return "";
      },
      log: (line: string) => logs.push(line),
    });

    expect(rebuilt).toBe(false);
    expect(commands).toEqual([]);
    expect(logs).toEqual(["[ensure-wasm] pkg is fresh"]);
  });

  it("rebuilds from the repo root when a crate source is newer than the pkg", () => {
    const { repoRoot, cratesDir, pkgWasm } = repoWith({
      sourceMtime: 2_000,
      pkgMtime: 1_000,
    });
    const commands: Array<[string, Record<string, unknown> | undefined]> = [];
    const logs: string[] = [];

    const rebuilt = ensureWasm({
      repoRoot,
      cratesDir,
      pkgWasm,
      runCommand: (command: string, options?: Record<string, unknown>) => {
        commands.push([command, options]);
        return "";
      },
      log: (line: string) => logs.push(line),
    });

    expect(rebuilt).toBe(true);
    expect(commands).toHaveLength(1);
    expect(commands[0][0]).toBe(
      "wasm-pack build crates/brink-web --target web --out-dir www/pkg",
    );
    // The command names its crate path relative to the repo root, so it can
    // only run from there.
    expect(commands[0][1]?.cwd).toBe(repoRoot);
    expect(logs).toContain(
      "[ensure-wasm] crates/ sources are newer than the built pkg — rebuilding",
    );
  });

  it("builds, and says so differently, when there is no pkg at all", () => {
    const { repoRoot, cratesDir, pkgWasm } = repoWith({ sourceMtime: 2_000 });
    const logs: string[] = [];

    const rebuilt = ensureWasm({
      repoRoot,
      cratesDir,
      pkgWasm,
      runCommand: () => "",
      log: (line: string) => logs.push(line),
    });

    expect(rebuilt).toBe(true);
    expect(logs).toContain("[ensure-wasm] no wasm pkg found — building");
  });

  it("does not treat its own output as a source that makes it stale", () => {
    // The pkg directory sits UNDER crates/, so without the `pkg` skip in
    // `newestSource` the freshly written glue next to the wasm would always
    // out-date the wasm itself and `dev` would rebuild on every run.
    const { repoRoot, cratesDir, pkgWasm } = repoWith({
      sourceMtime: 1_000,
      pkgMtime: 2_000,
    });
    writeAt(
      join(cratesDir, "brink-web/www/pkg/snippets/inline.rs"),
      "// emitted by wasm-bindgen\n",
      3_000,
    );
    const commands: string[] = [];

    const rebuilt = ensureWasm({
      repoRoot,
      cratesDir,
      pkgWasm,
      runCommand: (command: string) => {
        commands.push(command);
        return "";
      },
      log: () => {},
    });

    expect(rebuilt).toBe(false);
    expect(commands).toEqual([]);
  });
});

describe("defaultRunCommand — timeout diagnostic (#2702 review)", () => {
  // POSIX-only: `sleep` isn't a bare command on Windows the way it is here,
  // and the desktop lanes this guards are ubuntu/macOS.
  it.skipIf(process.platform === "win32")(
    "rethrows a house-style diagnostic naming the bound and BRINK_ENSURE_WASM_TIMEOUT_MS, not a bare execSync error",
    () => {
      expect(() => defaultRunCommand("sleep 5", { timeout: 30 })).toThrow(
        /TIMED OUT after 30ms.*BRINK_ENSURE_WASM_TIMEOUT_MS/s,
      );
    },
  );

  it("has a sane default when BRINK_ENSURE_WASM_TIMEOUT_MS is unset", () => {
    expect(DEFAULT_EXEC_TIMEOUT_MS).toBeGreaterThan(0);
  });

  it("BRINK_ENSURE_WASM_TIMEOUT_MS overrides the module default (#2702 review)", () => {
    // DEFAULT_EXEC_TIMEOUT_MS is read once at module-load time from
    // process.env, so proving the override works means importing it in a
    // FRESH process with the env var already set, not re-importing in this
    // one (ESM modules cache by specifier).
    const scriptPath = resolve(fileURLToPath(import.meta.url), "../../../scripts/ensure-wasm.mjs");
    const source = `const m = await import(${JSON.stringify(pathToFileURL(scriptPath).href)}); console.log(m.DEFAULT_EXEC_TIMEOUT_MS);`;
    const output = execFileSync(process.execPath, ["--input-type=module", "-e", source], {
      encoding: "utf8",
      env: { ...process.env, PATH: scratch(), BRINK_ENSURE_WASM_TIMEOUT_MS: "1234" },
    });
    expect(output.trim()).toBe("1234");
  });
});

describe("the main-guard", () => {
  const scriptPath = resolve(
    fileURLToPath(import.meta.url),
    "../../../scripts/ensure-wasm.mjs",
  );

  it("leaves the module inert on import", () => {
    // Import in a child node with an EMPTY `PATH`, so `wasm-pack` is
    // unreachable: without the guard, importing runs the real freshness
    // scan over this repo's `crates/` and then either exits 0 early (never
    // reaching the `console.log` below) or spawns `wasm-pack` and dies.
    // Node itself is spawned by absolute path, so the empty PATH costs
    // nothing else.
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
    "still rebuilds the pkg when the script is run standalone",
    () => {
      // `pnpm --filter @brink/desktop dev` runs `node scripts/ensure-wasm.mjs`
      // directly, so the guard has to fire for that invocation. The script is
      // copied into a throwaway tree laid out the same way (its defaults are
      // all relative to its own location) and given a `wasm-pack` shim on
      // `PATH`, so the real toolchain is never invoked.
      const base = scratch();
      const packageDir = join(base, "packages", "brink-desktop");
      mkdirSync(join(packageDir, "scripts"), { recursive: true });
      writeFileSync(
        join(packageDir, "scripts", "ensure-wasm.mjs"),
        readFileSync(scriptPath, "utf8"),
      );
      // A source but no pkg: the freshness check must decide "rebuild".
      writeAt(join(base, "crates", "brink-web", "src", "lib.rs"), "// source\n", 2_000);

      const shims = scratch();
      const marker = join(base, "wasm-pack-argv");
      writeFileSync(
        join(shims, "wasm-pack"),
        `#!/bin/sh\necho "$@" > ${JSON.stringify(marker)}\n`,
      );
      chmodSync(join(shims, "wasm-pack"), 0o755);

      execFileSync(process.execPath, [join(packageDir, "scripts", "ensure-wasm.mjs")], {
        encoding: "utf8",
        env: { ...process.env, PATH: shims },
      });

      expect(readFileSync(marker, "utf8").trim()).toBe(
        "build crates/brink-web --target web --out-dir www/pkg",
      );
    },
  );
});
