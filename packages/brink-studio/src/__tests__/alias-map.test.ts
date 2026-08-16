// @vitest-environment node
//
// This file loads the build tooling itself (vite, tsup) to compare what those
// tools would actually resolve, rather than scraping config text. Under the
// suite's default jsdom environment esbuild refuses to start — jsdom's
// TextEncoder does not produce a real Uint8Array — so this one file opts back
// out to node.
/**
 * Alias-map drift guard, this package's own (#2464).
 *
 * `@brink-lang/studio` resolves the same workspace specifiers in five places
 * — `vite.config.ts`, `vite.config.embed.ts`, `vitest.config.ts`,
 * `tsconfig.json`'s `paths` and `tsconfig.build.json`'s `paths`. Every one
 * was a hand-maintained copy, and the only thing comparing them lived in
 * `packages/brink-desktop` (PR #2460, #2450): the PRIVATE desktop package
 * kept the PUBLISHED studio's configs honest, so a studio-only change failed
 * a suite in a different package with attribution the author had to work out.
 *
 * The three bundler configs now import `../../alias-map.ts`, so they cannot
 * drift from each other at all. The two tsconfigs are JSON and cannot import
 * anything, so they stay copies — this file is what notices when a copy stops
 * matching, and what notices if a config re-inlines a map instead.
 *
 * This does NOT replace the cross-package guard: that one pins the
 * relationship between the desktop shell's map and these copies, which is a
 * different invariant (`docs/desktop-shell-spec.md` § "Alias map parity with
 * the playground" (#2450)). This file's own invariant is recorded in
 * `docs/brink-studio-spec.md` § "One alias map, owned by this package".
 *
 * A missing alias is silent rather than red — the specifier falls back to the
 * workspace symlink and a `dist/` that `pnpm install` does not build — so an
 * existence check on every target is part of the guard.
 */
import { describe, expect, it } from "vitest";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  BRINK_WEB_TEST_MOCK,
  DTS_ROLLUP_TSCONFIG,
  STUDIO_PACKAGE_ALIASES,
  STUDIO_WASM_ALIASES,
  studioBuildTsconfigPaths,
  studioPackageAliases,
  studioTestWasmAliases,
  studioTsconfigPaths,
  studioWasmAliases,
} from "../../alias-map.js";
import studioVite from "../../vite.config.js";
import studioEmbed from "../../vite.config.embed.js";
import studioVitest from "../../vitest.config.js";
import studioTsup from "../../tsup.config.js";

/** `packages/brink-studio`, the root every alias path is relative to. */
const packageRoot = resolve(fileURLToPath(new URL(".", import.meta.url)), "../..");

/** Specifier → absolute target path, the shape every bundler config produces. */
type AliasMap = Record<string, string>;

/** The subset of a vite/vitest config this guard reads. */
interface AliasConfig {
  readonly resolve?: { readonly alias?: unknown };
  readonly build?: { readonly rollupOptions?: { readonly external?: unknown } };
}

/**
 * Evaluate a config module's default export. `vite.config.ts` exports a
 * function of `{ command }` — that conditionality is the whole reason the
 * wasm pair is a separate map — while the embed and vitest configs export
 * plain objects.
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

/**
 * A `tsconfig` is JSONC by spec; these two carry only whole-line `//`
 * comments, and dropping those keeps that from becoming a trap without
 * pulling in a JSONC parser (the same treatment
 * `packages/brink-desktop/src/__tests__/alias-map.test.ts` gives its copy).
 */
function readTsconfigPaths(file: string): Record<string, string[]> {
  const text = readFileSync(resolve(packageRoot, file), "utf8")
    .split("\n")
    .filter((line) => !line.trim().startsWith("//"))
    .join("\n");
  const parsed: { compilerOptions: { paths: Record<string, string[]> } } = JSON.parse(text);
  return parsed.compilerOptions.paths;
}

function sourceOf(file: string): string {
  return readFileSync(resolve(packageRoot, file), "utf8");
}

