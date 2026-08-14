/**
 * Alias-map drift guard (#2418, gap 1).
 *
 * This package resolves the same nine workspace specifiers in three places:
 * `vite.config.ts` (the app), `vitest.config.ts` (this suite) and
 * `tsconfig.json`'s `paths` (`tsc --noEmit`). Until #2418 each was a
 * hand-maintained copy with nothing comparing them, and the copies had
 * already diverged — `vitest.config.ts` carried five of the nine, which is
 * why `export-artifact.test.ts` failed to resolve its entry point and
 * contributed zero tests behind a green-looking CI step (#2409).
 *
 * The two bundler configs now import `desktopAliases()` from
 * `../../alias-map.ts`, so they cannot drift from each other at all.
 * `tsconfig.json` is JSON and cannot import anything, so it remains a copy —
 * this file is what notices when that copy stops matching, and what notices
 * if either config re-inlines a map instead of going through the module.
 *
 * A missing alias is silent rather than red, so an existence check on every
 * target is part of the guard: a path that no longer points at a real file
 * would send the specifier back to a `dist/` fallback that `pnpm install`
 * does not build.
 */
import { describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  DESKTOP_ALIASES,
  desktopAliases,
  desktopTsconfigPaths,
} from "../../alias-map.js";

/** `packages/brink-desktop`, the root every alias path is relative to. */
const packageRoot = resolve(fileURLToPath(new URL(".", import.meta.url)), "../..");

/**
 * `tsconfig.json` is JSONC by spec; this one carries no comments today, and
 * dropping whole-line `//` comments keeps that from becoming a trap without
 * pulling in a JSONC parser.
 */
function readTsconfig(): { compilerOptions: { paths: Record<string, string[]> } } {
  const text = readFileSync(resolve(packageRoot, "tsconfig.json"), "utf8")
    .split("\n")
    .filter((line) => !line.trim().startsWith("//"))
    .join("\n");
  return JSON.parse(text);
}

describe("desktop alias map", () => {
  it("tsconfig paths match the shared alias map entry for entry", () => {
    expect(readTsconfig().compilerOptions.paths).toEqual(desktopTsconfigPaths());
  });

  it("resolves every alias to a target that exists on disk", () => {
    for (const [specifier, target] of Object.entries(desktopAliases(packageRoot))) {
      expect(existsSync(target), `${specifier} → ${target}`).toBe(true);
    }
    for (const [specifier, entry] of Object.entries(DESKTOP_ALIASES)) {
      const target = resolve(packageRoot, entry.types);
      expect(existsSync(target), `${specifier} (tsc) → ${target}`).toBe(true);
    }
  });

  it("keeps both bundler configs on the shared map rather than an inline copy", () => {
    for (const config of ["vite.config.ts", "vitest.config.ts"]) {
      const source = readFileSync(resolve(packageRoot, config), "utf8");
      expect(source, config).toContain("desktopAliases(__dirname)");
      // An inlined entry is the drift this module exists to prevent: the
      // alias keys belong in `alias-map.ts` and nowhere else.
      expect(source, config).not.toContain('"@brink-lang/editor":');
    }
  });
});
