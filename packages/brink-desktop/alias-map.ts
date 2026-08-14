// The single source of truth for this package's module aliases (#2418).
//
// Three configs used to carry three hand-maintained copies of this map —
// `vite.config.ts` (9 entries), `vitest.config.ts` (5) and `tsconfig.json`'s
// `paths` (9) — with nothing asserting they agreed. That divergence is not
// hypothetical: the missing vitest entries made `export-artifact.test.ts`
// fail to resolve its entry point and contribute zero tests behind a
// green-looking CI step (#2409's own `vitest.config.ts` comment records it),
// so the loss was invisible rather than red.
//
// `vite.config.ts` and `vitest.config.ts` now import `desktopAliases()` from
// here, so those two can no longer drift at all. `tsconfig.json` is JSON and
// cannot import anything, so it stays a copy — `src/__tests__/alias-map.test.ts`
// asserts that copy still matches `DESKTOP_ALIASES`, and that both configs
// still go through this module rather than re-inlining a map.
//
// Every path is relative to this package's root directory (the directory
// holding this file), which is exactly the form `tsconfig.json`'s `paths`
// entries take, so the two are compared as written rather than after some
// normalization that could paper over a real difference.

import { resolve } from "path";

/** The wasm-pack output directory `crates/brink-web/www/pkg`. */
export const WASM_PKG_DIR = "../../crates/brink-web/www/pkg";

/**
 * One aliased module. `bundler` is what Vite and Vitest resolve the
 * specifier to; `types` is what `tsconfig.json` maps it to. They are the
 * same string for every alias but one — see `brink-web` below.
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
 * Alias → target paths, relative to this package's root.
 *
 * Mirrors `packages/brink-studio/vite.config.ts`'s DEV-MODE resolution: the
 * studio and every internal package resolve to workspace SOURCE, and the
 * wasm glue resolves to the built pkg — so `tauri dev` and the unit suite
 * both run against the live tree without requiring `pnpm build` in five
 * packages first. Keep in sync with the playground's map when packages move.
 */
export const DESKTOP_ALIASES: Readonly<Record<string, AliasEntry>> = {
  // The one alias whose two consumers genuinely need different targets: a
  // bundler wants the ESM glue file, while `tsc` needs the package
  // DIRECTORY, whose `package.json` points at `brink_web.d.ts`. Mapping
  // `paths` straight at `brink_web.js` resolves to an untyped JS file and
  // fails `tsc --noEmit` with TS7016.
  "brink-web": { bundler: `${WASM_PKG_DIR}/brink_web.js`, types: WASM_PKG_DIR },
  "@brink-lang/web": same("../wasm/src/index.ts"),
  "@brink-lang/studio": same("../brink-studio/src/index.ts"),
  "@brink/wasm-types": same("../wasm-types/src/index.ts"),
  "@brink/ink-operations": same("../ink-operations/src/index.ts"),
  "@brink-lang/editor": same("../ink-editor/src/index.ts"),
  "@brink/studio-shell": same("../studio-shell/src/index.ts"),
  "@brink/studio-store": same("../studio-store/src/index.ts"),
  "@brink/studio-ui": same("../studio-ui/src/index.ts"),
};

/**
 * The alias map in the shape Vite and Vitest want: absolute paths, resolved
 * against `packageRoot` (pass the config file's own directory).
 */
export function desktopAliases(packageRoot: string): Record<string, string> {
  return Object.fromEntries(
    Object.entries(DESKTOP_ALIASES).map(([specifier, entry]) => [
      specifier,
      resolve(packageRoot, entry.bundler),
    ]),
  );
}

/**
 * The alias map in the shape `tsconfig.json`'s `paths` wants: one-element
 * arrays of package-root-relative paths. The drift test compares this to
 * the committed JSON.
 */
export function desktopTsconfigPaths(): Record<string, string[]> {
  return Object.fromEntries(
    Object.entries(DESKTOP_ALIASES).map(([specifier, entry]) => [
      specifier,
      [entry.types],
    ]),
  );
}
