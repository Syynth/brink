/**
 * Export Story (.inkb) (D3 slice 1, docs/desktop-shell-spec.md, #2391).
 *
 * Extracted out of `main.tsx` for testability — like `quit.ts`'s
 * `awaitSaveAllBeforeQuit`, `main.tsx` itself has side effects at module
 * load (it renders the landing screen and wires `listen()` calls as soon as
 * it's imported), so the exported-logic-only unit lives here instead.
 *
 * Reuses the *existing* compile surface — `dispatch("compile.run")` is the
 * same command the Player's Run button dispatches
 * (`registerOpenPlayerCommand` / `PlayerPane`) — rather than adding a second
 * compile road. `ProjectSession.compileProject()` is generation-cached, so
 * this costs nothing extra when nothing has changed since the last compile.
 * `compile.run` runs synchronously all the way down to the store
 * (`triggerCompile` → `deliverCompile` → `setCompileResult`), so
 * `api.getStoryBytes()` reflects THIS compile's outcome the instant
 * `dispatch` returns — no race with a debounced/async compile.
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
  api.dispatch("compile.run");
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
  } catch (e: unknown) {
    api.notify({
      severity: "error",
      source: "export",
      message: `Export failed: ${e instanceof Error ? e.message : String(e)}`,
    });
  }
}
