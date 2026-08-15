/**
 * Desktop shell entry (docs/desktop-shell-spec.md, D1).
 *
 * The shell is deliberately thin: pick a folder, build a
 * `TauriFileProvider`, hand everything to `mountStudio` — the same
 * embedder API the playground and any external host use. Everything below
 * this file is the editor packages' business.
 *
 * Project lifecycle (Open/Close) is driven from the native menu via shell
 * events (`menu:open-project` / `menu:close-project`, forwarded by
 * src-tauri) and from the landing screen's button — both funnel into the
 * same `chooseAndOpen` / `closeProject` pair, which own the single
 * `StudioHandle`.
 *
 * App quit (#2370) is separate from Close Project: the window's
 * `onCloseRequested` hook and the app-menu Quit item (`menu:quit`, routed
 * the same way as open/close-project rather than through
 * `PredefinedMenuItem::quit`) both funnel into `handleQuitRequested`, which
 * clears the autosave ticker then awaits the final `saveAll` (capped)
 * before the window is destroyed — the third teardown path alongside
 * `closeProject`'s explicit-close and reopen-via-`openProject` clears
 * (#2517; see `autosaveTimer`'s doc comment).
 *
 * File associations (D3, #2393) are a third way a project opens, bundled
 * `.app` only: double-clicking a `.ink`/`.brink` file reaches `handleFileOpen`
 * via `shell:file-open` events / the `take_pending_opens` boot pull, and
 * routes through this same `openProject`/`closeProject` pair — see
 * `file-open.ts` for the (unit-tested) focus-vs-reopen decision.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save } from "@tauri-apps/plugin-dialog";
import { mountStudio, type StudioHandle } from "@brink-lang/studio";
import type { FileChange } from "@brink-lang/editor";
import {
  TauriFileProvider,
  pickProjectFolder,
  projectRootExists,
  pruneRecent,
  pushRecent,
  readRecents,
  saveBytesDialog,
} from "./tauri-provider.js";
import { awaitSaveAllBeforeQuit } from "./quit.js";
import { runCli } from "./cli.js";
import { exportStoryToInkb } from "./export.js";
import { exportXliff, type ExportXliffApi } from "./export-xliff.js";
import { resolveFileOpenAction } from "./file-open.js";

/** Loadable project extensions; keep in sync with `list_files` in src-tauri. */
const ENTRY_FALLBACKS = ["story.brink", "main.ink", "main.brink", "story.ink"];

/**
 * Autosave cadence (2026-08-07 ruling, `docs/desktop-shell-spec.md`'s
 * autosave row): autosave IS `saveAll` (celeris §10.1.1) on a 2-minute
 * ticker whenever dirty files exist; clean ticks are no-ops inside the
 * command. Exported (module-level, not a function-local) so
 * `__tests__/autosave-reopen.test.ts` can import — rather than
 * restate — the exact value it pins (#2517); a Settings surface can make
 * this configurable later, but until then this is the single source of
 * truth `openProject`'s `setInterval` call reads.
 */
export const AUTOSAVE_MS = 120_000;

/** The one open project. D1 is single-window, single-project by ruling. */
let current: StudioHandle | null = null;
/** The open project's root path (absolute), or null when none is open —
 * the default Export filename derives from its final component, the D3
 * file-association handler's "is this file inside it?" test, and (with
 * `currentEntryFile`) the project-relative key `exportXliff` hands the
 * sidecar via the shell's own `resolve()` guard rather than an absolute
 * path built by hand here. Cleared on close, alongside `current`. */
let currentRoot: string | null = null;
/** The autosave ticker for the open project; cleared on close, on reopen
 * (via `closeProject`, called at the top of `openProject`), and on quit
 * (`handleQuitRequested`, #2517) — never left armed past the `StudioHandle`
 * it closes over. */
let autosaveTimer: ReturnType<typeof setInterval> | null = null;
/**
 * The open project's EFFECTIVE entry file — `StudioHandle.entryFile`,
 * i.e. `ProjectSession.getEntryFile()`'s result with `[project] entry`
 * precedence already applied (issue #2331), never the raw host fallback
 * `resolveEntryFile` computed before `mountStudio` ran (2026-08 review
 * finding: using the host fallback here would export a different story
 * than the editor compiles for any project whose `brink.toml` names an
 * entry outside `ENTRY_FALLBACKS`). Set once `mountStudio` resolves; see
 * `openProject`.
 */
