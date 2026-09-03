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
import {
  DEFAULT_FIX_ON_SAVE,
  runFixOnSave,
  type FixOnSaveMode,
  type FixProject,
} from "@brink/studio-ui";

export const FILE_SAVE_COMMAND_ID = "file.save";
export const FILE_SAVE_ALL_COMMAND_ID = "file.saveAll";

export interface FileCommandDeps {
  project: ProjectSession;
  documents: DocumentSessions;
  notify: (n: NotificationInput) => void;
  /**
   * The app-scope fix-on-save ceiling (`docs/autofix-spec.md` §6.2), read
   * fresh per save — a setting changed mid-session must take effect on the
   * next Ctrl-S, not at the next reload. Omitted ⇒ off.
   */
  fixOnSave?: () => FixOnSaveMode;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** Register `file.save` / `file.saveAll`. Returns a disposer. */
export function registerFileCommands(
  commands: CommandRegistry,
  { project, documents, notify, fixOnSave }: FileCommandDeps,
): () => void {
  /**
   * Fix on save (`docs/autofix-spec.md` §7, "run on the save road before the
   * write"), for one file.
   *
   * Runs AFTER the editor's text has been flushed into the session — so the
   * batch sees what the author just typed — and BEFORE the write, so the
   * bytes that reach disk are the fixed ones. Synchronous, and it writes
   * through the project's own `applyEdit` seam, which is what makes the
   * rewritten text part of the very dirty set this save is about to retire.
   *
   * Returns every path the batch actually rewrote (`runFixOnSave`'s own
   * return) — `path` itself when the batch only touched its own diagnostics,
   * plus any other file a cross-file fix also rewrote (issue #3462). A
   * narrowed single-path save must not silently leave those other files
   * dirty and unpersisted, so `file.save` below inspects this return value
   * to decide which save road to take.
   */
  const applyFixOnSave = (path: string): string[] => {
    const mode = fixOnSave?.() ?? DEFAULT_FIX_ON_SAVE;
    if (mode === "off") return [];
    return runFixOnSave(
      {
        project: project as unknown as FixProject,
        applyEdit: (p, source) => project.applyEdit(p, source),
        invalidate: (p) => documents.invalidateFile(p),
      },
      path,
      mode,
    );
  };

  /**
   * The trivial single-path retire: `paths` already matches disk (either it
   * never diverged from `before`, or the divergence was confirmed against
   * the provider's own content) — safe to mark clean right away.
   *
   * Shared by `file.save`'s "settled", disk-confirmed, and no-host-save
   * branches, which is why one `SAVE-PATH` marker below names all three ids
   * (`save-paths.ts`). `paths` is normally just `[focus]`; the no-host-save
   * branch also passes it a fix-on-save touched set so those files retire
   * (and are named) exactly like the host-save cross-file branch does,
   * without needing a second `markFilesSaved` call site.
   *
   * Fix-on-save deliberately raises no toast of its own for `focus` — only
   * `Saved ${focus}`, unchanged from before #3462. Any OTHER file the batch
   * touched gets named in a second, separate notification, since it is not
   * the save the author asked for and would otherwise clear silently.
   */
  const markSavedAndNotify = (paths: string[], focus: string): void => {
    // SAVE-PATH markFilesSaved: file.save, file.save (settled)
    // (checked against src/__tests__/save-paths.ts by
    // src/__tests__/save-path-enrolment.test.ts, issue #2480.) Two ids for
    // three callers of `markSavedAndNotify` — the settled branch, the
    // disk-confirmed branch, and the no-host-save branch — retire
    // through this one call site, so both `file.save` drivers sweep it.
    project.markFilesSaved(paths);
    notify({ severity: "info", source: "file", message: `Saved ${focus}` });
    const others = paths.filter((p) => p !== focus);
    if (others.length > 0) {
      notify({
        severity: "info",
        source: "file",
        message: `Fix on save also wrote ${others.join(", ")}`,
      });
    }
  };

  /**
   * Host-save confirm→retire for exactly `paths` (docs/embedder-api.md
   * "Confirm and retire in ONE synchronous step").
   *
   * This is `file.saveAll`'s own per-path dance, factored out so
   * `file.save`'s cross-file fix-on-save branch (issue #3462) can reuse the
   * identical algorithm and the identical `markFilesSaved` call site rather
   * than growing a second one: the confirm→retire safety already proven for
   * this call site by `save-retire-invariant.test.ts`'s `file.saveAll`
   * driver is a property of these statements, not of which command reached
   * them, so it covers both callers without a second sweep to maintain.
   *
   * `writePaths` is what is actually sent to `project.save(...)` —
   * `undefined` for `file.saveAll`'s truly unnarrowed "everything dirty"
   * write, or an explicit array for `file.save`'s cross-file branch, which
   * must persist exactly the touched set rather than sweeping in some
   * unrelated file the author has open and dirty elsewhere (that is what a
   * real Save All is for, not an implicit side effect of Ctrl-S).
   *
   * `onDone`/`onError` — rather than returning a `Promise` for the caller to
   * `.then` again — so this stays a SINGLE `project.save(...).then(ok, err)`
   * hop, exactly like the inlined code it replaces: an extra `.then` layer
   * here would push `onDone` one microtask later than `markFilesSaved`
   * above it, which is still synchronously correct but was measured to blow
   * the fixed microtask budget `file-egress.test.ts`'s queued-write races
   * (#2435) drive two overlapping saves through.
   */
  const hostSaveBatch = (
    paths: string[],
    writePaths: string[] | undefined,
    onDone: (result: { saved: string[]; stale: number }) => void,
    onError: (e: unknown) => void,
  ): void => {
    const before = project.getFiles();
    void project.save(writePaths).then(async () => {
      const current = project.getFiles();
      const settled = paths.filter((path) => current[path] === before[path]);
      const moved = paths.filter((path) => current[path] !== before[path]);
      // `moved` diverging from its pre-save snapshot doesn't by itself mean
      // each of those writes raced a genuine mid-flight edit (issue #2435):
      // `requestSave` calls are serialized (`TauriFileProvider`, #2403), so
      // a write queued behind another in-flight one can legitimately pick
      // up a later edit and persist `current`, not `before`. Confirm each
      // against the provider's own disk content rather than trusting the
      // pre-save snapshot — a path with a genuine mid-write divergence
      // still fails this check, since disk then holds the OLD content the
      // write persisted, not `current`. `current[path] !== undefined`
      // additionally guards a rejected read from vacuously matching a path
      // also absent from the pre-read snapshot.
      const confirmed = await Promise.all(
        moved.map(async (path) => {
          const onDisk = await project.readProviderFile(path).catch(() => undefined);
          return current[path] !== undefined && onDisk === current[path] ? path : null;
        }),
      );
      // Synchronous mark-time filter: `current` (captured before the
      // disk-confirmation reads above) is only trustworthy for a path that
      // hasn't moved on AGAIN while those reads were in flight — a settled
      // path can drift during that same await just as easily as a moved
      // one. Re-reading right here, one more time, immediately before
      // `markFilesSaved`, catches that window the same way the single-file
      // `file.save` guard does.
      //
      // ⚠ This read and the `markFilesSaved` below are ONE synchronous step
      // — no `await` may be introduced between them (docs/embedder-api.md
      // "Dirty state", "Confirm and retire in ONE synchronous step"; pinned
      // for every save path by src/__tests__/save-retire-invariant.test.ts).
      const atMark = project.getFiles();
      const saved = [
        ...settled,
        ...confirmed.filter((p): p is string => p !== null),
      ].filter((path) => atMark[path] === current[path]);
      // SAVE-PATH markFilesSaved: file.saveAll
      if (saved.length > 0) project.markFilesSaved(saved);
      onDone({ saved, stale: paths.length - saved.length });
    }, onError);
  };

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
        const written = path !== null ? applyFixOnSave(path) : [];
        project.flushFileChanges();
        if (path === null) {
          notify({ severity: "info", source: "file", message: "No editor focused — nothing to save" });
          return;
        }
        // Every OTHER file the fix-on-save batch just rewrote (issue #3462):
        // those are staged and dirty right now exactly like `path` is, and
        // a save narrowed to `[path]` alone would leave them silently
        // dirty. Computed AFTER `applyFixOnSave`/`flushFileChanges` so it
        // reflects what this save is actually about to retire.
        const otherWritten = written.filter((p) => p !== path);
        if (project.hasHostSave()) {
          if (otherWritten.length > 0) {
            // Cross-file fix: route the save through the SAME batch
            // confirm→retire road `file.saveAll` uses (issue #3462),
            // narrowed to exactly the touched set rather than every dirty
            // file — this is Ctrl-S plus what its own fix just wrote, not
            // an implicit Save All.
            const touched = [path, ...otherWritten];
            hostSaveBatch(
              touched,
              touched,
              ({ saved, stale }) => {
                if (saved.includes(path)) {
                  notify({ severity: "info", source: "file", message: `Saved ${path}` });
                }
                const others = saved.filter((p) => p !== path);
                if (others.length > 0) {
                  notify({
                    severity: "info",
                    source: "file",
                    message: `Fix on save also wrote ${others.join(", ")}`,
                  });
                }
                if (stale > 0) {
                  notify({
                    severity: "warning",
                    source: "file",
                    message: `${plural(stale, "file")} changed while saving — still unsaved`,
                  });
                }
              },
              (e: unknown) => {
                notify({
                  severity: "error",
                  source: "file",
                  message: `Save failed for ${path}: ${e instanceof Error ? e.message : String(e)}`,
                });
              },
            );
            return;
          }
          // Host-save branch (the overlay contract, 2026-08-07 D2 ruling): a
          // provider with `requestSave` owns the canonical write. Await it
          // and re-baseline ONLY on success — a rejected write keeps the
          // file dirty for retry instead of silently pretending it saved.
          // Without a host save (the standalone playground) the synchronous
          // flush-and-re-baseline path is byte-identical to before.
          const before = project.getFiles()[path];
          void project.save([path]).then(
            async () => {
              // Re-check immediately before re-baselining: an edit landing
              // on `path` while the host write was in flight persisted
              // something other than `before` — marking it clean here
              // would retire a stage the write never wrote (issue #2426;
              // same discipline as `OverlayPersistence.saveDirty`, PR
              // #2420).
              //
              // ⚠ This read and the `markFilesSaved` it gates (via
              // `markSavedAndNotify`) are ONE synchronous step — no `await`
              // may be introduced between them (docs/embedder-api.md "Dirty
              // state", "Confirm and retire in ONE synchronous step"; pinned
              // for every save path by
              // src/__tests__/save-retire-invariant.test.ts).
              const current = project.getFiles()[path];
              if (current === before) {
                markSavedAndNotify([path], path);
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
              // Re-read the session one more time, right here, rather than
              // reusing the `current` snapshot taken before this await: an
              // edit landing while `readProviderFile` was itself in flight
              // (a real Tauri IPC + disk round trip) would otherwise be
              // confirmed against a disk read that verified an OLDER
              // version, silently re-baselining to content nothing ever
              // confirmed. `onDisk !== undefined` additionally guards a
              // rejected read (e.g. a vanished path) from vacuously
              // matching a path also absent from the session snapshot.
              //
              // ⚠ This read and the `markFilesSaved` it gates are ONE
              // synchronous step — no `await` may be introduced between
              // them (docs/embedder-api.md "Dirty state", "Confirm and
              // retire in ONE synchronous step"; pinned for every save path
              // by src/__tests__/save-retire-invariant.test.ts).
              if (onDisk !== undefined && onDisk === project.getFiles()[path]) {
                markSavedAndNotify([path], path);
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
          // No `await` above this point, so nothing could have moved on —
          // safe to retire the whole touched set (`path` plus anything a
          // cross-file fix wrote) unconditionally, exactly like the
          // single-path no-host-save save always has (issue #3462).
          markSavedAndNotify([path, ...otherWritten], path);
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
        // Fix on save covers Save All too: the same ceiling, run per dirty
        // file before the batch write. Read the dirty set AFTER, so a file
        // the fixes rewrote is in the set this save retires — running it on
        // an already-captured list would leave the rewrite unsaved.
        for (const path of project.dirtyPaths()) applyFixOnSave(path);
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
          hostSaveBatch(
            dirty,
            undefined,
            ({ saved, stale }) => {
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
          // Synchronous no-host-save path — safe to call `markAllSaved`
          // unconditionally (see its ⚠ comment): nothing awaited above it,
          // so nothing could have moved on since `dirty` was captured.
          // SAVE-PATH-EXEMPT markAllSaved: no `await` sits between
          // `dirtyPaths()`/`getFiles()` above and this call, so the
          // confirm→retire race the sweep guards against cannot occur here
          // (checked by src/__tests__/save-path-enrolment.test.ts, #2480).
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
