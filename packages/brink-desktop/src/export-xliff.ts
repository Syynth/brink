/**
 * File > Export XLIFF… (D3, #2392). Extracted out of `main.tsx` (2026-08
 * review finding: logic living directly in `main.tsx` cannot be unit-tested
 * — `quit.ts` + `QuitSaveApi` exist for exactly this reason) behind an
 * injectable `{ studio, toXliff, saveBytes }` seam.
 *
 * This used to shell out to the `brink-cli` **sidecar** — a native binary
 * inside the signed `.app`, there only so that `export-xliff` had somewhere
 * to run. That put a second copy of the compiler core in the bundle, pinned
 * to the wasm by `.inkb`'s exact-match container version, which is the
 * coupling that made an over-the-air web-bundle update unsafe: an OTA'd
 * wasm would emit artifacts the stale sidecar silently refuses. The
 * operation now runs in the wasm alongside the compile that produced the
 * bytes, so there is nothing left to drift. See `docs/desktop-ota-spec.md`
 * Stage 1 and `docs/decision-log.md` (2026-09-14).
 *
 * Deliberately minimal: export the currently open project at the source
 * language, prompting only for where to save the `.xlf`. The fuller intl UI
 * (locale picker, compile-locale/regenerate batch ops) is future work —
 * `@brink-lang/web` exports `compileLocale`/`regenerateXliff` for it, but
 * no menu item reaches them yet.
 *
 * What is exported is the project's COMPILED artifact, reached through the
 * same `compile.run` road Export Story (.inkb) uses — never a re-read of
 * source, and never `.ink.json` (house rule; there is none in this flow to
 * begin with).
 */

import type { ExportApi } from "./export.js";
import { compiledStoryBytes } from "./export.js";

/**
 * The injectable seam.
 *
 * `studio` is the compile surface (shared with `export.ts`, so the two
 * export flows cannot diverge on what "the project's bytes" means);
 * `toXliff` is `@brink-lang/web`'s `exportXliff` binding; `saveBytes` is
 * the native save-dialog-and-write round trip, which resolves to the chosen
 * path or `null` if the user cancelled.
 */
export interface ExportXliffApi {
  studio: ExportApi;
  toXliff(storyBytes: Uint8Array, srcLang: string, trgLang?: string): string;
  saveBytes(defaultName: string, bytes: Uint8Array): Promise<string | null>;
}

/** The currently open project, as `exportXliff` needs it: the EFFECTIVE
 * entry file — `ProjectSession.getEntryFile()`'s result (issue #2331
 * `[project] entry` precedence already applied), project-relative, never
 * the raw host `entryFile` fallback `mountStudio` was given (2026-08 review
 * finding). Only the default save-dialog filename is derived from it now;
 * the bytes come from the compile. */
export interface ExportXliffProject {
  entryFile: string;
}

/**
 * The source language the export is tagged with. Matches `brink-cli`'s
 * `--src-lang` default, which is what this flow used to get by not passing
 * the flag — a locale picker is the future work above, not a silent change
 * of default.
 */
export const DEFAULT_SRC_LANG = "en";

/** Derive the default `.xlf` save-dialog filename from an entry file's
 * basename, stripping its `.ink`/`.brink` extension. Pure and exported for
 * direct testing. */
export function defaultXliffName(entryFile: string): string {
  const base = entryFile.split("/").at(-1)?.replace(/\.(ink|brink)$/, "");
  return `${base ?? "story"}.xlf`;
}

/**
 * Compile `project`, render its line tables as XLIFF, and write the result
 * through `api.saveBytes`, reporting either way through the studio's
 * notification sink. A `null` `project` (no project open) logs a warning
 * and returns without touching `api` — mirrors the original inline guard in
 * `main.tsx`.
 *
 * A failed compile is already reported by `compiledStoryBytes` under this
 * flow's own source tag, so it returns silently here rather than notifying
 * twice.
 */
export async function exportXliff(
  project: ExportXliffProject | null,
  api: ExportXliffApi,
): Promise<void> {
  if (project === null) {
    console.warn("[brink-desktop] Export XLIFF: no project open");
    return;
  }

  const bytes = await compiledStoryBytes(api.studio, "export-xliff", "Export XLIFF");
  if (bytes === null) return;

  try {
    const xml = api.toXliff(bytes, DEFAULT_SRC_LANG);
    const outputPath = await api.saveBytes(
      defaultXliffName(project.entryFile),
      new TextEncoder().encode(xml),
    );
    if (outputPath === null) return; // user cancelled
    api.studio.notify({
      severity: "info",
      source: "export-xliff",
      message: `Exported XLIFF to ${outputPath}`,
    });
  } catch (e: unknown) {
    api.studio.notify({
      severity: "error",
      source: "export-xliff",
      message: `Export XLIFF failed: ${e instanceof Error ? e.message : String(e)}`,
    });
  }
}
