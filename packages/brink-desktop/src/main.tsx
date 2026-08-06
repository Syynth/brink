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
    // Persistence (D1): the #154 egress writes through to disk — batches
    // are debounced ~500 ms and flush immediately on save and unmount.
    // See TauriFileProvider's doc for why this, and what D2 changes.
    onFilesChanged: (changes: FileChange[]) => {
      void provider.writeChanges(changes).catch((e: unknown) => {
        // Surfacing write failures properly is D2 (Output channel); for
        // the D1 spike a console error beats a silent data drop.
        console.error("[brink-desktop] failed to persist changes", e);
      });
    },
  });
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
 * Unmount the current project — unmount flushes the #154 egress, so no
 * edit is lost — and restore the landing screen. The dirty-state close
 * prompt is D2.
 */
function closeProject(): void {
  if (current === null) return;
  current.unmount();
  current = null;
  document.title = "Brink Studio";
  renderLanding();
}

void listen("menu:open-project", () => void chooseAndOpen());
void listen("menu:close-project", () => closeProject());

renderLanding();
