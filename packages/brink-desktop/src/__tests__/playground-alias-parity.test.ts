/**
 * Cross-package alias drift guard: this package's map vs the playground's (#2450).
 *
 * `alias-map.ts` states its own purpose as mirroring
 * `packages/brink-studio/vite.config.ts`'s dev-mode resolution, and asks that
 * the two be kept in sync "when packages move". Nothing compared them, so the
 * playground's copies (`vite.config.ts`, `vite.config.embed.ts`,
 * `vitest.config.ts`, `tsconfig.json`'s `paths` and `tsconfig.build.json`'s
 * `paths`) carried exactly the unguarded-copy risk #2418 removed inside this
 * package — and one of them had already drifted: the embed config was
 * missing `@brink/studio-shell`.
 *
 * `tsconfig.build.json` is guarded against a narrower expectation than the
 * other four, not the full `sharedSpecifiers` set — see
 * `DTS_ROLLUP_EXCLUDES` below.
 *
 * The two packages' maps are NOT one map, and folding them into a single
 * shared module would erase differences that are load-bearing:
 *
 *   - `@brink-lang/studio` — the desktop shell aliases the studio to workspace
 *     source; the studio cannot alias itself.
 *   - The playground applies the wasm pair (`brink-web`, `@brink-lang/web`)
 *     only under `command === "serve"`. Its LIBRARY build externalizes
 *     `@brink-lang/web` (`vite.config.ts`'s `rollupOptions.external`), so an
 *     unconditional alias would inline the wrapper into the published bundle.
 *   - Under vitest the playground resolves `brink-web` to its own jsdom mock,
 *     while this package resolves the real wasm-bindgen glue deliberately —
 *     `vitest.config.ts` records why (the mock would make
 *     `export-artifact.test.ts` prove nothing about a compiled artifact).
 *
 * So what is asserted here is the intended RELATIONSHIP rather than sameness:
 * one specifier set either side of one named exception, and one resolved
 * target per shared specifier. Adding an alias on either side without the
 * other, or repointing one copy, turns this file red.
 */
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DESKTOP_ALIASES, desktopAliases } from "../../alias-map.js";
import playgroundVite from "../../../brink-studio/vite.config.js";
import playgroundEmbed from "../../../brink-studio/vite.config.embed.js";
import playgroundVitest from "../../../brink-studio/vitest.config.js";

/** `packages/brink-desktop`, the root this package's alias paths are relative to. */
const packageRoot = resolve(fileURLToPath(new URL(".", import.meta.url)), "../..");

/** `packages/brink-studio`, the root the playground's alias paths are relative to. */
const playgroundRoot = resolve(packageRoot, "../brink-studio");

/**
 * The specifiers this package aliases and the playground cannot: the studio
 * is what the playground IS, so it has no self-alias. Every other entry is
 * expected on both sides — that is what makes the set comparison a trip-wire
 * rather than a subset check.
 */
const DESKTOP_ONLY = ["@brink-lang/studio"];

/**
 * `tsconfig.build.json` feeds vite-plugin-dts's published `d.ts` rollup for
 * `src/index.ts`, the public entry point. It omits both wasm specifiers, for
 * different reasons: `@brink-lang/web` resolves through `node_modules` on
 * purpose so the rollup keeps it as an external import instead of inlining
 * its (privately typed) classes, and `brink-web` has no mapping at all
 * because `src/index.ts` never imports that raw specifier — only
 * `@brink-lang/web`'s wrapper does, so `tsc` never needs to resolve it for
 * this entry point.
 */
const DTS_ROLLUP_EXCLUDES = ["@brink-lang/web", "brink-web"];

/** Specifier → absolute target path, the shape both bundler configs produce. */
type AliasMap = Record<string, string>;

/** The subset of a vite/vitest config this guard reads. */
interface AliasConfig {
  readonly resolve?: { readonly alias?: unknown };
}

/**
 * Evaluate a config module's default export. `vite.config.ts` exports a
 * function of `{ command }` (that conditionality is the whole reason the two
 * maps differ), while the embed and vitest configs export plain objects.
 */
function configOf(exported: unknown, command: "serve" | "build"): AliasConfig {
  const value =
    typeof exported === "function"
      ? (exported as (env: { command: string; mode: string }) => unknown)({
          command,
          mode: command === "serve" ? "development" : "production",
        })
      : exported;
  expect(value, "config must resolve synchronously").not.toBeInstanceOf(Promise);
  expect(typeof value, "config must be an object").toBe("object");
  return value as AliasConfig;
}

/**
 * `resolve.alias` in the record form every config here uses. The array form
 * is valid vite too, so it is rejected loudly rather than read as empty — an
 * unread map would make this guard pass by seeing nothing.
 */
function aliasesOf(exported: unknown, command: "serve" | "build"): AliasMap {
  const alias = configOf(exported, command).resolve?.alias;
  expect(alias, "resolve.alias must be present").toBeDefined();
  expect(Array.isArray(alias), "resolve.alias must use the record form").toBe(false);
  const record = alias as Record<string, unknown>;
  for (const [specifier, target] of Object.entries(record)) {
    expect(typeof target, `${specifier} must alias to a path string`).toBe("string");
  }
  return record as AliasMap;
}