describe("studio alias map", () => {
  it("resolves the dev server exactly as the shared map does", () => {
    expect(aliasesOf(studioVite, "serve")).toEqual({
      ...studioWasmAliases(packageRoot),
      ...studioPackageAliases(packageRoot),
    });
  });

  it("keeps the wasm pair off the library build, which externalizes @brink-lang/web", () => {
    // An unconditional wasm alias here would inline the @brink-lang/web
    // wrapper into the published bundle despite rollupOptions.external — so
    // this checks both sides of that claim, not just the alias absence:
    // removing "@brink-lang/web" from rollupOptions.external would leave the
    // alias assertions below green while the published bundle started
    // inlining the wrapper.
    const lib = aliasesOf(studioVite, "build");
    expect(lib).toEqual(studioPackageAliases(packageRoot));
    for (const specifier of Object.keys(STUDIO_WASM_ALIASES)) {
      expect(Object.keys(lib), specifier).not.toContain(specifier);
    }
    const external = configOf(studioVite, "build").build?.rollupOptions?.external;
    expect(external, "build.rollupOptions.external must be present").toBeDefined();
    expect(external).toContain("@brink-lang/web");
  });

  it("applies the wasm pair unconditionally in the embed app build", () => {
    expect(aliasesOf(studioEmbed, "build")).toEqual({
      ...studioWasmAliases(packageRoot),
      ...studioPackageAliases(packageRoot),
    });
  });

  it("points the unit suite's brink-web at the jsdom mock and nothing else", () => {
    const suite = aliasesOf(studioVitest, "serve");
    expect(suite).toEqual({
      ...studioTestWasmAliases(packageRoot),
      ...studioPackageAliases(packageRoot),
    });
    // Named rather than implied: this is the one specifier whose test target
    // differs from its dev target, and the mock is why the suite runs under
    // jsdom without wasm.
    expect(suite["brink-web"]).toBe(resolve(packageRoot, BRINK_WEB_TEST_MOCK));
    expect(suite["brink-web"]).not.toBe(studioWasmAliases(packageRoot)["brink-web"]);
  });

  it("tsconfig paths match the shared map, brink-web's tsc-only target included", () => {
    expect(readTsconfigPaths("tsconfig.json")).toEqual(studioTsconfigPaths());
    // The bundler wants the glue file, tsc the pkg directory — mapping paths
    // straight at brink_web.js resolves an untyped JS file (TS7016).
    expect(STUDIO_WASM_ALIASES["brink-web"].types).not.toBe(
      STUDIO_WASM_ALIASES["brink-web"].bundler,
    );
  });

  it("tsconfig.build.json drops exactly the wasm pair from the d.ts rollup", () => {
    // A second assertion re-deriving the dropped set from DTS_ROLLUP_EXCLUDES
    // would be vacuous: that constant is Object.keys(STUDIO_WASM_ALIASES) by
    // construction, and studioBuildTsconfigPaths() is built from the
    // disjoint STUDIO_PACKAGE_ALIASES — the two partition the specifier set
    // by definition, so no mutation of the committed JSON could fail it
    // without first failing the toEqual below.
    const paths = readTsconfigPaths(DTS_ROLLUP_TSCONFIG);
    expect(paths).toEqual(studioBuildTsconfigPaths());
  });

  it("keeps tsup pointed at the rollup tsconfig that drops the wasm pair", () => {
    // tsup.config.ts is the file this package typechecks nowhere else
    // (tsconfig.node.json, #2464). If it were repointed at tsconfig.json the
    // rollup would resolve @brink-lang/web to source and inline its classes.
    const tsup = studioTsup as { tsconfig?: string; external?: string[] };
    expect(tsup.tsconfig).toBe(DTS_ROLLUP_TSCONFIG);
    expect(tsup.external).toContain("@brink-lang/web");
  });

  it("resolves every alias to a target that exists on disk", () => {
    const bundler = {
      ...studioWasmAliases(packageRoot),
      ...studioPackageAliases(packageRoot),
    };
    for (const [specifier, target] of Object.entries(bundler)) {
      expect(existsSync(target), `${specifier} → ${target}`).toBe(true);
    }
    expect(
      existsSync(resolve(packageRoot, BRINK_WEB_TEST_MOCK)),
      `brink-web mock → ${BRINK_WEB_TEST_MOCK}`,
    ).toBe(true);
    for (const [specifier, targets] of Object.entries(studioTsconfigPaths())) {
      const target = resolve(packageRoot, targets[0]);
      expect(existsSync(target), `${specifier} (tsc) → ${target}`).toBe(true);
    }
  });

  it("keeps all three bundler configs on the shared map rather than an inline copy", () => {
    for (const config of ["vite.config.ts", "vite.config.embed.ts", "vitest.config.ts"]) {
      const source = sourceOf(config);
      expect(source, config).toContain("studioPackageAliases(__dirname)");
      // An inlined entry is the drift this module exists to prevent: the
      // alias keys belong in alias-map.ts and nowhere else.
      for (const specifier of Object.keys(STUDIO_PACKAGE_ALIASES)) {
        expect(source, `${config} inlines ${specifier}`).not.toContain(`"${specifier}":`);
      }
    }
  });

  it("typechecks every root-level config module in this package's own program", () => {
    // The typecheck-by-accident half of #2464: tsconfig.json's include is
    // ["src"], so before tsconfig.node.json none of the root-level modules
    // were in a program of this package's own — three were reached only by
    // @brink/desktop importing them, and tsup.config.ts by nothing at all.
    // Asserting against the DIRECTORY rather than a fixed list is what makes
    // a config module added later fail this instead of slipping through.
    const program: { include: string[] } = JSON.parse(
      readFileSync(resolve(packageRoot, "tsconfig.node.json"), "utf8")
        .split("\n")
        .filter((line) => !line.trim().startsWith("//"))
        .join("\n"),
    );
    const rootModules = readdirSync(packageRoot)
      .filter(
        (name) =>
          (name.endsWith(".ts") || name.endsWith(".mts") || name.endsWith(".cts")) &&
          !name.endsWith(".d.ts") &&
          !name.endsWith(".d.mts") &&
          !name.endsWith(".d.cts"),
      )
      .sort();
    expect(rootModules.length, "root-level .ts/.mts/.cts modules").toBeGreaterThan(0);

    // "include" lists every root-level module by name, no globs — a config
    // module added as any of the three extensions has to be added here
    // explicitly, or this comparison fails.
    expect([...program.include].sort()).toEqual(rootModules);
    // And that the package script actually runs it — an unrun program checks
    // nothing.
    const pkg: { scripts: Record<string, string> } = JSON.parse(
      readFileSync(resolve(packageRoot, "package.json"), "utf8"),
    );
    expect(pkg.scripts.typecheck).toContain("-p tsconfig.node.json");
  });

  it("typechecks e2e test specs in their own program", () => {
    // #2607: e2e specs live outside src/ and were typechecked by nothing
    // before tsconfig.e2e.json, only transformed at run time by Playwright's
    // esbuild-based loader. The e2e program ensures they're type-checked
    // before running, like src/ and config modules are.
    const e2eProgram: { include: string[] } = JSON.parse(
      readFileSync(resolve(packageRoot, "tsconfig.e2e.json"), "utf8")
        .split("\n")
        .filter((line) => !line.trim().startsWith("//"))
        .join("\n"),
    );
    expect(e2eProgram.include, "tsconfig.e2e.json must include e2e directory").toContain("e2e");

    // Verify e2e specs actually exist and are checked
    const e2eDir = resolve(packageRoot, "e2e");
    expect(existsSync(e2eDir), "e2e directory must exist").toBe(true);
    const e2eSpecs = readdirSync(e2eDir)
      .filter((name) => name.endsWith(".spec.ts"))
      .sort();
    expect(e2eSpecs.length, "e2e/*.spec.ts files").toBeGreaterThan(0);

    // Verify the package script runs it — an unrun program checks nothing
    const pkg: { scripts: Record<string, string> } = JSON.parse(
      readFileSync(resolve(packageRoot, "package.json"), "utf8"),
    );
    expect(pkg.scripts.typecheck).toContain("-p tsconfig.e2e.json");
  });
});
