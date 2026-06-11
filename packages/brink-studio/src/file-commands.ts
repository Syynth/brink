/**
 * File save commands (issue #154, docs/embedder-api.md "File egress").
 *
 * `file.save` (Mod-S) flushes the focused editor document to the wasm
 * session immediately — bypassing both the editor's compile debounce and
 * the egress debounce — and delivers every pending change notification to
 * the host (`onFilesChanged`) right away. `file.saveAll` does the same for
 * every mounted view and re-baselines the whole project.
 *
 * Both commands work without a host hook (the standalone playground):
 * the internal flush still happens, dirty state clears, and an info
 * notification confirms the save — they never error.
 *
 * Registered at the app boundary (mount.tsx); extracted here so the flush /
 * notify behavior is unit-testable without the bootstrap.
 */

import type { CommandRegistry, NotificationInput } from "@brink/studio-shell";
import type { DocumentSessions, ProjectSession } from "@brink/studio-store";

export const FILE_SAVE_COMMAND_ID = "file.save";
export const FILE_SAVE_ALL_COMMAND_ID = "file.saveAll";

export interface FileCommandDeps {
  project: ProjectSession;
  documents: DocumentSessions;
  notify: (n: NotificationInput) => void;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** Register `file.save` / `file.saveAll`. Returns a disposer. */
export function registerFileCommands(
  commands: CommandRegistry,
  { project, documents, notify }: FileCommandDeps,
): () => void {
  const disposers = [
    commands.register({
      id: FILE_SAVE_COMMAND_ID,
      title: "File: Save",
      keybinding: "Mod-S",
      run: () => {
        // Push the focused view's text to the session now. With no focused
        // editor (player/tool-window focus) there is nothing doc-shaped to
        // save — still flush pending egress so Mod-S always syncs the host.
        const path = documents.flushFocused();
        project.flushFileChanges();
        if (path !== null) {
          project.markFilesSaved([path]);
          notify({ severity: "info", source: "file", message: `Saved ${path}` });
        } else {
          notify({ severity: "info", source: "file", message: "No editor focused — nothing to save" });
        }
      },
    }),

    commands.register({
      id: FILE_SAVE_ALL_COMMAND_ID,
      title: "File: Save All",
      run: () => {
        // Push every mounted view first, so the dirty set (and the egress
        // batch) reflects the very latest editor text.
        documents.flushAll();
        const dirty = project.dirtyPaths();
        project.flushFileChanges();
        project.markAllSaved();
        notify({
          severity: "info",
          source: "file",
          message:
            dirty.length === 0 ? "No unsaved changes" : `Saved ${plural(dirty.length, "file")}`,
        });
      },
    }),
  ];

  return () => {
    for (const dispose of disposers) dispose();
  };
}