function specifiers(map: Record<string, unknown>): string[] {
  return Object.keys(map).sort();
}

/** The specifiers both packages are expected to carry. */
const sharedSpecifiers = specifiers(DESKTOP_ALIASES).filter(
  (specifier) => !DESKTOP_ONLY.includes(specifier),
);

/** This package's map, resolved the way `vite.config.ts` resolves it. */
const desktopBundler = desktopAliases(packageRoot);

/**
 * `tsconfig.json` is JSONC by spec; the playground's carries no comments
 * today, and dropping whole-line `//` comments keeps that from becoming a
 * trap without pulling in a JSONC parser (same treatment `alias-map.test.ts`
 * gives this package's copy).
 */
function playgroundTsconfigPaths(): Record<string, string[]> {
  const text = readFileSync(resolve(playgroundRoot, "tsconfig.json"), "utf8")
    .split("\n")
    .filter((line) => !line.trim().startsWith("//"))
    .join("\n");
  const parsed: { compilerOptions: { paths: Record<string, string[]> } } = JSON.parse(text);
  return parsed.compilerOptions.paths;
}

/**
 * `tsconfig.build.json` extends `tsconfig.json` but replaces `paths`
 * wholesale (TypeScript does not merge `paths` across `extends`), so this
 * reads the file's own `paths` rather than the base config's.
 */
function playgroundTsconfigBuildPaths(): Record<string, string[]> {
  const text = readFileSync(resolve(playgroundRoot, "tsconfig.build.json"), "utf8")
    .split("\n")
    .filter((line) => !line.trim().startsWith("//"))
    .join("\n");
  const parsed: { compilerOptions: { paths: Record<string, string[]> } } = JSON.parse(text);
  return parsed.compilerOptions.paths;
}

describe("desktop ↔ playground alias parity", () => {
  it("aliases the same specifiers as the playground dev server, bar the self-alias", () => {
    expect(specifiers(aliasesOf(playgroundVite, "serve"))).toEqual(sharedSpecifiers);
  });

  it("resolves every shared specifier to the same file as the playground dev server", () => {
    for (const [specifier, target] of Object.entries(aliasesOf(playgroundVite, "serve"))) {
      expect(target, specifier).toBe(desktopBundler[specifier]);
    }
  });

  it("matches the playground's standalone app build entry for entry", () => {
    const embed = aliasesOf(playgroundEmbed, "build");
    expect(specifiers(embed)).toEqual(sharedSpecifiers);
    for (const [specifier, target] of Object.entries(embed)) {
      expect(target, specifier).toBe(desktopBundler[specifier]);
    }
  });

  it("matches the playground's vitest map, save the deliberate brink-web mock", () => {
    const suite = aliasesOf(playgroundVitest, "serve");
    expect(specifiers(suite)).toEqual(sharedSpecifiers);
    for (const [specifier, target] of Object.entries(suite)) {
      if (specifier === "brink-web") {
        // The playground runs under jsdom and must not touch real wasm; this
        // package's suite must (`vitest.config.ts`). The exception is named
        // here so it stays a decision rather than drift.
        expect(target).toBe(resolve(playgroundRoot, "src/__mocks__/brink-web.ts"));
        continue;
      }
      expect(target, specifier).toBe(desktopBundler[specifier]);
    }
  });

  it("leaves the playground's wasm aliases dev-only, so its library build still externalizes them", () => {
    const lib = specifiers(aliasesOf(playgroundVite, "build"));
    expect(lib).not.toContain("@brink-lang/web");
    expect(lib).not.toContain("brink-web");
    // Everything else is unconditional on both sides — a wasm-only exception.
    expect(lib).toEqual(
      sharedSpecifiers.filter(
        (specifier) => specifier !== "@brink-lang/web" && specifier !== "brink-web",
      ),
    );
  });

  it("agrees with the playground's tsconfig paths, brink-web's tsc-only target included", () => {
    const paths = playgroundTsconfigPaths();
    expect(specifiers(paths)).toEqual(sharedSpecifiers);
    for (const [specifier, targets] of Object.entries(paths)) {
      expect(targets, specifier).toHaveLength(1);
      expect(resolve(playgroundRoot, targets[0]), specifier).toBe(
        resolve(packageRoot, DESKTOP_ALIASES[specifier].types),
      );
    }
  });

  it("agrees with the playground's d.ts-rollup tsconfig, the wasm pair excluded by design", () => {
    const paths = playgroundTsconfigBuildPaths();
    const expected = sharedSpecifiers.filter(
      (specifier) => !DTS_ROLLUP_EXCLUDES.includes(specifier),
    );
    expect(specifiers(paths)).toEqual(expected);
    for (const [specifier, targets] of Object.entries(paths)) {
      expect(targets, specifier).toHaveLength(1);
      expect(resolve(playgroundRoot, targets[0]), specifier).toBe(
        resolve(packageRoot, DESKTOP_ALIASES[specifier].types),
      );
    }
  });
});
