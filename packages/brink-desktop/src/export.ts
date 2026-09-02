/**
 * Export Story (.inkb) (D3 slice 1, docs/desktop-shell-spec.md, #2391).
 *
 * Extracted out of `main.tsx` for testability — like `quit.ts`'s
 * `awaitSaveAllBeforeQuit`, `main.tsx` itself has side effects at module
 * load (it renders the landing screen and wires `listen()` calls as soon as
 * it's imported), so the exported-logic-only unit lives here instead.
 *
 * Reuses the *existing* compile surface — `dispatch("compile.run")` is the
 * same command the palette runs, so export can never diverge from the
 * compile road. `ProjectSession.compileProjectAsync()` is generation-cached,
 * so this costs nothing extra when nothing has changed since the last
 * compile. Since the worker architecture (W4+), `compile.run` lands its
 * result ASYNCHRONOUSLY — `exportStoryToInkb` awaits the landing (a
 * diagnostics/bytes store update after the dispatch) before reading
 * `api.getStoryBytes()`, bounded by a timeout that surfaces as an export
 * error rather than a hang.
 */

import type { StudioApi } from "@brink-lang/studio";

/** The `StudioApi` slice the export flow needs — narrowed for testability. */
export type ExportApi = Pick<StudioApi, "dispatch" | "getStoryBytes" | "select" | "notify">;

/** Default export filename: `<project-folder>.inkb` (per the issue's shape). */
export function defaultExportName(root: string): string {
  const folderName = root.split("/").at(-1) ?? root;
  return `${folderName}.inkb`;
}

/**
 * Compile the open project and hand the resulting bytes to `saveDialog`. A
 * failed compile surfaces as an error notification through the studio
 * surface (the backup-ring-failure precedent in `main.tsx`) — never a
 * silent no-op — and never reaches the dialog.
 */
export async function exportStoryToInkb(
  api: ExportApi,
  root: string,
  saveDialog: (defaultName: string, bytes: Uint8Array) => Promise<string | null>,
): Promise<void> {
  const before = api.select((s) => s.diagnostics);
  const bytesBefore = api.getStoryBytes();
  api.dispatch("compile.run");
  // The compile lands asynchronously (worker road, W4): wait for the store
  // to reflect THIS compile — either the diagnostics object identity moves
  // (landCompileResult always replaces it) or the bytes do.
  const deadline = Date.now() + 15_000;
  for (;;) {
    if (api.select((s) => s.diagnostics) !== before) break;
    if (api.getStoryBytes() !== bytesBefore) break;
    if (Date.now() > deadline) {
      api.notify({
        severity: "error",
        source: "export",
        message: "Export failed: the compile did not finish in time.",
      });
      return;
    }
    await new Promise((r) => setTimeout(r, 25));
  }
  const bytes = api.getStoryBytes();
  if (bytes === null) {
    const { errors } = api.select((s) => s.diagnostics);
    api.notify({
      severity: "error",
      source: "export",
      message: `Export failed: ${errors} compile error(s) — fix them and try again.`,
    });
    return;
  }

  try {
    await saveDialog(defaultExportName(root), bytes);
    // The RESOLVED dialogue dialect rides beside the story (#3393, RULED
    // 2026-08-30): an engine reads `<name>.dialect.json` + @brink-lang/dialect
    // to apply the project's conventions the way the studio does. Only
    // when the project declares one — no dialect, no file.
    const dialect = api.select((s) => s.projectDialect);
    if (dialect) {
      await saveDialog(
        defaultExportName(root).replace(/\.inkb$/, ".dialect.json"),
        new TextEncoder().encode(JSON.stringify(dialect, null, 2)),
      );
    }
  } catch (e: unknown) {
    api.notify({
      severity: "error",
      source: "export",
      message: `Export failed: ${e instanceof Error ? e.message : String(e)}`,
    });
  }
}
