// The single source of truth for this package's module aliases (#2464).
//
// `@brink-lang/studio` resolves the same workspace specifiers in five places:
// `vite.config.ts` (dev server + the published library build),
// `vite.config.embed.ts` (the standalone playground app), `vitest.config.ts`
// (this package's unit suite), `tsconfig.json`'s `paths` (`pnpm typecheck`)
// and `tsconfig.build.json`'s `paths` (the published `d.ts` rollup). All five
// were hand-maintained copies, and the only thing comparing them lived in
// `packages/brink-desktop` — a private package keeping a published one
// honest, so a studio-only change failed a suite in a different package
// (#2450 / PR #2460).
//
// The three bundler configs now import their maps from here, so those three
// cannot drift from each other at all. The two tsconfigs are JSON and cannot
// import anything, so they stay copies — `src/__tests__/alias-map.test.ts` is
// what notices when a copy stops matching, and what notices if a config
// re-inlines a map instead of going through this module.
//
// The differences between the five are DELIBERATE, and they stay spelled out
// as separate exports rather than flattened into one map — the reasoning PR
// #2460 recorded when it decided against a shared module:
//
//   - the wasm pair (`brink-web`, `@brink-lang/web`) is DEV-ONLY in
//     `vite.config.ts`: the library build externalizes `@brink-lang/web`
//     (`rollupOptions.external`), so an unconditional alias would inline the
//     wrapper into the published bundle;
//   - `vitest.config.ts` repoints `brink-web` at this package's jsdom mock,
//     because the unit suite must not load real wasm (the desktop suite
//     resolves the real glue on purpose — see that package's
//     `vitest.config.ts`);
//   - `tsconfig.json` maps `brink-web` at the pkg DIRECTORY rather than the
//     glue file, because `tsc` needs the `package.json` that names
//     `brink_web.d.ts`; mapping it straight at `brink_web.js` fails TS7016;
//   - `tsconfig.build.json` drops the wasm pair entirely (#2465) — see
//     `DTS_ROLLUP_EXCLUDES` below.
//
// This module does NOT replace PR #2460's cross-package guard
// (`packages/brink-desktop/src/__tests__/playground-alias-parity.test.ts`),
// which still compares these copies against the desktop shell's own map.
// That guard pins the RELATIONSHIP between two packages; this one gives the
// studio ownership of its own five copies. This invariant is recorded in
// `docs/brink-studio-spec.md` § "One alias map, owned by this package"; the
// cross-package one is `docs/desktop-shell-spec.md` § "Alias map parity with
// the playground" (#2450).

import { resolve } from "path";

/** The wasm-pack output directory `crates/brink-web/www/pkg`. */
export const WASM_PKG_DIR = "../../crates/brink-web/www/pkg";

/** This package's jsdom stand-in for the wasm glue, used only by `vitest.config.ts`. */
export const BRINK_WEB_TEST_MOCK = "src/__mocks__/brink-web.ts";

/**
 * One aliased module. `bundler` is what Vite and Vitest resolve the
 * specifier to; `types` is what a `tsconfig` `paths` entry maps it to. They
 * are the same string for every alias but one — see `brink-web` below.
 */
export interface AliasEntry {
  readonly bundler: string;
  readonly types: string;
}

/** An alias whose bundler and `tsc` targets are the same path. */
function same(path: string): AliasEntry {
  return { bundler: path, types: path };
}

/**
 * The private workspace packages the studio BUNDLES into its own artifacts,
 * resolved to source so the dev server, the embed build and the unit suite
 * all run against the live tree without `pnpm build` in five packages first.
 *
 * Every config applies these unconditionally — they are the part with no
 * exceptions, which is what makes the exceptions below readable.
 *
 * Paths are relative to this package's root (the directory holding this
 * file), which is exactly the form a `tsconfig` `paths` entry takes, so the
 * two are compared as written rather than after a normalization that could
 * paper over a real difference.
 */
export const STUDIO_PACKAGE_ALIASES: Readonly<Record<string, AliasEntry>> = {
  "@brink/wasm-types": same("../wasm-types/src/index.ts"),
  "@brink/ink-operations": same("../ink-operations/src/index.ts"),
  "@brink-lang/editor": same("../ink-editor/src/index.ts"),
  "@brink/studio-shell": same("../studio-shell/src/index.ts"),
  "@brink/studio-store": same("../studio-store/src/index.ts"),
  "@brink/studio-ui": same("../studio-ui/src/index.ts"),
};

