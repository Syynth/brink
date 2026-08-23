/**
 * The story door's governing-config warning (#3010/#3021 — compare against
 * `docs/design/project-open-flow/Conflict.dc.html`: the banner on the
 * app's own `--bs-conflict-banner-bg`, and the "How the config was found"
 * walk-up trace).
 *
 * Rendered by the shell into its own banner host ABOVE the mounted studio
 * (the shell owns project identity, so the shell owns this banner), not
 * through the studio's notification surface: the ruling wants a persistent
 * banner with actions, not a toast.
 *
 * Pure DOM construction from a {@link ConflictModel} + action callbacks,
 * kept out of `main.tsx` so it unit-tests under jsdom without any IPC.
 * Every path is inserted via `textContent`, never HTML interpolation.
 */

import type { ConflictModel } from "./project-open.js";

export interface ConflictBannerActions {
  /** Reopen on the toml door, rewriting the recents entry in place. Only
   *  offered when the opened file IS the config's declared entry (the
   *  ruling's exact condition). */
  switchToProject(): void;
  /** Dismiss — the explicit open stands (it already won; the banner is
   *  what keeps that from being silent). */
  keepStandalone(): void;
}

const WARN_ICON = `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>`;

export function renderConflictBanner(
  host: HTMLElement,
  model: ConflictModel,
  actions: ConflictBannerActions,
): void {
  host.replaceChildren();

  const banner = document.createElement("div");
  banner.className = "conflict-banner";

  const icon = document.createElement("span");
  icon.className = "conflict-icon";
  icon.innerHTML = WARN_ICON; // static markup constant, no interpolation
  banner.appendChild(icon);

  const body = document.createElement("div");
  body.className = "conflict-body";

  const msg = document.createElement("div");
  msg.className = "conflict-msg";
  msg.append("Opened as a standalone project. A project config governs this file — ");
  const code = document.createElement("code");
  code.textContent = model.relConfig;
  msg.append(code);
  msg.append(model.openedIsEntry ? " — and names it as its entry." : ".");
  body.appendChild(msg);

  for (const warning of model.warnings) {
    const line = document.createElement("div");
    line.className = "conflict-warning";
    line.textContent = warning;
    body.appendChild(line);
  }

  const buttons = document.createElement("div");
  buttons.className = "conflict-actions";
  if (model.openedIsEntry) {
    const switchBtn = document.createElement("button");
    switchBtn.className = "conflict-switch";
    switchBtn.textContent = "Switch to that project";
    switchBtn.addEventListener("click", () => actions.switchToProject());
    buttons.appendChild(switchBtn);
  }
  const keepBtn = document.createElement("button");
  keepBtn.className = "conflict-keep";
  keepBtn.textContent = "Keep standalone";
  keepBtn.addEventListener("click", () => actions.keepStandalone());
  buttons.appendChild(keepBtn);
  body.appendChild(buttons);

  // The walk-up trace — how the config was found. Collapsed by default:
  // it is the explanation, not the decision.
  const details = document.createElement("details");
  details.className = "conflict-trace";
  const summary = document.createElement("summary");
  summary.textContent = "How the config was found";
  details.appendChild(summary);
  const rows = document.createElement("div");
  rows.className = "conflict-trace-rows";
  for (const row of model.trace) {
    const rowEl = document.createElement("div");
    rowEl.className = row.found ? "trace-row trace-found" : "trace-row";
    const step = document.createElement("span");
    step.className = "trace-step";
    step.textContent = String(row.step);
    const path = document.createElement("span");
    path.className = "trace-path";
    path.textContent = row.path;
    const note = document.createElement("span");
    note.className = "trace-note";
    note.textContent = row.note;
    rowEl.append(step, path, note);
    rows.appendChild(rowEl);
  }
  details.appendChild(rows);
  const hint = document.createElement("div");
  hint.className = "conflict-trace-hint";
  hint.textContent =
    "Discovery walks up, exactly as the compiler does — a config two folders above still governs.";
  details.appendChild(hint);
  body.appendChild(details);

  banner.appendChild(body);
  host.appendChild(banner);
}

export function clearConflictBanner(host: HTMLElement): void {
  host.replaceChildren();
}