let currentEntryFile: string | null = null;

/**
 * Resolve the tab to open first, for a configless project (no `brink.toml`,
 * or one that doesn't set `[project] entry`). `ProjectSession` now owns
 * `[project] entry` precedence itself (issue #2331, ruled 2026-08-07
 * "`[project] entry` beats `mountStudio`'s `entryFile`") — it discovers
 * `brink.toml` and supersedes whatever `entryFile` `mountStudio` was given
 * the moment a valid `entry` is found, so this shell no longer needs to
 * peek at the TOML itself to guess which tab wins: it just picks a
 * reasonable fallback and lets `ProjectSession` override it. The regex peek
 * this function used to do that job is DELETED, not merely unused — the
 * schema slot `brink-project-config` now carries makes it redundant, and a
 * dumb regex duplicating real TOML parsing was always the second-best way
 * to answer this question.
 */
function resolveEntryFile(files: Record<string, string>): string {
  for (const candidate of ENTRY_FALLBACKS) {
    if (files[candidate] !== undefined) return candidate;
  }
  const sources = Object.keys(files)
    .filter((p) => p.endsWith(".brink") || p.endsWith(".ink"))
    .sort();
  return sources[0] ?? Object.keys(files).sort()[0] ?? "main.ink";
}

function appRoot(): HTMLElement {
  const el = document.getElementById("app");
  if (!el) throw new Error("Missing #app container");
  return el;
}

/**
 * Render the landing screen, including the recent-projects list under the
 * Open button (#2394). Paths are inserted via `textContent`, never HTML
 * interpolation — filesystem paths are not attacker input in this shell,
 * but a folder name is still untrusted-enough text that it shouldn't be
 * concatenated into `innerHTML`.
 */
async function renderLanding(): Promise<void> {
  const root = appRoot();
  root.innerHTML = `
    <div id="landing">
      <button id="open-project">Open Project Folder…</button>
      <div class="hint">A folder containing .brink / .ink files (and optionally a brink.toml) — or ⌘O</div>
      <ul id="recent-projects" class="recent-projects"></ul>
    </div>`;
  const button = document.getElementById("open-project");
  button?.addEventListener("click", () => void chooseAndOpen());

  const list = document.getElementById("recent-projects");
  if (list === null) return;
  const recents = await readRecents().catch((e: unknown) => {
    console.error("[brink-desktop] read_recents failed", e);
    return [];
  });
  for (const path of recents) {
    const item = document.createElement("li");
    const entry = document.createElement("button");
    entry.className = "recent-project";
    entry.textContent = path;
    entry.title = path;
    entry.addEventListener("click", () => void openRecent(path));
    item.appendChild(entry);
    list.appendChild(item);
  }
}

/**
 * Exported (only) so `__tests__/autosave-reopen.test.ts` can drive the real
 * open/close pair directly (#2486) — every in-app caller still reaches this
 * through the same `chooseAndOpen` / menu-event / `handleFileOpen` wiring as
 * before; the export adds no new call site.
 */