/**
 * The wasm pair. Applied only where the wasm is actually loaded: the dev
 * server (`vite.config.ts` under `command === "serve"`) and the
 * self-contained embed app (`vite.config.embed.ts`). The library build
 * deliberately applies neither.
 */
export const STUDIO_WASM_ALIASES: Readonly<Record<string, AliasEntry>> = {
  // The one alias whose two consumers genuinely need different targets: a
  // bundler wants the ESM glue file, while `tsc` needs the package
  // DIRECTORY, whose `package.json` points at `brink_web.d.ts`.
  "brink-web": { bundler: `${WASM_PKG_DIR}/brink_web.js`, types: WASM_PKG_DIR },
  "@brink-lang/web": same("../wasm/src/index.ts"),
};

/**
 * The specifiers `tsconfig.build.json` omits — exactly the wasm pair, for
 * two different reasons (#2465):
 *
 *   - `@brink-lang/web` is left to resolve through `node_modules` to
 *     `packages/wasm/dist/index.d.ts` so the rollup keeps it as an external
 *     import rather than inlining its classes, which are nominally typed via
 *     private fields;
 *   - `brink-web` needs no mapping at all, because `src/index.ts` — the only
 *     entry that rollup type-checks — never imports that specifier; only
 *     `@brink-lang/web`'s wrapper does.
 */
export const DTS_ROLLUP_EXCLUDES: readonly string[] = Object.keys(STUDIO_WASM_ALIASES);

/** The `tsconfig` that `tsup.config.ts` must run the `d.ts` rollup against. */
export const DTS_ROLLUP_TSCONFIG = "tsconfig.build.json";

/** Absolute bundler targets for a map, resolved against `packageRoot`. */
function bundlerTargets(
  entries: Readonly<Record<string, AliasEntry>>,
  packageRoot: string,
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(entries).map(([specifier, entry]) => [
      specifier,
      resolve(packageRoot, entry.bundler),
    ]),
  );
}

/**
 * The bundled workspace packages, in the shape Vite and Vitest want. Every
 * config in this package spreads this (pass the config file's `__dirname`).
 */
export function studioPackageAliases(packageRoot: string): Record<string, string> {
  return bundlerTargets(STUDIO_PACKAGE_ALIASES, packageRoot);
}

/**
 * The wasm pair as the dev server and the embed app build resolve it: the
 * real wasm-pack glue and the `@brink-lang/web` wrapper source.
 */
export function studioWasmAliases(packageRoot: string): Record<string, string> {
  return bundlerTargets(STUDIO_WASM_ALIASES, packageRoot);
}

/**
 * The wasm pair as the UNIT SUITE resolves it: `brink-web` is repointed at
 * this package's jsdom mock, since vitest must not load real wasm. The
 * wrapper still resolves to source, so the code under test is the real one.
 */
export function studioTestWasmAliases(packageRoot: string): Record<string, string> {
  return {
    ...bundlerTargets(STUDIO_WASM_ALIASES, packageRoot),
    "brink-web": resolve(packageRoot, BRINK_WEB_TEST_MOCK),
  };
}

/** `paths` targets for a map, in the one-element-array shape `tsconfig` wants. */
function typesTargets(
  entries: Readonly<Record<string, AliasEntry>>,
): Record<string, string[]> {
  return Object.fromEntries(
    Object.entries(entries).map(([specifier, entry]) => [specifier, [entry.types]]),
  );
}

/**
 * `tsconfig.json`'s `paths`: every specifier, with `brink-web` on its
 * `tsc`-only directory target. The drift test compares this to the committed
 * JSON.
 */
export function studioTsconfigPaths(): Record<string, string[]> {
  return {
    ...typesTargets(STUDIO_WASM_ALIASES),
    ...typesTargets(STUDIO_PACKAGE_ALIASES),
  };
}

/**
 * `tsconfig.build.json`'s `paths`: the bundled packages only, the wasm pair
 * dropped per `DTS_ROLLUP_EXCLUDES`.
 */
export function studioBuildTsconfigPaths(): Record<string, string[]> {
  return typesTargets(STUDIO_PACKAGE_ALIASES);
}
