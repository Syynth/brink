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
        if (path === null) {
          notify({ severity: "info", source: "file", message: "No editor focused — nothing to save" });
          return;
        }
        // Host-save branch (the overlay contract, 2026-08-07 D2 ruling): a
        // provider with `requestSave` owns the canonical write. Await it
        // and re-baseline ONLY on success — a rejected write keeps the file
        // dirty for retry instead of silently pretending it saved. Without
        // a host save (the standalone playground) the synchronous
        // flush-and-re-baseline path is byte-identical to before.
        if (project.hasHostSave()) {
          const before = project.getFiles()[path];
          void project.save([path]).then(
            () => {
              // Re-check immediately before re-baselining: an edit landing
              // on `path` while the host write was in flight persisted
              // `before`, not whatever is current now — marking it clean
              // here would retire a stage the write never wrote (issue
              // #2426; same discipline as `OverlayPersistence.saveDirty`,
              // PR #2420). Leave it dirty for the next save instead.
              const current = project.getFiles()[path];
              if (current !== before) {
                notify({
                  severity: "warning",
                  source: "file",
                  message: `${path} changed while saving — still unsaved`,
                });
                return;
              }
              project.markFilesSaved([path]);
              notify({ severity: "info", source: "file", message: `Saved ${path}` });
            },
            (e: unknown) => {
              notify({
                severity: "error",
                source: "file",
                message: `Save failed for ${path}: ${e instanceof Error ? e.message : String(e)}`,
              });
            },
          );
        } else {
          project.markFilesSaved([path]);
          notify({ severity: "info", source: "file", message: `Saved ${path}` });
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
        const report = (savedCount: number): void =>
          notify({
            severity: "info",
            source: "file",
            message:
              savedCount === 0 ? "No unsaved changes" : `Saved ${plural(savedCount, "file")}`,
          });
        // Same host-save branch as `file.save` — see the comment there. The
        // write covers the whole batch at once, so re-baselining must
        // re-check EACH path individually immediately before
        // `markFilesSaved`: a path that moved on while the write was in
        // flight was never persisted with its new content and must stay
        // dirty — `markAllSaved` would retire it against unwritten content
        // (issue #2426; same discipline as `OverlayPersistence.saveDirty`,
        // PR #2420). `markAllSaved` itself is only safe on the no-host-save
        // (synchronous) path below, where nothing could have moved on.
        if (project.hasHostSave()) {
          const before = project.getFiles();
          void project.save().then(
            () => {
              const current = project.getFiles();
              const saved = dirty.filter((path) => current[path] === before[path]);
              const stale = dirty.length - saved.length;
              if (saved.length > 0) project.markFilesSaved(saved);
              if (stale > 0) {
                notify({
                  severity: "warning",
                  source: "file",
                  message: `${plural(stale, "file")} changed while saving — still unsaved`,
                });
              }
              // Skip the redundant "No unsaved changes" notice when the
              // warning above already explains why nothing was saved.
              if (saved.length > 0 || stale === 0) report(saved.length);
            },
            (e: unknown) => {
              notify({
                severity: "error",
                source: "file",
                message: `Save failed: ${e instanceof Error ? e.message : String(e)}`,
              });
            },
          );
        } else {
          project.markAllSaved();
          report(dirty.length);
        }
      },
    }),
  ];

  return () => {
    for (const dispose of disposers) dispose();
  };
}