export async function openProject(root: string): Promise<void> {
  const provider = new TauriFileProvider(root);
  const paths = await provider.listFiles();
  const files: Record<string, string> = {};
  for (const path of paths) {
    files[path] = await provider.readFile(path);
  }

  // Tear down any previous project only after the new one's files loaded,
  // so a cancelled or failed open never leaves a blank window.
  closeProject();
  currentRoot = root;

  const el = appRoot();
  el.replaceChildren();

  const folderName = root.split("/").at(-1) ?? root;
  document.title = `${folderName} — Brink Studio`;

  // A configless-project fallback ONLY — `ProjectSession.initialize()` may
  // supersede this with a `brink.toml`-named entry the instant mountStudio
  // resolves (issue #2331). `currentEntryFile` is set from the resolved
  // `StudioHandle.entryFile` below, never from this local, so callers like
  // `exportXliff` always see the effective entry.
  const entryFile = resolveEntryFile(files);
  currentRoot = root;

  current = await mountStudio(el, {
    files,
    provider,
    entryFile,
    // The overlay contract (D2, 2026-08-07 ruling): egress delivery is NOT
    // persistence — dirty means "diverges from the last canonical save".
    // Canonical writes happen through provider.requestSave, awaited by the
    // save commands, which only re-baseline on success.
    egressPersists: false,
    // The #154 egress feeds the BACKUP RING — crash protection at ~500 ms
    // granularity, bounded (25 entries / 10 MB) in the shell, orthogonal
    // to dirty. Backups never clear dirty; ⌘S does.
    onFilesChanged: (changes: FileChange[]) => {
      void provider.ringBackups(changes).catch((e: unknown) => {
        // Ring failures must never block editing — but after two silent
        // -failure hunts (the unregistered command; the unwired hook),
        // they must be VISIBLE: route through the studio's notification
        // surface once the handle exists. The ref is assigned right after
        // mountStudio resolves; a failure in the first ~500 ms window
        // falls back to the console.
        const api = current?.api;
        if (api) {
          api.notify({
            severity: "error",
            source: "backup",
            message: `Backup ring append failed: ${e instanceof Error ? e.message : String(e)}`,
          });
        } else {
          console.error("[brink-desktop] backup ring append failed", e);
        }
      });
    },
  });
  // The EFFECTIVE entry (issue #2331 precedence already applied), not the
  // host fallback computed above — see `currentEntryFile`'s doc comment.
  currentEntryFile = current.entryFile;

  // Autosave IS saveAll (celeris §10.1.1): one save path, one artifact
  // class. Clean ticks are no-ops inside the command. See `AUTOSAVE_MS`'s
  // doc comment for the ruling and why it's a module-level export.
  const api = current.api;
  autosaveTimer = setInterval(() => {
    if (api.getDirtyFiles().length > 0) api.dispatch("file.saveAll");
  }, AUTOSAVE_MS);

  // Record the successful open (#2394). Fire-and-forget: a recents-store
  // hiccup must never block the project the user just opened. The shell
  // also keeps the native Open Recent submenu in sync as part of this
  // command (`rebuild_menu` in src-tauri).
  void pushRecent(root).catch((e: unknown) => {
    console.error("[brink-desktop] push_recent failed", e);
  });
}

async function chooseAndOpen(): Promise<void> {
  const root = await pickProjectFolder();
  if (root === null) return; // user cancelled
  try {
    await openProject(root);
  } catch (e: unknown) {
    console.error("[brink-desktop] failed to open project", e);
    if (current === null) {
      currentRoot = null;
      void renderLanding();
    }
  }
}

/**
 * Open a project chosen from the recents list (native Open Recent submenu
 * or the landing-screen list). Unlike {@link chooseAndOpen}, a failure here
 * lazily prunes the stale entry (#2394's "do not crash the open flow;
 * prune lazily") — the path came from a persisted list, not a fresh folder
 * pick, so a missing/moved folder is an expected failure mode, not a rare
 * edge case.
 *
 * Pruning is gated on {@link projectRootExists} (2026-08 review finding),
 * not fired on every rejection: `openProject` can also reject from a
 * permission error, a file deleted mid-listing, or a `mountStudio` failure
 * — none of which mean the project itself is gone. Only an actually-missing
 * root gets removed from `recents.json` and the native Open Recent submenu;
 * every other failure just surfaces to the console and re-shows the
 * landing screen with the entry intact.
 */
async function openRecent(path: string): Promise<void> {
  try {
    await openProject(path);
  } catch (e: unknown) {
    console.error("[brink-desktop] failed to open recent project", e);
    // Default to "exists" on a failed check itself, so a transient
    // check-command error can never masquerade as evidence of deletion.
    const rootGone = !(await projectRootExists(path).catch(() => true));
    if (rootGone) {
      await pruneRecent(path).catch(() => {});
    }
    if (current === null) void renderLanding();
  }
}

/**
 * Unmount the current project and restore the landing screen. Before the
 * unmount, a best-effort canonical save of anything dirty (the unmount's
 * own egress flush only feeds the RING under the overlay contract — the
 * ring protects the work, but close should leave canonical files current
 * too). Ruled 2026-08-07 (docs/decision-log.md, "Desktop close: no dirty
 * prompt; quit awaits the final save"): no dirty-state close confirmation
 * prompt — dead UI, not implemented.
 *
 * Clearing `autosaveTimer` here — and `openProject` always calling this
 * before arming a fresh one — is the entire reason a reopened project never
 * ends up with two autosave tickers running at once. That "no-duplicate-
 * interval" property is pinned by `__tests__/autosave-reopen.test.ts`
 * (#2486): dropping or reordering the clear below is a data-loss-adjacent
 * regression, not a cosmetic one (docs/desktop-shell-spec.md, autosave row).
 * Exported (only) so that test can call it directly — see `openProject`'s
 * export comment.
 */
