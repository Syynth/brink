/**
 * Desktop shell entry (docs/desktop-shell-spec.md, D1).
 *
 * The shell is deliberately thin: pick a folder, build a
 * `TauriFileProvider`, hand everything to `mountStudio` — the same
 * embedder API the playground and any external host use. Everything below
 * this file is the editor packages' business.
 */

import { mountStudio, type StudioHandle } from "@brink-lang/studio";
import type { FileChange } from "@brink-lang/editor";
import { TauriFileProvider, pickProjectFolder } from "./tauri-provider.js";

/** Loadable project extensions; keep in sync with `list_files` in src-tauri. */
const ENTRY_FALLBACKS = ["story.brink", "main.ink", "main.brink", "story.ink"];

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

async function openProject(root: string): Promise<StudioHandle> {
  const provider = new TauriFileProvider(root);
  const paths = await provider.listFiles();
  const files: Record<string, string> = {};
  for (const path of paths) {
    files[path] = await provider.readFile(path);
  }

  const appRoot = document.getElementById("app");
  if (!appRoot) throw new Error("Missing #app container");
  appRoot.replaceChildren(); // drop the landing screen

  const folderName = root.split("/").at(-1) ?? root;
  document.title = `${folderName} — Brink Studio`;

  return mountStudio(appRoot, {
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

function bindLanding(): void {
  const button = document.getElementById("open-project");
  if (!button) return;
  button.addEventListener("click", () => {
    void (async () => {
      const root = await pickProjectFolder();
      if (root === null) return; // user cancelled
      button.setAttribute("disabled", "true");
      try {
        await openProject(root);
      } catch (e: unknown) {
        button.removeAttribute("disabled");
        console.error("[brink-desktop] failed to open project", e);
      }
    })();
  });
}

bindLanding();
