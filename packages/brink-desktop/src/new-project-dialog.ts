/**
 * The New Project dialog (#3012 — compare against
 * `docs/design/project-open-flow/NewProject.dc.html`): pick an existing
 * folder, name the entry file, and see EXACTLY what will be created —
 * `main.ink` (a starter story that plays on first Run) plus a `brink.toml`
 * naming it. The "Will create" panel is the anti-#3010 guarantee made
 * visible: a new project is never born without an entry point.
 *
 * Pure DOM construction over an injected {@link NewProjectApi}, kept out
 * of `main.tsx` for unit-testability (the same reason `quit.ts` and
 * `file-open.ts` exist). All user-influenced strings are inserted via
 * `textContent`.
 */

import { validateEntryName } from "./project-open.js";

export interface NewProjectApi {
  /** Native folder picker; null when cancelled. */
  chooseFolder(): Promise<string | null>;
  /** The shell's `create_project` command; resolves to the created
   *  brink.toml's absolute path, rejects with a human-readable reason. */
  create(dir: string, entry: string): Promise<string>;
  /** Open the created project (the toml door). */
  open(tomlPath: string): Promise<void>;
}

/** Show the dialog as a modal overlay on `document.body`. Returns the
 *  overlay element (removed on Cancel/Escape/successful create) so tests
 *  can drive it directly. At most one instance: a second call while one
 *  is open focuses the existing dialog instead of stacking. */
export function showNewProjectDialog(api: NewProjectApi): HTMLElement {
  const existing = document.getElementById("new-project-overlay");
  if (existing !== null) {
    return existing;
  }

  const overlay = document.createElement("div");
  overlay.id = "new-project-overlay";

  const dialog = document.createElement("div");
  dialog.className = "np-dialog";
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-label", "New Project");

  // Header
  const header = document.createElement("div");
  header.className = "np-header";
  const title = document.createElement("div");
  title.className = "np-title";
  title.textContent = "New Project";
  const sub = document.createElement("div");
  sub.className = "np-sub";
  sub.textContent = "Creates a story file and a project config, so the project plays immediately.";
  header.append(title, sub);

  // Location field
  const fields = document.createElement("div");
  fields.className = "np-fields";
  const locLabel = document.createElement("div");
  locLabel.className = "np-label";
  locLabel.textContent = "Location";
  const locRow = document.createElement("div");
  locRow.className = "np-loc-row";
  const locPath = document.createElement("div");
  locPath.className = "np-loc-path";
  locPath.textContent = "No folder chosen";
  locPath.classList.add("np-loc-empty");
  const chooseBtn = document.createElement("button");
  chooseBtn.className = "np-choose";
  chooseBtn.textContent = "Choose…";
  locRow.append(locPath, chooseBtn);

  // Entry field
  const entryLabel = document.createElement("div");
  entryLabel.className = "np-label";
  entryLabel.textContent = "Entry file";
  const entryInput = document.createElement("input");
  entryInput.className = "np-entry";
  entryInput.type = "text";
  entryInput.value = "main.ink";
  entryInput.spellcheck = false;

  // Will-create panel
  const panel = document.createElement("div");
  panel.className = "np-panel";
  const panelCap = document.createElement("div");
  panelCap.className = "np-panel-cap";
  panelCap.textContent = "Will create";
  const panelRows = document.createElement("div");
  panelRows.className = "np-panel-rows";
  const storyRow = document.createElement("div");
  storyRow.className = "np-panel-row";
  const storyName = document.createElement("code");
  storyName.className = "np-create-ink";
  const storyNote = document.createElement("span");
  storyNote.textContent = "a starter story — plays on first run";
  storyRow.append(storyName, storyNote);
  const tomlRow = document.createElement("div");
  tomlRow.className = "np-panel-row";
  const tomlName = document.createElement("code");
  tomlName.className = "np-create-toml";
  tomlName.textContent = "brink.toml";
  const tomlNote = document.createElement("span");
  tomlNote.className = "np-toml-note";
  tomlRow.append(tomlName, tomlNote);
  panelRows.append(storyRow, tomlRow);
  panel.append(panelCap, panelRows);

  // Error line
  const error = document.createElement("div");
  error.className = "np-error";
  error.hidden = true;

  fields.append(locLabel, locRow, entryLabel, entryInput, panel, error);

  // Footer
  const footer = document.createElement("div");
  footer.className = "np-footer";
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "np-cancel";
  cancelBtn.textContent = "Cancel";
  const createBtn = document.createElement("button");
  createBtn.className = "np-create";
  createBtn.textContent = "Create Project";
  footer.append(cancelBtn, createBtn);

  dialog.append(header, fields, footer);
  overlay.appendChild(dialog);
  document.body.appendChild(overlay);

  // ── State + behavior ──
  let dir: string | null = null;
  let creating = false;

  const refresh = (): void => {
    const entry = entryInput.value.trim();
    const problem = validateEntryName(entry);
    storyName.textContent = problem === null ? entry : "—";
    tomlNote.textContent = problem === null ? `entry = "${entry}"` : "—";
    if (problem !== null) {
      error.hidden = false;
      error.textContent = `Entry file ${problem}.`;
    } else {
      error.hidden = true;
      error.textContent = "";
    }
    createBtn.disabled = creating || dir === null || problem !== null;
  };

  const close = (): void => {
    overlay.remove();
    document.removeEventListener("keydown", onKey);
  };
  const onKey = (e: KeyboardEvent): void => {
    if (e.key === "Escape") close();
  };
  // DISMISS-NET-EXEMPT: shell-level modal on the landing screen, outside
  // any studio mount — no dismiss-registry is active there. The Escape
  // handling is self-contained: the listener is added with the overlay and
  // removed in close() on every exit path (Cancel, Escape, create).
  document.addEventListener("keydown", onKey);

  chooseBtn.addEventListener("click", () => {
    void (async () => {
      const picked = await api.chooseFolder();
      if (picked === null) return;
      dir = picked;
      locPath.textContent = picked;
      locPath.classList.remove("np-loc-empty");
      refresh();
    })();
  });

  entryInput.addEventListener("input", refresh);
  cancelBtn.addEventListener("click", close);

  createBtn.addEventListener("click", () => {
    void (async () => {
      if (dir === null) return;
      const entry = entryInput.value.trim();
      creating = true;
      refresh();
      try {
        const tomlPath = await api.create(dir, entry);
        close();
        await api.open(tomlPath);
      } catch (e: unknown) {
        creating = false;
        refresh();
        error.hidden = false;
        error.textContent = e instanceof Error ? e.message : String(e);
      }
    })();
  });

  refresh();
  entryInput.focus();
  return overlay;
}
