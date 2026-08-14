/**
 * File save commands (issue #154, docs/embedder-api.md "File egress").
 *
 * `file.save` (Mod-S) flushes the focused editor document to the wasm
 * session immediately — bypassing both the editor's compile debounce and
 * the egress debounce — and delivers every pending change notification to
 * the host (`onFilesChanged`) right away. `file.saveAll` does the same for
 * every mounted view. With a host save in flight (the overlay contract),
 * both commands re-baseline only the paths confirmed to have actually
 * persisted their current content — a path whose write genuinely diverged
 * mid-flight stays dirty (issue #2426) — so `file.saveAll` re-baselines the
 * verified subset of the pre-save dirty set, not unconditionally every
 * non-mounted file. "Confirmed" is two-tier: a path whose content still
 * matches its pre-save snapshot is trivially fine, and one that doesn't is
 * re-checked against the provider's own disk content (`readProviderFile`)
 * before being called stale — a write queued behind another in-flight one
 * can legitimately pick up a later edit and persist content newer than the
 * pre-save snapshot, and treating that as unsaved was a false-positive
 * "changed while saving" warning on ordinary overlapping autosaves (issue
 * #2435), not a real divergence.
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
        const markSavedAndNotify = (): void => {
          project.markFilesSaved([path]);
          notify({ severity: "info", source: "file", message: `Saved ${path}` });
        };
        if (project.hasHostSave()) {
          const before = project.getFiles()[path];
          void project.save([path]).then(
            async () => {
              // Re-check immediately before re-baselining: an edit landing
              // on `path` while the host write was in flight persisted
              // something other than `before` — marking it clean here
              // would retire a stage the write never wrote (issue #2426;
              // same discipline as `OverlayPersistence.saveDirty`, PR
              // #2420).
              const current = project.getFiles()[path];
              if (current === before) {
                markSavedAndNotify();
                return;
              }
              // `current` diverging from the pre-save snapshot doesn't by
              // itself mean the write raced a genuine mid-flight edit
              // (issue #2435): `requestSave` calls are serialized
              // (`TauriFileProvider`, #2403), so a write queued behind
              // another in-flight one can legitimately pick up this same
              // later edit by the time it actually runs, and persist
              // `current` rather than `before`. Confirm against what the
              // provider actually has on disk now rather than trusting the
              // pre-save snapshot — a genuine mid-write divergence still
              // fails this check, since disk then holds the OLD content
              // the write persisted, not `current`.
              const onDisk = await project.readProviderFile(path).catch(() => undefined);
              if (onDisk === current) {
                markSavedAndNotify();
                return;
              }
              notify({
                severity: "warning",
                source: "file",
                message: `${path} changed while saving — still unsaved`,
              });
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
          markSavedAndNotify();
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
        // `markFilesSaved`: a path that genuinely moved on while the write
        // was in flight was never persisted with its new content and must
        // stay dirty — `markAllSaved` would retire it against unwritten
        // content (issue #2426; same discipline as
        // `OverlayPersistence.saveDirty`, PR #2420). `markAllSaved` itself
        // is only safe on the no-host-save (synchronous) path below, where
        // nothing could have moved on.
        if (project.hasHostSave()) {
          const before = project.getFiles();
          void project.save().then(
            async () => {
              const current = project.getFiles();
              const settled = dirty.filter((path) => current[path] === before[path]);
              const moved = dirty.filter((path) => current[path] !== before[path]);
              // `moved` diverging from its pre-save snapshot doesn't by
              // itself mean each of those writes raced a genuine mid-flight
              // edit (issue #2435): `requestSave` calls are serialized
              // (`TauriFileProvider`, #2403), so a write queued behind
              // another in-flight one can legitimately pick up a later edit
              // and persist `current`, not `before`. Confirm each against
              // the provider's own disk content rather than trusting the
              // pre-save snapshot — a path with a genuine mid-write
              // divergence still fails this check, since disk then holds
              // the OLD content the write persisted, not `current`.
              const confirmed = await Promise.all(
                moved.map(async (path) => {
                  const onDisk = await project.readProviderFile(path).catch(() => undefined);
                  return onDisk === current[path] ? path : null;
                }),
              );
              const saved = [...settled, ...confirmed.filter((p): p is string => p !== null)];
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