export function closeProject(): void {
  if (current === null) return;
  if (autosaveTimer !== null) {
    clearInterval(autosaveTimer);
    autosaveTimer = null;
  }
  if (current.api.getDirtyFiles().length > 0) {
    current.api.dispatch("file.saveAll");
  }
  current.unmount();
  current = null;
  currentRoot = null;
  currentEntryFile = null;
  document.title = "Brink Studio";
  void renderLanding();
}

/**
 * File > Export XLIFF… (D3, #2392) — proves the `brink-cli` sidecar path
 * end to end. The export logic itself lives in `export-xliff.ts` (2026-08
 * review finding: logic living directly in `main.tsx` cannot be unit-tested
 * — `quit.ts` + `QuitSaveApi` exist for exactly this reason); this wrapper's
 * only job is gathering the currently-open project (root + EFFECTIVE entry,
 * `currentEntryFile`, never the host fallback) and the studio's notify sink,
 * then handing them to the extracted, unit-tested function.
 */
async function handleExportXliff(): Promise<void> {
  const api = current?.api;
  if (currentRoot === null || currentEntryFile === null || api === undefined) {
    console.warn("[brink-desktop] Export XLIFF: no project open");
    return;
  }
  const exportApi: ExportXliffApi = {
    runCli: (invocation) => runCli(invocation),
    save,
    notify: (entry) => api.notify(entry),
  };
  await exportXliff({ root: currentRoot, entryFile: currentEntryFile }, exportApi);
}

/**
 * Export Story (.inkb) (D3 slice 1, #2391). The actual compile-then-save
 * logic lives in `export.ts` (extracted for testability, like `quit.ts`);
 * this wrapper just supplies the live `StudioApi`, the open project's root,
 * and the real save dialog.
 */
async function handleExportInkb(): Promise<void> {
  if (current === null || currentRoot === null) return;
  await exportStoryToInkb(current.api, currentRoot, saveBytesDialog);
}

/**
 * App quit (#2370, docs/decision-log.md 2026-08-07 "Desktop close: no dirty
 * prompt; quit awaits the final save"): NO confirmation prompt — explicitly
 * ruled out. The one safety piece is that the window must not actually
 * close until the final `saveAll` has had its chance to land, so quitting
 * can never race the in-flight canonical write. `awaitSaveAllBeforeQuit` is
 * capped (~3s) — a hung write cannot make the app unquittable, since the
 * backup ring (#154) already bounds the loss.
 *
 * `autosaveTimer` is cleared here too, BEFORE the `await` below (#2517):
 * this is `closeProject`'s structurally parallel sibling for teardown, and
 * leaving the ticker armed through the (up to ~3s) save-wait means an
 * autosave tick landing in that window would fire `dispatch("file.saveAll")`
 * against a project mid-quit — redundant at best alongside
 * `awaitSaveAllBeforeQuit`'s own dispatch/redispatch, and a needless extra
 * write path racing the teardown at worst. Disarming it up front removes
 * that window entirely rather than relying on the 120s cadence being long
 * enough in practice. Exported (only) so
 * `__tests__/autosave-quit.test.ts` can drive it directly, same as
 * `openProject`/`closeProject` for `__tests__/autosave-reopen.test.ts`.
 */
export async function handleQuitRequested(): Promise<void> {
  if (current !== null) {
    if (autosaveTimer !== null) {
      clearInterval(autosaveTimer);
      autosaveTimer = null;
    }
    await awaitSaveAllBeforeQuit(current.api);
  }
  await getCurrentWindow().destroy();
}

/**
 * Focus an already-open project file through the studio's own navigation
 * protocol (`editor.reveal`, docs/studio-shell-spec.md §6.1) — the same
 * command Problems/Search/Story Graph dispatch, reused rather than forked
 * (the span is a no-op placeholder; a source Location just needs a valid
 * span shape to resolve, and reveal-at-file-open has no meaningful offset).
 */
function focusFile(rel: string): void {
  current?.api.dispatch("editor.reveal", {
    kind: "source",
    file: rel,
    span: { start: 0, end: 0 },
  });
}

