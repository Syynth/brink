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
 */

import { listen } from "@tauri-apps/api/event";
import { mountStudio, type StudioHandle } from "@brink-lang/studio";
import type { FileChange } from "@brink-lang/editor";
import { TauriFileProvider, pickProjectFolder } from "./tauri-provider.js";

/** Loadable project extensions; keep in sync with `list_files` in src-tauri. */
const ENTRY_FALLBACKS = ["story.brink", "main.ink", "main.brink", "story.ink"];

/** The one open project. D1 is single-window, single-project by ruling. */
let current: StudioHandle | null = null;
/** The autosave ticker for the open project; cleared on close. */
let autosaveTimer: ReturnType<typeof setInterval> | null = null;

/**
 * Resolve the tab to open first. `ProjectSession` discovers `brink.toml`
 * itself (#2324) — dialect, conventions, lints all flow from discovery —
 * but `mountStudio` needs an `entryFile` for the initial tab before any
 * discovery has run, so the shell peeks at `[project] entry` with a
 * deliberately dumb regex (full TOML fidelity lives in Rust; a miss here
 * costs only which tab opens first).
 */
function resolveEntryFile(files: Record<string, string>): string {
  const toml = files["brink.toml"];
  if (toml) {
    const m = toml.match(/^\s*entry\s*=\s*"([^"]+)"\s*$/m);
    if (m && files[m[1]] !== undefined) return m[1];
  }
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

  current = await mountStudio(el, {
    files,
    provider,
    entryFile: resolveEntryFile(files),
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
 * too). The dirty-state close PROMPT is still queued in the epic.
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
  document.title = "Brink Studio";
  renderLanding();
}

void listen("menu:open-project", () => void chooseAndOpen());
void listen("menu:close-project", () => closeProject());

renderLanding();
