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
 * awaits the final `saveAll` (capped) before the window is destroyed.
 */

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save } from "@tauri-apps/plugin-dialog";
import { mountStudio, type StudioHandle } from "@brink-lang/studio";
import type { FileChange } from "@brink-lang/editor";
import { TauriFileProvider, pickProjectFolder } from "./tauri-provider.js";
import { awaitSaveAllBeforeQuit } from "./quit.js";
import { runCli } from "./cli.js";

/** Loadable project extensions; keep in sync with `list_files` in src-tauri. */
const ENTRY_FALLBACKS = ["story.brink", "main.ink", "main.brink", "story.ink"];

/** The one open project. D1 is single-window, single-project by ruling. */
let current: StudioHandle | null = null;
/** The autosave ticker for the open project; cleared on close. */
let autosaveTimer: ReturnType<typeof setInterval> | null = null;
/**
 * The open project's absolute root and resolved entry file — tracked
 * alongside `current` so `exportXliff` (D3, #2392) has an absolute input
 * path to hand the sidecar without re-deriving it from the provider.
 */
let currentRoot: string | null = null;
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

function renderLanding(): void {
  const root = appRoot();
  root.innerHTML = `
    <div id="landing">
      <button id="open-project">Open Project Folder…</button>
      <div class="hint">A folder containing .brink / .ink files (and optionally a brink.toml) — or ⌘O</div>
    </div>`;
  const button = document.getElementById("open-project");
  button?.addEventListener("click", () => void chooseAndOpen());
}

async function openProject(root: string): Promise<void> {
  const provider = new TauriFileProvider(root);
  const paths = await provider.listFiles();
  const files: Record<string, string> = {};
  for (const path of paths) {
    files[path] = await provider.readFile(path);
  }

  // Tear down any previous project only after the new one's files loaded,
  // so a cancelled or failed open never leaves a blank window.
  closeProject();

  const el = appRoot();
  el.replaceChildren();

  const folderName = root.split("/").at(-1) ?? root;
  document.title = `${folderName} — Brink Studio`;

  const entryFile = resolveEntryFile(files);
  currentRoot = root;
  currentEntryFile = entryFile;

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

  // Autosave IS saveAll (celeris §10.1.1): one save path, one artifact
  // class. Clean ticks are no-ops inside the command. 2-minute default
  // (2026-08-07 ruling); a Settings surface can adjust later.
  const AUTOSAVE_MS = 120_000;
  const api = current.api;
  autosaveTimer = setInterval(() => {
    if (api.getDirtyFiles().length > 0) api.dispatch("file.saveAll");
  }, AUTOSAVE_MS);
}

async function chooseAndOpen(): Promise<void> {
  const root = await pickProjectFolder();
  if (root === null) return; // user cancelled
  try {
    await openProject(root);
  } catch (e: unknown) {
    console.error("[brink-desktop] failed to open project", e);
    if (current === null) renderLanding();
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
 */
function closeProject(): void {
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
  renderLanding();
}

/**
 * File > Export XLIFF… (D3, #2392) — proves the `brink-cli` sidecar path
 * end to end. Deliberately minimal: export the currently open project's
 * entry file at the source language, prompting only for where to save the
 * `.xlf`. The fuller intl UI (locale picker, compile-locale/regenerate
 * batch ops, progress from the streamed `cli:output` events) is future
 * work — this item exists to exercise one real path, not to be it.
 *
 * The input handed to the sidecar is always the entry file's `.ink`/
 * `.brink` source (never `.ink.json` — house rule, and there is no
 * `.ink.json` anywhere in this flow to begin with).
 */
async function exportXliff(): Promise<void> {
  const api = current?.api;
  if (currentRoot === null || currentEntryFile === null || api === undefined) {
    console.warn("[brink-desktop] Export XLIFF: no project open");
    return;
  }
  const inputPath = `${currentRoot}/${currentEntryFile}`;
  const defaultName = `${currentEntryFile.split("/").at(-1)?.replace(/\.(ink|brink)$/, "") ?? "story"}.xlf`;
  const outputPath = await save({
    defaultPath: defaultName,
    filters: [{ name: "XLIFF", extensions: ["xlf"] }],
  });
  if (outputPath === null) return; // user cancelled

  try {
    const exitCode = await runCli(["export-xliff", inputPath, "--output", outputPath]);
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

/**
 * App quit (#2370, docs/decision-log.md 2026-08-07 "Desktop close: no dirty
 * prompt; quit awaits the final save"): NO confirmation prompt — explicitly
 * ruled out. The one safety piece is that the window must not actually
 * close until the final `saveAll` has had its chance to land, so quitting
 * can never race the in-flight canonical write. `awaitSaveAllBeforeQuit` is
 * capped (~3s) — a hung write cannot make the app unquittable, since the
 * backup ring (#154) already bounds the loss.
 */
async function handleQuitRequested(): Promise<void> {
  if (current !== null) {
    await awaitSaveAllBeforeQuit(current.api);
  }
  await getCurrentWindow().destroy();
}

void listen("menu:open-project", () => void chooseAndOpen());
void listen("menu:close-project", () => closeProject());
void listen("menu:export-xliff", () => void exportXliff());
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

renderLanding();