/**
 * Handle one OS file-open request (D3, #2393): `resolveFileOpenAction`
 * (file-open.ts) is the pure decision — focus in place, or open a new
 * project root. Opening reuses `openProject`, which already close-saves
 * any previously open project before mounting the new one (the ruled
 * close flow) — this never forks a second open path.
 *
 * Mirrors {@link chooseAndOpen}'s failure handling (2026-08 review finding):
 * `openProject` can realistically reject (an unreadable subdirectory, a
 * non-UTF-8 file, a `mountStudio` throw). Without a catch here, a failed
 * open left `current === null` with the landing screen never re-rendered
 * (blank window) and `currentRoot` already pointing at the failed root, so
 * every later double-click under that root would resolve to a dead
 * `focus` action against a null `current`. On failure, fall back to the
 * landing screen exactly like `chooseAndOpen` and skip the focus dispatch
 * — there is nothing mounted to focus into.
 */
async function handleFileOpen(path: string): Promise<void> {
  const action = resolveFileOpenAction(path, currentRoot);
  if (action.kind === "open") {
    try {
      await openProject(action.root);
    } catch (e: unknown) {
      console.error("[brink-desktop] failed to open project from file-open", e);
      if (current === null) {
        currentRoot = null;
        void renderLanding();
      }
      return;
    }
  }
  focusFile(action.rel);
}

/**
 * File associations (docs/desktop-shell-spec.md D3; #2393) ONLY bite in the
 * bundled `.app` — `bundle.fileAssociations` registers `.ink`/`.brink` with
 * the OS, and a dev run (`pnpm tauri dev`) never receives an OS file-open
 * launch, so nothing below ever fires there. On a bundled build,
 * double-clicking (or Dock-dropping) an associated file reaches
 * `RunEvent::Opened` in src-tauri/src/lib.rs, which forwards paths here as
 * live `shell:file-open` events. `take_pending_opens` is a one-time pull at
 * boot for paths that arrived before this listener was attached (a cold
 * launch — the Rust side buffers until this exact call, then switches to
 * live emits; see `PendingOpens`'s doc comment there).
 *
 * The pull is sequenced strictly after the listener is registered
 * (2026-08 review finding): `listen()` is itself an async IPC round-trip
 * (`invoke('plugin:event|listen', …)` under the hood), so firing
 * `take_pending_opens` at module top level without awaiting `listen()`
 * first raced the two — `take_pending_opens` flips the Rust side's
 * `PendingOpens` state to "ready" (live-emit) the instant it runs, which
 * could land before the listener actually existed, silently dropping an
 * `Opened` delivered in that exact window (Tauri's JS event bus has no
 * replay). Chaining `.then()` off `listen()`'s own resolution closes that
 * gap.
 */
void listen<string[]>("shell:file-open", (event) => {
  void (async () => {
    for (const path of event.payload) {
      await handleFileOpen(path);
    }
  })();
})
  .then(() => invoke<string[]>("take_pending_opens"))
  .then(async (paths) => {
    for (const path of paths) {
      await handleFileOpen(path);
    }
  })
  .catch((e: unknown) => {
    console.error("[brink-desktop] file-open wiring failed", e);
  });

void listen("menu:open-project", () => void chooseAndOpen());
void listen("menu:close-project", () => closeProject());
void listen("menu:export-inkb", () => void handleExportInkb());
void listen("menu:export-xliff", () => void handleExportXliff());
// File → Open Recent (#2394): src-tauri emits the chosen path as the event
// payload (see `on_menu_event` in src-tauri/src/lib.rs).
void listen<string>("menu:open-recent", (event) => void openRecent(event.payload));
// The app-menu Quit item (⌘Q) is routed here as a plain shell event — like
// open/close-project — instead of `PredefinedMenuItem::quit`, specifically
// so it funnels through the same guarded path as the window close below
// rather than the OS quit item's own (unguarded) native teardown.
void listen("menu:quit", () => void handleQuitRequested());

void getCurrentWindow().onCloseRequested(async (event) => {
  // Always prevent the native close first. (`@tauri-apps/api`'s own default
  // — skip `preventDefault()` and it auto-destroys once this handler
  // resolves — would work identically here, since nothing below ever
  // rejects; calling it explicitly just keeps "await save, then destroy"
  // visible in this file instead of resting on that SDK default.)
  event.preventDefault();
  await handleQuitRequested();
});

void renderLanding();
