/**
 * File > Export XLIFF… (D3, #2392) — proves the `brink-cli` sidecar path
 * end to end. Extracted out of `main.tsx` (2026-08 review finding: logic
 * living directly in `main.tsx` cannot be unit-tested — `quit.ts` +
 * `QuitSaveApi` exist for exactly this reason) behind an injectable
 * `{ runCli, save, notify }` seam.
 *
 * Deliberately minimal: export the currently open project's entry file at
 * the source language, prompting only for where to save the `.xlf`. The
 * fuller intl UI (locale picker, compile-locale/regenerate batch ops,
 * progress from the streamed `cli:output` events) is future work — this
 * item exists to exercise one real path, not to be it.
 *
 * The input handed to the sidecar is always the entry file's `.ink`/
 * `.brink` source (never `.ink.json` — house rule, and there is no
 * `.ink.json` anywhere in this flow to begin with).
 */

import type { CliInvocation } from "./cli.js";

/** One save-dialog filter entry, matching `@tauri-apps/plugin-dialog`'s shape. */
export interface SaveDialogFilter {
  name: string;
  extensions: string[];
}

/** The slice of `@tauri-apps/plugin-dialog`'s `save()` this seam needs. */
export type SaveFn = (options: {
  defaultPath: string;
  filters: SaveDialogFilter[];
}) => Promise<string | null>;

/** One studio notification, matching `StudioApi.notify`'s entry shape. */
export interface ExportXliffNotification {
  severity: "info" | "error";
  source: string;
  message: string;
}

/** The injectable seam: `runCli`/`save` are the two IPC calls this flow
 * makes, `notify` is the studio's notification sink. Kept minimal and
 * structural, mirroring `quit.ts`'s `QuitSaveApi`. */
export interface ExportXliffApi {
  runCli(invocation: CliInvocation): Promise<number>;
  save: SaveFn;
  notify(entry: ExportXliffNotification): void;
}

/** The currently open project, as `exportXliff` needs it: an absolute root
 * and the EFFECTIVE entry file — `ProjectSession.getEntryFile()`'s result
 * (issue #2331 `[project] entry` precedence already applied), project-
 * relative, never the raw host `entryFile` fallback `mountStudio` was
 * given (2026-08 review finding: using the host fallback here exports a
 * different story than the editor compiles, for any project whose
 * `brink.toml` names an entry outside `ENTRY_FALLBACKS`). */
export interface ExportXliffProject {
  root: string;
  entryFile: string;
}

/** Derive the default `.xlf` save-dialog filename from an entry file's
 * basename, stripping its `.ink`/`.brink` extension. Pure and exported for
 * direct testing. */
export function defaultXliffName(entryFile: string): string {
  const base = entryFile.split("/").at(-1)?.replace(/\.(ink|brink)$/, "");
  return `${base ?? "story"}.xlf`;
}

/**
 * Export `project`'s entry file to a user-chosen `.xlf` via `api.save`,
 * then run the `export-xliff` sidecar subcommand and report success/failure
 * through `api.notify`. A `null` `project` (no project open) logs a warning
 * and returns without touching `api` — mirrors the original inline guard in
 * `main.tsx`.
 */
export async function exportXliff(
  project: ExportXliffProject | null,
  api: ExportXliffApi,
): Promise<void> {
  if (project === null) {
    console.warn("[brink-desktop] Export XLIFF: no project open");
    return;
  }

  const defaultName = defaultXliffName(project.entryFile);
  const outputPath = await api.save({
    defaultPath: defaultName,
    filters: [{ name: "XLIFF", extensions: ["xlf"] }],
  });
  if (outputPath === null) return; // user cancelled

  try {
    const exitCode = await api.runCli({
      root: project.root,
      rel: project.entryFile,
      subcommand: "export-xliff",
      rest: ["--output", outputPath],
    });
    if (exitCode === 0) {
      api.notify({ severity: "info", source: "cli", message: `Exported XLIFF to ${outputPath}` });
    } else {
      api.notify({
        severity: "error",
        source: "cli",
        message: `export-xliff exited with code ${exitCode}`,
      });
    }
  } catch (e: unknown) {
    api.notify({
      severity: "error",
      source: "cli",
      message: `export-xliff failed: ${e instanceof Error ? e.message : String(e)}`,
    });
  }
}
